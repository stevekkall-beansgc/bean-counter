use crate::{
    outbox::{Delivery, Head, Intention, Mutation, Query, Snapshot, State, Sweep},
    store::errors::StoreError,
};
use sqlx::{Row, SqliteConnection};
pub(super) async fn read(c: &mut SqliteConnection, query: Query) -> Result<Snapshot, StoreError> {
    let installation = super::read::installation(c).await?;
    let r=sqlx::query("SELECT owner,generation,lease_until_us,enabled,revision FROM dispatcher_head WHERE singleton=1").fetch_one(&mut *c).await?;
    let head = Head {
        owner: r.try_get(0)?,
        generation: r.try_get(1)?,
        until: r.try_get(2)?,
        enabled: r.try_get::<i64, _>(3)? != 0,
        revision: r.try_get(4)?,
    };
    let (after, key, due, limit) = match query {
        Query::Control => (String::new(), None, None, 0_i64),
        Query::Page { after, due } => (after, None, due, crate::outbox::PAGE_SIZE as i64),
        Query::Key(key) => (String::new(), Some(key), None, 1_i64),
    };
    let mut items = Vec::new();
    let rows = sqlx::query("SELECT i.id,length(i.canonical_bytes) FROM intentions i LEFT JOIN delivery_state d ON d.tenant=i.tenant AND d.environment=i.environment AND d.intention_id=i.id LEFT JOIN delivery_quarantines q ON q.tenant=i.tenant AND q.environment=i.environment AND q.intention_id=i.id WHERE i.tenant=?1 AND i.environment=?2 AND i.id>?3 AND (?4 IS NULL OR i.id=?4) AND (?5 IS NULL OR (d.state IN ('held','pending','retry') AND d.attempts<20 AND d.next_attempt_us<=?5 AND q.intention_id IS NULL)) ORDER BY i.id LIMIT ?6").bind(&installation.scope.tenant).bind(&installation.scope.environment).bind(&after).bind(&key).bind(due).bind(limit).fetch_all(&mut *c).await?;
    let mut total = 0_i64;
    for meta in rows {
        let id: String = meta.try_get(0)?;
        let size: i64 = meta.try_get(1)?;
        if size > ledgerlab_core::canonical::BUNDLE_LIMIT as i64 {
            return Err(StoreError::InvalidStore("outbox scan limit"));
        }
        if total + size > 8 * 1024 * 1024 {
            break;
        }
        total += size;
        let r = sqlx::query("SELECT i.id,i.destination_id,i.idempotency_key,i.canonical_bytes,i.content_hash,d.state,d.attempts,d.next_attempt_us,d.lease_owner,d.generation,d.lease_until_us,d.last_observation,q.digest FROM intentions i LEFT JOIN delivery_state d ON d.tenant=i.tenant AND d.environment=i.environment AND d.intention_id=i.id LEFT JOIN delivery_quarantines q ON q.tenant=i.tenant AND q.environment=i.environment AND q.intention_id=i.id WHERE i.tenant=?1 AND i.environment=?2 AND i.id=?3").bind(&installation.scope.tenant).bind(&installation.scope.environment).bind(&id).fetch_one(&mut *c).await?;

        items.push((
            Intention {
                id: r.try_get(0)?,
                destination: r.try_get(1)?,
                key: r.try_get(2)?,
                bytes: r.try_get(3)?,
                hash: r.try_get(4)?,
            },
            Delivery {
                intention_id: r.try_get(0)?,
                state: State::parse(&r.try_get::<String, _>(5)?)?,
                attempts: r.try_get(6)?,
                next_attempt_us: r.try_get(7)?,
                owner: r.try_get(8)?,
                generation: r.try_get(9)?,
                until: r.try_get(10)?,
                last_observation: r.try_get(11)?,
                quarantine: r.try_get(12)?,
            },
        ));
    }
    let report = sqlx::query(
        "SELECT digest,canonical_bytes FROM reconciliation_reports ORDER BY sequence DESC LIMIT 1",
    )
    .fetch_optional(&mut *c)
    .await?;
    let report = report
        .map(|r| Ok::<_, StoreError>((r.try_get(0)?, r.try_get(1)?)))
        .transpose()?;
    Ok(Snapshot {
        installation,
        head,
        items,
        report,
    })
}
pub(super) async fn write(c: &mut SqliteConnection, m: &Mutation) -> Result<(), StoreError> {
    let s = &m.snapshot;
    let h = &s.head;
    let i = &s.installation;
    sqlx::query(
        "UPDATE installation SET generation=?,dispatch_hold=?,dispatch_enabled=? WHERE singleton=1",
    )
    .bind(i.generation)
    .bind(i64::from(i.dispatch_hold))
    .bind(i64::from(i.dispatch_enabled))
    .execute(&mut *c)
    .await?;
    sqlx::query("UPDATE dispatcher_head SET owner=?,generation=?,lease_until_us=?,enabled=?,revision=? WHERE singleton=1").bind(&h.owner).bind(h.generation).bind(h.until).bind(i64::from(h.enabled)).bind(h.revision).execute(&mut *c).await?;
    if let Some(sweep) = &m.sweep {
        let (restore, expired) = match sweep {
            Sweep::Leases => (0_i64, None),
            Sweep::Expired(now) => (0_i64, Some(*now)),
            Sweep::Restore => (1_i64, None),
        };
        sqlx::query("UPDATE delivery_state SET state='unknown',lease_owner=NULL,lease_until_us=NULL WHERE tenant=?1 AND environment=?2 AND ((?3=1 AND state<>'rejected') OR (state='leased' AND (?4 IS NULL OR lease_until_us<=?4)))").bind(&i.scope.tenant).bind(&i.scope.environment).bind(restore).bind(expired).execute(&mut *c).await?;
    }
    for (_, d) in &s.items {
        sqlx::query("UPDATE delivery_state SET state=?,attempts=?,next_attempt_us=?,lease_owner=?,generation=?,lease_until_us=?,last_observation=? WHERE tenant=? AND environment=? AND intention_id=?").bind(d.state.name()).bind(d.attempts).bind(d.next_attempt_us).bind(&d.owner).bind(d.generation).bind(d.until).bind(&d.last_observation).bind(&i.scope.tenant).bind(&i.scope.environment).bind(&d.intention_id).execute(&mut *c).await?;
    }
    if let Some((id, hash, body)) = &m.quarantine {
        sqlx::query("INSERT INTO delivery_quarantines (tenant,environment,intention_id,digest,canonical_bytes) VALUES (?1,?2,?3,?4,?5)").bind(&i.scope.tenant).bind(&i.scope.environment).bind(id).bind(hash).bind(body).execute(&mut *c).await?;
    }
    if let Some((id, attempt, started, hash, body)) = &m.attempt {
        sqlx::query("INSERT INTO dispatch_attempts (tenant,environment,intention_id,attempt,generation,started_us,request_hash,canonical_bytes) VALUES (?,?,?,?,?,?,?,?)").bind(&i.scope.tenant).bind(&i.scope.environment).bind(id).bind(attempt).bind(h.generation).bind(started).bind(hash).bind(body).execute(&mut *c).await?;
    }
    sqlx::query(
        "INSERT INTO delivery_observations (sequence,generation,canonical_bytes) VALUES (?,?,?)",
    )
    .bind(h.revision)
    .bind(i.generation)
    .bind(&m.observation)
    .execute(&mut *c)
    .await?;
    if let Some((digest, body)) = &m.report {
        sqlx::query("INSERT INTO reconciliation_reports (digest,generation,canonical_bytes,sequence) VALUES (?,?,?,?) ON CONFLICT (digest) DO NOTHING").bind(digest).bind(i.generation).bind(body).bind(h.revision).execute(&mut *c).await?;
    }
    Ok(())
}
