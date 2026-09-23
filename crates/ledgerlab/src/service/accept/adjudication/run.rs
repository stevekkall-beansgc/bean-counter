//! One acceptance loop for central and gateway journals. The configured host
//! resolves trusted sources and original documents; the coordinator alone plans.
use super::*;
use crate::{
    service::store_error,
    store::{
        errors::CommitError,
        outcomes::{OutcomeLock, OutcomeLockClass, OutcomeLockMode},
        ports::AcceptanceTx,
    },
    ServiceError,
};
use ledgerlab_core::adjudication::{
    self as r3,
    runtime::{self as rt, accounting::Worksheet, points as p},
};
use serde_json::json;
use std::future::Future;
use tokio::time::Instant;
#[derive(Clone, Debug)]
pub(crate) enum SourceRequest {
    Exact(Box<wire::Proof>),
    Enrollment { journal: JournalIdentity },
}
pub(crate) trait AdjudicationHost<T: AdjudicationTx>: AdjudicationAuthority {
    /// Complete currently known host/legacy authority and original base locks.
    /// Coordinator adds discovered point locks and restarts before acquiring them.
    fn guards(
        &self,
        command: &ParsedCommand,
        journal: &JournalIdentity,
    ) -> Result<Vec<Guard>, ServiceError>;
    fn source(
        &self,
        request: &SourceRequest,
    ) -> impl Future<Output = Result<VerifiedSource, ServiceError>> + Send;
    /// The returned private value includes original-profile writes. Implementations
    /// call original prepare_fresh_base with THIS transaction's locked snapshot.
    fn fresh_base(
        &self,
        tx: &mut T,
        terms: &wire::Enroll,
        inputs: &LockedInputs,
    ) -> impl Future<Output = Result<FreshBaseAcceptance, ServiceError>> + Send;
}
fn core<T>(value: ledgerlab_core::Result<T>) -> Result<T, ServiceError> {
    value.map_err(|e| ServiceError::Rejection(e.code.into()))
}
fn required(ok: bool) -> Result<(), ServiceError> {
    if ok {
        Ok(())
    } else {
        Err(ServiceError::IntegrityFailure)
    }
}
fn normalized(mut guards: Vec<Guard>) -> Result<Vec<Guard>, ServiceError> {
    guards.sort_by_key(Guard::order_key);
    let mut out: Vec<Guard> = Vec::new();
    for g in guards {
        if let Some(previous) = out.last_mut() {
            if previous.order_key() == g.order_key() {
                match (previous, &g) {
                    (Guard::Legacy(a), Guard::Legacy(b))
                        if a.class == b.class && a.key == b.key =>
                    {
                        a.mode = a.mode.max(b.mode);
                        continue;
                    }
                    (a, b) if a == b => continue,
                    _ => return Err(ServiceError::IntegrityFailure),
                }
            }
        }
        out.push(g);
    }
    required(out.len() <= 512)?;
    Ok(out)
}
fn point_guard(head: &HeadKey) -> Guard {
    let class = match head.kind {
        HeadKind::Enrollment | HeadKind::Authority | HeadKind::Delivery => {
            GuardClass::EnrollmentNamespace
        }
        HeadKind::Grant
        | HeadKind::GrantRegistry
        | HeadKind::Token
        | HeadKind::Allocation
        | HeadKind::Receipt
        | HeadKind::Resource
        | HeadKind::Counter
        | HeadKind::VerifiedCursor => GuardClass::CapacityAllocation,
        HeadKind::Gateway | HeadKind::Round => GuardClass::GatewayRound,
        HeadKind::Family => GuardClass::FamilyPrerequisite,
        HeadKind::Case => GuardClass::Case,
        HeadKind::Entitlement => GuardClass::Entitlement,
        HeadKind::Supplier => GuardClass::SupplierPool,
        HeadKind::Adjustment => GuardClass::AdjustmentPool,
    };
    Guard::R3 {
        class,
        host: head.journal.host.clone(),
        key: head.full_key.clone(),
    }
}
fn unknown(key: &wire::Delivery) -> ServiceError {
    ServiceError::OutcomeUnknown {
        scope: [key.0 .0.as_str().into(), key.0 .1.as_str().into()],
        source: key.1.as_str().into(),
        external_id: key.2.as_str().into(),
    }
}
fn command_sources(command: &ParsedCommand) -> Result<Vec<wire::Proof>, ServiceError> {
    let v = core(rt::command_value(command.command()))?;
    let p = &v["payload"];
    let mut proofs = Vec::new();
    for name in ["proof", "begin"] {
        if let Some(value) = p.get(name) {
            proofs.push(
                serde_json::from_value(value.clone())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            );
        }
    }
    if let Some(values) = p.get("preparations") {
        for value in values.as_array().ok_or(ServiceError::IntegrityFailure)? {
            proofs.push(
                serde_json::from_value(value.clone())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            );
        }
    }
    required(proofs.len() <= 6)?;
    Ok(proofs)
}
fn head(
    journal: &JournalIdentity,
    kind: HeadKind,
    tag: [u8; 8],
    id: &str,
) -> Result<HeadKey, ServiceError> {
    Ok(HeadKey {
        journal: journal.clone(),
        kind,
        full_key: core(rt::index_key(tag, &[id.as_bytes()]))?,
    })
}
enum Attempt {
    Restart(Vec<HeadKey>, Vec<Guard>),
    Reply(wire::CommandResult),
    Plan(Box<ValidatedAdjudicationPlan>, Box<CommitCapability>),
}
#[allow(clippy::too_many_arguments)]
async fn attempt<T: AdjudicationTx, H: AdjudicationHost<T>>(
    tx: &mut T,
    host: &H,
    journal: &JournalIdentity,
    command: &ParsedCommand,
    key: &wire::Delivery,
    guards: &[Guard],
    heads: &[HeadKey],
) -> Result<Attempt, ServiceError> {
    tx.lock_adjudication(guards).await.map_err(store_error)?;
    let saved = tx
        .lookup_adjudication(journal, key)
        .await
        .map_err(store_error)?;
    let resolution = tx
        .resolve_adjudication(&ResolveRequest {
            journal: journal.clone(),
            key: key.clone(),
            guards: guards.to_vec(),
            objects: command_sources(command)?,
            heads: heads.to_vec(),
        })
        .await
        .map_err(store_error)?;
    let mut inputs = match resolution {
        Resolution::Complete(i) => *i,
        Resolution::MoreLocks(g) => return Ok(Attempt::Restart(vec![], g)),
        Resolution::Missing(_) => return Err(ServiceError::IntegrityFailure),
    };
    required(inputs.journal == *journal && inputs.heads.len() <= 256)?;
    if let Some(saved) = saved {
        let previous = core(ParsedCommand::parse(&saved.command))?;
        let observation = host.current(command, &inputs, AuthorityAccess::ReadSavedResult)?;
        let mut missing = Vec::new();
        for current in &observation.current_heads {
            required(current.key.journal == *journal)?;
            if let Some(locked) = inputs.heads.iter().find(|h| h.key == current.key) {
                required(locked.revision == current.revision && locked.value == current.value)?;
            } else {
                missing.push(current.key.clone());
            }
        }
        if !missing.is_empty() {
            let guards = missing.iter().map(point_guard).collect();
            return Ok(Attempt::Restart(missing, guards));
        }
        let sources = core(authority::Sources::new(&observation.exact_sources))?;
        let target = inputs
            .heads
            .iter()
            .filter_map(|h| h.value.as_deref())
            .find_map(|raw| match prepare::parsed_state(raw).ok() {
                Some(p::State::Enrollment(e)) => Some(e.terms.target.clone()),
                _ => None,
            });
        core(sources.current(
            command,
            &observation,
            target.as_ref(),
            AuthorityAccess::ReadSavedResult,
        ))?;
        if core(rt::command_digest(previous.command()))?
            != core(rt::command_digest(command.command()))?
        {
            return Err(ServiceError::Rejection("IDENTITY_CONFLICT".into()));
        }
        let mut result = saved.result;
        result.status = wire::CommandResultStatus::Duplicate;
        result.code = "EXACT_RETRY".into();
        result.root = inputs.prefix.root().clone();
        return Ok(Attempt::Reply(result));
    }
    let cap = tx.commit_capability().await.map_err(store_error)?;
    for proof in command_sources(command)? {
        let source = host
            .source(&SourceRequest::Exact(Box::new(proof.clone())))
            .await?;
        required(source.proof() == &proof)?;
        if inputs.sources.iter().all(|s| s.proof() != &proof) {
            inputs.sources.push(source);
        }
    }
    let mut base = None;
    let mut scan = None;
    loop {
        required(inputs.sources.len() <= 6)?;
        if let wire::Command::Enroll { payload, .. } = command.command() {
            if base.is_none() {
                base = Some(host.fresh_base(tx, payload, &inputs).await?);
            }
        }
        match core(prepare::prepare_with_seal(
            command,
            &inputs,
            &cap,
            host,
            base.as_ref(),
            guards,
            scan.as_ref(),
        ))? {
            Prepared::Need(next) => {
                let mut extra = Vec::new();
                for h in &next {
                    required(h.journal == *journal)?;
                    let g = point_guard(h);
                    if !guards.iter().any(|held| held == &g) {
                        extra.push(g);
                    }
                }
                // Point discovery cannot add a new lock after a later-ranked guard was held.
                // Rollback and reacquire the complete ordered set; no draft escapes.
                return Ok(Attempt::Restart(next, extra));
            }
            Prepared::NeedEnrollment {
                journal,
                registration,
            } => {
                required(inputs.sources.len() < 6)?;
                let source = host
                    .source(&SourceRequest::Enrollment {
                        journal: journal.clone(),
                    })
                    .await?;
                required(
                    source.prefix().journal() == &journal
                        && source.proof().fact_kind == wire::FactKind::Enrollment
                        && serde_json::to_value(&source.proof().full_key).ok()
                            == Some(json!(registration)),
                )?;
                required(inputs.sources.iter().all(|s| s.proof() != source.proof()))?;
                inputs.sources.push(source);
            }
            Prepared::NeedSealScan {
                round,
                gateway,
                cutoff,
                high,
            } => {
                required(scan.is_none())?;
                scan = Some(
                    seal::scan(
                        tx,
                        &cap,
                        key,
                        guards,
                        seal::Selection {
                            round,
                            gateway,
                            cutoff,
                            high,
                        },
                    )
                    .await?,
                );
            }
            Prepared::Plan(plan) => return Ok(Attempt::Plan(plan, Box::new(cap))),
        }
    }
}
pub(crate) async fn run<S: AdjudicationStore, H: AdjudicationHost<S::Tx>>(
    store: &S,
    host: &H,
    journal: JournalIdentity,
    command: ParsedCommand,
    deadline: Instant,
) -> Result<wire::CommandResult, ServiceError> {
    let v = core(rt::command_value(command.command()))?;
    let kind = v["kind"].as_str().ok_or(ServiceError::IntegrityFailure)?;
    let key: wire::Delivery =
        serde_json::from_value(v["key"].clone()).map_err(|_| ServiceError::IntegrityFailure)?;
    let owner = match kind {
        "PREPARE_ENROLL" | "ACTIVATE" | "RECEIVE" | "RETURN_UNUSED" | "LOCAL_TERMINAL"
        | "SEAL_BEGIN" | "SEALED" | "INSTALL" | "PREPARE_ROUND" | "REPLACE_WRITER" => {
            v["payload"]["gateway"].as_str()
        }
        "LOCAL_GRANT" => v["payload"]["grant"]["gateway"].as_str(),
        "EXTEND_RESOURCES" => v["payload"]["host"].as_str(),
        _ => Some(journal.store.as_str()),
    };
    if owner != Some(journal.host.as_str()) || key.0 != journal.scope {
        return Err(ServiceError::Rejection("WRONG_OWNER".into()));
    }
    let digest = core(rt::command_digest(command.command()))?;
    let maximum = core(
        core(Worksheet::frozen())?
            .template(kind)
            .and_then(|t| t.resources()),
    )?;
    let work = WorkRequest {
        journal: journal.clone(),
        owner: core(Id::parse(digest.as_str()))?,
        transition: digest,
        mandatory: !matches!(
            kind,
            "PREPARE_ENROLL"
                | "ENROLL"
                | "LOCAL_GRANT"
                | "REGISTER_GRANT"
                | "ISSUE"
                | "DECIDE"
                | "CORRECT"
                | "SUPPLEMENT"
                | "EXTEND_RESOURCES"
                | "REPLACE_WRITER"
        ),
        maximum,
    };
    let mut heads = vec![head(
        &journal,
        HeadKind::Enrollment,
        *b"ENROLL__",
        journal.registration.as_str(),
    )?];
    let mut guards = host.guards(&command, &journal)?;
    guards.push(Guard::Legacy(OutcomeLock {
        class: OutcomeLockClass::Admission,
        key: core(r3::canonical_bytes(
            &json!([journal.scope]),
            r3::COMMAND_BYTES,
        ))?,
        mode: OutcomeLockMode::Write,
    }));
    guards.push(point_guard(&heads[0]));
    guards = normalized(guards)?;
    for _ in 0..512 {
        if Instant::now() >= deadline {
            return Err(ServiceError::Retryable);
        }
        let mut tx = store
            .begin_adjudication(&work, deadline)
            .await
            .map_err(store_error)?;
        match attempt(&mut tx, host, &journal, &command, &key, &guards, &heads).await {
            Ok(Attempt::Restart(next, extra)) => {
                tx.rollback().await.map_err(store_error)?;
                let before = (heads.len(), guards.len());
                for h in next {
                    if !heads.contains(&h) {
                        heads.push(h);
                    }
                }
                guards = normalized([guards, extra].concat())?;
                required(heads.len() <= 256 && (heads.len(), guards.len()) != before)?;
            }
            Ok(Attempt::Reply(result)) => {
                tx.rollback().await.map_err(store_error)?;
                return Ok(result);
            }
            Ok(Attempt::Plan(plan, cap)) => {
                if let Err(e) = tx.append_adjudication(&plan, &cap).await {
                    tx.rollback().await.map_err(store_error)?;
                    return Err(store_error(e));
                }
                let result = plan.segment().result.clone();
                match tx.commit().await {
                    Ok(()) => return Ok(result),
                    Err(CommitError::OutcomeUnknown) => return Err(unknown(&key)),
                    Err(CommitError::RolledBack(e)) => return Err(store_error(e)),
                }
            }
            Err(e) => {
                tx.rollback().await.map_err(store_error)?;
                return Err(e);
            }
        }
    }
    Err(ServiceError::Retryable)
}
