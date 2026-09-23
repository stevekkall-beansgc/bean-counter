//! Original v2 base projection for atomic R3 enrollment. No contingent
//! reservation/settlement receipt is manufactured by this adapter.
use super::*;
use crate::service::accept::outcome::ValidatedOriginalBasePlan;

impl SqliteTx {
    pub(in crate::store::sqlite) async fn append_original_base(
        &mut self,
        p: &ValidatedOriginalBasePlan,
        journal: &[u8],
    ) -> Result<(), StoreError> {
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
        let result = timeout_at(
            self.deadline,
            append(self.conn(), p, journal, &mut boundaries),
        )
        .await
        .map_err(|_| StoreError::Deadline)?;
        match &result {
            Ok(()) => self.failed = false,
            Err(e) if e.disables_writes() => self.store.disabled.store(true, Ordering::Release),
            _ => {}
        }
        result
    }
}
async fn append(
    c: &mut SqliteConnection,
    p: &ValidatedOriginalBasePlan,
    journal: &[u8],
    b: &mut Boundaries,
) -> Result<(), StoreError> {
    let q = p.resolution();
    let delivery = p.delivery_key();
    valid_locks(&q.locks)?;
    if delivery != &q.delivery
        || q.family_key.is_some()
        || p.observed_heads().len() != q.locks.len()
    {
        return Err(invalid());
    }
    for (observed, lock) in p.observed_heads().iter().zip(&q.locks) {
        if observed.lock != *lock || json(&lock.key)?[0] != serde_json::json!(delivery.scope) {
            return Err(invalid());
        }
    }
    assert_observed(c, p.observed_heads()).await?;
    if !anchors(c, q).await?.is_empty() || lookup(c, delivery).await?.is_some() {
        return Err(StoreError::DeliveryConflict);
    }
    let occupied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM r3_deliveries WHERE tenant=? AND environment=? AND source=? AND external_id=?)").bind(&delivery.scope[0]).bind(&delivery.scope[1]).bind(&delivery.source).bind(&delivery.external_id).fetch_one(&mut *c).await?;
    if occupied {
        return Err(StoreError::DeliveryConflict);
    }
    let mut keys = BTreeSet::new();
    for w in p.head_writes() {
        if w.lock.mode != OutcomeLockMode::Write
            || !q.locks.contains(&w.lock)
            || !keys.insert((w.lock.class, w.lock.key.clone()))
        {
            return Err(invalid());
        }
    }
    let receipt = record_ref(p.receipt())?;
    if receipt.scope != delivery.scope || receipt.kind != "base-acceptance" {
        return Err(invalid());
    }
    for bytes in p.records() {
        let r = record_ref(bytes)?;
        if r.scope != delivery.scope {
            return Err(invalid());
        }
        insert_record(c, bytes, b).await?;
        b.edge().await?;
        sqlx::query("INSERT INTO outcome_members VALUES (?,?,?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(&r.scope[0])
            .bind(&r.scope[1])
            .bind(&q.target)
            .bind(&q.invocation_id)
            .bind(&r.kind)
            .bind(&r.id)
            .bind(&r.content_hash)
            .execute(&mut *c)
            .await?;
        b.edge().await?;
    }
    if retained(c, &receipt).await?.as_deref() != Some(p.receipt()) {
        return Err(invalid());
    }
    b.edge().await?;
    sqlx::query("INSERT INTO outcome_anchors VALUES (?,?,?,?,?,?,?)")
        .bind(&receipt.scope[0])
        .bind(&receipt.scope[1])
        .bind(&q.target)
        .bind(&q.invocation_id)
        .bind(&receipt.kind)
        .bind(&receipt.id)
        .bind(&receipt.content_hash)
        .execute(&mut *c)
        .await?;
    b.edge().await?;
    for w in p.head_writes() {
        let old = p
            .observed_heads()
            .iter()
            .find(|h| h.lock == w.lock)
            .ok_or_else(invalid)?;
        b.edge().await?;
        let changed=match (&old.revision,&old.value) {
            (None,None)=>sqlx::query("INSERT INTO outcome_heads VALUES (?,?,?,?)").bind(class(w.lock.class)).bind(&w.lock.key).bind(&w.revision).bind(&w.value).execute(&mut *c).await?.rows_affected(),
            (Some(rev),Some(value))=>sqlx::query("UPDATE outcome_heads SET revision=?,value=? WHERE class=? AND key=? AND revision=? AND value=?").bind(&w.revision).bind(&w.value).bind(class(w.lock.class)).bind(&w.lock.key).bind(rev).bind(value).execute(&mut *c).await?.rows_affected(),
            _=>return Err(invalid())
        };
        if changed != 1 {
            return Err(StoreError::ExpectedCurrent);
        }
        b.edge().await?;
    }
    let value = canonical(
        &serde_json::json!({"kind":"OriginalBase","receipt":{"kind":receipt.kind,"id":json(&receipt.id)?,"content_hash":receipt.content_hash},"ingress_hash":p.ingress_hash()}),
    )?;
    b.edge().await?;
    sqlx::query("INSERT INTO r3_deliveries VALUES (?,?,?,?,?,?)")
        .bind(&delivery.scope[0])
        .bind(&delivery.scope[1])
        .bind(&delivery.source)
        .bind(&delivery.external_id)
        .bind(journal)
        .bind(value)
        .execute(&mut *c)
        .await?;
    b.edge().await?;
    Ok(())
}
