use crate::{
    outbox::{Delivery, Head, Intention, Mutation, Snapshot, State},
    store::errors::StoreError,
};
use tokio_postgres::GenericClient;
pub(super) async fn read<C: GenericClient + Sync>(c: &C) -> Result<Snapshot, StoreError> {
    c.query_one(
        "SELECT singleton FROM ledgerlab.installation WHERE singleton=1 FOR UPDATE",
        &[],
    )
    .await?;
    let installation = super::read::installation(c).await?;
    let r=c.query_one("SELECT owner,generation,lease_until_us,enabled,revision FROM ledgerlab.dispatcher_head WHERE singleton=1 FOR UPDATE", &[]).await?;
    let head = Head {
        owner: r.try_get(0)?,
        generation: r.try_get(1)?,
        until: r.try_get(2)?,
        enabled: r.try_get::<_, i64>(3)? != 0,
        revision: r.try_get(4)?,
    };
    let sizes = c.query_one("SELECT COUNT(*),COALESCE(SUM(octet_length(canonical_bytes)),0) FROM ledgerlab.intentions WHERE tenant=$1 AND environment=$2", &[&installation.scope.tenant,&installation.scope.environment]).await?;
    if sizes.try_get::<_, i64>(0)? > 1000 || sizes.try_get::<_, i64>(1)? > 8 * 1024 * 1024 {
        return Err(StoreError::InvalidStore("outbox scan limit"));
    }
    let rows=c.query("SELECT i.id,i.destination_id,i.idempotency_key,i.canonical_bytes,i.content_hash,d.state,d.attempts,d.next_attempt_us,d.lease_owner,d.generation,d.lease_until_us,d.last_observation FROM ledgerlab.intentions i LEFT JOIN ledgerlab.delivery_state d ON d.tenant=i.tenant AND d.environment=i.environment AND d.intention_id=i.id WHERE i.tenant=$1 AND i.environment=$2 ORDER BY i.id LIMIT 1001", &[&installation.scope.tenant,&installation.scope.environment]).await?;
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
                state: State::parse(&r.try_get::<_, String>(5)?)?,
                attempts: r.try_get(6)?,
                next_attempt_us: r.try_get(7)?,
                owner: r.try_get(8)?,
                generation: r.try_get(9)?,
                until: r.try_get(10)?,
                last_observation: r.try_get(11)?,
            },
        ));
    }
    let report=c.query_opt("SELECT digest,canonical_bytes FROM ledgerlab.reconciliation_reports ORDER BY sequence DESC LIMIT 1", &[]).await?;
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
pub(super) async fn write<C: GenericClient + Sync>(c: &C, m: &Mutation) -> Result<(), StoreError> {
    let s = &m.snapshot;
    let h = &s.head;
    let i = &s.installation;
    c.execute("UPDATE ledgerlab.installation SET generation=$1,dispatch_hold=$2,dispatch_enabled=$3 WHERE singleton=1", &[&i.generation,&i64::from(i.dispatch_hold),&i64::from(i.dispatch_enabled)]).await?;
    c.execute("UPDATE ledgerlab.dispatcher_head SET owner=$1,generation=$2,lease_until_us=$3,enabled=$4,revision=$5 WHERE singleton=1", &[&h.owner,&h.generation,&h.until,&i64::from(h.enabled),&h.revision]).await?;
    for (_, d) in &s.items {
        c.execute("UPDATE ledgerlab.delivery_state SET state=$1,attempts=$2,next_attempt_us=$3,lease_owner=$4,generation=$5,lease_until_us=$6,last_observation=$7 WHERE tenant=$8 AND environment=$9 AND intention_id=$10", &[&d.state.name(),&d.attempts,&d.next_attempt_us,&d.owner,&d.generation,&d.until,&d.last_observation,&i.scope.tenant,&i.scope.environment,&d.intention_id]).await?;
    }
    if let Some((id, attempt, started, hash, body)) = &m.attempt {
        c.execute("INSERT INTO ledgerlab.dispatch_attempts (tenant,environment,intention_id,attempt,generation,started_us,request_hash,canonical_bytes) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)", &[&i.scope.tenant,&i.scope.environment,&id,&attempt,&h.generation,&started,&hash,&body]).await?;
    }
    c.execute("INSERT INTO ledgerlab.delivery_observations (sequence,generation,canonical_bytes) VALUES ($1,$2,$3)", &[&h.revision,&i.generation,&m.observation]).await?;
    if let Some((digest, body)) = &m.report {
        c.execute("INSERT INTO ledgerlab.reconciliation_reports (digest,generation,canonical_bytes,sequence) VALUES ($1,$2,$3,$4) ON CONFLICT (digest) DO NOTHING", &[&digest,&i.generation,&body,&h.revision]).await?;
    }
    Ok(())
}
