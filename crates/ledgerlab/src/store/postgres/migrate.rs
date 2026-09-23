//! Explicit owner-only initialization. Open never migrates or repairs schemas.
use crate::store::{errors::StoreError, records::Installation};
use tokio_postgres::{Client, GenericClient};
pub(crate) const SQL: &str = include_str!("../../../migrations/postgres/0001_first_slice.sql");
const OUTBOX: &str = include_str!("../../../migrations/postgres/0002_outbox.sql");
const SAFETY: &str = include_str!("../../../migrations/postgres/0003_outbox_safety.sql");
const OUTCOMES: &str = include_str!("../../../migrations/postgres/0004_outcomes.sql");
const ADJUDICATION: &str = include_str!("../../../migrations/postgres/0005_phase4.sql");
const PUBLICATION: &str = include_str!("../../../migrations/postgres/0006_publication_witness.sql");
fn publication_checksum() -> String {
    ledgerlab_core::canonical::hash(ledgerlab_core::canonical::Domain::Document, &PUBLICATION)
        .expect("static SQL text")
}
fn adjudication_checksum() -> String {
    ledgerlab_core::canonical::hash(ledgerlab_core::canonical::Domain::Document, &ADJUDICATION)
        .expect("static SQL text")
}

fn outcomes_checksum() -> String {
    ledgerlab_core::canonical::hash(ledgerlab_core::canonical::Domain::Document, &OUTCOMES)
        .expect("static SQL text")
}
fn safety_checksum() -> String {
    ledgerlab_core::canonical::hash(ledgerlab_core::canonical::Domain::Document, &SAFETY)
        .expect("static SQL text")
}
fn outbox_checksum() -> String {
    ledgerlab_core::canonical::hash(ledgerlab_core::canonical::Domain::Document, &OUTBOX)
        .expect("static SQL text")
}
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
    tx.batch_execute(OUTBOX).await?;
    tx.batch_execute(SAFETY).await?;
    tx.batch_execute(OUTCOMES).await?;
    tx.batch_execute(ADJUDICATION).await?;
    tx.batch_execute(PUBLICATION).await?;
    tx.execute(
        "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES (6,$1)",
        &[&publication_checksum()],
    )
    .await?;
    tx.execute(
        "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES (5,$1)",
        &[&adjudication_checksum()],
    )
    .await?;
    tx.execute(
        "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES (4,$1)",
        &[&outcomes_checksum()],
    )
    .await?;
    tx.execute(
        "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES (3,$1)",
        &[&safety_checksum()],
    )
    .await?;
    tx.execute(
        "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES (2,$1)",
        &[&outbox_checksum()],
    )
    .await?;
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
    grant_base_runtime(&tx, runtime_role).await?;
    grant_outcomes(&tx, runtime_role).await?;
    grant_adjudication(&tx, runtime_role).await?;
    grant_publication(&tx, runtime_role).await?;
    tx.batch_execute(&format!("GRANT UPDATE ON ledgerlab.delivery_state,ledgerlab.dispatcher_head TO {runtime_role}; GRANT UPDATE (dispatch_hold,dispatch_enabled) ON ledgerlab.installation TO {runtime_role}; GRANT INSERT ON ledgerlab.dispatch_attempts,ledgerlab.delivery_observations,ledgerlab.reconciliation_reports,ledgerlab.delivery_quarantines TO {runtime_role};")).await?;
    tx.commit().await?;
    Ok(())
}
pub(crate) async fn verify<C: GenericClient + Sync>(client: &C) -> Result<(), StoreError> {
    if version(client).await? != 6 {
        return Err(StoreError::InvalidStore(
            "unsupported PostgreSQL write schema",
        ));
    }
    Ok(())
}
async fn version<C: GenericClient + Sync>(client: &C) -> Result<i64, StoreError> {
    let rows = client
        .query(
            "SELECT version,checksum FROM ledgerlab.migration_history ORDER BY version",
            &[],
        )
        .await?;
    let checksums = [
        checksum(),
        outbox_checksum(),
        safety_checksum(),
        outcomes_checksum(),
        adjudication_checksum(),
        publication_checksum(),
    ];
    if rows.is_empty() || rows.len() > 6 {
        return Err(StoreError::InvalidStore(
            "PostgreSQL migration checksum mismatch",
        ));
    }
    for (i, row) in rows.iter().enumerate() {
        if row.try_get::<_, i64>(0)? != i as i64 + 1 || row.try_get::<_, String>(1)? != checksums[i]
        {
            return Err(StoreError::InvalidStore(
                "PostgreSQL migration checksum mismatch",
            ));
        }
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
    Ok(rows.len() as i64)
}

pub(crate) async fn upgrade(
    config: super::PostgresConfig,
    expected_id: &str,
    role: &str,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    let mut session = config.connect().await?;
    let result = upgrade_client(
        &mut session.client,
        expected_id,
        role,
        #[cfg(test)]
        false,
    )
    .await;
    session.discard().await;
    result
}
async fn upgrade_client(
    client: &mut Client,
    expected_id: &str,
    role: &str,
    #[cfg(test)] lose_ack: bool,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    use crate::maintenance::{UpgradeError, UpgradeResult};
    if role.is_empty()
        || role.len() > 63
        || !role
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(UpgradeError::Refused);
    }
    super::verify_server(client).await?;
    let tx = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::Serializable)
        .start()
        .await?;
    tx.batch_execute("SET LOCAL statement_timeout='10s'; SET LOCAL lock_timeout='500ms'; SET LOCAL idle_in_transaction_session_timeout='10s'; SELECT pg_advisory_xact_lock(714215261);").await?;
    // Match application lock order: installation first, dispatcher second.
    tx.query_one(
        "SELECT singleton FROM ledgerlab.installation WHERE singleton=1 FOR UPDATE",
        &[],
    )
    .await?;
    let installation = super::read::installation(&tx).await?;
    let stopped: bool = tx.query_one("SELECT owner IS NULL AND lease_until_us IS NULL AND enabled=0 FROM ledgerlab.dispatcher_head WHERE singleton=1 FOR UPDATE", &[]).await?.get(0);
    if expected_id.is_empty()
        || installation.logical_store_id != expected_id
        || installation.admission != "frozen"
        || !installation.dispatch_hold
        || installation.dispatch_enabled
        || !stopped
    {
        return Err(UpgradeError::Refused);
    }
    let safe: bool = tx.query_one("SELECT NOT (r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR has_schema_privilege(r.oid,'ledgerlab','CREATE') OR has_schema_privilege(r.oid,'public','CREATE') OR EXISTS(SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' AND (pg_has_role(r.oid,c.relowner,'USAGE') OR (c.relkind='r' AND (has_table_privilege(r.oid,c.oid,'TRUNCATE') OR has_table_privilege(r.oid,c.oid,'DELETE')))))) FROM pg_catalog.pg_roles r WHERE r.rolname=$1", &[&role]).await?.get(0);
    if !safe {
        return Err(UpgradeError::Refused);
    }
    let from = version(&tx).await?;
    if from >= 6 {
        // This entry point has no external owner. A bound installation needs
        // separately coordinated anchored maintenance; never bypass its fence.
        super::require_unbound(&tx).await?;
    }
    for (v, sql, hash) in [
        (2_i64, OUTBOX, outbox_checksum()),
        (3, SAFETY, safety_checksum()),
        (4, OUTCOMES, outcomes_checksum()),
        (5, ADJUDICATION, adjudication_checksum()),
        (6, PUBLICATION, publication_checksum()),
    ] {
        if v > from {
            tx.batch_execute(sql).await?;
            tx.execute(
                "INSERT INTO ledgerlab.migration_history (version,checksum) VALUES ($1,$2)",
                &[&v, &hash],
            )
            .await?;
        }
    }
    // Grant only operational permissions introduced after schema 1; retain all
    // existing runtime grants and never give the runtime migration ownership.
    tx.batch_execute(&format!("GRANT SELECT ON ledgerlab.dispatch_attempts,ledgerlab.delivery_observations,ledgerlab.reconciliation_reports,ledgerlab.delivery_quarantines TO {role}; GRANT INSERT ON ledgerlab.dispatch_attempts,ledgerlab.delivery_observations,ledgerlab.reconciliation_reports,ledgerlab.delivery_quarantines TO {role}; GRANT UPDATE ON ledgerlab.delivery_state,ledgerlab.dispatcher_head TO {role}; GRANT UPDATE (dispatch_hold,dispatch_enabled) ON ledgerlab.installation TO {role};")).await?;
    grant_outcomes(&tx, role).await?;
    grant_adjudication(&tx, role).await?;
    grant_publication(&tx, role).await?;
    verify(&tx).await?;
    tx.commit()
        .await
        .map_err(|_| UpgradeError::OutcomeUnknown)?;
    #[cfg(test)]
    if lose_ack {
        return Err(UpgradeError::OutcomeUnknown);
    }
    Ok(if from == 6 {
        UpgradeResult::AlreadyCurrent
    } else {
        UpgradeResult::Upgraded
    })
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod upgrade_tests;

