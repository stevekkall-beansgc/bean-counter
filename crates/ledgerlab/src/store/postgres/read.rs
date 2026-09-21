use crate::store::{errors::StoreError, records::*};
use tokio_postgres::GenericClient;

pub(super) async fn identity<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    source: &str,
    external: &str,
) -> Result<Option<StoredIdentity>, StoreError> {
    let row=c.query_opt("SELECT k.canonical_event_id,k.ingress_hash,r.canonical_bytes,r.content_hash,k.canonical_bytes,k.content_hash FROM ledgerlab.delivery_keys k JOIN ledgerlab.accepted_receipts r ON r.tenant=k.tenant AND r.environment=k.environment AND r.event_id=k.canonical_event_id WHERE k.tenant=$1 AND k.environment=$2 AND k.source=$3 AND k.external_id=$4", &[&(&s.tenant),&(&s.environment),&(source),&(external)]).await?;
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
pub(super) async fn claim<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    source: &str,
    operation: &str,
    kind: &str,
    token: &str,
) -> Result<Option<StoredClaim>, StoreError> {
    let row=c.query_opt("SELECT c.event_id,c.facts_hash,r.canonical_bytes,r.content_hash FROM ledgerlab.claims c JOIN ledgerlab.accepted_receipts r ON r.tenant=c.tenant AND r.environment=c.environment AND r.event_id=c.event_id WHERE c.tenant=$1 AND c.environment=$2 AND c.source=$3 AND c.operation_id=$4 AND c.kind=$5 AND c.token=$6", &[&(&s.tenant),&(&s.environment),&(source),&(operation),&(kind),&(token)]).await?;
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
pub(super) async fn chain<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    id: &str,
) -> Result<Option<Chain>, StoreError> {
    let row=c.query_opt("SELECT id,customer,currency,scale,binding_set_doc,context_doc,revision,event_count FROM ledgerlab.chains WHERE tenant=$1 AND environment=$2 AND id=$3 FOR UPDATE", &[&(&s.tenant),&(&s.environment),&(id)]).await?;
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
pub(super) async fn installation<C: GenericClient + Sync>(
    c: &C,
) -> Result<Installation, StoreError> {
    let r=c.query_one("SELECT tenant,environment,logical_store_id,mode,admission,dispatch_hold,dispatch_enabled,generation FROM ledgerlab.installation WHERE singleton=1 FOR SHARE", &[]).await?;
    Ok(Installation {
        scope: Scope {
            tenant: r.try_get(0)?,
            environment: r.try_get(1)?,
        },
        logical_store_id: r.try_get(2)?,
        mode: r.try_get(3)?,
        admission: r.try_get(4)?,
        dispatch_hold: r.try_get::<_, i64>(5)? != 0,
        dispatch_enabled: r.try_get::<_, i64>(6)? != 0,
        generation: r.try_get(7)?,
    })
}
pub(super) async fn document<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    id: &str,
) -> Result<Option<(String, CanonicalRecord)>, StoreError> {
    let row=c.query_opt("SELECT kind,canonical_bytes,content_hash FROM ledgerlab.documents WHERE tenant=$1 AND environment=$2 AND id=$3", &[&s.tenant,&s.environment,&id]).await?;
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
pub(super) async fn authority<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    id: &str,
) -> Result<Option<AuthorityHead>, StoreError> {
    let row=c.query_opt("SELECT grant_id,revision,active FROM ledgerlab.authority_heads WHERE tenant=$1 AND environment=$2 AND id=$3 FOR SHARE", &[&(&s.tenant),&(&s.environment),&(id)]).await?;
    row.map(|r| {
        Ok(AuthorityHead {
            scope: s.clone(),
            id: id.into(),
            grant_id: r.try_get(0)?,
            revision: r.try_get(1)?,
            active: r.try_get::<_, i64>(2)? != 0,
        })
    })
    .transpose()
}
pub(super) async fn binding<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    id: &str,
) -> Result<Option<BindingHead>, StoreError> {
    let row=c.query_opt("SELECT selector_doc,binding_id,revision,active FROM ledgerlab.binding_heads WHERE tenant=$1 AND environment=$2 AND id=$3 FOR SHARE", &[&(&s.tenant),&(&s.environment),&(id)]).await?;
    row.map(|r| {
        Ok(BindingHead {
            scope: s.clone(),
            id: id.into(),
            selector_doc: r.try_get(0)?,
            binding_id: r.try_get(1)?,
            revision: r.try_get(2)?,
            active: r.try_get::<_, i64>(3)? != 0,
        })
    })
    .transpose()
}
pub(super) async fn grant_document<C: GenericClient + Sync>(
    c: &C,
    s: &Scope,
    id: &str,
) -> Result<Option<String>, StoreError> {
    c.query_opt("SELECT grant_doc FROM ledgerlab.source_grants WHERE tenant=$1 AND environment=$2 AND id=$3", &[&s.tenant,&s.environment,&id]).await?.map(|r|r.try_get(0).map_err(StoreError::from)).transpose()
}
