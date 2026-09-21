use crate::store::{errors::StoreError, records::*};
use sqlx::{Row, SqliteConnection};

pub(super) async fn identity(
    c: &mut SqliteConnection,
    s: &Scope,
    source: &str,
    external: &str,
) -> Result<Option<StoredIdentity>, StoreError> {
    let row=sqlx::query("SELECT k.canonical_event_id,k.ingress_hash,r.canonical_bytes,r.content_hash,k.canonical_bytes,k.content_hash FROM delivery_keys k JOIN accepted_receipts r ON r.tenant=k.tenant AND r.environment=k.environment AND r.event_id=k.canonical_event_id WHERE k.tenant=? AND k.environment=? AND k.source=? AND k.external_id=?")
        .bind(&s.tenant).bind(&s.environment).bind(source).bind(external).fetch_optional(c).await?;
    row.map(|r| {
        Ok(StoredIdentity {
            key: CanonicalRecord {
                canonical_bytes: r.try_get(4)?,
                content_hash: r.try_get(5)?,
            },
            event_id: r.try_get(0)?,
            ingress_hash: r.try_get(1)?,
            receipt: CanonicalRecord {
                canonical_bytes: r.try_get(2)?,
                content_hash: r.try_get(3)?,
            },
        })
    })
    .transpose()
}
pub(super) async fn claim(
    c: &mut SqliteConnection,
    s: &Scope,
    source: &str,
    operation: &str,
    kind: &str,
    token: &str,
) -> Result<Option<StoredClaim>, StoreError> {
    let row=sqlx::query("SELECT c.event_id,c.facts_hash,r.canonical_bytes,r.content_hash FROM claims c JOIN accepted_receipts r ON r.tenant=c.tenant AND r.environment=c.environment AND r.event_id=c.event_id WHERE c.tenant=? AND c.environment=? AND c.source=? AND c.operation_id=? AND c.kind=? AND c.token=?")
        .bind(&s.tenant).bind(&s.environment).bind(source).bind(operation).bind(kind).bind(token).fetch_optional(c).await?;
    row.map(|r| {
        Ok(StoredClaim {
            event_id: r.try_get(0)?,
            facts_hash: r.try_get(1)?,
            receipt: CanonicalRecord {
                canonical_bytes: r.try_get(2)?,
                content_hash: r.try_get(3)?,
            },
        })
    })
    .transpose()
}
pub(super) async fn chain(
    c: &mut SqliteConnection,
    s: &Scope,
    id: &str,
) -> Result<Option<Chain>, StoreError> {
    let row=sqlx::query("SELECT id,customer,currency,scale,binding_set_doc,context_doc,revision,event_count FROM chains WHERE tenant=? AND environment=? AND id=?").bind(&s.tenant).bind(&s.environment).bind(id).fetch_optional(c).await?;
    row.map(|r| {
        Ok(Chain {
            scope: s.clone(),
            id: r.try_get(0)?,
            customer: r.try_get(1)?,
            currency: r.try_get(2)?,
            scale: r.try_get(3)?,
            binding_set_doc: r.try_get(4)?,
            context_doc: r.try_get(5)?,
            revision: r.try_get(6)?,
            event_count: r.try_get(7)?,
        })
    })
    .transpose()
}
pub(super) async fn installation(c: &mut SqliteConnection) -> Result<Installation, StoreError> {
    let r=sqlx::query("SELECT tenant,environment,logical_store_id,mode,admission,dispatch_hold,dispatch_enabled,generation FROM installation WHERE singleton=1").fetch_one(c).await?;
    Ok(Installation {
        scope: Scope {
            tenant: r.try_get(0)?,
            environment: r.try_get(1)?,
        },
        logical_store_id: r.try_get(2)?,
        mode: r.try_get(3)?,
        admission: r.try_get(4)?,
        dispatch_hold: r.try_get(5)?,
        dispatch_enabled: r.try_get(6)?,
        generation: r.try_get(7)?,
    })
}
pub(super) async fn document(
    c: &mut SqliteConnection,
    s: &Scope,
    id: &str,
) -> Result<Option<(String, CanonicalRecord)>, StoreError> {
    let row=sqlx::query("SELECT kind,canonical_bytes,content_hash FROM documents WHERE tenant=? AND environment=? AND id=?").bind(&s.tenant).bind(&s.environment).bind(id).fetch_optional(c).await?;
    row.map(|r| {
        Ok((
            r.try_get(0)?,
            CanonicalRecord {
                canonical_bytes: r.try_get(1)?,
                content_hash: r.try_get(2)?,
            },
        ))
    })
    .transpose()
}
pub(super) async fn authority(
    c: &mut SqliteConnection,
    s: &Scope,
    id: &str,
) -> Result<Option<AuthorityHead>, StoreError> {
    let row=sqlx::query("SELECT grant_id,revision,active FROM authority_heads WHERE tenant=? AND environment=? AND id=?").bind(&s.tenant).bind(&s.environment).bind(id).fetch_optional(c).await?;
    row.map(|r| {
        Ok(AuthorityHead {
            scope: s.clone(),
            id: id.into(),
            grant_id: r.try_get(0)?,
            revision: r.try_get(1)?,
            active: r.try_get(2)?,
        })
    })
    .transpose()
}
pub(super) async fn binding(
    c: &mut SqliteConnection,
    s: &Scope,
    id: &str,
) -> Result<Option<BindingHead>, StoreError> {
    let row=sqlx::query("SELECT selector_doc,binding_id,revision,active FROM binding_heads WHERE tenant=? AND environment=? AND id=?").bind(&s.tenant).bind(&s.environment).bind(id).fetch_optional(c).await?;
    row.map(|r| {
        Ok(BindingHead {
            scope: s.clone(),
            id: id.into(),
            selector_doc: r.try_get(0)?,
            binding_id: r.try_get(1)?,
            revision: r.try_get(2)?,
            active: r.try_get(3)?,
        })
    })
    .transpose()
}
#[cfg(test)]
pub(super) async fn delivery(
    c: &mut SqliteConnection,
    s: &Scope,
    id: &str,
) -> Result<Option<StoredDelivery>, StoreError> {
    let row=sqlx::query("SELECT state,attempts,generation,next_attempt_us,lease_owner,lease_until_us,last_observation FROM delivery_state WHERE tenant=? AND environment=? AND intention_id=?").bind(&s.tenant).bind(&s.environment).bind(id).fetch_optional(c).await?;
    row.map(|r| {
        Ok(StoredDelivery {
            state: r.try_get(0)?,
            attempts: r.try_get(1)?,
            generation: r.try_get(2)?,
            next_attempt_us: r.try_get(3)?,
            lease_owner: r.try_get(4)?,
            lease_until_us: r.try_get(5)?,
            last_observation: r.try_get(6)?,
        })
    })
    .transpose()
}

pub(super) async fn grant_document(
    c: &mut SqliteConnection,
    s: &Scope,
    id: &str,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(
        "SELECT grant_doc FROM source_grants WHERE tenant=? AND environment=? AND id=?",
    )
    .bind(&s.tenant)
    .bind(&s.environment)
    .bind(id)
    .fetch_optional(c)
    .await?)
}
