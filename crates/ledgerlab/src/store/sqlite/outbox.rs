use crate::{
    outbox::{Delivery, Head, Intention, Mutation, Snapshot, State},
    store::errors::StoreError,
};
use sqlx::{Row, SqliteConnection};
pub(super) async fn read(c: &mut SqliteConnection) -> Result<Snapshot, StoreError> {
    let installation = super::read::installation(c).await?;
    let r=sqlx::query("SELECT owner,generation,lease_until_us,enabled,revision FROM dispatcher_head WHERE singleton=1").fetch_one(&mut *c).await?;
    let head = Head {
        owner: r.try_get(0)?,
        generation: r.try_get(1)?,
        until: r.try_get(2)?,
        enabled: r.try_get::<i64, _>(3)? != 0,
        revision: r.try_get(4)?,
    };
    let sizes = sqlx::query("SELECT COUNT(*),COALESCE(SUM(length(canonical_bytes)),0) FROM intentions WHERE tenant=? AND environment=?").bind(&installation.scope.tenant).bind(&installation.scope.environment).fetch_one(&mut *c).await?;
    if sizes.try_get::<i64, _>(0)? > 1000 || sizes.try_get::<i64, _>(1)? > 8 * 1024 * 1024 {
        return Err(StoreError::InvalidStore("outbox scan limit"));
    }
    let rows=sqlx::query("SELECT i.id,i.destination_id,i.idempotency_key,i.canonical_bytes,i.content_hash,d.state,d.attempts,d.next_attempt_us,d.lease_owner,d.generation,d.lease_until_us,d.last_observation FROM intentions i LEFT JOIN delivery_state d ON d.tenant=i.tenant AND d.environment=i.environment AND d.intention_id=i.id WHERE i.tenant=? AND i.environment=? ORDER BY i.id LIMIT 1001").bind(&installation.scope.tenant).bind(&installation.scope.environment).fetch_all(&mut *c).await?;
    let mut items = Vec::new();
    for r in rows {
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
    for (_, d) in &s.items {
        sqlx::query("UPDATE delivery_state SET state=?,attempts=?,next_attempt_us=?,lease_owner=?,generation=?,lease_until_us=?,last_observation=? WHERE tenant=? AND environment=? AND intention_id=?").bind(d.state.name()).bind(d.attempts).bind(d.next_attempt_us).bind(&d.owner).bind(d.generation).bind(d.until).bind(&d.last_observation).bind(&i.scope.tenant).bind(&i.scope.environment).bind(&d.intention_id).execute(&mut *c).await?;
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
