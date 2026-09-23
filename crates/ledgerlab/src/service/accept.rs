//! The sole acceptance algorithm. Store adapters neither resolve authority nor price.
use super::{hooks::Hooks, project, store_error};
use crate::{
    store::{
        errors::CommitError,
        ports::{AcceptanceStore, AcceptanceTx},
        records::*,
    },
    *,
};
use ledgerlab_core::{
    canonical::{self, CanonicalBytes, Domain},
    domain::{self, AcceptanceContext, Candidate, Document, ResolvedInput, Revision},
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::Instant;

fn core(e: ledgerlab_core::Error) -> ServiceError {
    ServiceError::Rejection(e.code.into())
}
fn reject(code: &str) -> ServiceError {
    ServiceError::Rejection(code.into())
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, ServiceError> {
    v[key].as_str().ok_or(ServiceError::IntegrityFailure)
}
fn unknown(c: &Candidate) -> ServiceError {
    ServiceError::OutcomeUnknown {
        scope: [c.scope().tenant().into(), c.scope().environment().into()],
        source: c.source().into(),
        external_id: c.external_id().into(),
    }
}
fn scope(c: &Candidate) -> crate::store::records::Scope {
    crate::store::records::Scope {
        tenant: c.scope().tenant().into(),
        environment: c.scope().environment().into(),
    }
}

pub(crate) async fn run<S: AcceptanceStore>(
    store: &S,
    cmd: &AcceptCommand,
    hooks: &Hooks,
) -> Result<AcceptResult, ServiceError>
where
    S::Tx: 'static,
{
    let candidate = match domain::normalize(
        &cmd.bytes,
        cmd.principal.scope.clone(),
        &cmd.principal.source,
    ) {
        Ok(c) => c,
        Err(e) => {
            return Ok(AcceptResult::Rejected {
                code: e.code.into(),
            })
        }
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    for attempt in 0..5 {
        let mut tx = match hooks.call("begin", store.begin(deadline)).await {
            Ok(tx) => tx,
            Err(e) if e.retryable_after_rollback() && attempt < 4 && Instant::now() < deadline => {
                continue
            }
            Err(e) => return Err(store_error(e)),
        };
        let work = prepare(&mut tx, &candidate, cmd, hooks, false).await;
        match work {
            Ok(Work::End(result)) => {
                hooks
                    .call("rollback", tx.rollback())
                    .await
                    .map_err(store_error)?;
                return Ok(result);
            }
            Err(error) => {
                hooks
                    .call("rollback", tx.rollback())
                    .await
                    .map_err(store_error)?;
                hooks.call("cleanup", std::future::ready(())).await;
                if error == ServiceError::Retryable && attempt < 4 && Instant::now() < deadline {
                    continue;
                }
                return match error {
                    ServiceError::Rejection(code) => Ok(AcceptResult::Rejected { code }),
                    e => Err(e),
                };
            }
            Ok(Work::Preview(_)) => unreachable!("accept never requests a preview"),
            Ok(Work::Commit(result)) => {
                if let Err(e) = hooks.before_commit() {
                    hooks
                        .call("rollback", tx.rollback())
                        .await
                        .map_err(store_error)?;
                    return Err(e);
                }
                #[cfg(test)]
                if let Some(durable) = hooks.unknown() {
                    hooks.hold_unknown(tx, durable);
                    return Err(unknown(&candidate));
                }
                match hooks.call("commit", tx.commit()).await {
                    Ok(()) => {
                        hooks.after_commit()?;
                        hooks.call("acknowledged", std::future::ready(())).await;
                        return Ok(result);
                    }
                    Err(CommitError::OutcomeUnknown) => return Err(unknown(&candidate)),
                    Err(CommitError::RolledBack(e))
                        if e.retryable_after_rollback() && attempt < 4 =>
                    {
                        continue
                    }
                    Err(CommitError::RolledBack(e)) => return Err(store_error(e)),
                }
            }
        }
    }
    Err(ServiceError::Retryable)
}
enum Work {
    End(AcceptResult),
    Commit(AcceptResult),
    Preview(Vec<Value>),
}
async fn document<T: AcceptanceTx>(
    tx: &mut T,
    s: &crate::store::records::Scope,
    id: &str,
    name: &str,
    hooks: &Hooks,
) -> Result<Document, ServiceError> {
    let (kind, record) = hooks
        .call(&format!("document:{name}"), tx.load_document(s, id))
        .await
        .map_err(store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let doc =
        Document::parse(&record.canonical_bytes).map_err(|_| ServiceError::IntegrityFailure)?;
    if doc.id() != id
        || doc.document_type() != kind
        || doc.content_hash() != record.content_hash
        || doc.bytes().as_slice() != record.canonical_bytes
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(doc)
}
// Retained duplicate data crosses the same integrity boundary as documents.
// Recompute canonical bytes and framed hashes before returning an old receipt.
pub(super) fn retained_record(record: &CanonicalRecord, kind: &str) -> Result<Value, ServiceError> {
    let body =
        canonical::parse(&record.canonical_bytes).map_err(|_| ServiceError::IntegrityFailure)?;
    if CanonicalBytes::from_value(&body)
        .map_err(|_| ServiceError::IntegrityFailure)?
        .as_slice()
        != record.canonical_bytes
        || canonical::digest(Domain::RecordContent, &json!([kind, 1, body]))
            .map_err(|_| ServiceError::IntegrityFailure)?
            != record.content_hash
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(body)
}
fn retained_receipt(record: &CanonicalRecord, event_id: &str) -> Result<(), ServiceError> {
    let body = retained_record(record, "receipt")?;
    if body["schema"] != "ledger-receipt/1"
        || body["event_id"] != event_id
        || body["id"]
            != canonical::identity(Domain::Receipt, &json!([event_id]))
                .map_err(|_| ServiceError::IntegrityFailure)?
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(())
}
async fn prepare<T: AcceptanceTx>(
    tx: &mut T,
    c: &Candidate,
    cmd: &AcceptCommand,
    hooks: &Hooks,
    preview: bool,
) -> Result<Work, ServiceError> {
    let s = scope(c);
    // SQLite BEGIN IMMEDIATE already holds the complete writer scope. Other stores
    // implement these reads with the ordered admission/authority/binding/chain locks.
    let installation = hooks
        .call("admission_lock", tx.load_installation())
        .await
        .map_err(store_error)?;
    if installation.scope != s {
        return Err(reject("SOURCE_UNAUTHORIZED"));
    }
    if installation.admission != "open" {
        return Err(ServiceError::Unavailable);
    }
    let who = &cmd.principal;
    if !who.can_read || c.source() != who.source {
        return Err(reject("SOURCE_UNAUTHORIZED"));
    }
    let authority = hooks
        .call("authority", tx.load_authority(&s, &who.authority_head))
        .await
        .map_err(store_error)?
        .ok_or_else(|| reject("SOURCE_UNAUTHORIZED"))?;
    if !authority.active || authority.revision <= 0 {
        return Err(reject("SOURCE_UNAUTHORIZED"));
    }
    let grant_id = hooks
        .call("grant", tx.load_grant_document(&s, &authority.grant_id))
        .await
        .map_err(store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let grant = document(tx, &s, &grant_id, "grant", hooks).await?;
    let g = grant.body();
    if text(g, "id")? != authority.grant_id
        || text(g, "principal_id")? != who.principal_id
        || text(g, "source")? != c.source()
        || !g["permissions"]
            .as_array()
            .is_some_and(|v| v.contains(&json!("read")))
        || Timestamp::parse(text(g, "starts_at")?)
            .map_err(core)?
            .micros()
            > cmd.received_at.micros()
        || g.get("ends_at")
            .and_then(Value::as_str)
            .map(Timestamp::parse)
            .transpose()
            .map_err(core)?
            .is_some_and(|t| cmd.received_at.micros() >= t.micros())
    {
        return Err(reject("SOURCE_UNAUTHORIZED"));
    }
    let stored_identity = match hooks
        .call(
            "identity",
            tx.load_identity(&s, c.source(), c.external_id()),
        )
        .await
    {
        Ok(stored) => stored,
        Err(crate::store::errors::StoreError::DeliveryConflict) => {
            return Ok(Work::End(AcceptResult::Conflict(ConflictKind::Identity)));
        }
        Err(error) => return Err(store_error(error)),
    };
    if let Some(stored) = stored_identity {
        let key = retained_record(&stored.key, "delivery-key")?;
        if key["schema"] != "ledger-delivery-key/1"
            || key["scope"] != json!([s.tenant, s.environment])
            || key["source"] != c.source()
            || key["external_id"] != c.external_id()
            || key["canonical_event_id"] != stored.event_id
            || key["ingress_hash"] != stored.ingress_hash
        {
            return Err(ServiceError::IntegrityFailure);
        }
        retained_receipt(&stored.receipt, &stored.event_id)?;
        let ingress = CanonicalBytes::from_value(&key["ingress"])
            .map_err(|_| ServiceError::IntegrityFailure)?;
        return Ok(Work::End(
            if stored.ingress_hash == c.ingress_hash() && ingress == *c.ingress_bytes() {
                AcceptResult::Duplicate {
                    kind: DuplicateKind::Identity,
                    receipt: stored.receipt.canonical_bytes,
                }
            } else {
                AcceptResult::Conflict(ConflictKind::Identity)
            },
        ));
    }
    let event = c.clone().resolve(None).map_err(core)?;
    if !event.dto().kind.is_work() || event.dto().links.as_ref().is_some_and(|v| !v.is_empty()) {
        return Err(reject("UNSUPPORTED_SLICE"));
    }
    let facts_hash = canonical::digest(
        Domain::ClaimFacts,
        &event.completion_facts(&[]).map_err(core)?,
    )
    .map_err(core)?;
    if let Some(stored) = hooks
        .call(
            "claim",
            tx.load_claim(&s, c.source(), c.operation_id(), "completion", "completion"),
        )
        .await
        .map_err(store_error)?
    {
        retained_receipt(&stored.receipt, &stored.event_id)?;
        if stored.facts_hash != facts_hash {
            return Ok(Work::End(AcceptResult::Conflict(ConflictKind::Semantic)));
        }
        let body = json!({"schema":"ledger-delivery-key/1","scope":[s.tenant,s.environment],"source":c.source(),"external_id":c.external_id(),"canonical_event_id":stored.event_id,"kind":"alias","ingress":canonical::parse(c.ingress_bytes().as_slice()).map_err(core)?,"ingress_hash":c.ingress_hash()});
        let record = JournalRecord {
            scope: s,
            canonical: CanonicalRecord {
                canonical_bytes: CanonicalBytes::from_value(&body).map_err(core)?.into_vec(),
                content_hash: canonical::digest(
                    Domain::RecordContent,
                    &json!(["delivery-key", 1, body]),
                )
                .map_err(core)?,
            },
            row: JournalRow::DeliveryKey {
                source: c.source().into(),
                external_id: c.external_id().into(),
                ingress_hash: c.ingress_hash().into(),
                canonical_event_id: stored.event_id,
                kind: "alias".into(),
                observed_us: cmd.received_at.micros(),
            },
        };
        if !preview {
            hooks
                .call("alias", tx.write(&WriteOp::Journal(Box::new(record))))
                .await
                .map_err(store_error)?;
        }
        return Ok(Work::Commit(AcceptResult::Duplicate {
            kind: DuplicateKind::Semantic,
            receipt: stored.receipt.canonical_bytes,
        }));
    }
    if !who.can_submit {
        return Err(reject("SOURCE_UNAUTHORIZED"));
    }
    if installation.mode != "sandbox" {
        return Err(reject("TERMS_NOT_ACCEPTED"));
    }
    let binding = hooks
        .call("binding", tx.load_binding(&s, &cmd.binding_selector))
        .await
        .map_err(store_error)?
        .ok_or_else(|| reject("TERMS_NOT_ACCEPTED"))?;
    if !binding.active {
        return Err(reject("TERMS_NOT_ACCEPTED"));
    }
    let chain = match hooks
        .call("chain", tx.load_chain(&s, event.chain()))
        .await
        .map_err(store_error)?
    {
        Some(c) => c,
        None => {
            return Ok(Work::End(AcceptResult::Waiting {
                missing: vec![format!("chain:{}", event.chain())],
            }))
        }
    };
    if chain.customer != event.dto().customer || chain.context_doc != chain.binding_set_doc {
        return Err(reject("CHAIN_CONTEXT"));
    }
    let context = document(tx, &s, &chain.context_doc, "context", hooks).await?;
    if context.body()["binding_ids"] != json!([binding.binding_id])
        || context.body()["currency"] != chain.currency
        || context.body()["scale"] != chain.scale
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let binding_doc = document(tx, &s, &binding.selector_doc, "binding", hooks).await?;
    if text(binding_doc.body(), "id")? != binding.binding_id {
        return Err(ServiceError::IntegrityFailure);
    }
    let policy = document(tx, &s, text(binding_doc.body(), "policy")?, "policy", hooks).await?;
    let roles = document(tx, &s, text(binding_doc.body(), "roles")?, "roles", hooks).await?;
    let assent = document(tx, &s, text(binding_doc.body(), "assent")?, "assent", hooks).await?;
    let input = ResolvedInput::new(
        event.clone(),
        vec![policy, roles, assent, grant, context, binding_doc],
        AcceptanceContext {
            principal_id: who.principal_id.clone(),
            grant_revision: Revision::new(authority.revision as u64).map_err(core)?,
            grant_active: authority.active,
            binding_active: binding.active,
            chain_revision: Revision::new(chain.revision as u64).map_err(core)?,
            chain_event_count: Revision::new(chain.event_count as u64).map_err(core)?,
            received_at: cmd.received_at.clone(),
        },
    )
    .map_err(core)?;
    let plan = hooks.evaluate(&input).map_err(core)?;
    if preview {
        let records = plan
            .records()
            .iter()
            .map(|r| canonical::parse(r.bytes()?.as_slice()))
            .collect::<ledgerlab_core::Result<Vec<_>>>()
            .map_err(core)?;
        // A candidate receipt is never exposed as evidence of acceptance.
        return Ok(Work::Preview(
            records
                .into_iter()
                .filter(|r| r["kind"] != "receipt")
                .collect(),
        ));
    }
    for (name, item, op) in project::writes(&plan, &event, cmd.received_at.micros()) {
        hooks.write(name, item, false)?;
        hooks
            .call(&format!("write:{name}:{item}"), tx.write(&op))
            .await
            .map_err(store_error)?;
        hooks.write(name, item, true)?;
    }
    Ok(Work::Commit(AcceptResult::Accepted {
        receipt: plan.receipt().bytes().map_err(core)?.into_vec(),
    }))
}

/// Shares normalization, locked authority checks and evaluation with acceptance.
/// No write call and no commit occurs, including the semantic-alias path.
pub(crate) async fn preview<S: AcceptanceStore>(
    store: &S,
    cmd: &AcceptCommand,
) -> Result<PreviewResult, ServiceError>
where
    S::Tx: 'static,
{
    let candidate = match domain::normalize(
        &cmd.bytes,
        cmd.principal.scope.clone(),
        &cmd.principal.source,
    ) {
        Ok(c) => c,
        Err(e) => {
            return Ok(PreviewResult::Rejected {
                code: e.code.into(),
            })
        }
    };
    let hooks = Hooks::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    for attempt in 0..5 {
        let mut tx = match store.begin(deadline).await {
            Ok(tx) => tx,
            Err(e) if e.retryable_after_rollback() && attempt < 4 && Instant::now() < deadline => {
                continue
            }
            Err(e) => return Err(store_error(e)),
        };
        let result = prepare(&mut tx, &candidate, cmd, &hooks, true).await;
        tx.rollback().await.map_err(store_error)?;
        return match result {
            Ok(Work::Preview(records)) => Ok(PreviewResult::WouldAccept { records }),
            Ok(Work::End(result) | Work::Commit(result)) => match result {
                AcceptResult::Duplicate { kind, receipt } => {
                    let body = canonical::parse(&receipt).map_err(core)?;
                    Ok(PreviewResult::Duplicate {
                        kind,
                        event_id: text(&body, "event_id")?.into(),
                    })
                }
                AcceptResult::Conflict(kind) => Ok(PreviewResult::Conflict(kind)),
                AcceptResult::Rejected { code } => Ok(PreviewResult::Rejected { code }),
                AcceptResult::Waiting { missing } => Ok(PreviewResult::Waiting { missing }),
                AcceptResult::Accepted { .. } => unreachable!("preview skips the write plan"),
            },
            Err(ServiceError::Retryable) if attempt < 4 && Instant::now() < deadline => continue,
            Err(ServiceError::Rejection(code)) => Ok(PreviewResult::Rejected { code }),
            Err(e) => Err(e),
        };
    }
    Err(ServiceError::Retryable)
}

#[cfg(test)]
mod integrity_tests {
    use super::*;
    #[test]
    fn retained_receipt_rejects_byte_hash_and_event_corruption() {
        let value: Value =
            include_str!("../../../../fixtures/journals/first-slice/accepted-records.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|v| v["kind"] == "receipt")
                .unwrap();
        let mut record = CanonicalRecord {
            canonical_bytes: serde_json::to_vec(&value["body"]).unwrap(),
            content_hash: value["content_hash"].as_str().unwrap().into(),
        };
        let event = value["body"]["event_id"].as_str().unwrap();
        assert!(retained_receipt(&record, event).is_ok());
        assert_eq!(
            retained_receipt(&record, "other-event"),
            Err(ServiceError::IntegrityFailure)
        );
        record.canonical_bytes.push(b' ');
        assert_eq!(
            retained_receipt(&record, event),
            Err(ServiceError::IntegrityFailure)
        );
        record.canonical_bytes.pop();
        record.content_hash = "sha256:untrusted".into();
        assert_eq!(
            retained_receipt(&record, event),
            Err(ServiceError::IntegrityFailure)
        );
    }
}

pub(crate) mod outcome;

pub(crate) mod adjudication;
