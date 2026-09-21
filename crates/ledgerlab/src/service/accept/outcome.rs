//! Shared outcome acceptance. Only this module can construct a validated plan.
#![allow(dead_code)] // Adapter entry points are integrated by the owning lanes.
use crate::store::outcomes::*;

#[derive(Clone, Debug)]
pub(crate) struct ValidatedOutcomePlan {
    resolve: OutcomeResolve,
    anchors: Vec<ScopedRecordRef>,
    economic: Vec<Vec<u8>>,
    settlement: Vec<Vec<u8>>,
    observed: ObservedOutcomeHeads,
    writes: Vec<OutcomeHeadWrite>,
    delivery: StoredCompositeDelivery,
}
impl ValidatedOutcomePlan {
    pub(crate) fn resolution(&self) -> &OutcomeResolve {
        &self.resolve
    }
    pub(crate) fn anchors(&self) -> &[ScopedRecordRef] {
        &self.anchors
    }
    pub(crate) fn economic_records(&self) -> &[Vec<u8>] {
        &self.economic
    }
    pub(crate) fn settlement_records(&self) -> &[Vec<u8>] {
        &self.settlement
    }
    pub(crate) fn observed_heads(&self) -> &[ObservedOutcomeHead] {
        &self.observed
    }
    pub(crate) fn head_writes(&self) -> &[OutcomeHeadWrite] {
        &self.writes
    }
    pub(crate) fn delivery(&self) -> &StoredCompositeDelivery {
        &self.delivery
    }
}