async fn grant_base_runtime<C: GenericClient + Sync>(
    tx: &C,
    runtime_role: &str,
) -> Result<(), StoreError> {
    tx.batch_execute(&format!("GRANT USAGE ON SCHEMA ledgerlab TO {runtime_role}; GRANT SELECT ON ALL TABLES IN SCHEMA ledgerlab TO {runtime_role}; GRANT INSERT ON ledgerlab.documents,ledgerlab.snapshots,ledgerlab.events,ledgerlab.delivery_keys,ledgerlab.claims,ledgerlab.effects,ledgerlab.actions,ledgerlab.action_sources,ledgerlab.action_dependencies,ledgerlab.explanations,ledgerlab.intentions,ledgerlab.control_transitions,ledgerlab.chain_revisions,ledgerlab.decision_manifests,ledgerlab.accepted_receipts,ledgerlab.delivery_state TO {runtime_role}; GRANT UPDATE (revision,event_count) ON ledgerlab.chains TO {runtime_role}; GRANT UPDATE (generation) ON ledgerlab.installation TO {runtime_role}; GRANT UPDATE (revision) ON ledgerlab.authority_heads,ledgerlab.binding_heads TO {runtime_role};")).await?;
    Ok(())
}

async fn grant_outcomes<C: GenericClient + Sync>(tx: &C, role: &str) -> Result<(), StoreError> {
    tx.batch_execute(&format!("GRANT SELECT ON ledgerlab.outcome_scope_locks,ledgerlab.outcome_heads,ledgerlab.outcome_records,ledgerlab.outcome_members,ledgerlab.outcome_anchors,ledgerlab.outcome_deliveries,ledgerlab.acceptance_delivery_namespace,ledgerlab.outcome_held_intentions TO {role}; GRANT INSERT ON ledgerlab.outcome_scope_locks,ledgerlab.outcome_heads,ledgerlab.outcome_records,ledgerlab.outcome_members,ledgerlab.outcome_anchors,ledgerlab.outcome_deliveries,ledgerlab.outcome_held_intentions TO {role}; GRANT UPDATE (revision,value) ON ledgerlab.outcome_heads TO {role}; GRANT UPDATE (key) ON ledgerlab.outcome_scope_locks TO {role};")).await?;
    Ok(())
}

