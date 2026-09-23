//! Concrete byte-preserving persistence for coordinator-validated outcomes.
//! No policy, settlement arithmetic, authority decision, or public append API.
use crate::store::{errors::StoreError, outcomes::*};
use ledgerlab_core::canonical::{self, CanonicalBytes};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
};
use tokio_postgres::GenericClient;

const HISTORY_LIMIT: i64 = 8 * 1024 * 1024;
#[derive(Default)]
pub(super) struct Locked {
    locks: Vec<OutcomeLock>,
    resolved: Option<OutcomeResolve>,
    pub inserted_guard: bool,
}
#[derive(Default)]
pub(super) struct Steps {
    #[cfg(test)]
    pub fail_at: Option<usize>,
    #[cfg(test)]
    pub count: usize,
}
impl Steps {
    fn boundary(&mut self) -> Result<(), StoreError> {
        #[cfg(test)]
        {
            let at = self.count;
            self.count += 1;
            if self.fail_at == Some(at) {
                return Err(StoreError::Integrity("injected outcome write boundary"));
            }
        }
        Ok(())
    }
    async fn run<T>(
        &mut self,
        f: impl Future<Output = Result<T, tokio_postgres::Error>>,
    ) -> Result<T, StoreError> {
        self.boundary()?;
        let value = f.await?;
        self.boundary()?;
        Ok(value)
    }
}
fn invalid() -> StoreError {
    StoreError::Integrity("outcome storage projection")
}
fn bytes(v: &Value) -> Result<Vec<u8>, StoreError> {
    CanonicalBytes::from_value(v)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| invalid())
}
fn parsed(b: &[u8]) -> Result<Value, StoreError> {
    let v = canonical::parse_bounded(b, 4 * 1024 * 1024).map_err(|_| invalid())?;
    if bytes(&v)? != b {
        return Err(invalid());
    }
    Ok(v)
}
fn scope(v: &Value) -> Result<[String; 2], StoreError> {
    let a = v.as_array().ok_or_else(invalid)?;
    if a.len() != 2 {
        return Err(invalid());
    }
    Ok([
        a[0].as_str().ok_or_else(invalid)?.into(),
        a[1].as_str().ok_or_else(invalid)?.into(),
    ])
}
fn lock_scope(lock: &OutcomeLock) -> Result<[String; 2], StoreError> {
    let v = parsed(&lock.key)?;
    scope(v.get(0).ok_or_else(invalid)?)
}
fn class(lock: &OutcomeLock) -> i16 {
    lock.class as i16
}
fn revision(s: &str) -> Result<i64, StoreError> {
    let n = s.parse::<i64>().map_err(|_| invalid())?;
    if n < 0 || n.to_string() != s {
        return Err(invalid());
    }
    Ok(n)
}
fn same_key(a: &OutcomeLock, b: &OutcomeLock) -> bool {
    a.class == b.class && a.key == b.key
}
fn has(held: &[OutcomeLock], need: &OutcomeLock) -> bool {
    held.iter()
        .any(|l| same_key(l, need) && l.mode >= need.mode)
}
fn ordered(scopes: &[OutcomeLock]) -> bool {
    !scopes.is_empty()
        && scopes
            .windows(2)
            .all(|w| (w[0].class, &w[0].key) < (w[1].class, &w[1].key))
}

pub(super) async fn lock<C: GenericClient + Sync>(
    c: &C,
    held: &mut Locked,
    scopes: &[OutcomeLock],
) -> Result<(), StoreError> {
    // A single complete sorted acquisition. Discovery requires a new transaction.
    if !held.locks.is_empty() || !ordered(scopes) {
        return Err(StoreError::Integrity("outcome lock order"));
    }
    // This also participates in existing v1 admission and maintenance fencing.
    if scopes[0].class == OutcomeLockClass::Admission && scopes[0].mode == OutcomeLockMode::Write {
        c.query_one(
            "SELECT singleton FROM ledgerlab.installation WHERE singleton=1 FOR UPDATE",
            &[],
        )
        .await?;
    }
    let installation = super::read::installation(c).await?;
    for l in scopes {
        if lock_scope(l)?
            != [
                installation.scope.tenant.clone(),
                installation.scope.environment.clone(),
            ]
        {
            return Err(invalid());
        }
    }
    for l in scopes {
        held.inserted_guard |= lock_one(c, l).await?;
    }
    held.locks = scopes.to_vec();
    Ok(())
}