use super::{outcome_base as b, outcome_economic as economic, outcome_settlement as settlement};
use crate::{
    store::{
        errors::{CommitError, StoreError},
        ports::AcceptanceTx,
    },
    PrincipalContext, ServiceError,
};
use b::{array, bytes, check, core, hash, integrity, reference, text, Base, Records, Result};
use ledgerlab_core::{
    canonical::{self, outcome as codec},
    domain::Timestamp,
    policy::chaining::outcomes as o,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::{Duration, Instant};

/// Private host-resolved input. Ingress cannot provide permission/price flags.
#[derive(Clone, Debug)]
pub(crate) enum OutcomeOperation {
    /// Host-resolved frozen documents and proposed complete base bridge. The
    /// coordinator replays the original inputs and verifies every projection.
    FinalBase { seed: Vec<Vec<u8>> },
    Economic {
        ingress: Vec<u8>,
        evidence: Vec<Vec<u8>>,
    },
    Close {
        source: String,
        external_id: String,
        expected_revision: String,
        reason: String,
    },
}
#[derive(Clone, Debug)]
pub(crate) struct OutcomeCommand {
    pub principal: PrincipalContext,
    pub target: String,
    pub invocation_id: String,
    pub operation: OutcomeOperation,
    pub received_at: Timestamp,
    pub accepted_at: Timestamp,
    /// Host-selected immutable authority/authentication documents, resolved
    /// under the authority lock; never accepted as caller proof of authority.
    pub required: Vec<ScopedRecordRef>,
}
/// Mandatory trusted-host verification boundary, called while all scopes remain
/// locked. Implementations verify actual scoped authentication, grant permissions,
/// evidence, assent/offer/delegation and separate early-closure rights. Merely
/// having a document or a syntactically valid reference must never return a proof.
/// There is deliberately no permissive production implementation or default.
pub(crate) trait OutcomeAuthority: Send + Sync {
    fn verify(
        &self,
        command: &OutcomeCommand,
        snapshot: &OutcomeSnapshot,
        write: bool,
    ) -> Result<AuthorityProof>;
}
#[derive(Clone, Debug)]
pub(crate) struct AuthorityProof {
    pub scope: [String; 2],
    pub target: String,
    pub invocation_id: String,
    pub source: String,
    /// Frozen reservation authority shape; permissions derive from verification.
    pub authority: Value,
    pub authentication: Value,
    pub verified_terms: Vec<String>,
    pub finality: bool,
    pub authorized_early_close: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OutcomeResult {
    Accepted(StoredCompositeDelivery),
    Duplicate(StoredCompositeDelivery),
    IdentityConflict,
    SemanticConflict,
    Waiting(Vec<ScopedRecordRef>),
}
fn scoped(command: &OutcomeCommand) -> [String; 2] {
    [
        command.principal.scope.tenant().into(),
        command.principal.scope.environment().into(),
    ]
}
fn lock(
    c: &OutcomeCommand,
    class: OutcomeLockClass,
    parts: Vec<Value>,
    mode: OutcomeLockMode,
) -> Result<OutcomeLock> {
    let mut key = vec![json!(scoped(c))];
    key.extend(parts);
    Ok(OutcomeLock {
        class,
        key: bytes(&json!(key))?,
        mode,
    })
}
fn baseline_locks(c: &OutcomeCommand) -> Result<Vec<OutcomeLock>> {
    use OutcomeLockClass as L;
    use OutcomeLockMode as M;
    Ok(vec![
        lock(c, L::Admission, vec![], M::Read)?,
        lock(
            c,
            L::Authority,
            vec![json!(c.principal.authority_head)],
            M::Read,
        )?,
        lock(c, L::Reservation, vec![json!(c.invocation_id)], M::Write)?,
        lock(c, L::Target, vec![json!(c.target)], M::Write)?,
        lock(
            c,
            L::InvocationConsumption,
            vec![json!(c.invocation_id)],
            M::Write,
        )?,
        lock(c, L::BaseReversal, vec![json!(c.target)], M::Write)?,
    ])
}
fn normalize_locks(locks: Vec<OutcomeLock>) -> Result<Vec<OutcomeLock>> {
    let mut map: BTreeMap<(OutcomeLockClass, Vec<u8>), OutcomeLockMode> = BTreeMap::new();
    for l in locks {
        let v = core(canonical::parse(&l.key))?;
        check(bytes(&v)? == l.key && !array(&v)?.is_empty())?;
        map.entry((l.class, l.key))
            .and_modify(|m| *m = (*m).max(l.mode))
            .or_insert(l.mode);
    }
    Ok(map
        .into_iter()
        .map(|((class, key), mode)| OutcomeLock { class, key, mode })
        .collect())
}
fn full_locks(c: &OutcomeCommand, base: &Base, records: &Records) -> Result<Vec<OutcomeLock>> {
    use OutcomeLockClass as L;
    use OutcomeLockMode as M;
    let mut locks = baseline_locks(c)?;
    for r in records
        .rows
        .iter()
        .filter(|r| r["kind"] == "binding-snapshot")
    {
        locks.push(lock(
            c,
            L::Binding,
            vec![r["body"]["binding_id"].clone()],
            M::Read,
        )?);
        locks.push(lock(
            c,
            L::BindingAggregate,
            vec![json!(c.target), r["body"]["binding_id"].clone()],
            M::Write,
        )?);
    }
    for f in base.target.policy().families.iter() {
        let binding = base
            .evaluation
            .bundle()
            .policies()
            .iter()
            .find(|p| p.binding.id == f.binding_id)
            .ok_or_else(integrity)?;
        locks.push(lock(
            c,
            L::Claim,
            vec![
                json!(binding.binding.agreement),
                json!(f.family),
                json!(c.target),
            ],
            M::Write,
        )?);
    }
    normalize_locks(locks)
}
fn head<'a>(s: &'a OutcomeSnapshot, l: &OutcomeLock) -> Result<&'a ObservedOutcomeHead> {
    s.heads
        .iter()
        .find(|h| h.lock.class == l.class && h.lock.key == l.key)
        .ok_or_else(integrity)
}
fn head_value(h: &ObservedOutcomeHead) -> Result<Option<Value>> {
    check(h.revision.is_some() == h.value.is_some())?;
    if let Some(rev) = &h.revision {
        core(ledgerlab_core::domain::Revision::parse(rev))?;
    }
    h.value
        .as_ref()
        .map(|v| {
            let p = core(canonical::parse_bounded(v, canonical::BUNDLE_LIMIT))?;
            check(bytes(&p)? == *v)?;
            Ok(p)
        })
        .transpose()
}
fn current(
    s: &OutcomeSnapshot,
    c: &OutcomeCommand,
    class: OutcomeLockClass,
    parts: Vec<Value>,
) -> Result<Option<Value>> {
    head_value(head(s, &lock(c, class, parts, OutcomeLockMode::Read)?)?)
}
fn new_write(s: &OutcomeSnapshot, l: OutcomeLock, value: Value) -> Result<OutcomeHeadWrite> {
    let old = head(s, &l)?;
    let n = old
        .revision
        .as_ref()
        .map(|v| v.parse::<u64>())
        .transpose()
        .map_err(|_| integrity())?
        .map_or(Some(0), |n| n.checked_add(1))
        .ok_or_else(integrity)?;
    check(n <= i64::MAX as u64)?;
    Ok(OutcomeHeadWrite {
        lock: l,
        revision: n.to_string(),
        value: bytes(&value)?,
    })
}
fn ref_value(r: &ScopedRecordRef) -> Result<Value> {
    Ok(json!({"kind":r.kind,"id":core(canonical::parse(&r.id))?,"content_hash":r.content_hash}))
}
fn scoped_ref(scope: [String; 2], r: &Value) -> Result<ScopedRecordRef> {
    Ok(ScopedRecordRef {
        scope,
        kind: text(&r["kind"])?.into(),
        id: bytes(&r["id"])?,
        content_hash: text(&r["content_hash"])?.into(),
    })
}
fn operation(c: &OutcomeCommand) -> Result<(Value, Value, ScopedDelivery)> {
    let scope = json!(scoped(c));
    let (event, command) = match &c.operation {
        OutcomeOperation::FinalBase { seed } => {
            let records = Records::new(seed, scope.clone())?;
            let e = records.one("event")?.clone();
            let base = records.one("base-evaluation")?;
            (
                e["body"].clone(),
                json!({"kind":"register","source":e["body"]["data"]["source"],"external_id":e["body"]["data"]["external_id"],"invocation_id":c.invocation_id,"economic_ingress_hash":base["body"]["original_ingress_hash"]}),
            )
        }
        OutcomeOperation::Economic { ingress, .. } => {
            let v = core(canonical::parse(ingress))?;
            check(bytes(&v)? == *ingress)?;
            let e = b::row("event", &scope, v)?;
            core(codec::decode(&bytes(&e)?))?;
            let d = &e["body"]["data"];
            check(d["type"] == "outcome" || d["type"] == "correction")?;
            (
                e["body"].clone(),
                json!({"kind":if d["type"]=="outcome"{"ordinary"}else{"post_hoc"},"source":d["source"],"external_id":d["external_id"],"invocation_id":c.invocation_id,"economic_ingress_hash":hash("ingress",&e["body"])?,"family":{"agreement_id":d["agreement_id"],"family_id":d["family_id"],"target":d["target"]}}),
            )
        }
        OutcomeOperation::Close {
            source,
            external_id,
            expected_revision,
            reason,
        } => {
            let v = json!({"kind":"close","source":source,"external_id":external_id,"invocation_id":c.invocation_id,"expected_revision":expected_revision,"reason":reason});
            (v.clone(), v)
        }
    };
    let key = ScopedDelivery {
        scope: scoped(c),
        source: text(&command["source"])?.into(),
        external_id: text(&command["external_id"])?.into(),
    };
    check(key.source == c.principal.source)?;
    Ok((event, command, key))
}
fn proof_records(records: &Records, c: &OutcomeCommand) -> Result<Records> {
    let mut output = Records::new(
        &records.rows.iter().map(bytes).collect::<Result<Vec<_>>>()?,
        records.scope.clone(),
    )?;
    if let OutcomeOperation::Economic { evidence, .. } = &c.operation {
        for raw in evidence {
            let r = core(codec::decode(raw))?;
            check(r["kind"] == "evidence")?;
            if let Ok(old) = output.get(&r["id"], "evidence") {
                check(*old == r)?;
            } else {
                output.insert(r)?;
            }
        }
    }
    output.documents()?;
    Ok(output)
}
fn verify_proof(
    c: &OutcomeCommand,
    s: &OutcomeSnapshot,
    p: &AuthorityProof,
    records: &Records,
    source: &str,
    write: bool,
    kind: &str,
) -> Result<()> {
    check(
        p.scope == scoped(c)
            && p.target == c.target
            && p.invocation_id == c.invocation_id
            && p.source == source,
    )?;
    let supplied = proof_records(records, c)?;
    let records = &supplied;
    let a = &p.authority;
    check(
        text(&a["grant_revision"])?
            .parse::<u64>()
            .map_err(|_| integrity())?
            > 0,
    )?;
    check(a["principal"] == c.principal.principal_id && a["active"] == true)?;
    let h = head(
        s,
        &lock(
            c,
            OutcomeLockClass::Authority,
            vec![json!(c.principal.authority_head)],
            OutcomeLockMode::Read,
        )?,
    )?;
    let hv = head_value(h)?.ok_or_else(integrity)?;
    check(
        hv["active"] == true
            && hv["grant"] == a["grant"]
            && h.revision.as_deref() == a["grant_revision"].as_str(),
    )?;
    records.deref(&a["grant"])?;
    records.deref(&p.authentication)?;
    for r in array(&a["evidence"])? {
        records.deref(r)?;
    }
    let perms = array(&a["permissions"])?;
    check(perms.contains(&json!("read")) && c.principal.can_read)?;
    if write {
        let permission = match kind {
            "register" | "ordinary" => "submit",
            "post_hoc" => "correct",
            "close" => "close",
            _ => return Err(integrity()),
        };
        check(
            perms.contains(&json!(permission))
                && (permission != "submit" || c.principal.can_submit),
        )?;
    }
    Ok(())
}
fn classified(e: StoreError) -> ServiceError {
    crate::service::store_error(e)
}
fn unknown(k: &ScopedDelivery) -> ServiceError {
    ServiceError::OutcomeUnknown {
        scope: k.scope.clone(),
        source: k.source.clone(),
        external_id: k.external_id.clone(),
    }
}

