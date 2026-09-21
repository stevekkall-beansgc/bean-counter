use crate::{
    outbox::{Delivery, Head, Intention, Mutation, Query, Snapshot, State, Sweep},
    store::errors::StoreError,
};
use tokio_postgres::GenericClient;
pub(super) async fn read<C: GenericClient + Sync>(
    c: &C,
    query: Query,
) -> Result<Snapshot, StoreError> {
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
    let (after, key, due, limit) = match query {
        Query::Control => (String::new(), None, None, 0_i64),
        Query::Page { after, due } => (after, None, due, crate::outbox::PAGE_SIZE as i64),
        Query::Key(key) => (String::new(), Some(key), None, 1_i64),
    };
    let mut items = Vec::new();
    let rows = c.query("SELECT i.id,octet_length(i.canonical_bytes)::bigint FROM ledgerlab.intentions i LEFT JOIN ledgerlab.delivery_state d ON d.tenant=i.tenant AND d.environment=i.environment AND d.intention_id=i.id LEFT JOIN ledgerlab.delivery_quarantines q ON q.tenant=i.tenant AND q.environment=i.environment AND q.intention_id=i.id WHERE i.tenant=$1 AND i.environment=$2 AND i.id>$3 AND ($4::text IS NULL OR i.id=$4) AND ($5::bigint IS NULL OR (d.state IN ('held','pending','retry') AND d.attempts<20 AND d.next_attempt_us<=$5 AND q.intention_id IS NULL)) ORDER BY i.id LIMIT $6", &[&installation.scope.tenant,&installation.scope.environment,&after,&key,&due,&limit]).await?;
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
        let r = c.query_one("SELECT i.id,i.destination_id,i.idempotency_key,i.canonical_bytes,i.content_hash,d.state,d.attempts,d.next_attempt_us,d.lease_owner,d.generation,d.lease_until_us,d.last_observation,q.digest FROM ledgerlab.intentions i LEFT JOIN ledgerlab.delivery_state d ON d.tenant=i.tenant AND d.environment=i.environment AND d.intention_id=i.id LEFT JOIN ledgerlab.delivery_quarantines q ON q.tenant=i.tenant AND q.environment=i.environment AND q.intention_id=i.id WHERE i.tenant=$1 AND i.environment=$2 AND i.id=$3", &[&installation.scope.tenant,&installation.scope.environment,&id]).await?;

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
                quarantine: r.try_get(12)?,
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
    if let Some(sweep) = &m.sweep {
        let (restore, expired) = match sweep {
            Sweep::Leases => (0_i64, None),
            Sweep::Expired(now) => (0_i64, Some(*now)),
            Sweep::Restore => (1_i64, None),
        };
        c.execute("UPDATE ledgerlab.delivery_state SET state='unknown',lease_owner=NULL,lease_until_us=NULL WHERE tenant=$1 AND environment=$2 AND (($3::bigint=1 AND state<>'rejected') OR (state='leased' AND ($4::bigint IS NULL OR lease_until_us<=$4)))", &[&i.scope.tenant,&i.scope.environment,&restore,&expired]).await?;
    }
    for (_, d) in &s.items {
        c.execute("UPDATE ledgerlab.delivery_state SET state=$1,attempts=$2,next_attempt_us=$3,lease_owner=$4,generation=$5,lease_until_us=$6,last_observation=$7 WHERE tenant=$8 AND environment=$9 AND intention_id=$10", &[&d.state.name(),&d.attempts,&d.next_attempt_us,&d.owner,&d.generation,&d.until,&d.last_observation,&i.scope.tenant,&i.scope.environment,&d.intention_id]).await?;
    }
    if let Some((id, hash, body)) = &m.quarantine {
        c.execute("INSERT INTO ledgerlab.delivery_quarantines (tenant,environment,intention_id,digest,canonical_bytes) VALUES ($1,$2,$3,$4,$5)", &[&i.scope.tenant,&i.scope.environment,&id,&hash,&body]).await?;
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
