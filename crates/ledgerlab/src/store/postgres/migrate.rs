//! Explicit owner-only initialization. Open never migrates or repairs schemas.
use crate::store::{errors::StoreError, records::Installation};
use tokio_postgres::{Client, GenericClient};
pub(crate) const SQL: &str = include_str!("../../../migrations/postgres/0001_first_slice.sql");
fn checksum() -> String {
    // A framed digest of the exact migration text, independent of economic records.
    ledgerlab_core::canonical::hash(ledgerlab_core::canonical::Domain::Document, &SQL)
        .expect("static SQL text")
}
#[allow(dead_code)] // Explicit migration-owner provisioning; never run by open.
pub(crate) async fn create(
    client: &mut Client,
    installation: Installation,
    runtime_role: &str,
) -> Result<(), StoreError> {
    if runtime_role.is_empty()
        || !runtime_role
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(StoreError::InvalidStore("invalid runtime role identifier"));
    }
    super::verify_server(client).await?;
    let tx = client.transaction().await?;
    tx.batch_execute(
        "SELECT pg_advisory_xact_lock(714215261); REVOKE CREATE ON SCHEMA public FROM PUBLIC;",
    )
    .await?;
    tx.batch_execute(SQL).await?;
    super::write::operation(
        &tx,
        &crate::store::records::WriteOp::SeedInstallation(installation),
    )
    .await?;
    tx.execute(
        "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES (1,$1)",
        &[&checksum()],
    )
    .await?;
    // Role name above is an identifier with a strict ASCII allowlist; all values
    // in runtime reads/writes are bound parameters. No runtime migration rights.
    tx.batch_execute(&format!("GRANT USAGE ON SCHEMA ledgerlab TO {runtime_role}; GRANT SELECT ON ALL TABLES IN SCHEMA ledgerlab TO {runtime_role}; GRANT INSERT ON ledgerlab.documents,ledgerlab.snapshots,ledgerlab.events,ledgerlab.delivery_keys,ledgerlab.claims,ledgerlab.effects,ledgerlab.actions,ledgerlab.action_sources,ledgerlab.action_dependencies,ledgerlab.explanations,ledgerlab.intentions,ledgerlab.control_transitions,ledgerlab.chain_revisions,ledgerlab.decision_manifests,ledgerlab.accepted_receipts,ledgerlab.delivery_state TO {runtime_role}; GRANT UPDATE (revision,event_count) ON ledgerlab.chains TO {runtime_role}; GRANT UPDATE (generation) ON ledgerlab.installation TO {runtime_role}; GRANT UPDATE (revision) ON ledgerlab.authority_heads,ledgerlab.binding_heads TO {runtime_role};")).await?;
    tx.commit().await?;
    Ok(())
}
pub(crate) async fn verify<C: GenericClient + Sync>(client: &C) -> Result<(), StoreError> {
    let rows = client
        .query(
            "SELECT version,checksum FROM ledgerlab.migration_history ORDER BY version",
            &[],
        )
        .await?;
    if rows.len() != 1
        || rows[0].try_get::<_, i64>(0)? != 1
        || rows[0].try_get::<_, String>(1)? != checksum()
    {
        return Err(StoreError::InvalidStore(
            "PostgreSQL migration checksum mismatch",
        ));
    }
    let temporary:bool=client.query_one("SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' AND c.relkind='r' AND c.relpersistence <> 'p')",&[]).await?.try_get(0)?;
    if temporary {
        return Err(StoreError::InvalidStore(
            "permanent PostgreSQL tables required",
        ));
    }
    let installation = super::read::installation(client).await?;
    if installation.logical_store_id.is_empty() {
        return Err(StoreError::InvalidStore("missing installation identity"));
    }
    Ok(())
}
