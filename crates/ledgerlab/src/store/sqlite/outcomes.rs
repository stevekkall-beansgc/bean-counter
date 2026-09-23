//! SQLite mapping of the coordinator-owned outcome protocol. No economics here.
mod original;
use super::{SqliteStore, SqliteTx};
use crate::store::{errors::StoreError, outcomes::*, ports::AcceptanceStore};
use ledgerlab_core::canonical::{parse_bounded, CanonicalBytes};
use serde_json::Value;
use sqlx::{Row, SqliteConnection};
use std::{collections::BTreeSet, sync::atomic::Ordering, time::Duration};
use tokio::time::{timeout_at, Instant};

const HISTORY_LIMIT: usize = 8 * 1024 * 1024;
const RECORD_LIMIT: i64 = 4096;
fn invalid() -> StoreError {
    StoreError::Integrity("outcome persistence projection mismatch")
}
fn json(bytes: &[u8]) -> Result<Value, StoreError> {
    parse_bounded(bytes, HISTORY_LIMIT).map_err(|_| invalid())
}
fn canonical(value: &Value) -> Result<Vec<u8>, StoreError> {
    CanonicalBytes::from_value(value)
        .map(|b| b.into_vec())
        .map_err(|_| invalid())
}
fn record_ref(bytes: &[u8]) -> Result<ScopedRecordRef, StoreError> {
    let v = json(bytes)?;
    if canonical(&v)? != bytes
        || canonical(&v["body"])?.len() > 262144
        || v.as_object().is_none_or(|o| o.len() != 5)
        || v["scope"].as_array().is_none_or(|s| s.len() != 2)
    {
        return Err(invalid());
    }
    Ok(ScopedRecordRef {
        scope: [
            v["scope"][0].as_str().ok_or_else(invalid)?.into(),
            v["scope"][1].as_str().ok_or_else(invalid)?.into(),
        ],
        kind: v["kind"].as_str().ok_or_else(invalid)?.into(),
        id: canonical(&v["id"])?,
        content_hash: v["content_hash"].as_str().ok_or_else(invalid)?.into(),
    })
}
fn class(c: OutcomeLockClass) -> i64 {
    match c {
        OutcomeLockClass::Admission => 0,
        OutcomeLockClass::Authority => 1,
        OutcomeLockClass::Binding => 2,
        OutcomeLockClass::Reservation => 3,
        OutcomeLockClass::Target => 4,
        OutcomeLockClass::Claim => 5,
        OutcomeLockClass::BindingAggregate => 6,
        OutcomeLockClass::InvocationConsumption => 7,
        OutcomeLockClass::BaseReversal => 8,
    }
}
fn lock_identity(l: &OutcomeLock) -> (OutcomeLockClass, &[u8]) {
    (l.class, &l.key)
}
fn valid_locks(locks: &[OutcomeLock]) -> Result<(), StoreError> {
    if locks.len() > 256
        || locks
            .windows(2)
            .any(|w| lock_identity(&w[0]) >= lock_identity(&w[1]))
    {
        return Err(invalid());
    }
    for l in locks {
        let v = json(&l.key)?;
        if l.key.len() > 4096
            || canonical(&v)? != l.key
            || !v.is_array()
            || v[0].as_array().is_none_or(|s| {
                s.len() != 2 || s.iter().any(|p| p.as_str().is_none_or(str::is_empty))
            })
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn holds(held: &[OutcomeLock], wanted: &OutcomeLock) -> bool {
    held.iter()
        .any(|l| lock_identity(l) == lock_identity(wanted) && l.mode >= wanted.mode)
}
async fn head(
    c: &mut SqliteConnection,
    lock: &OutcomeLock,
) -> Result<ObservedOutcomeHead, StoreError> {
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT revision,value FROM outcome_heads WHERE class=? AND key=?")
            .bind(class(lock.class))
            .bind(&lock.key)
            .fetch_optional(c)
            .await?;
    Ok(ObservedOutcomeHead {
        lock: lock.clone(),
        revision: row.as_ref().map(|r| r.0.clone()),
        value: row.map(|r| r.1),
    })
}
async fn retained(
    c: &mut SqliteConnection,
    r: &ScopedRecordRef,
) -> Result<Option<Vec<u8>>, StoreError> {
    let row: Option<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT content_hash,canonical_bytes FROM outcome_records WHERE tenant=? AND environment=? AND kind=? AND id=?",
    ).bind(&r.scope[0]).bind(&r.scope[1]).bind(&r.kind).bind(&r.id).fetch_optional(c).await?;
    match row {
        Some((hash, bytes)) if hash == r.content_hash && record_ref(&bytes)? == *r => {
            Ok(Some(bytes))
        }
        Some(_) => Err(invalid()),
        None => Ok(None),
    }
}
async fn lookup(
    c: &mut SqliteConnection,
    key: &ScopedDelivery,
) -> Result<Option<StoredCompositeDelivery>, StoreError> {
    let row = sqlx::query("SELECT d.canonical_source,d.canonical_external_id,d.command,d.ingress,d.ingress_hash,e.canonical_bytes,s.canonical_bytes,d.economic_kind FROM outcome_deliveries d LEFT JOIN outcome_records e ON e.tenant=d.tenant AND e.environment=d.environment AND e.kind=d.economic_kind AND e.id=d.economic_id AND e.content_hash=d.economic_hash LEFT JOIN outcome_records s ON s.tenant=d.tenant AND s.environment=d.environment AND s.kind=d.settlement_kind AND s.id=d.settlement_id AND s.content_hash=d.settlement_hash WHERE d.tenant=? AND d.environment=? AND d.source=? AND d.external_id=?")
        .bind(&key.scope[0]).bind(&key.scope[1]).bind(&key.source).bind(&key.external_id).fetch_optional(&mut *c).await?;
    if let Some(r) = row {
        let economic_receipt: Option<Vec<u8>> = r.try_get(5)?;
        let settlement_receipt: Option<Vec<u8>> = r.try_get(6)?;
        let economic_kind: Option<String> = r.try_get(7)?;
        if economic_kind.is_some() != economic_receipt.is_some() {
            return Err(invalid());
        }
        Ok(Some(StoredCompositeDelivery {
            key: key.clone(),
            canonical_key: ScopedDelivery {
                scope: key.scope.clone(),
                source: r.try_get(0)?,
                external_id: r.try_get(1)?,
            },
            command: r.try_get(2)?,
            ingress: r.try_get(3)?,
            ingress_hash: r.try_get(4)?,
            economic_receipt,
            settlement_receipt: settlement_receipt.ok_or_else(invalid)?,
        }))
    } else {
        // A legacy identity cannot be represented as a fabricated composite pair.
        let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM delivery_keys WHERE tenant=?1 AND environment=?2 AND source=?3 AND external_id=?4) OR EXISTS(SELECT 1 FROM r3_deliveries WHERE tenant=?1 AND environment=?2 AND source=?3 AND external_id=?4)")
            .bind(&key.scope[0]).bind(&key.scope[1]).bind(&key.source).bind(&key.external_id).fetch_one(c).await?;
        if occupied {
            Err(StoreError::DeliveryConflict)
        } else {
            Ok(None)
        }
    }
}
async fn anchors(
    c: &mut SqliteConnection,
    q: &OutcomeResolve,
) -> Result<Vec<ScopedRecordRef>, StoreError> {
    let rows = sqlx::query("SELECT kind,id,content_hash FROM outcome_anchors WHERE tenant=? AND environment=? AND target=? AND invocation_id=? ORDER BY kind,id LIMIT 4097")
        .bind(&q.delivery.scope[0]).bind(&q.delivery.scope[1]).bind(&q.target).bind(&q.invocation_id).fetch_all(c).await?;
    if rows.len() > RECORD_LIMIT as usize {
        return Err(invalid());
    }
    rows.into_iter()
        .map(|r| {
            Ok(ScopedRecordRef {
                scope: q.delivery.scope.clone(),
                kind: r.try_get(0)?,
                id: r.try_get(1)?,
                content_hash: r.try_get(2)?,
            })
        })
        .collect()
}
async fn resolve(
    c: &mut SqliteConnection,
    q: &OutcomeResolve,
) -> Result<OutcomeResolution, StoreError> {
    let roots = anchors(c, q).await?;
    let (count,total): (i64,i64) = sqlx::query_as("SELECT count(*),coalesce(sum(length(r.canonical_bytes)),0) FROM outcome_members m JOIN outcome_records r ON r.tenant=m.tenant AND r.environment=m.environment AND r.kind=m.kind AND r.id=m.id AND r.content_hash=m.content_hash WHERE m.tenant=? AND m.environment=? AND m.target=? AND m.invocation_id=?")
        .bind(&q.delivery.scope[0]).bind(&q.delivery.scope[1]).bind(&q.target).bind(&q.invocation_id).fetch_one(&mut *c).await?;
    if count > RECORD_LIMIT || total > HISTORY_LIMIT as i64 {
        return Err(invalid());
    }

    let mut records: Vec<Vec<u8>> = sqlx::query_scalar("SELECT r.canonical_bytes FROM outcome_members m JOIN outcome_records r ON r.tenant=m.tenant AND r.environment=m.environment AND r.kind=m.kind AND r.id=m.id AND r.content_hash=m.content_hash WHERE m.tenant=? AND m.environment=? AND m.target=? AND m.invocation_id=? ORDER BY r.kind,r.id LIMIT 4097")
        .bind(&q.delivery.scope[0]).bind(&q.delivery.scope[1]).bind(&q.target).bind(&q.invocation_id).fetch_all(&mut *c).await?;
    let mut missing = Vec::new();
    for r in q.required.iter().chain(&roots) {
        if r.scope != q.delivery.scope {
            return Err(invalid());
        }
        match retained(c, r).await? {
            Some(b) => {
                if !records.contains(&b) {
                    records.push(b);
                    if records.iter().map(Vec::len).sum::<usize>() > HISTORY_LIMIT {
                        return Err(invalid());
                    }
                }
            }
            None => missing.push(r.clone()),
        }
    }
    if records.len() > RECORD_LIMIT as usize
        || records.iter().map(Vec::len).sum::<usize>() > HISTORY_LIMIT
    {
        return Err(invalid());
    }
    if !missing.is_empty() {
        return Ok(OutcomeResolution::Missing(missing));
    }
    let mut heads = Vec::new();
    for l in &q.locks {
        heads.push(head(c, l).await?);
    }
    Ok(OutcomeResolution::Complete(OutcomeSnapshot {
        anchors: roots,
        records,
        heads,
    }))
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Fault {
    pub cut: usize,
    pub pause: bool,
    pub reached: tokio::sync::Notify,
    pub steps: std::sync::atomic::AtomicUsize,
}
struct Boundaries {
    #[cfg(test)]
    fault: Option<std::sync::Arc<Fault>>,
}
impl Boundaries {
    async fn edge(&mut self) -> Result<(), StoreError> {
        #[cfg(test)]
        if let Some(f) = &self.fault {
            let n = f.steps.fetch_add(1, Ordering::SeqCst);
            if n == f.cut {
                f.reached.notify_one();
                if f.pause {
                    std::future::pending::<()>().await;
                }
                return Err(StoreError::Deadline);
            }
        }
        Ok(())
    }
}
async fn insert_record(
    c: &mut SqliteConnection,
    bytes: &[u8],
    b: &mut Boundaries,
) -> Result<ScopedRecordRef, StoreError> {
    let r = record_ref(bytes)?;
    if let Some(existing) = retained(c, &r).await? {
        if existing != bytes {
            return Err(invalid());
        }
        return Ok(r);
    }
    b.edge().await?;
    sqlx::query("INSERT INTO outcome_records (tenant,environment,kind,id,content_hash,canonical_bytes) VALUES (?,?,?,?,?,?)")
        .bind(&r.scope[0]).bind(&r.scope[1]).bind(&r.kind).bind(&r.id).bind(&r.content_hash).bind(bytes).execute(&mut *c).await?;
    b.edge().await?;
    Ok(r)
}
async fn assert_observed(
    c: &mut SqliteConnection,
    observed: &[ObservedOutcomeHead],
) -> Result<(), StoreError> {
    for old in observed {
        if head(c, &old.lock).await? != *old {
            return Err(StoreError::ExpectedCurrent);
        }
    }
    Ok(())
}
async fn append(
    c: &mut SqliteConnection,
    p: &ValidatedOutcomePlan,
    b: &mut Boundaries,
) -> Result<(), StoreError> {
    let q = p.resolution();
    if p.delivery().key.scope != q.delivery.scope || p.delivery().key != q.delivery {
        return Err(StoreError::Integrity("delivery and resolution disagree"));
    }
    valid_locks(&q.locks)?;
    for l in &q.locks {
        if json(&l.key)?[0] != serde_json::json!(q.delivery.scope) {
            return Err(StoreError::Integrity(
                "head scope differs from delivery scope",
            ));
        }
    }
    if p.observed_heads().len() != q.locks.len() {
        return Err(StoreError::Integrity("incomplete observed head set"));
    }
    for (observed, lock) in p.observed_heads().iter().zip(&q.locks) {
        if observed.lock != *lock {
            return Err(StoreError::Integrity(
                "observed lock differs from resolved lock",
            ));
        }
    }
    assert_observed(c, p.observed_heads()).await?;
    let mut keys = BTreeSet::new();
    for w in p.head_writes() {
        if w.lock.mode != OutcomeLockMode::Write
            || !q.locks.contains(&w.lock)
            || !keys.insert((w.lock.class, w.lock.key.clone()))
        {
            return Err(StoreError::Integrity(
                "head write lacks unique declared write lock",
            ));
        }
    }
    let prior_roots = anchors(c, q).await?;
    let mut supplied = p.anchors().to_vec();
    supplied.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    if !prior_roots.is_empty() && prior_roots != supplied {
        return Err(StoreError::Integrity("original outcome anchors changed"));
    }
    let d = p.delivery();
    if d.canonical_key.scope != d.key.scope {
        return Err(StoreError::Integrity("alias crosses scope"));
    }
    if d.key != d.canonical_key {
        let original = lookup(c, &d.canonical_key).await?.ok_or_else(invalid)?;
        if original.key != original.canonical_key
            || original.economic_receipt != d.economic_receipt
            || original.settlement_receipt != d.settlement_receipt
            || !p.economic_records().is_empty()
            || !p.settlement_records().is_empty()
            || !p.head_writes().is_empty()
        {
            return Err(StoreError::Integrity(
                "alias changes original receipt or economic state",
            ));
        }
    }
    for bytes in p.economic_records().iter().chain(p.settlement_records()) {
        let r = record_ref(bytes)?;
        if r.scope != d.key.scope {
            return Err(StoreError::Integrity("record crosses delivery scope"));
        }
        insert_record(c, bytes, b).await?;
        b.edge().await?;
        sqlx::query("INSERT INTO outcome_members (tenant,environment,target,invocation_id,kind,id,content_hash) VALUES (?,?,?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(&r.scope[0]).bind(&r.scope[1]).bind(&q.target).bind(&q.invocation_id).bind(&r.kind).bind(&r.id).bind(&r.content_hash).execute(&mut *c).await?;
        b.edge().await?;
    }
    for r in &supplied {
        if r.scope != d.key.scope || retained(c, r).await?.is_none() {
            return Err(StoreError::Integrity(
                "missing or cross-scope original anchor",
            ));
        }
        b.edge().await?;
        sqlx::query("INSERT INTO outcome_anchors (tenant,environment,target,invocation_id,kind,id,content_hash) VALUES (?,?,?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(&r.scope[0]).bind(&r.scope[1]).bind(&q.target).bind(&q.invocation_id).bind(&r.kind).bind(&r.id).bind(&r.content_hash).execute(&mut *c).await?;
        b.edge().await?;
    }
    for w in p.head_writes() {
        let old = p
            .observed_heads()
            .iter()
            .find(|h| h.lock == w.lock)
            .ok_or_else(invalid)?;
        b.edge().await?;
        let n = match (&old.revision, &old.value) {
            (None,None) => sqlx::query("INSERT INTO outcome_heads (class,key,revision,value) VALUES (?,?,?,?)")
                .bind(class(w.lock.class)).bind(&w.lock.key).bind(&w.revision).bind(&w.value).execute(&mut *c).await?.rows_affected(),
            (Some(rev),Some(value)) => sqlx::query("UPDATE outcome_heads SET revision=?,value=? WHERE class=? AND key=? AND revision=? AND value=?")
                .bind(&w.revision).bind(&w.value).bind(class(w.lock.class)).bind(&w.lock.key).bind(rev).bind(value).execute(&mut *c).await?.rows_affected(),
            _ => return Err(invalid()),
        };
        if n != 1 {
            return Err(StoreError::ExpectedCurrent);
        }
        b.edge().await?;
    }
    let economic = d.economic_receipt.as_deref().map(record_ref).transpose()?;
    let settlement = record_ref(&d.settlement_receipt)?;
    if settlement.scope != d.key.scope || settlement.kind != "reservation-receipt" {
        return Err(StoreError::Integrity(
            "invalid settlement receipt projection",
        ));
    }
    if let Some(r) = &economic {
        if r.scope != d.key.scope || !matches!(r.kind.as_str(), "base-acceptance" | "receipt") {
            return Err(StoreError::Integrity("invalid economic receipt projection"));
        }
    }
    if retained(c, &settlement).await?.as_deref() != Some(&d.settlement_receipt)
        || match &economic {
            Some(r) => retained(c, r).await? != d.economic_receipt,
            None => false,
        }
    {
        return Err(StoreError::Integrity(
            "receipt bytes differ from retained records",
        ));
    }
    b.edge().await?;
    sqlx::query("INSERT INTO outcome_deliveries (tenant,environment,source,external_id,canonical_source,canonical_external_id,command,ingress,ingress_hash,economic_kind,economic_id,economic_hash,settlement_kind,settlement_id,settlement_hash) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&d.key.scope[0]).bind(&d.key.scope[1]).bind(&d.key.source).bind(&d.key.external_id)
        .bind(&d.canonical_key.source).bind(&d.canonical_key.external_id).bind(&d.command).bind(&d.ingress).bind(&d.ingress_hash)
        .bind(economic.as_ref().map(|r|&r.kind)).bind(economic.as_ref().map(|r|&r.id)).bind(economic.as_ref().map(|r|&r.content_hash))
        .bind(&settlement.kind).bind(&settlement.id).bind(&settlement.content_hash).execute(c).await?;
    b.edge().await?;
    Ok(())
}
impl OutcomeStore for SqliteStore {
    type Tx = SqliteTx;
    async fn begin_outcome(&self, deadline: Instant) -> Result<SqliteTx, StoreError> {
        self.begin(deadline).await
    }
}

#[cfg(test)]
impl SqliteStore {
    /// Only original-profile evidence and current authority/binding heads. An
    /// accepted base/receipt/target cannot enter through this test setup helper.
    pub(crate) async fn test_provision_outcome_evidence(
        &self,
        records: &[Vec<u8>],
        heads: &[ObservedOutcomeHead],
    ) -> Result<(), StoreError> {
        use crate::store::ports::AcceptanceTx;
        let mut tx = self.begin(Instant::now() + Duration::from_secs(5)).await?;
        let installation = tx.load_installation().await?;
        for bytes in records {
            let r = record_ref(bytes)?;
            if r.kind != "evidence"
                || r.scope
                    != [
                        installation.scope.tenant.clone(),
                        installation.scope.environment.clone(),
                    ]
            {
                return Err(invalid());
            }
            insert_record(tx.conn(), bytes, &mut Boundaries { fault: None }).await?;
        }
        for h in heads {
            if let (Some(revision), Some(value)) = (&h.revision, &h.value) {
                if !matches!(
                    h.lock.class,
                    OutcomeLockClass::Authority | OutcomeLockClass::Binding
                ) {
                    return Err(invalid());
                }
                sqlx::query("INSERT INTO outcome_heads VALUES (?,?,?,?)")
                    .bind(class(h.lock.class))
                    .bind(&h.lock.key)
                    .bind(revision)
                    .bind(value)
                    .execute(tx.conn())
                    .await?;
            } else if h.revision.is_some() || h.value.is_some() {
                return Err(invalid());
            }
        }
        tx.commit().await.map_err(|_| StoreError::WritesDisabled)
    }
}
impl OutcomeTx for SqliteTx {
    async fn lock_scopes(&mut self, scopes: &[OutcomeLock]) -> Result<(), StoreError> {
        if self.failed {
            return Err(invalid());
        }
        self.failed = true;
        valid_locks(scopes)?;
        if !self.outcome_locks.is_empty() && self.outcome_locks != scopes {
            return Err(invalid());
        }
        self.outcome_locks = scopes.to_vec();
        self.failed = false;
        Ok(())
    }
    async fn lookup_outcome_delivery(
        &mut self,
        key: &ScopedDelivery,
    ) -> Result<Option<StoredCompositeDelivery>, StoreError> {
        let failed = self.failed;
        self.failed = true;
        let r = timeout_at(
            self.deadline.min(Instant::now() + Duration::from_secs(2)),
            lookup(self.conn(), key),
        )
        .await
        .map_err(|_| StoreError::Deadline)?;
        match &r {
            Ok(_) => self.failed = failed,
            Err(e) if e.disables_writes() => self.store.disabled.store(true, Ordering::Release),
            _ => {}
        }
        r
    }
    async fn resolve_outcome(
        &mut self,
        q: &OutcomeResolve,
    ) -> Result<OutcomeResolution, StoreError> {
        let failed = self.failed;
        self.failed = true;
        valid_locks(&q.locks)?;
        for l in &q.locks {
            if json(&l.key)?[0] != serde_json::json!(q.delivery.scope) {
                return Err(invalid());
            }
        }
        if q.required.len() > 1024 {
            return Err(invalid());
        }
        if q.locks.iter().any(|l| !holds(&self.outcome_locks, l)) {
            self.failed = failed;
            return Ok(OutcomeResolution::MoreLocks(q.locks.clone()));
        }
        let r = timeout_at(
            self.deadline.min(Instant::now() + Duration::from_secs(2)),
            resolve(self.conn(), q),
        )
        .await
        .map_err(|_| StoreError::Deadline)?;
        match &r {
            Ok(_) => self.failed = failed,
            Err(e) if e.disables_writes() => self.store.disabled.store(true, Ordering::Release),
            _ => {}
        }
        r
    }
    async fn append_outcome(&mut self, p: &ValidatedOutcomePlan) -> Result<(), StoreError> {
        if self.failed {
            return Err(invalid());
        }
        self.failed = true;
        if p.resolution()
            .locks
            .iter()
            .any(|l| !holds(&self.outcome_locks, l))
        {
            return Err(invalid());
        }
        let mut boundaries = Boundaries {
            #[cfg(test)]
            fault: self.outcome_fault.clone(),
        };
        let r = timeout_at(self.deadline, append(self.conn(), p, &mut boundaries))
            .await
            .map_err(|_| StoreError::Deadline)?;
        match &r {
            Ok(()) => self.failed = false,
            Err(e) if e.disables_writes() => self.store.disabled.store(true, Ordering::Release),
            _ => {}
        }
        r
    }
}

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "outcome_atomic_tests.rs"]
mod atomic_tests;

impl SqliteStore {
    /// Explicit trusted host control-plane provisioning. Only exact evidence and
    /// current Authority/Binding heads may be written; never accepted base state.
    pub(crate) async fn provision_original_authority(
        &self,
        records: &[Vec<u8>],
        changes: &[(OutcomeLock, Option<String>, Vec<u8>)],
    ) -> Result<(), StoreError> {
        use crate::store::ports::AcceptanceTx;
        use ledgerlab_core::canonical::{self as c, outcome as codec, Domain};
        if self.inner._owner.fence.is_none()
            || !self.inner.adjudication_enabled.load(Ordering::Acquire)
            || records.len() > 128
            || records.iter().map(Vec::len).sum::<usize>() > 1_048_576
            || changes.is_empty()
            || changes.len() > 33
        {
            return Err(invalid());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut tx = self.begin(deadline).await?;
        let installation = tx.load_installation().await?;
        let scope = [installation.scope.tenant, installation.scope.environment];
        let mut locks = vec![OutcomeLock {
            class: OutcomeLockClass::Admission,
            key: canonical(&serde_json::json!([scope]))?,
            mode: OutcomeLockMode::Write,
        }];
        locks.extend(changes.iter().map(|h| h.0.clone()));
        tx.lock_scopes(&locks).await?;
        tx.failed = true;
        let result = timeout_at(deadline, async {
            for bytes in records {
                if bytes.len() > 262144 { return Err(invalid()); }
                let v = codec::decode(bytes).map_err(|_|invalid())?;
                let r = record_ref(bytes)?;
                if r.kind != "evidence" || r.scope != scope { return Err(invalid()); }
                let body=&v["body"];
                let raw=body["utf8"].as_str().ok_or_else(invalid)?;
                let doc=c::parse_bounded(raw.as_bytes(),262144).map_err(|_|invalid())?;
                let hash=c::digest(Domain::Document,&serde_json::json!([body["document_type"],1,doc])).map_err(|_|invalid())?;
                if canonical(&doc)?!=raw.as_bytes() || body["document_version"]!=1 || body["document_hash"]!=hash || body["document_id"]!=format!("doc_{}",&hash[7..]) { return Err(invalid()); }
                insert_record(tx.conn(),bytes,&mut Boundaries{#[cfg(test)] fault:None}).await?;
            }
            for (lock,expected,value) in changes {
                if !matches!(lock.class,OutcomeLockClass::Authority|OutcomeLockClass::Binding)
                    || lock.mode!=OutcomeLockMode::Write || value.len()>16384
                    || json(&lock.key)?[0]!=serde_json::json!(scope)
                    || canonical(&json(value)?)?!=*value
                { return Err(invalid()); }
                let old=head(tx.conn(),lock).await?;
                if old.revision!=*expected { return Err(StoreError::ExpectedCurrent); }
                let revision=expected.as_ref().map(|r|r.parse::<u64>().map_err(|_|invalid())).transpose()?.unwrap_or(0).checked_add(1).filter(|n|*n<=i64::MAX as u64).ok_or_else(invalid)?.to_string();
                let changed=if let Some(expected)=expected {
                    sqlx::query("UPDATE outcome_heads SET revision=?,value=? WHERE class=? AND key=? AND revision=?").bind(&revision).bind(value).bind(class(lock.class)).bind(&lock.key).bind(expected).execute(tx.conn()).await?.rows_affected()
                } else {
                    sqlx::query("INSERT INTO outcome_heads VALUES(?,?,?,?)").bind(class(lock.class)).bind(&lock.key).bind(&revision).bind(value).execute(tx.conn()).await?.rows_affected()
                };
                if changed!=1 { return Err(StoreError::ExpectedCurrent); }
            }
            Ok::<(),StoreError>(())
        }).await.map_err(|_|StoreError::Deadline)?;
        if let Err(error) = result {
            tx.rollback().await?;
            return Err(error);
        }
        tx.failed = false;
        tx.commit().await.map_err(|_| StoreError::WritesDisabled)
    }
}