pub(crate) async fn run<S: OutcomeStore, A: OutcomeAuthority>(
    store: &S,
    c: &OutcomeCommand,
    authority: &A,
) -> Result<OutcomeResult> {
    let (event, command, key) = operation(c)?;
    let mut locks = normalize_locks(baseline_locks(c)?)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    for attempt in 0..5 {
        if Instant::now() >= deadline {
            return Err(ServiceError::Retryable);
        }
        let mut tx = match store.begin_outcome(deadline).await {
            Ok(tx) => tx,
            Err(e) if e.retryable_after_rollback() && attempt < 4 => continue,
            Err(e) => return Err(classified(e)),
        };
        let result = prepare(&mut tx, c, authority, &event, &command, &key, &locks).await;
        match result {
            Ok(Prepared::More(next)) => {
                tx.rollback().await.map_err(classified)?;
                let all = normalize_locks([locks.clone(), next].concat())?;
                check(all != locks)?;
                locks = all;
            }
            Ok(Prepared::End(v)) => {
                tx.rollback().await.map_err(classified)?;
                return Ok(v);
            }
            Ok(Prepared::Append(plan, duplicate)) => {
                match tx.append_outcome(&plan).await {
                    Err(StoreError::DeliveryConflict) => {
                        tx.rollback().await.map_err(classified)?;
                        return Ok(OutcomeResult::IdentityConflict);
                    }
                    Err(e) => {
                        let retry = e.retryable_after_rollback();
                        tx.rollback().await.map_err(classified)?;
                        if retry && attempt < 4 {
                            continue;
                        }
                        return Err(classified(e));
                    }
                    Ok(()) => {}
                }
                let delivery = plan.delivery.clone();
                match tx.commit().await {
                    Ok(()) => {
                        return Ok(if duplicate {
                            OutcomeResult::Duplicate(delivery)
                        } else {
                            OutcomeResult::Accepted(delivery)
                        })
                    }
                    Err(CommitError::OutcomeUnknown) => return Err(unknown(&key)),
                    Err(CommitError::RolledBack(e))
                        if e.retryable_after_rollback() && attempt < 4 =>
                    {
                        continue
                    }
                    Err(CommitError::RolledBack(e)) => return Err(classified(e)),
                }
            }
            Err(e) => {
                tx.rollback().await.map_err(classified)?;
                if e == ServiceError::Retryable && attempt < 4 {
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(ServiceError::Retryable)
}
#[allow(clippy::large_enum_variant)] // Short-lived private transaction step owns its complete plan.
enum Prepared {
    More(Vec<OutcomeLock>),
    End(OutcomeResult),
    Append(ValidatedOutcomePlan, bool),
}
async fn prepare<T: OutcomeTx, A: OutcomeAuthority>(
    tx: &mut T,
    c: &OutcomeCommand,
    authority: &A,
    event: &Value,
    command: &Value,
    key: &ScopedDelivery,
    locks: &[OutcomeLock],
) -> Result<Prepared> {
    tx.lock_scopes(locks).await.map_err(classified)?;
    let installation = tx.load_installation().await.map_err(classified)?;
    check(
        installation.scope.tenant == key.scope[0] && installation.scope.environment == key.scope[1],
    )?;
    if installation.admission != "open" {
        return Err(ServiceError::Unavailable);
    }
    let resolve = OutcomeResolve {
        delivery: key.clone(),
        target: c.target.clone(),
        invocation_id: c.invocation_id.clone(),
        family_key: command
            .get("family")
            .map(|f| {
                bytes(&json!([
                    scoped(c),
                    f["agreement_id"],
                    f["family_id"],
                    f["target"]
                ]))
            })
            .transpose()?,
        required: c.required.clone(),
        locks: locks.to_vec(),
    };
    let snapshot = match tx.resolve_outcome(&resolve).await.map_err(classified)? {
        OutcomeResolution::MoreLocks(v) => return Ok(Prepared::More(v)),
        OutcomeResolution::Missing(v) => return Ok(Prepared::End(OutcomeResult::Waiting(v))),
        OutcomeResolution::Complete(s) => s,
    };
    validate_snapshot_heads(&snapshot, locks, c)?;
    let records = Records::new(&snapshot.records, json!(key.scope))?;
    records.documents()?;
    let proof = authority.verify(c, &snapshot, false)?;
    verify_proof(
        c,
        &snapshot,
        &proof,
        &records,
        &key.source,
        false,
        text(&command["kind"])?,
    )?;
    if current(
        &snapshot,
        c,
        OutcomeLockClass::Target,
        vec![json!(c.target)],
    )?
    .is_some()
    {
        let retained = historical_records(&snapshot, c, &records)?;
        let econ = economic_only(&retained)?;
        let target = current(
            &snapshot,
            c,
            OutcomeLockClass::Target,
            vec![json!(c.target)],
        )?
        .ok_or_else(integrity)?;
        let base = b::decode_base(&econ, &target["base"])?;
        let needed = full_locks(c, &base, &econ)?;
        if needed.iter().any(|l| {
            !locks
                .iter()
                .any(|held| held.class == l.class && held.key == l.key && held.mode >= l.mode)
        }) {
            return Ok(Prepared::More(needed));
        }
        validate_registered(&snapshot, c, &records)?;
    }
    let stored = match tx.lookup_outcome_delivery(key).await {
        Ok(v) => v,
        Err(StoreError::DeliveryConflict) => {
            return Ok(Prepared::End(OutcomeResult::IdentityConflict))
        }
        Err(e) => return Err(classified(e)),
    };
    if let Some(stored) = stored {
        check(stored.key == *key)?;
        validate_delivery(&stored, &records)?;
        let ingress = ingress(c, event)?;
        return Ok(Prepared::End(
            if stored.command == bytes(command)?
                && stored.ingress == ingress
                && stored.ingress_hash == ingress_hash(c, event, command)?
            {
                OutcomeResult::Duplicate(stored)
            } else {
                OutcomeResult::IdentityConflict
            },
        ));
    }
    let (mut economic_records, base) = if let OutcomeOperation::FinalBase { seed } = &c.operation {
        let seed_records = Records::new(seed, json!(key.scope))?;
        let base = b::decode_base(
            &seed_records,
            &reference(seed_records.one("base-acceptance")?),
        )?;
        check(base.snapshot["body"]["target"] == c.target)?;
        (seed_records, base)
    } else {
        let target = current(
            &snapshot,
            c,
            OutcomeLockClass::Target,
            vec![json!(c.target)],
        )?
        .ok_or_else(integrity)?;
        let base = b::decode_base(&records, &target["base"])?;
        (economic_only(&records)?, base)
    };
    let needed = full_locks(c, &base, &economic_records)?;
    if needed.iter().any(|l| {
        !locks
            .iter()
            .any(|held| held.class == l.class && held.key == l.key && held.mode >= l.mode)
    }) {
        return Ok(Prepared::More(needed));
    }
    let proof = authority.verify(c, &snapshot, false)?;
    // Semantic duplicates are evaluated with read rights before write permission.
    let prepared = build(
        c,
        event,
        command,
        key,
        &snapshot,
        &resolve,
        &mut economic_records,
        &base,
        &records,
        &proof,
    )?;
    Ok(prepared)
}
fn ingress(c: &OutcomeCommand, event: &Value) -> Result<Vec<u8>> {
    if let OutcomeOperation::FinalBase { seed } = &c.operation {
        let r = Records::new(seed, json!(scoped(c)))?;
        Ok(
            text(&r.one("base-evaluation")?["body"]["original_ingress_utf8"])?
                .as_bytes()
                .to_vec(),
        )
    } else {
        bytes(event)
    }
}
fn ingress_hash(c: &OutcomeCommand, event: &Value, command: &Value) -> Result<String> {
    Ok(if matches!(c.operation, OutcomeOperation::Close { .. }) {
        settlement::settlement_hash("request", command)?
    } else {
        let _ = event;
        text(&command["economic_ingress_hash"])?.into()
    })
}
fn economic_only(records: &Records) -> Result<Records> {
    Records::new(
        &records
            .rows
            .iter()
            .filter(|r| !text(&r["kind"]).unwrap_or("").starts_with("reservation-"))
            .map(bytes)
            .collect::<Result<Vec<_>>>()?,
        records.scope.clone(),
    )
}
fn validate_snapshot_heads(
    snapshot: &OutcomeSnapshot,
    locks: &[OutcomeLock],
    c: &OutcomeCommand,
) -> Result<()> {
    check(snapshot.heads.len() == locks.len())?;
    let mut seen = BTreeSet::new();
    for h in &snapshot.heads {
        check(seen.insert((h.lock.class, h.lock.key.clone())) && locks.contains(&h.lock))?;
        head_value(h)?;
        let v = core(canonical::parse(&h.lock.key))?;
        check(v[0] == json!(scoped(c)))?;
    }
    Ok(())
}
fn validate_delivery(d: &StoredCompositeDelivery, records: &Records) -> Result<()> {
    let settle = core(codec::decode(&d.settlement_receipt))?;
    check(
        settle["kind"] == "reservation-receipt"
            && settle["scope"] == json!(d.key.scope)
            && d.canonical_key.scope == d.key.scope
            && settle["body"]["source"] == d.canonical_key.source
            && settle["body"]["external_id"] == d.canonical_key.external_id,
    )?;
    records.deref(&reference(&settle))?;
    let command = core(canonical::parse(&d.command))?;
    let ingress = core(canonical::parse(&d.ingress))?;
    check(bytes(&command)? == d.command && bytes(&ingress)? == d.ingress)?;
    let observation = records.deref(&settle["body"]["observation"])?;
    let original = &observation["body"]["command"];
    check(
        command["source"] == d.key.source
            && command["external_id"] == d.key.external_id
            && command["invocation_id"] == original["invocation_id"]
            && command["kind"] == original["kind"],
    )?;
    if d.key == d.canonical_key {
        check(command == *original)?;
    } else {
        // Semantic aliases exist only for ordinary claims. Their wrapper may
        // differ, but they retain the original source, invocation and family.
        check(
            command["kind"] == "ordinary"
                && d.key.source == d.canonical_key.source
                && command["family"] == original["family"],
        )?;
    }
    match text(&command["kind"])? {
        "close" => check(
            ingress == command
                && d.ingress_hash == settlement::settlement_hash("request", &command)?,
        )?,
        "register" => {
            let base = records.one("base-evaluation")?;
            check(
                d.ingress == text(&base["body"]["original_ingress_utf8"])?.as_bytes()
                    && command["economic_ingress_hash"] == base["body"]["original_ingress_hash"]
                    && json!(d.ingress_hash) == command["economic_ingress_hash"],
            )?;
        }
        "ordinary" | "post_hoc" => {
            check(
                hash("ingress", &ingress)? == d.ingress_hash
                    && json!(d.ingress_hash) == command["economic_ingress_hash"]
                    && ingress["data"]["source"] == d.key.source
                    && ingress["data"]["external_id"] == d.key.external_id,
            )?;
        }
        _ => return Err(integrity()),
    }
    match &d.economic_receipt {
        Some(raw) => {
            let e = core(codec::decode(raw))?;
            check(settle["body"]["economic_receipt"] == reference(&e))?;
            records.deref(&reference(&e))?;
            if d.key == d.canonical_key && e["kind"] == "receipt" {
                check(records.get(&e["body"]["event_id"], "event")?["body"] == ingress)?;
            }
        }
        None => check(settle["body"].get("economic_receipt").is_none())?,
    }
    Ok(())
}

fn decision_groups(records: &Records, base: &Base) -> Result<Vec<Vec<Value>>> {
    let mut prior: BTreeSet<Vec<u8>> = array(&base.acceptance["body"]["members"])?
        .iter()
        .map(|r| bytes(&r["id"]))
        .collect::<Result<_>>()?;
    prior.insert(bytes(&base.acceptance["id"])?);
    let mut receipts = records
        .rows
        .iter()
        .filter(|r| r["kind"] == "receipt")
        .collect::<Vec<_>>();
    receipts.sort_by_key(|r| {
        r["body"]["revision"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    });
    let mut result = vec![];
    for receipt in receipts {
        let manifest = records.get(&receipt["body"]["decision_id"], "decision-manifest")?;
        let mut rows = vec![];
        for r in array(&manifest["body"]["members"])? {
            if !prior.contains(&bytes(&r["id"])?) {
                rows.push(records.deref(r)?.clone());
            }
        }
        rows.extend([manifest.clone(), receipt.clone()]);
        rows.sort_by(|a, b| {
            (text(&a["kind"]).unwrap(), bytes(&a["id"]).unwrap())
                .cmp(&(text(&b["kind"]).unwrap(), bytes(&b["id"]).unwrap()))
        });
        for r in &rows {
            check(prior.insert(bytes(&r["id"])?))?;
        }
        result.push(rows);
    }
    check(prior.len() == records.rows.len())?;
    Ok(result)
}
fn historical_records(
    snapshot: &OutcomeSnapshot,
    c: &OutcomeCommand,
    all: &Records,
) -> Result<Records> {
    let t = current(snapshot, c, OutcomeLockClass::Target, vec![json!(c.target)])?
        .ok_or_else(integrity)?;
    let refs = array(&t["records"])?;
    check(b::ordered(refs.clone())? == *refs)?;
    let raw = refs
        .iter()
        .map(|r| all.deref(r).and_then(bytes))
        .collect::<Result<Vec<_>>>()?;
    let required = c
        .required
        .iter()
        .map(|r| {
            check(r.scope == scoped(c))?;
            ref_value(r)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut expected = refs.clone();
    for r in required {
        if !expected.contains(&r) {
            expected.push(r);
        }
    }
    check(expected.len() == all.rows.len())?;
    for r in &expected {
        all.deref(r)?;
    }
    let roots = vec![t["base"].clone(), t["registration"].clone()];
    check(snapshot.anchors.len() == 2 && snapshot.anchors.iter().all(|r| r.scope == scoped(c)))?;
    check(
        b::ordered(
            snapshot
                .anchors
                .iter()
                .map(ref_value)
                .collect::<Result<_>>()?,
        )? == b::ordered(roots)?,
    )?;
    Records::new(&raw, all.scope.clone())
}
fn validate_registered(
    snapshot: &OutcomeSnapshot,
    c: &OutcomeCommand,
    all: &Records,
) -> Result<(Records, Base, Vec<o::Decision>, Vec<Value>)> {
    let records = historical_records(snapshot, c, all)?;
    let economic = economic_only(&records)?;
    let target = current(snapshot, c, OutcomeLockClass::Target, vec![json!(c.target)])?
        .ok_or_else(integrity)?;
    let base = b::decode_base(&economic, &target["base"])?;
    check(base.snapshot["body"]["target"] == c.target)?;
    let groups = decision_groups(&economic, &base)?;
    let decisions = economic::replay(&economic, &base, &groups)?;
    let mut prefix = vec![];
    let mut econ_prefix = array(&base.acceptance["body"]["members"])?
        .iter()
        .map(|r| economic.deref(r).cloned())
        .collect::<Result<Vec<_>>>()?;
    econ_prefix.push(base.acceptance.clone());
    let mut used = BTreeSet::new();
    let mut economic_used = BTreeSet::new();
    loop {
        let next = records
            .rows
            .iter()
            .filter(|r| r["kind"] == "reservation-observation")
            .filter(|r| {
                if prefix.is_empty() {
                    r["body"].get("previous").is_none()
                } else {
                    r["body"]["previous"] == reference(prefix.last().unwrap())
                }
            })
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        check(next.len() == 1)?;
        let obs = next[0];
        check(used.insert(bytes(&obs["id"])?))?;
        let ob = &obs["body"];
        let command = &ob["command"];
        let receipt = records
            .rows
            .iter()
            .find(|r| {
                r["kind"] == "reservation-receipt" && r["body"]["observation"] == reference(obs)
            })
            .ok_or_else(integrity)?;
        let econ = ob
            .get("economic_receipt")
            .map(|r| economic.deref(r))
            .transpose()?;
        let mut amount = None;
        if let Some(e) = econ {
            check(economic_used.insert(bytes(&e["id"])?))?;
            if e["kind"] == "receipt" {
                let group = groups
                    .iter()
                    .find(|g| g.iter().any(|r| r["id"] == e["id"]))
                    .ok_or_else(integrity)?;
                let au = &group
                    .iter()
                    .find(|r| r["kind"] == "authority-decision")
                    .ok_or_else(integrity)?["body"];
                for key in ["principal", "grant_revision", "active"] {
                    check(ob["authority"][key] == au[key])?
                }
                check(
                    ob["authority"]["grant"] == reference(economic.get(&au["grant"], "evidence")?)
                        && ob["received_at"] == au["received_at"]
                        && ob["accepted_at"] == au["accepted_at"],
                )?;
                let event = group
                    .iter()
                    .find(|r| r["kind"] == "event")
                    .ok_or_else(integrity)?;
                let data = &event["body"]["data"];
                check(
                    command["economic_ingress_hash"] == hash("ingress", &event["body"])?
                        && command["source"] == data["source"]
                        && command["external_id"] == data["external_id"]
                        && command["family"]
                            == json!({"agreement_id":data["agreement_id"],"family_id":data["family_id"],"target":data["target"]}),
                )?;
                check((command["kind"] == "post_hoc") == (data["type"] == "correction"))?;
                let rev = group
                    .iter()
                    .find(|r| r["kind"] == "claim-revision")
                    .ok_or_else(integrity)?;
                amount = Some(b::money(&rev["body"]["amount"])?.atoms());
                econ_prefix.extend(group.clone());
            } else {
                check(
                    *e == base.acceptance
                        && command["kind"] == "register"
                        && command["economic_ingress_hash"]
                            == base.evaluation.event().candidate().ingress_hash()
                        && command["source"] == base.evaluation.event().source()
                        && command["external_id"]
                            == base.evaluation.event().candidate().external_id(),
                )?;
            }
        }
        let projected = settlement::project(
            &economic,
            &base,
            settlement::SettlementInput {
                command: command.clone(),
                authority: ob["authority"].clone(),
                received: ob["received_at"].clone(),
                accepted: ob["accepted_at"].clone(),
                economic: econ,
                economic_prefix: &econ_prefix,
                prior: &prefix,
                amount,
                authorized_close: command["reason"] == "authorized",
            },
        )?;
        for r in &projected {
            check(*records.deref(&reference(r))? == *r)?;
        }
        check(projected.last() == Some(receipt))?;
        prefix.extend(projected);
    }
    check(
        prefix.len()
            == records
                .rows
                .iter()
                .filter(|r| text(&r["kind"]).unwrap().starts_with("reservation-"))
                .count()
            && economic_used.len() == groups.len() + 1,
    )?;
    check(!prefix.is_empty() && target["registration"] == reference(&prefix[1]))?;
    let steps = prefix
        .iter()
        .filter(|r| r["kind"] == "reservation-receipt")
        .count();
    check(
        head(
            snapshot,
            &lock(
                c,
                OutcomeLockClass::Target,
                vec![json!(c.target)],
                OutcomeLockMode::Read,
            )?,
        )?
        .revision
        .as_deref()
            == Some((steps - 1).to_string().as_str()),
    )?;
    let last = prefix.last().ok_or_else(integrity)?;
    let reservation = head(
        snapshot,
        &lock(
            c,
            OutcomeLockClass::Reservation,
            vec![json!(c.invocation_id)],
            OutcomeLockMode::Read,
        )?,
    )?;
    check(
        head_value(reservation)? == Some(last["body"]["result"].clone())
            && reservation.revision.as_deref() == last["body"]["result"]["revision"].as_str(),
    )?;
    validate_economic_heads(snapshot, c, &economic, &base)?;
    Ok((economic, base, decisions, prefix))
}
fn validate_economic_heads(
    s: &OutcomeSnapshot,
    c: &OutcomeCommand,
    r: &Records,
    base: &Base,
) -> Result<()> {
    use OutcomeLockClass as L;
    check(
        current(s, c, L::BaseReversal, vec![json!(c.target)])? == Some(json!({"reversed":false})),
    )?;
    check(
        current(s, c, L::InvocationConsumption, vec![json!(c.invocation_id)])?
            == Some(
                json!({"target":c.target,"registration":current(s,c,L::Target,vec![json!(c.target)])?.ok_or_else(integrity)?["registration"]}),
            ),
    )?;
    for binding in r.rows.iter().filter(|r| r["kind"] == "binding-snapshot") {
        // Current selectors cannot reprice or hide a retained receipt. Their
        // exact locked bytes remain observed guards; the host verifier checks
        // current write rights against the original frozen binding.
        let mut latest: BTreeMap<String, &Value> = BTreeMap::new();
        for rv in r.rows.iter().filter(|rv| {
            rv["kind"] == "claim-revision"
                && rv["body"]["binding_id"] == binding["body"]["binding_id"]
        }) {
            let id = text(&rv["body"]["claim_id"])?;
            if latest.get(id).is_none_or(|old| {
                text(&old["body"]["number"])
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    < text(&rv["body"]["number"]).unwrap().parse::<u64>().unwrap()
            }) {
                latest.insert(id.into(), rv);
            }
        }
        let expected =
            json!({"revisions":b::ordered(latest.values().map(|r|reference(r)).collect())?});
        check(
            current(
                s,
                c,
                L::BindingAggregate,
                vec![json!(c.target), binding["body"]["binding_id"].clone()],
            )? == Some(expected),
        )?;
    }
    for f in &base.target.policy().families {
        let binding = base
            .evaluation
            .bundle()
            .policies()
            .iter()
            .find(|p| p.binding.id == f.binding_id)
            .ok_or_else(integrity)?;
        let rv = r
            .rows
            .iter()
            .filter(|r| r["kind"] == "claim-revision")
            .filter(|rv| {
                r.get(&rv["body"]["claim_id"], "claim").is_ok_and(|cl| {
                    cl["body"]["family_id"] == f.family
                        && cl["body"]["agreement_id"] == binding.binding.agreement
                })
            })
            .max_by_key(|r| text(&r["body"]["number"]).unwrap().parse::<u64>().unwrap());
        let expected=rv.map(|rv|->Result<Value>{Ok(json!({"revision":reference(rv),"original_receipt":reference(r.get(&rv["body"]["original_receipt"],"receipt")?)}))}).transpose()?;
        check(
            current(
                s,
                c,
                L::Claim,
                vec![
                    json!(binding.binding.agreement),
                    json!(f.family),
                    json!(c.target),
                ],
            )? == expected,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build(
    c: &OutcomeCommand,
    event: &Value,
    command: &Value,
    key: &ScopedDelivery,
    snapshot: &OutcomeSnapshot,
    resolve: &OutcomeResolve,
    new_records: &mut Records,
    base_proposal: &Base,
    all: &Records,
    proof: &AuthorityProof,
) -> Result<Prepared> {
    use OutcomeLockClass as L;
    use OutcomeLockMode as M;
    let registering = matches!(c.operation, OutcomeOperation::FinalBase { .. });
    let kind = text(&command["kind"])?;
    let mut history = vec![];
    let mut prior = vec![];
    let mut owned = None;
    if !registering {
        let (r, b, h, p) = validate_registered(snapshot, c, all)?;
        owned = Some((r, b));
        history = h;
        prior = p;
    }
    let (records, base) = if let Some((r, b)) = &owned {
        (r, b)
    } else {
        (&*new_records, base_proposal)
    };
    if registering {
        for observed in &snapshot.heads {
            if matches!(observed.lock.class, L::Claim | L::BindingAggregate) {
                check(observed.revision.is_none() && observed.value.is_none())?;
            }
        }
        check(
            snapshot.anchors.is_empty()
                && current(snapshot, c, L::Target, vec![json!(c.target)])?.is_none()
                && current(snapshot, c, L::Reservation, vec![json!(c.invocation_id)])?.is_none()
                && current(
                    snapshot,
                    c,
                    L::InvocationConsumption,
                    vec![json!(c.invocation_id)],
                )?
                .is_none()
                && current(snapshot, c, L::BaseReversal, vec![json!(c.target)])?.is_none(),
        )?;
        check(
            proof.finality
                && base
                    .target
                    .verification()
                    .verified_assents
                    .iter()
                    .chain(&base.target.verification().verified_offers)
                    .chain(&base.target.verification().verified_delegations)
                    .all(|d| proof.verified_terms.contains(d)),
        )?;
    }
    if registering {
        let original = base.evaluation.source_authority();
        check(
            original.source == key.source
                && original.grant == all.document(&proof.authority["grant"]["id"])?
                && serde_json::to_value(original.revision).map_err(|_| integrity())?
                    == proof.authority["grant_revision"]
                && base.evaluation.received_at() == Some(&c.received_at)
                && base.target.verification().accepted_at == c.accepted_at,
        )?;
        let (_, anchor, _) = settlement::checkpoint(records, base, &c.invocation_id)?;
        for required in [
            base.target.policy().document.clone(),
            records.document(&base.snapshot["body"]["finality_evidence"])?,
            records.document(&anchor["invocation_authorization"]["id"])?,
        ] {
            check(proof.verified_terms.contains(&required))?;
        }
    }
    let mut active = Records::new(
        &records.rows.iter().map(bytes).collect::<Result<Vec<_>>>()?,
        records.scope.clone(),
    )?;
    let mut evidence = vec![];
    if let OutcomeOperation::Economic { evidence: raw, .. } = &c.operation {
        for raw in raw {
            let r = core(codec::decode(raw))?;
            check(
                r["kind"] == "evidence" && array(&event["data"]["evidence"])?.contains(&r["id"]),
            )?;
            active.insert(r.clone())?;
            evidence.push(r);
        }
    }
    active.documents()?;
    let mut econ = if registering {
        active.rows.clone()
    } else {
        vec![]
    };
    let mut amount = None;
    if matches!(c.operation, OutcomeOperation::Economic { .. }) {
        let req = b::request(&active, base, &event["data"])?;
        // Verified rights come exclusively from the mandatory host proof tied to a
        // locked current grant. Ingress flags never enter this value.
        verify_proof(c, snapshot, proof, all, &key.source, false, kind)?;
        let perms = array(&proof.authority["permissions"])?;
        let authority = json!({"event_id":core(codec::key(codec::ECONOMIC,"event",&active.scope,event))?,"target":c.target,"agreement_id":event["data"]["agreement_id"],"family_id":event["data"]["family_id"],"source":key.source,"principal":c.principal.principal_id,"grant":proof.authority["grant"]["id"],"grant_revision":proof.authority["grant_revision"],"active":true,"may_read":true,"may_submit":perms.contains(&json!("submit")),"may_correct":perms.contains(&json!("correct")),"verified_evidence":event["data"]["evidence"],"received_at":c.received_at,"accepted_at":c.accepted_at});
        for e in array(&event["data"]["evidence"])? {
            let doc = active.document(e)?;
            check(array(&proof.authority["evidence"])?.iter().any(|r| {
                active
                    .deref(r)
                    .is_ok_and(|r| r["body"]["document_id"] == doc)
            }))?;
        }
        let verified = b::verified(&active, &req, &authority)?;
        let evaluated = match o::evaluate(
            &req,
            &verified,
            std::slice::from_ref(&base.target),
            std::slice::from_ref(&base.evaluation),
            &history,
        ) {
            Ok(v) => v,
            Err(e) if e.code == "CLAIM_CONFLICT" => {
                return Ok(Prepared::End(OutcomeResult::SemanticConflict))
            }
            Err(e) if e.code == "IDENTITY_CONFLICT" => {
                return Ok(Prepared::End(OutcomeResult::IdentityConflict))
            }
            Err(e) => return Err(ServiceError::Rejection(e.code.into())),
        };
        match evaluated {
            o::Submission::Duplicate(index) => {
                let original = &history[index];
                let group = decision_groups(records, base)?
                    .into_iter()
                    .find(|g| {
                        g.iter().any(|r| {
                            r["kind"] == "event"
                                && r["body"]["data"]["external_id"] == original.request().id
                                && r["body"]["data"]["source"] == original.request().source
                        })
                    })
                    .ok_or_else(integrity)?;
                let econ_receipt = group
                    .iter()
                    .find(|r| r["kind"] == "receipt")
                    .ok_or_else(integrity)?;
                let settlement = prior
                    .iter()
                    .find(|r| {
                        r["kind"] == "reservation-receipt"
                            && r["body"]["economic_receipt"] == reference(econ_receipt)
                    })
                    .ok_or_else(integrity)?;
                let delivery = StoredCompositeDelivery {
                    key: key.clone(),
                    canonical_key: ScopedDelivery {
                        scope: key.scope.clone(),
                        source: original.request().source.clone(),
                        external_id: original.request().id.clone(),
                    },
                    command: bytes(command)?,
                    ingress: ingress(c, event)?,
                    ingress_hash: ingress_hash(c, event, command)?,
                    economic_receipt: Some(bytes(econ_receipt)?),
                    settlement_receipt: bytes(settlement)?,
                };
                return Ok(Prepared::Append(
                    ValidatedOutcomePlan {
                        resolve: resolve.clone(),
                        anchors: snapshot.anchors.clone(),
                        economic: vec![],
                        settlement: vec![],
                        observed: snapshot.heads.clone(),
                        writes: vec![],
                        delivery,
                    },
                    true,
                ));
            }
            o::Submission::Accepted(decision) => {
                verify_proof(c, snapshot, proof, all, &key.source, true, kind)?;
                let policy = active
                    .rows
                    .iter()
                    .find(|r| {
                        r["kind"] == "policy-snapshot"
                            && r["body"]["family_id"] == req.family
                            && r["body"]["binding_id"] == decision.binding().id
                    })
                    .ok_or_else(integrity)?;
                check(decision.binding().book == ledgerlab_core::policy::chaining::Book::Supplier)?;
                let aggregate = head(
                    snapshot,
                    &lock(
                        c,
                        L::BindingAggregate,
                        vec![json!(c.target), json!(decision.binding().id)],
                        M::Write,
                    )?,
                )?;
                let target = head(
                    snapshot,
                    &lock(c, L::Target, vec![json!(c.target)], M::Write)?,
                )?;
                let au = b::row("authority-decision", &active.scope, authority.clone())?;
                let admission = json!({"event_id":authority["event_id"],"principal":authority["principal"],"credential_revision":proof.authority["grant_revision"],"authentication":proof.authentication["id"],"grant":authority["grant"],"grant_revision":authority["grant_revision"],"target_guard_revision":target.revision,"target_state":"final_unreversed","aggregate_guard_revision":aggregate.revision,"authorized_source":key.source,"agreement_id":req.agreement,"payer":policy["body"]["roles"]["payer"],"book":policy["body"]["book"],"family_id":req.family,"permission":event["data"]["type"],"target_snapshot":base.snapshot["id"],"authority_decision":au["id"],"binding_id":decision.binding().id,"received_at":c.received_at,"accepted_at":c.accepted_at,"decision":"allow","policy_snapshot":policy["id"],"basis":base.basis["id"]});
                amount = Some(decision.current().atoms());
                econ = economic::project(
                    &active,
                    base,
                    event.clone(),
                    authority,
                    admission,
                    &decision,
                    evidence,
                )?;
            }
        }
    } else {
        verify_proof(c, snapshot, proof, all, &key.source, true, kind)?;
    }
    if registering {
        for r in active
            .rows
            .iter()
            .filter(|r| r["kind"] == "binding-snapshot")
        {
            check(
                current(
                    snapshot,
                    c,
                    L::Binding,
                    vec![r["body"]["binding_id"].clone()],
                )? == Some(json!({"active":true,"binding":reference(r)})),
            )?;
        }
    }
    let economic_receipt = econ.iter().find(|r| {
        r["kind"]
            == if registering {
                "base-acceptance"
            } else {
                "receipt"
            }
    });
    let mut econ_prefix = records.rows.clone();
    if !registering {
        econ_prefix.extend(econ.clone());
    }
    // Retained history has already passed full replay. Failures of a new
    // request against its current reservation are ordinary domain rejections.
    if let Some(previous) = prior
        .iter()
        .rev()
        .find(|r| r["kind"] == "reservation-receipt")
    {
        let before = &previous["body"]["result"];
        let reject = |allowed, code: &str| {
            if allowed {
                Ok(())
            } else {
                Err(ServiceError::Rejection(code.into()))
            }
        };
        if kind == "ordinary" {
            let family = array(&before["families"])?
                .iter()
                .find(|f| f["key"] == command["family"])
                .ok_or_else(integrity)?;
            reject(family["status"] == "open", "ORDINARY_CLOSED")?;
            reject(
                c.accepted_at.micros() <= b::time(&family["accepted_by"])?.micros(),
                "ORDINARY_DEADLINE",
            )?;
        } else if kind == "close" {
            reject(
                command["expected_revision"] == before["revision"],
                "EXPECTED_REVISION",
            )?;
            if command["reason"] == "deadline" {
                let latest = array(&before["families"])?
                    .iter()
                    .map(|f| b::time(&f["accepted_by"]).map(|t| t.micros()))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .max()
                    .ok_or_else(integrity)?;
                reject(c.accepted_at.micros() > latest, "CLOSE_DEADLINE")?;
            }
        }
    }
    let settlement = settlement::project(
        records,
        base,
        settlement::SettlementInput {
            command: command.clone(),
            authority: proof.authority.clone(),
            received: json!(c.received_at),
            accepted: json!(c.accepted_at),
            economic: economic_receipt,
            economic_prefix: &econ_prefix,
            prior: &prior,
            amount,
            authorized_close: proof.authorized_early_close,
        },
    )?;
    let receipt = settlement.last().ok_or_else(integrity)?;
    let registration = if registering {
        reference(receipt)
    } else {
        reference(
            prior
                .iter()
                .find(|r| r["kind"] == "reservation-receipt")
                .ok_or_else(integrity)?,
        )
    };
    let mut all_members = if registering {
        vec![]
    } else {
        array(
            &current(snapshot, c, L::Target, vec![json!(c.target)])?.ok_or_else(integrity)?
                ["records"],
        )?
        .clone()
    };
    all_members.extend(econ.iter().chain(&settlement).map(reference));
    let target_value = json!({"base":reference(&base.acceptance),"registration":registration,"records":b::ordered(all_members)?});
    let mut writes = vec![new_write(
        snapshot,
        lock(c, L::Target, vec![json!(c.target)], M::Write)?,
        target_value,
    )?];
    if registering
        || settlement
            .iter()
            .any(|r| r["kind"] == "reservation-transition")
    {
        writes.push(OutcomeHeadWrite {
            lock: lock(c, L::Reservation, vec![json!(c.invocation_id)], M::Write)?,
            revision: text(&receipt["body"]["result"]["revision"])?.into(),
            value: bytes(&receipt["body"]["result"])?,
        });
    }
    if registering {
        for r in active
            .rows
            .iter()
            .filter(|r| r["kind"] == "binding-snapshot")
        {
            writes.push(new_write(
                snapshot,
                lock(
                    c,
                    L::BindingAggregate,
                    vec![json!(c.target), r["body"]["binding_id"].clone()],
                    M::Write,
                )?,
                json!({"revisions":[]}),
            )?);
        }
        writes.push(new_write(
            snapshot,
            lock(
                c,
                L::InvocationConsumption,
                vec![json!(c.invocation_id)],
                M::Write,
            )?,
            json!({"target":c.target,"registration":registration}),
        )?);
        writes.push(new_write(
            snapshot,
            lock(c, L::BaseReversal, vec![json!(c.target)], M::Write)?,
            json!({"reversed":false}),
        )?);
    } else if let Some(rv) = econ.iter().find(|r| r["kind"] == "claim-revision") {
        let family = &command["family"];
        let original = if kind == "ordinary" {
            economic_receipt.ok_or_else(integrity)?
        } else {
            records.get(&rv["body"]["original_receipt"], "receipt")?
        };
        writes.push(OutcomeHeadWrite {
            lock: lock(
                c,
                L::Claim,
                vec![
                    family["agreement_id"].clone(),
                    family["family_id"].clone(),
                    json!(c.target),
                ],
                M::Write,
            )?,
            revision: text(&rv["body"]["number"])?.into(),
            value: bytes(
                &json!({"revision":reference(rv),"original_receipt":reference(original)}),
            )?,
        });
        let mut refs = array(
            &current(
                snapshot,
                c,
                L::BindingAggregate,
                vec![json!(c.target), rv["body"]["binding_id"].clone()],
            )?
            .ok_or_else(integrity)?["revisions"],
        )?
        .clone();
        refs.retain(|r| {
            records
                .deref(r)
                .is_ok_and(|r| r["body"]["claim_id"] != rv["body"]["claim_id"])
        });
        refs.push(reference(rv));
        writes.push(new_write(
            snapshot,
            lock(
                c,
                L::BindingAggregate,
                vec![json!(c.target), rv["body"]["binding_id"].clone()],
                M::Write,
            )?,
            json!({"revisions":b::ordered(refs)?}),
        )?);
    }
    let delivery = StoredCompositeDelivery {
        key: key.clone(),
        canonical_key: key.clone(),
        command: bytes(command)?,
        ingress: ingress(c, event)?,
        ingress_hash: ingress_hash(c, event, command)?,
        economic_receipt: economic_receipt.map(bytes).transpose()?,
        settlement_receipt: bytes(receipt)?,
    };
    let anchors = vec![
        scoped_ref(scoped(c), &reference(&base.acceptance))?,
        scoped_ref(scoped(c), &registration)?,
    ];
    Ok(Prepared::Append(
        ValidatedOutcomePlan {
            resolve: resolve.clone(),
            anchors,
            economic: econ.iter().map(bytes).collect::<Result<_>>()?,
            settlement: settlement.iter().map(bytes).collect::<Result<_>>()?,
            observed: snapshot.heads.clone(),
            writes,
            delivery,
        },
        false,
    ))
}

#[cfg(test)]
#[path = "outcome_fixture.rs"]
pub(crate) mod fixture;

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;