async fn guard_exists<C: GenericClient + Sync>(c: &C, l: &OutcomeLock) -> Result<bool, StoreError> {
    let s = lock_scope(l)?;
    Ok(c.query_opt("SELECT key FROM ledgerlab.outcome_scope_locks WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4", &[&s[0], &s[1], &class(l), &l.key]).await?.is_some())
}
pub(super) async fn missing_guards<C: GenericClient + Sync>(
    c: &C,
    scopes: &[OutcomeLock],
) -> Result<bool, StoreError> {
    for l in scopes {
        if !guard_exists(c, l).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

// Used by the native R3 adapter to interleave unchanged legacy tags with R3
// guards in the shared global acquisition order. Never upgrades existing locks.
pub(super) async fn lock_one<C: GenericClient + Sync>(
    c: &C,
    l: &OutcomeLock,
) -> Result<bool, StoreError> {
    let s = lock_scope(l)?;
    // Existing read guards do not issue a nominally idempotent write. Missing
    // rows are preflighted by the supervisor before taking a publication lease.
    let inserted = if guard_exists(c, l).await? {
        false
    } else {
        c.execute("INSERT INTO ledgerlab.outcome_scope_locks(tenant,environment,class,key) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING", &[&s[0],&s[1],&class(l),&l.key]).await? != 0
    };
    let sql = match l.mode {
            OutcomeLockMode::Read => "SELECT key FROM ledgerlab.outcome_scope_locks WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4 FOR SHARE",
            OutcomeLockMode::Write => "SELECT key FROM ledgerlab.outcome_scope_locks WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4 FOR UPDATE",
        };
    c.query_one(sql, &[&s[0], &s[1], &class(l), &l.key]).await?;
    if l.mode == OutcomeLockMode::Write {
        // The guard row is immutable: waiting on it cannot invalidate an
        // older SERIALIZABLE snapshot. Lock the existing mutable head too,
        // so a concurrent revision raises 40001 here, before replay/planning.
        // An absent head remains protected by the scope guard and append CAS.
        c.query_opt("SELECT revision FROM ledgerlab.outcome_heads WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4 FOR UPDATE", &[&s[0],&s[1],&class(l),&l.key]).await?;
    }
    Ok(inserted)
}
pub(super) fn native_empty(held: &Locked) -> bool {
    held.locks.is_empty() && held.resolved.is_none()
}
pub(super) fn native_record(held: &mut Locked, locks: Vec<OutcomeLock>) -> Result<(), StoreError> {
    if !native_empty(held) || !ordered(&locks) {
        return Err(invalid());
    }
    held.locks = locks;
    Ok(())
}
async fn head<C: GenericClient + Sync>(
    c: &C,
    l: &OutcomeLock,
) -> Result<ObservedOutcomeHead, StoreError> {
    let s = lock_scope(l)?;
    let row = c.query_opt("SELECT revision,value FROM ledgerlab.outcome_heads WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4", &[&s[0],&s[1],&class(l),&l.key]).await?;
    let (revision, value) = match row {
        Some(r) => (
            Some(r.try_get::<_, i64>(0)?.to_string()),
            Some(r.try_get(1)?),
        ),
        None => (None, None),
    };
    Ok(ObservedOutcomeHead {
        lock: l.clone(),
        revision,
        value,
    })
}
fn envelope(b: &[u8]) -> Result<ScopedRecordRef, StoreError> {
    let v = parsed(b)?;
    Ok(ScopedRecordRef {
        scope: scope(&v["scope"])?,
        kind: v["kind"].as_str().ok_or_else(invalid)?.into(),
        id: bytes(v.get("id").ok_or_else(invalid)?)?,
        content_hash: v["content_hash"].as_str().ok_or_else(invalid)?.into(),
    })
}
async fn record<C: GenericClient + Sync>(
    c: &C,
    r: &ScopedRecordRef,
) -> Result<Option<Vec<u8>>, StoreError> {
    let row=c.query_opt("SELECT content_hash,envelope FROM ledgerlab.outcome_records WHERE tenant=$1 AND environment=$2 AND kind=$3 AND id=$4", &[&r.scope[0],&r.scope[1],&r.kind,&r.id]).await?;
    row.map(|row| {
        let b: Vec<u8> = row.try_get(1)?;
        if row.try_get::<_, String>(0)? != r.content_hash || envelope(&b)? != *r {
            return Err(invalid());
        }
        Ok(b)
    })
    .transpose()
}

pub(super) async fn lookup<C: GenericClient + Sync>(
    c: &C,
    k: &ScopedDelivery,
) -> Result<Option<StoredCompositeDelivery>, StoreError> {
    let row = c.query_opt("SELECT d.canonical_source,d.canonical_external_id,d.command,d.ingress,d.ingress_hash,e.envelope,s.envelope,d.economic_kind FROM ledgerlab.outcome_deliveries d LEFT JOIN ledgerlab.outcome_records e ON (e.tenant,e.environment,e.kind,e.id)=(d.tenant,d.environment,d.economic_kind,d.economic_id) LEFT JOIN ledgerlab.outcome_records s ON (s.tenant,s.environment,s.kind,s.id)=(d.tenant,d.environment,d.settlement_kind,d.settlement_id) WHERE d.tenant=$1 AND d.environment=$2 AND d.source=$3 AND d.external_id=$4", &[&k.scope[0],&k.scope[1],&k.source,&k.external_id]).await?;
    if row.is_none() {
        if let Some(r) = c.query_opt("SELECT profile FROM ledgerlab.acceptance_delivery_namespace WHERE tenant=$1 AND environment=$2 AND source=$3 AND external_id=$4", &[&k.scope[0],&k.scope[1],&k.source,&k.external_id]).await? {
            return Err(if r.try_get::<_,String>(0)? == "v1" { StoreError::DeliveryConflict } else { invalid() });
        }
    }
    row.map(|r| {
        let economic_receipt: Option<Vec<u8>> = r.try_get(5)?;
        if r.try_get::<_, Option<String>>(7)?.is_some() != economic_receipt.is_some() {
            return Err(invalid());
        }

        let settlement_receipt = r.try_get::<_, Option<Vec<u8>>>(6)?.ok_or_else(invalid)?;
        Ok(StoredCompositeDelivery {
            key: k.clone(),
            canonical_key: ScopedDelivery {
                scope: k.scope.clone(),
                source: r.try_get(0)?,
                external_id: r.try_get(1)?,
            },
            command: r.try_get(2)?,
            ingress: r.try_get(3)?,
            ingress_hash: r.try_get(4)?,
            economic_receipt,
            settlement_receipt,
        })
    })
    .transpose()
}

pub(super) async fn resolve<C: GenericClient + Sync>(
    c: &C,
    held: &mut Locked,
    q: &OutcomeResolve,
) -> Result<OutcomeResolution, StoreError> {
    held.resolved = None;
    let more: Vec<_> = q
        .locks
        .iter()
        .filter(|l| !has(&held.locks, l))
        .cloned()
        .collect();
    if !more.is_empty() {
        return Ok(OutcomeResolution::MoreLocks(more));
    }
    if !ordered(&q.locks) || q.required.len() > 1024 {
        return Err(invalid());
    }
    for l in &q.locks {
        if lock_scope(l)? != q.delivery.scope {
            return Err(invalid());
        }
    }
    let s = &q.delivery.scope;
    let size=c.query_one("SELECT COALESCE(sum(length(r.envelope)),0)::bigint FROM ledgerlab.outcome_members m JOIN ledgerlab.outcome_records r USING(tenant,environment,kind,id) WHERE m.tenant=$1 AND m.environment=$2 AND m.target=$3 AND m.invocation_id=$4", &[&s[0],&s[1],&q.target,&q.invocation_id]).await?.try_get::<_,i64>(0)?;
    if size > HISTORY_LIMIT {
        return Err(StoreError::Integrity(
            "outcome history bound; never truncate",
        ));
    }
    let rows=c.query("SELECT r.kind,r.id,r.content_hash,r.envelope FROM ledgerlab.outcome_members m JOIN ledgerlab.outcome_records r USING(tenant,environment,kind,id) WHERE m.tenant=$1 AND m.environment=$2 AND m.target=$3 AND m.invocation_id=$4 ORDER BY r.kind,r.id", &[&s[0],&s[1],&q.target,&q.invocation_id]).await?;
    let mut records = BTreeMap::new();
    let mut total = size;
    for row in rows {
        let b: Vec<u8> = row.try_get(3)?;
        let r = envelope(&b)?;
        if r.scope != *s
            || r.kind != row.try_get::<_, String>(0)?
            || r.id != row.try_get::<_, Vec<u8>>(1)?
            || r.content_hash != row.try_get::<_, String>(2)?
        {
            return Err(invalid());
        }
        records.insert((r.kind, r.id), b);
    }
    let mut missing = vec![];
    for r in &q.required {
        if r.scope != *s {
            return Err(invalid());
        }
        match record(c, r).await? {
            None => missing.push(r.clone()),
            Some(b) => {
                if !records.contains_key(&(r.kind.clone(), r.id.clone())) {
                    total += b.len() as i64;
                }
                if total > HISTORY_LIMIT {
                    return Err(invalid());
                }
                records.insert((r.kind.clone(), r.id.clone()), b);
            }
        }
    }
    if !missing.is_empty() {
        return Ok(OutcomeResolution::Missing(missing));
    }
    let anchors=c.query("SELECT kind,id,content_hash FROM ledgerlab.outcome_anchors WHERE tenant=$1 AND environment=$2 AND target=$3 AND invocation_id=$4 ORDER BY kind,id", &[&s[0],&s[1],&q.target,&q.invocation_id]).await?.into_iter().map(|r| Ok(ScopedRecordRef { scope:s.clone(),kind:r.try_get(0)?,id:r.try_get(1)?,content_hash:r.try_get(2)? })).collect::<Result<Vec<_>,StoreError>>()?;
    let mut heads = vec![];
    for l in &q.locks {
        heads.push(head(c, l).await?);
    }
    held.resolved = Some(q.clone());
    Ok(OutcomeResolution::Complete(OutcomeSnapshot {
        anchors,
        records: records.into_values().collect(),
        heads,
    }))
}

async fn insert_record<C: GenericClient + Sync>(
    c: &C,
    q: &OutcomeResolve,
    b: &[u8],
    steps: &mut Steps,
) -> Result<(), StoreError> {
    let r = envelope(b)?;
    if r.scope != q.delivery.scope {
        return Err(invalid());
    }
    let n=steps.run(c.execute("INSERT INTO ledgerlab.outcome_records(tenant,environment,kind,id,content_hash,envelope) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(tenant,environment,kind,id) DO NOTHING", &[&r.scope[0],&r.scope[1],&r.kind,&r.id,&r.content_hash,&b])).await?;
    if n == 0 && record(c, &r).await?.as_deref() != Some(b) {
        return Err(StoreError::Integrity("immutable outcome collision"));
    }
    steps.run(c.execute("INSERT INTO ledgerlab.outcome_members(tenant,environment,target,invocation_id,kind,id) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING", &[&r.scope[0],&r.scope[1],&q.target,&q.invocation_id,&r.kind,&r.id])).await?;
    if r.kind == "intention" {
        steps.run(c.execute("INSERT INTO ledgerlab.outcome_held_intentions(tenant,environment,id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING", &[&r.scope[0],&r.scope[1],&r.id])).await?;
    }
    Ok(())
}

pub(super) async fn append<C: GenericClient + Sync>(
    c: &C,
    held: &Locked,
    plan: &ValidatedOutcomePlan,
    steps: &mut Steps,
) -> Result<(), StoreError> {
    let q = plan.resolution();
    if held.resolved.as_ref() != Some(q)
        || plan.delivery().key != q.delivery
        || plan.observed_heads().len() != q.locks.len()
    {
        return Err(invalid());
    }
    for (l, expected) in q.locks.iter().zip(plan.observed_heads()) {
        if expected.lock != *l || !has(&held.locks, l) {
            return Err(invalid());
        }
        if head(c, l).await? != *expected {
            return Err(StoreError::ExpectedCurrent);
        }
    }
    let mut writes = BTreeSet::new();
    for w in plan.head_writes() {
        if w.lock.mode != OutcomeLockMode::Write
            || !has(&held.locks, &w.lock)
            || !q.locks.iter().any(|l| same_key(l, &w.lock))
        {
            return Err(invalid());
        }
        let key = (w.lock.class, &w.lock.key);
        if !writes.insert(key) {
            return Err(invalid());
        }
    }
    let size: usize = plan
        .economic_records()
        .iter()
        .chain(plan.settlement_records())
        .map(Vec::len)
        .sum();
    if size > 4 * 1024 * 1024 {
        return Err(invalid());
    }
    for b in plan
        .economic_records()
        .iter()
        .chain(plan.settlement_records())
    {
        insert_record(c, q, b, steps).await?;
    }
    for r in plan.anchors() {
        if r.scope != q.delivery.scope {
            return Err(invalid());
        }
        let s = &r.scope;
        let n=steps.run(c.execute("INSERT INTO ledgerlab.outcome_anchors(tenant,environment,target,invocation_id,kind,id,content_hash) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING", &[&s[0],&s[1],&q.target,&q.invocation_id,&r.kind,&r.id,&r.content_hash])).await?;
        if n == 0 {
            let hash:String=c.query_one("SELECT content_hash FROM ledgerlab.outcome_anchors WHERE tenant=$1 AND environment=$2 AND target=$3 AND invocation_id=$4 AND kind=$5 AND id=$6", &[&s[0],&s[1],&q.target,&q.invocation_id,&r.kind,&r.id]).await?.try_get(0)?;
            if hash != r.content_hash {
                return Err(invalid());
            }
        }
    }
    for w in plan.head_writes() {
        let old = plan
            .observed_heads()
            .iter()
            .find(|h| same_key(&h.lock, &w.lock))
            .ok_or_else(invalid)?;
        write_head(c, old, w, steps).await?;
    }
    insert_delivery(c, plan.delivery(), steps).await?;
    Ok(())
}
async fn write_head<C: GenericClient + Sync>(
    c: &C,
    old: &ObservedOutcomeHead,
    w: &OutcomeHeadWrite,
    steps: &mut Steps,
) -> Result<(), StoreError> {
    let s = lock_scope(&w.lock)?;
    let new = revision(&w.revision)?;
    parsed(&w.value)?;
    let n=match (&old.revision,&old.value) {
        (None,None) => steps.run(c.execute("INSERT INTO ledgerlab.outcome_heads(tenant,environment,class,key,revision,value) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING", &[&s[0],&s[1],&class(&w.lock),&w.lock.key,&new,&w.value])).await?,
        (Some(rev),Some(value)) => {
            let from=revision(rev)?;
            if from.checked_add(1)!=Some(new) { return Err(invalid()); }
            steps.run(c.execute("UPDATE ledgerlab.outcome_heads SET revision=$5,value=$6 WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4 AND revision=$7 AND value=$8", &[&s[0],&s[1],&class(&w.lock),&w.lock.key,&new,&w.value,&from,value])).await?
        },
        _ => return Err(invalid()),
    };
    if n != 1 {
        return Err(StoreError::ExpectedCurrent);
    }
    Ok(())
}
async fn insert_delivery<C: GenericClient + Sync>(
    c: &C,
    d: &StoredCompositeDelivery,
    steps: &mut Steps,
) -> Result<(), StoreError> {
    if d.key.scope != d.canonical_key.scope {
        return Err(invalid());
    }
    let settlement = envelope(&d.settlement_receipt)?;
    let economic = d.economic_receipt.as_deref().map(envelope).transpose()?;
    if settlement.scope != d.key.scope
        || settlement.kind != "reservation-receipt"
        || economic.as_ref().is_some_and(|r| {
            r.scope != d.key.scope || !matches!(r.kind.as_str(), "receipt" | "base-acceptance")
        })
    {
        return Err(invalid());
    }
    if record(c, &settlement).await?.as_ref() != Some(&d.settlement_receipt) {
        return Err(invalid());
    }
    if let Some(r) = &economic {
        if record(c, r).await?.as_ref() != d.economic_receipt.as_ref() {
            return Err(invalid());
        }
    }
    if d.key != d.canonical_key {
        let original = lookup(c, &d.canonical_key).await?.ok_or_else(invalid)?;
        if original.canonical_key != d.canonical_key
            || original.economic_receipt != d.economic_receipt
            || original.settlement_receipt != d.settlement_receipt
        {
            return Err(invalid());
        }
    }
    let kind = economic.as_ref().map(|r| &r.kind);
    let id = economic.as_ref().map(|r| &r.id);
    steps.run(c.execute("INSERT INTO ledgerlab.outcome_deliveries(tenant,environment,source,external_id,canonical_source,canonical_external_id,command,ingress,ingress_hash,economic_kind,economic_id,settlement_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)", &[&d.key.scope[0],&d.key.scope[1],&d.key.source,&d.key.external_id,&d.canonical_key.source,&d.canonical_key.external_id,&d.command,&d.ingress,&d.ingress_hash,&kind,&id,&settlement.id])).await?;
    Ok(())
}

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;

/// Original-profile v2 acceptance in the caller's existing transaction. The
/// coordinator validated exact records; no settlement receipt is manufactured.
pub(super) async fn append_original<C: GenericClient + Sync>(
    c: &C,
    held: &Locked,
    p: &crate::service::accept::outcome::ValidatedOriginalBasePlan,
    journal: &[u8],
    steps: &mut Steps,
) -> Result<(), StoreError> {
    let q = p.resolution();
    let d = p.delivery_key();
    if held.resolved.as_ref() != Some(q)
        || q.family_key.is_some()
        || d != &q.delivery
        || p.observed_heads().len() != q.locks.len()
        || p.records().len() > 128
        || p.records().iter().map(Vec::len).sum::<usize>() > 1_048_576
    {
        return Err(invalid());
    }
    for (l, h) in q.locks.iter().zip(p.observed_heads()) {
        if h.lock != *l || !has(&held.locks, l) || head(c, l).await? != *h {
            return Err(StoreError::ExpectedCurrent);
        }
    }
    if lookup(c,d).await?.is_some()||c.query_opt("SELECT 1 FROM ledgerlab.outcome_anchors WHERE tenant=$1 AND environment=$2 AND target=$3 AND invocation_id=$4 LIMIT 1",&[&d.scope[0],&d.scope[1],&q.target,&q.invocation_id]).await?.is_some()||c.query_opt("SELECT 1 FROM ledgerlab.r3_deliveries WHERE tenant=$1 AND environment=$2 AND source=$3 AND external_id=$4",&[&d.scope[0],&d.scope[1],&d.source,&d.external_id]).await?.is_some(){return Err(StoreError::DeliveryConflict);}
    let receipt = envelope(p.receipt())?;
    if receipt.kind != "base-acceptance" || receipt.scope != d.scope {
        return Err(invalid());
    }
    for b in p.records() {
        insert_record(c, q, b, steps).await?;
    }
    if record(c, &receipt).await?.as_deref() != Some(p.receipt()) {
        return Err(invalid());
    }
    steps.run(c.execute("INSERT INTO ledgerlab.outcome_anchors(tenant,environment,target,invocation_id,kind,id,content_hash) VALUES($1,$2,$3,$4,$5,$6,$7)",&[&d.scope[0],&d.scope[1],&q.target,&q.invocation_id,&receipt.kind,&receipt.id,&receipt.content_hash])).await?;
    let mut keys = BTreeSet::new();
    for w in p.head_writes() {
        if w.lock.mode != OutcomeLockMode::Write
            || !has(&held.locks, &w.lock)
            || !q.locks.contains(&w.lock)
            || !keys.insert((w.lock.class, w.lock.key.clone()))
        {
            return Err(invalid());
        }
        let old = p
            .observed_heads()
            .iter()
            .find(|h| h.lock == w.lock)
            .ok_or_else(invalid)?;
        write_head(c, old, w, steps).await?;
    }
    let value = bytes(
        &serde_json::json!({"kind":"OriginalBase","receipt":{"kind":receipt.kind,"id":parsed(&receipt.id)?,"content_hash":receipt.content_hash},"ingress_hash":p.ingress_hash()}),
    )?;
    steps.run(c.execute("INSERT INTO ledgerlab.r3_deliveries(tenant,environment,source,external_id,journal,value) VALUES($1,$2,$3,$4,$5,$6)",&[&d.scope[0],&d.scope[1],&d.source,&d.external_id,&journal,&value])).await?;
    Ok(())
}