async fn grant_adjudication<C: GenericClient + Sync>(tx: &C, role: &str) -> Result<(), StoreError> {
    tx.batch_execute(&format!("GRANT SELECT ON ledgerlab.r3_scope_locks,ledgerlab.r3_journals,ledgerlab.r3_storage_profile,ledgerlab.r3_segments,ledgerlab.r3_segment_pages,ledgerlab.r3_objects,ledgerlab.r3_object_pages,ledgerlab.r3_heads,ledgerlab.r3_head_versions,ledgerlab.r3_commands,ledgerlab.r3_namespaces,ledgerlab.r3_deliveries,ledgerlab.r3_index_pages,ledgerlab.r3_index_roots,ledgerlab.r3_held_intentions,ledgerlab.r3_unresolved_work TO {role}; GRANT INSERT ON ledgerlab.r3_scope_locks,ledgerlab.r3_journals,ledgerlab.r3_segments,ledgerlab.r3_segment_pages,ledgerlab.r3_objects,ledgerlab.r3_object_pages,ledgerlab.r3_heads,ledgerlab.r3_head_versions,ledgerlab.r3_commands,ledgerlab.r3_namespaces,ledgerlab.r3_deliveries,ledgerlab.r3_index_pages,ledgerlab.r3_index_roots,ledgerlab.r3_held_intentions TO {role}; GRANT UPDATE (full_key) ON ledgerlab.r3_scope_locks TO {role}; GRANT UPDATE (ordinal,segment,replay_root) ON ledgerlab.r3_journals TO {role}; GRANT UPDATE (revision,value) ON ledgerlab.r3_heads TO {role}; GRANT UPDATE (legacy_used) ON ledgerlab.r3_storage_profile TO {role}; GRANT UPDATE (generation,state,backend_pid,backend_start,journal,delivery,command_hash) ON ledgerlab.r3_unresolved_work TO {role};")).await?;
    Ok(())
}

#[cfg(test)]
#[path = "adjudication_schema_tests.rs"]
mod adjudication_schema_tests;

async fn grant_publication<C: GenericClient + Sync>(tx: &C, role: &str) -> Result<(), StoreError> {
    tx.batch_execute(&format!("GRANT SELECT ON ledgerlab.r3_commit_witness TO {role}; GRANT UPDATE (witness) ON ledgerlab.r3_commit_witness TO {role};")).await?;
    Ok(())
}
