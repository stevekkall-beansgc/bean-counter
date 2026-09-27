use crate::store::errors::StoreError;
use sha2::{Digest, Sha256};
use sqlx::{
    migrate::{Migration, MigrationType, Migrator},
    SqlSafeStr, SqliteConnection,
};
const SQL: &str = include_str!("../../../migrations/sqlite/0001_first_slice.sql");
const OUTBOX: &str = include_str!("../../../migrations/sqlite/0002_outbox.sql");
const SAFETY: &str = include_str!("../../../migrations/sqlite/0003_outbox_safety.sql");
const OUTCOMES: &str = include_str!("../../../migrations/sqlite/0004_outcomes.sql");
const PHASE4: &str = include_str!("../../../migrations/sqlite/0005_phase4.sql");
const READER_BOUNDS: &str = include_str!("../../../migrations/sqlite/0006_r3_reader_bounds.sql");
const BILLING: &str = include_str!("../../../migrations/sqlite/0007_local_billing.sql");
const BILLING_ALIASES: &str = include_str!("../../../migrations/sqlite/0008_billing_aliases.sql");
const CUSTOMERS: &str = include_str!("../../../migrations/sqlite/0009_customer_agreements.sql");
const M3_HISTORY: &str = include_str!("../../../migrations/sqlite/0010_m3_history.sql");
fn migrator() -> Migrator {
    Migrator::with_migrations(vec![
        Migration::new(
            1,
            "first slice".into(),
            MigrationType::Simple,
            SQL.into_sql_str(),
            false,
        ),
        Migration::new(
            2,
            "outbox".into(),
            MigrationType::Simple,
            OUTBOX.into_sql_str(),
            false,
        ),
        Migration::new(
            3,
            "outbox safety".into(),
            MigrationType::Simple,
            SAFETY.into_sql_str(),
            false,
        ),
        Migration::new(
            4,
            "outcomes".into(),
            MigrationType::Simple,
            OUTCOMES.into_sql_str(),
            false,
        ),
        Migration::new(
            5,
            "phase4".into(),
            MigrationType::Simple,
            PHASE4.into_sql_str(),
            false,
        ),
        Migration::new(
            6,
            "R3 native reader bounds".into(),
            MigrationType::Simple,
            READER_BOUNDS.into_sql_str(),
            false,
        ),
        Migration::new(
            7,
            "local retail billing".into(),
            MigrationType::Simple,
            BILLING.into_sql_str(),
            false,
        ),
        Migration::new(
            8,
            "billing delivery aliases".into(),
            MigrationType::Simple,
            BILLING_ALIASES.into_sql_str(),
            false,
        ),
        Migration::new(
            9,
            "customer agreements".into(),
            MigrationType::Simple,
            CUSTOMERS.into_sql_str(),
            false,
        ),
        Migration::new(
            10,
            "M3 billing history".into(),
            MigrationType::Simple,
            M3_HISTORY.into_sql_str(),
            false,
        ),
    ])
}
#[allow(dead_code)] // Explicit migration-owner provisioning; never run by open.
pub(super) async fn create(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    migrator().run(conn).await?;
    Ok(())
}
pub(super) async fn verify(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let current: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    // Keep the shared non-billing facade open on the supported schema-9
    // predecessor and current schema 10. Billing-specific reads and writes
    // enforce schema 10; the ordinary opener never runs M3 migration implicitly.
    if current != 9 && current != 10 {
        return Err(StoreError::InvalidStore("unsupported SQLite write schema"));
    }
    version(conn).await?;
    Ok(())
}
async fn version(conn: &mut SqliteConnection) -> Result<i64, StoreError> {
    let v: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if !(1..=10).contains(&v) {
        return Err(StoreError::InvalidStore("unsupported SQLite write schema"));
    }
    let migrations = migrator();
    let rows: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(conn)
            .await?;
    if rows.len() != v as usize
        || rows.iter().enumerate().any(|(i, row)| {
            row.0 != (i + 1) as i64
                || !row.1
                || row.2.as_slice() != migrations.migrations[i].checksum.as_ref()
        })
    {
        return Err(StoreError::InvalidStore(
            "SQLite migration checksum mismatch",
        ));
    }
    Ok(v)
}

pub(crate) async fn upgrade(
    path: &std::path::Path,
    expected_id: &str,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    use crate::maintenance::UpgradeError;
    use sqlx::Connection;
    let owner = super::owner::Owner::acquire(path)?;
    let mut conn = super::connect::initial(&owner).await?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        owner.verify_path()?;
        super::connect::verify(&mut conn, false).await?;
        upgrade_connection(
            &mut conn,
            expected_id,
            #[cfg(test)]
            false,
        )
        .await
    })
    .await
    .unwrap_or(Err(UpgradeError::OutcomeUnknown));
    // Drain queued rollback before releasing the OS owner, including timeout.
    if conn.close().await.is_err() {
        return Err(UpgradeError::OutcomeUnknown);
    }
    result
}

async fn upgrade_connection(
    conn: &mut SqliteConnection,
    expected_id: &str,
    #[cfg(test)] lose_ack: bool,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    use crate::maintenance::{UpgradeError, UpgradeResult};
    use sqlx::Connection;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let from = version(&mut tx).await?;
    if from > 8 {
        return Err(UpgradeError::Refused);
    }
    let installation = super::read::installation(&mut tx).await?;
    let stopped: bool = sqlx::query_scalar("SELECT owner IS NULL AND lease_until_us IS NULL AND enabled=0 FROM dispatcher_head WHERE singleton=1")
        .fetch_one(&mut *tx).await?;
    if expected_id.is_empty()
        || installation.logical_store_id != expected_id
        || installation.admission != "frozen"
        || !installation.dispatch_hold
        || installation.dispatch_enabled
        || !stopped
    {
        return Err(UpgradeError::Refused);
    }
    super::connect::integrity(&mut tx).await?;
    // The historical frozen-store path must never activate billing schema 9.
    for migration in migrator()
        .iter()
        .filter(|m| m.version > from && m.version <= 8)
    {
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_ref()))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO _sqlx_migrations (version,description,success,checksum,execution_time) VALUES (?,?,TRUE,?,0)")
            .bind(migration.version).bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref()).execute(&mut *tx).await?;
    }
    if version(&mut tx).await? != 8 {
        return Err(UpgradeError::Refused);
    }
    super::connect::integrity(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|_| UpgradeError::OutcomeUnknown)?;
    #[cfg(test)]
    if lose_ack {
        return Err(UpgradeError::OutcomeUnknown);
    }
    Ok(if from == 8 {
        UpgradeResult::AlreadyCurrent
    } else {
        UpgradeResult::Upgraded
    })
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod upgrade_tests;

/// Mechanical seed already validated by the billing coordinator. Canonical
/// setup bytes and primitive identity fields are checked again under the owner
/// and write transaction; the store never evaluates terms or authority.
#[derive(Clone, Debug)]
pub(crate) struct BillingUpgradeSeed {
    pub store_id: String,
    pub tenant: String,
    pub environment: String,
    pub customer: String,
    pub source: String,
    pub agreement_id: String,
    pub effective_at_us: i64,
    pub recorded_at_us: i64,
    pub setup_bytes: Vec<u8>,
    /// Digest from `preflight_billing`, supplied after coordinator validation.
    pub snapshot_digest: [u8; 32],
    /// The schema the digest was taken from, retained so a retry after an
    /// unknown commit re-derives exactly the same pre-M3 bytes.
    pub source_version: i64,
    /// The durable target index the coordinator derived from the validated
    /// receipts. Every field is re-checked against the retained row bytes.
    pub index: Vec<BillingUpgradeIndex>,
}

/// One retained decision's derived index row. `target`, `kind` and
/// `accepted_at_us` come from the coordinator's validated receipt decoding; the
/// store only checks that they match the retained row it already holds.
#[derive(Clone, Debug)]
pub(crate) struct BillingUpgradeIndex {
    pub ordinal: i64,
    pub customer: String,
    pub source: String,
    pub external_id: String,
    pub semantic_key: Vec<u8>,
    pub target: String,
    pub kind: String,
    pub accepted_at_us: i64,
}

/// The exact pre-M3 billing history that the billing coordinator must validate
/// before allowing the first schema-10 write. The digest includes row
/// identities, ordering, and original BLOB bytes, not merely the interpreted
/// billing facts.
pub(crate) struct BillingUpgradePreflight {
    pub version: i64,
    /// The schema actually on disk. It differs from `version` only on a
    /// schema-10 store, where `version` names the frozen pre-M3 schema whose
    /// digest a retry must reproduce.
    pub store_version: i64,
    pub snapshot: super::BillingSnapshot,
    pub digest: [u8; 32],
}

/// Read the retained billing history under the exclusive directory owner. This
/// never runs migration SQL. The coordinator validates `snapshot` as a complete
/// pre-M3 history, then supplies `digest` with the upgrade seed.
///
/// A schema-10 store is re-read through the same frozen reader that recorded it,
/// so repeating a seed after an unknown commit compares the identical digest
/// instead of a differently framed one.
#[allow(dead_code)] // Wired by the M2 billing coordinator integration.
pub(crate) async fn preflight_billing(
    path: &std::path::Path,
) -> Result<BillingUpgradePreflight, crate::maintenance::UpgradeError> {
    use crate::maintenance::UpgradeError;
    use sqlx::Connection;
    let owner = super::owner::Owner::acquire(path)?;
    let mut conn = super::connect::initial(&owner).await?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        owner.verify_path()?;
        super::connect::verify(&mut conn, false).await?;
        sqlx::query("PRAGMA query_only=ON")
            .execute(&mut conn)
            .await?;
        let store_version = version(&mut conn).await?;
        if store_version != 8 && store_version != 9 && store_version != 10 {
            return Err(UpgradeError::Refused);
        }
        super::connect::integrity(&mut conn).await?;
        let mut tx = conn.begin().await?;
        let mut snapshot = if store_version == 8 {
            legacy_billing_snapshot(&mut tx).await?
        } else if store_version == 9 {
            schema9_snapshot(&mut tx).await?
        } else {
            m3_billing_snapshot(&mut tx).await?
        };
        snapshot.store_version = store_version;
        tx.commit().await?;
        Ok(snapshot)
    })
    .await
    .unwrap_or(Err(UpgradeError::Refused));
    if conn.close().await.is_err() {
        return Err(UpgradeError::Refused);
    }
    result
}

/// Re-derive the pre-M3 history of an already migrated schema-10 store. The
/// migration never touches the frozen tier rows, so the reader recorded in
/// `billing_m3_upgrade` reproduces the original digest byte for byte. A store
/// created directly at schema 10 has no marker and no pre-M3 history, so it is
/// only reconcilable while every frozen tier is still empty.
async fn m3_billing_snapshot(
    conn: &mut SqliteConnection,
) -> Result<BillingUpgradePreflight, crate::maintenance::UpgradeError> {
    use crate::maintenance::UpgradeError;
    let source: Option<i64> =
        sqlx::query_scalar("SELECT source_version FROM billing_m3_upgrade WHERE singleton=1")
            .fetch_optional(&mut *conn)
            .await?;
    let mut snapshot = match source {
        Some(8) => legacy_billing_snapshot(conn).await?,
        Some(9) => schema9_snapshot(conn).await?,
        Some(_) => return Err(UpgradeError::Refused),
        None => {
            let retained: i64 = sqlx::query_scalar(
                "SELECT (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)+(SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases)",
            )
            .fetch_one(&mut *conn)
            .await?;
            if retained != 0 {
                return Err(UpgradeError::Refused);
            }
            schema9_snapshot(conn).await?
        }
    };
    snapshot.store_version = 10;
    Ok(snapshot)
}

fn hash_bytes(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

fn hash_number(hash: &mut Sha256, value: i64) {
    hash.update(value.to_be_bytes());
}

async fn legacy_billing_snapshot(
    conn: &mut SqliteConnection,
) -> Result<BillingUpgradePreflight, crate::maintenance::UpgradeError> {
    use super::{BillingAlias, BillingEntry, BillingSnapshot};
    use crate::maintenance::UpgradeError;
    let setup: Vec<u8> =
        sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup WHERE singleton=1")
            .fetch_one(&mut *conn)
            .await?;
    let (entry_count, entry_last, entry_size): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),COALESCE(max(ordinal),0),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0) FROM billing_entries",
    )
    .fetch_one(&mut *conn)
    .await?;
    let (alias_count, alias_size): (i64, i64) =
        sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_aliases")
            .fetch_one(&mut *conn)
            .await?;
    let (permission_count, permission_max): (i64, i64) =
        sqlx::query_as("SELECT count(*),COALESCE(max(revision),1) FROM billing_permissions")
            .fetch_one(&mut *conn)
            .await?;
    if setup.is_empty()
        || setup.len() > 65_536
        || entry_count > 1_000
        || entry_count != entry_last
        || entry_size > 33_554_432
        || alias_count > 1_000
        || alias_size > 33_554_432
        || permission_count > 1_000
        || permission_max != permission_count + 1
    {
        return Err(UpgradeError::Refused);
    }
    type EntryRow = (i64, String, String, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
    let entry_rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT ordinal,source,external_id,semantic_key,ingress,facts,bundle FROM billing_entries ORDER BY ordinal",
    )
    .fetch_all(&mut *conn)
    .await?;
    type AliasRow = (String, String, Vec<u8>, i64);
    let alias_rows: Vec<AliasRow> = sqlx::query_as(
        "SELECT source,external_id,ingress,ordinal FROM billing_aliases ORDER BY source,external_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let permission_rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT revision,canonical_bytes FROM billing_permissions ORDER BY revision",
    )
    .fetch_all(&mut *conn)
    .await?;
    if entry_rows.len() != entry_count as usize
        || alias_rows.len() != alias_count as usize
        || permission_rows.len() != permission_count as usize
    {
        return Err(UpgradeError::Refused);
    }
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/billing-schema8-snapshot/v1\0");
    hash_bytes(&mut hash, &setup);
    hash_number(&mut hash, permission_count);
    for (revision, bytes) in &permission_rows {
        hash_number(&mut hash, *revision);
        hash_bytes(&mut hash, bytes);
    }
    hash_number(&mut hash, entry_count);
    for (ordinal, source, external_id, semantic_key, ingress, facts, bundle) in &entry_rows {
        hash_number(&mut hash, *ordinal);
        hash_bytes(&mut hash, source.as_bytes());
        hash_bytes(&mut hash, external_id.as_bytes());
        hash_bytes(&mut hash, semantic_key);
        hash_bytes(&mut hash, ingress);
        hash_bytes(&mut hash, facts);
        hash_bytes(&mut hash, bundle);
    }
    hash_number(&mut hash, alias_count);
    for (source, external_id, ingress, ordinal) in &alias_rows {
        hash_bytes(&mut hash, source.as_bytes());
        hash_bytes(&mut hash, external_id.as_bytes());
        hash_bytes(&mut hash, ingress);
        hash_number(&mut hash, *ordinal);
    }
    let digest = hash.finalize().into();
    Ok(BillingUpgradePreflight {
        version: 8,
        store_version: 8,
        snapshot: BillingSnapshot {
            setup,
            permissions: permission_rows
                .into_iter()
                .map(|(_, bytes)| bytes)
                .collect(),
            scoped_permissions: vec![],
            customers: vec![],
            agreements: vec![],
            controls: vec![],
            entries: entry_rows
                .into_iter()
                .map(
                    |(ordinal, source, external_id, semantic_key, ingress, facts, bundle)| {
                        BillingEntry {
                            ordinal,
                            customer: None,
                            source,
                            external_id,
                            semantic_key,
                            ingress,
                            facts,
                            bundle,
                            accepted_at_us: None,
                            agreement_id: None,
                            agreement_version: None,
                        }
                    },
                )
                .collect(),
            aliases: alias_rows
                .into_iter()
                .map(|(source, external_id, ingress, ordinal)| BillingAlias {
                    customer: None,
                    source,
                    external_id,
                    ingress,
                    ordinal,
                })
                .collect(),
            retained: true,
            indexed: false,
            entry_count,
            alias_count,
            ledger_time_max: None,
            // A pre-M3 store has no durable index yet.
            index: vec![],
        },
        digest,
    })
}

/// The exact schema-9 billing history, read for the explicit schema-9 to
/// schema-10 transition. It is deliberately separate from the frozen schema-8
/// digest above: the M2 qualification contract covers the original v0.4.3
/// schema-8 bytes only, and this reader never re-derives that digest.
async fn schema9_snapshot(
    conn: &mut SqliteConnection,
) -> Result<BillingUpgradePreflight, crate::maintenance::UpgradeError> {
    use super::{BillingAlias, BillingEntry, BillingSnapshot};
    use crate::maintenance::UpgradeError;
    let setup: Vec<u8> =
        sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup WHERE singleton=1")
            .fetch_one(&mut *conn)
            .await?;
    let (entry_count, entry_size): (i64, i64) = sqlx::query_as(
        "SELECT count(*),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0) FROM billing_entries",
    )
    .fetch_one(&mut *conn)
    .await?;
    let (m2_count, m2_size): (i64, i64) = sqlx::query_as(
        "SELECT count(*),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0) FROM billing_m2_entries",
    )
    .fetch_one(&mut *conn)
    .await?;
    let (alias_count, alias_size): (i64, i64) =
        sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_aliases")
            .fetch_one(&mut *conn)
            .await?;
    let (m2_alias_count, m2_alias_size): (i64, i64) =
        sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_m2_aliases")
            .fetch_one(&mut *conn)
            .await?;
    let (permission_count, permission_max): (i64, i64) =
        sqlx::query_as("SELECT count(*),COALESCE(max(revision),1) FROM billing_permissions")
            .fetch_one(&mut *conn)
            .await?;
    let (scoped_count, customer_count, agreement_count, control_count): (i64, i64, i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM billing_m2_permissions),(SELECT count(*) FROM billing_customers),(SELECT count(*) FROM billing_agreements),(SELECT count(*) FROM billing_m2_changes)")
            .fetch_one(&mut *conn)
            .await?;
    if setup.is_empty()
        || setup.len() > 65_536
        || entry_count + m2_count > 1_000
        || entry_size + m2_size > 33_554_432
        || alias_count + m2_alias_count > 1_000
        || alias_size + m2_alias_size > 33_554_432
        || permission_count > 1_000
        || permission_max != permission_count + 1
        || permission_count + scoped_count > 1_000
        || customer_count > 1_000
        || agreement_count > 1_000
        || control_count > 2_000
    {
        return Err(UpgradeError::Refused);
    }
    type EntryRow = (
        i64,
        Option<String>,
        String,
        String,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    );
    let entry_rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT ordinal,NULL,source,external_id,semantic_key,ingress,facts,bundle,NULL,NULL,NULL FROM billing_entries ORDER BY ordinal",
    )
    .fetch_all(&mut *conn)
    .await?;
    let m2_rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version FROM billing_m2_entries ORDER BY ordinal",
    )
    .fetch_all(&mut *conn)
    .await?;
    type AliasRow = (Option<String>, String, String, Vec<u8>, i64);
    // customer, source, revision, transition, agreement_id, agreement_version,
    // effective_at_us, recorded_at_us, setup_bytes
    type AgreementRow = (
        String,
        String,
        i64,
        String,
        String,
        i64,
        i64,
        i64,
        Option<Vec<u8>>,
    );
    // customer, source, change_id, operation, request, response, recorded_at_us
    type ControlRow = (String, String, String, String, Vec<u8>, Vec<u8>, i64);
    let alias_rows: Vec<AliasRow> = sqlx::query_as(
        "SELECT NULL,source,external_id,ingress,ordinal FROM billing_aliases ORDER BY source,external_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let m2_alias_rows: Vec<AliasRow> = sqlx::query_as(
        "SELECT customer,source,external_id,ingress,ordinal FROM billing_m2_aliases ORDER BY customer,source,external_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let permission_rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT revision,canonical_bytes FROM billing_permissions ORDER BY revision",
    )
    .fetch_all(&mut *conn)
    .await?;
    let scoped_rows: Vec<(String, String, i64, Vec<u8>, i64)> = sqlx::query_as(
        "SELECT customer,source,revision,canonical_bytes,recorded_at_us FROM billing_m2_permissions ORDER BY customer,source,revision",
    )
    .fetch_all(&mut *conn)
    .await?;
    let customer_rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT customer,tenant,environment FROM billing_customers ORDER BY customer",
    )
    .fetch_all(&mut *conn)
    .await?;
    let agreement_rows: Vec<AgreementRow> = sqlx::query_as(
        "SELECT customer,source,revision,transition,agreement_id,agreement_version,effective_at_us,recorded_at_us,setup_bytes FROM billing_agreements ORDER BY customer,source,revision",
    )
    .fetch_all(&mut *conn)
    .await?;
    let control_rows: Vec<ControlRow> = sqlx::query_as(
        "SELECT customer,source,change_id,operation,request,response,recorded_at_us FROM billing_m2_changes ORDER BY customer,source,change_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut entries = entry_rows;
    entries.extend(m2_rows);
    entries.sort_by_key(|row| row.0);
    let mut aliases = alias_rows;
    aliases.extend(m2_alias_rows);
    aliases.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    // Ordinals are one continuous installation-wide sequence across the legacy
    // and M2 tables, so contiguity is proved on the combined set rather than per
    // table: a schema-9 store migrated from schema 8 keeps its early ordinals in
    // billing_entries and continues in billing_m2_entries.
    if entries.len() != (entry_count + m2_count) as usize
        || aliases.len() != (alias_count + m2_alias_count) as usize
        || permission_rows.len() != permission_count as usize
        || scoped_rows.len() != scoped_count as usize
        || customer_rows.len() != customer_count as usize
        || agreement_rows.len() != agreement_count as usize
        || control_rows.len() != control_count as usize
        || entries
            .iter()
            .enumerate()
            .any(|(index, row)| row.0 != index as i64 + 1)
    {
        return Err(UpgradeError::Refused);
    }
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/billing-schema9-snapshot/v1\0");
    hash_bytes(&mut hash, &setup);
    for (customer, tenant, environment) in &customer_rows {
        hash_bytes(&mut hash, customer.as_bytes());
        hash_bytes(&mut hash, tenant.as_bytes());
        hash_bytes(&mut hash, environment.as_bytes());
    }
    for row in &agreement_rows {
        hash_bytes(&mut hash, row.0.as_bytes());
        hash_bytes(&mut hash, row.1.as_bytes());
        hash_number(&mut hash, row.2);
        hash_bytes(&mut hash, row.3.as_bytes());
        hash_bytes(&mut hash, row.4.as_bytes());
        hash_number(&mut hash, row.5);
        hash_number(&mut hash, row.6);
        hash_number(&mut hash, row.7);
        hash_bytes(&mut hash, row.8.as_deref().unwrap_or_default());
    }
    for row in &control_rows {
        hash_bytes(&mut hash, row.0.as_bytes());
        hash_bytes(&mut hash, row.1.as_bytes());
        hash_bytes(&mut hash, row.2.as_bytes());
        hash_bytes(&mut hash, row.3.as_bytes());
        hash_bytes(&mut hash, &row.4);
        hash_bytes(&mut hash, &row.5);
        hash_number(&mut hash, row.6);
    }
    for (revision, bytes) in &permission_rows {
        hash_number(&mut hash, *revision);
        hash_bytes(&mut hash, bytes);
    }
    for row in &scoped_rows {
        hash_bytes(&mut hash, row.0.as_bytes());
        hash_bytes(&mut hash, row.1.as_bytes());
        hash_number(&mut hash, row.2);
        hash_bytes(&mut hash, &row.3);
        hash_number(&mut hash, row.4);
    }
    for row in &entries {
        hash_number(&mut hash, row.0);
        hash_bytes(&mut hash, row.1.as_deref().unwrap_or_default().as_bytes());
        hash_bytes(&mut hash, row.2.as_bytes());
        hash_bytes(&mut hash, row.3.as_bytes());
        hash_bytes(&mut hash, &row.4);
        hash_bytes(&mut hash, &row.5);
        hash_bytes(&mut hash, &row.6);
        hash_bytes(&mut hash, &row.7);
        hash_number(&mut hash, row.8.unwrap_or_default());
        hash_bytes(&mut hash, row.9.as_deref().unwrap_or_default().as_bytes());
        hash_number(&mut hash, row.10.unwrap_or_default());
    }
    for row in &aliases {
        hash_bytes(&mut hash, row.0.as_deref().unwrap_or_default().as_bytes());
        hash_bytes(&mut hash, row.1.as_bytes());
        hash_bytes(&mut hash, row.2.as_bytes());
        hash_bytes(&mut hash, &row.3);
        hash_number(&mut hash, row.4);
    }
    let digest = hash.finalize().into();
    Ok(BillingUpgradePreflight {
        version: 9,
        store_version: 9,
        snapshot: BillingSnapshot {
            setup,
            permissions: permission_rows
                .into_iter()
                .map(|(_, bytes)| bytes)
                .collect(),
            scoped_permissions: scoped_rows
                .into_iter()
                .map(
                    |(customer, source, revision, canonical_bytes, recorded_at_us)| {
                        super::billing::BillingScopedPermission {
                            customer,
                            source,
                            revision,
                            canonical_bytes,
                            recorded_at_us,
                        }
                    },
                )
                .collect(),
            customers: customer_rows
                .into_iter()
                .map(|(customer, tenant, environment)| super::BillingCustomer {
                    customer,
                    tenant,
                    environment,
                })
                .collect(),
            agreements: agreement_rows
                .into_iter()
                .map(
                    |(
                        customer,
                        source,
                        revision,
                        transition,
                        agreement_id,
                        agreement_version,
                        effective_at_us,
                        recorded_at_us,
                        setup_bytes,
                    )| {
                        super::BillingAgreement {
                            customer,
                            source,
                            revision,
                            transition,
                            agreement_id,
                            agreement_version,
                            effective_at_us,
                            recorded_at_us,
                            setup: setup_bytes,
                        }
                    },
                )
                .collect(),
            controls: control_rows
                .into_iter()
                .map(
                    |(
                        customer,
                        source,
                        change_id,
                        operation,
                        request,
                        response,
                        recorded_at_us,
                    )| {
                        super::billing::BillingControl {
                            customer,
                            source,
                            change_id,
                            operation,
                            request,
                            response,
                            recorded_at_us,
                        }
                    },
                )
                .collect(),
            aliases: aliases
                .into_iter()
                .map(
                    |(customer, source, external_id, ingress, ordinal)| BillingAlias {
                        customer,
                        source,
                        external_id,
                        ingress,
                        ordinal,
                    },
                )
                .collect(),
            retained: true,
            indexed: false,
            entry_count: entry_count + m2_count,
            alias_count: alias_count + m2_alias_count,
            ledger_time_max: None,
            // A pre-M3 store has no durable index yet.
            index: vec![],
            entries: entries
                .into_iter()
                .map(
                    |(
                        ordinal,
                        customer,
                        source,
                        external_id,
                        semantic_key,
                        ingress,
                        facts,
                        bundle,
                        accepted_at_us,
                        agreement_id,
                        agreement_version,
                    )| {
                        BillingEntry {
                            ordinal,
                            customer,
                            source,
                            external_id,
                            semantic_key,
                            ingress,
                            facts,
                            bundle,
                            accepted_at_us,
                            agreement_id,
                            agreement_version,
                        }
                    },
                )
                .collect(),
        },
        digest,
    })
}

/// Explicit local billing transition to the M3 schema. Schema 8 reaches schema
/// 10 in one invocation; schema 9 reaches schema 10 only through this explicit
/// path. Caller cancellation leaves the supervised operation running with its
/// exclusive directory owner. Repeating with the same commercial seed
/// reconciles an unknown commit result.
#[allow(dead_code)] // Wired by the M2 billing coordinator integration.
pub(crate) async fn upgrade_billing(
    path: &std::path::Path,
    seed: &BillingUpgradeSeed,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    let path = path.to_owned();
    let seed = seed.clone();
    tokio::spawn(async move { upgrade_billing_owned(&path, &seed).await })
        .await
        .unwrap_or(Err(crate::maintenance::UpgradeError::OutcomeUnknown))
}

async fn upgrade_billing_owned(
    path: &std::path::Path,
    seed: &BillingUpgradeSeed,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    use crate::maintenance::UpgradeError;
    use sqlx::Connection;
    let owner = super::owner::Owner::acquire(path)?;
    let mut conn = super::connect::initial(&owner).await?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        owner.verify_path()?;
        super::connect::verify(&mut conn, false).await?;
        upgrade_billing_connection(
            &mut conn,
            seed,
            #[cfg(test)]
            0,
        )
        .await
    })
    .await
    .unwrap_or(Err(UpgradeError::OutcomeUnknown));
    // Drain rollback/commit on the SQLx worker before releasing the OS lock.
    if conn.close().await.is_err() {
        return Err(UpgradeError::OutcomeUnknown);
    }
    result
}

async fn upgrade_billing_connection(
    conn: &mut SqliteConnection,
    seed: &BillingUpgradeSeed,
    #[cfg(test)] cut: u8,
) -> Result<crate::maintenance::UpgradeResult, crate::maintenance::UpgradeError> {
    use crate::maintenance::{UpgradeError, UpgradeResult};
    use sqlx::Connection;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let from = version(&mut tx).await?;
    if from != 8 && from != 9 && from != 10 {
        return Err(UpgradeError::Refused);
    }
    if seed.source_version != 8 && seed.source_version != 9 {
        return Err(UpgradeError::Refused);
    }
    // The retained digest below is re-derived from the schema the coordinator
    // validated, so that schema has to be the one this installation actually
    // came from. An uncommitted schema-8 or schema-9 store must still be at
    // exactly the seeded version: a preflight that went stale because the
    // installation advanced between that read and this transaction validated
    // history the store no longer reports, yet its frozen rows are still there
    // byte for byte and its digest would still be reproduced. Matching the
    // recorded source version instead is what a committed schema-10 retry needs,
    // and that store is reconciled below against its own marker.
    if from != 10 && seed.source_version != from {
        return Err(UpgradeError::Refused);
    }
    let installation = super::read::installation(&mut tx).await?;
    let stopped: bool = sqlx::query_scalar("SELECT owner IS NULL AND lease_until_us IS NULL AND enabled=0 FROM dispatcher_head WHERE singleton=1")
        .fetch_one(&mut *tx).await?;
    if seed.store_id.is_empty()
        || installation.logical_store_id != seed.store_id
        || installation.scope.tenant != seed.tenant
        || installation.scope.environment != seed.environment
        || installation.mode != "real"
        || installation.admission != "open"
        || !installation.dispatch_hold
        || installation.dispatch_enabled
        || !stopped
    {
        return Err(UpgradeError::Refused);
    }
    super::connect::integrity(&mut tx).await?;
    let bytes: Vec<u8> =
        sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
    if bytes != seed.setup_bytes || bytes.is_empty() || bytes.len() > 65536 {
        return Err(UpgradeError::Refused);
    }
    let setup = ledgerlab_core::canonical::parse_bounded(&bytes, 65536)
        .map_err(|_| UpgradeError::Refused)?;
    if ledgerlab_core::canonical::CanonicalBytes::from_value(&setup)
        .map_err(|_| UpgradeError::Refused)?
        .as_slice()
        != bytes
        || setup["schema"] != "ledger-local-billing/1"
        || setup["store_id"] != seed.store_id
        || setup["scope"] != serde_json::json!([seed.tenant, seed.environment])
        || setup["customer"] != seed.customer
        || setup["source"] != seed.source
        || setup["agreement"] != seed.agreement_id
        || ledgerlab_core::domain::Timestamp::parse(
            setup["accepted_at"].as_str().ok_or(UpgradeError::Refused)?,
        )
        .map_err(|_| UpgradeError::Refused)?
        .micros()
            != seed.effective_at_us
    {
        return Err(UpgradeError::Refused);
    }
    let unrelated: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM events)+(SELECT count(*) FROM outcome_records)+(SELECT count(*) FROM r3_commit_witness)")
        .fetch_one(&mut *tx).await?;
    let (count, last, size, wrong_source): (i64, i64, i64, i64) = sqlx::query_as("SELECT count(*),COALESCE(max(ordinal),0),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0),COALESCE(sum(source<>?),0) FROM billing_entries")
        .bind(&seed.source).fetch_one(&mut *tx).await?;
    let (aliases, alias_size, wrong_alias_source): (i64, i64, i64) = sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0),COALESCE(sum(source<>?),0) FROM billing_aliases")
        .bind(&seed.source).fetch_one(&mut *tx).await?;
    let (permissions, permission_max): (i64, i64) =
        sqlx::query_as("SELECT count(*),COALESCE(max(revision),1) FROM billing_permissions")
            .fetch_one(&mut *tx)
            .await?;
    if unrelated != 0
        || count > 1000
        || count != last
        || size > 33_554_432
        || wrong_source != 0
        || aliases > 1000
        || alias_size > 33_554_432
        || wrong_alias_source != 0
        || permissions > 1000
        || permission_max != permissions + 1
    {
        return Err(UpgradeError::Refused);
    }
    // The digest is always re-derived from the same pre-M3 schema the seed was
    // taken under, so a schema-10 retry after an unknown commit still proves
    // that no retained byte changed underneath the coordinator. On a retry the
    // seed's source version must be the one this store was actually migrated
    // from, and a store that was never migrated has nothing to re-derive.
    if from == 10 {
        let source: Option<i64> =
            sqlx::query_scalar("SELECT source_version FROM billing_m3_upgrade WHERE singleton=1")
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(source) = source {
            if source != seed.source_version {
                return Err(UpgradeError::Refused);
            }
        } else {
            let empty: i64 = sqlx::query_scalar(
                "SELECT (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)+(SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases)",
            )
            .fetch_one(&mut *tx)
            .await?;
            if empty != 0 {
                return Err(UpgradeError::Refused);
            }
        }
    }
    let digest = if seed.source_version == 8 {
        legacy_billing_snapshot(&mut tx).await?.digest
    } else {
        schema9_snapshot(&mut tx).await?.digest
    };
    if digest != seed.snapshot_digest {
        return Err(UpgradeError::Refused);
    }
    if from == 8 {
        let migration = migrator().migrations[8].clone();
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_ref()))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO _sqlx_migrations (version,description,success,checksum,execution_time) VALUES (?,?,TRUE,?,0)")
            .bind(migration.version).bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES(?,?,?)")
            .bind(&seed.customer)
            .bind(&seed.tenant)
            .bind(&seed.environment)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO billing_agreements(customer,source,revision,agreement_id,agreement_version,transition,effective_at_us,recorded_at_us,setup_bytes) VALUES(?,?,1,?,1,'start',?,?,?)")
            .bind(&seed.customer).bind(&seed.source).bind(&seed.agreement_id)
            .bind(seed.effective_at_us).bind(seed.recorded_at_us).bind(&seed.setup_bytes)
            .execute(&mut *tx).await?;
        #[cfg(test)]
        if cut == 1 {
            return Err(UpgradeError::Refused);
        }
    }
    // A schema-9 history alone is insufficient: reconciliation requires the
    // exact original mapping and initial terms, including retained scope.
    let seeded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_customers c JOIN billing_agreements a ON c.customer=a.customer WHERE c.customer=? AND c.tenant=? AND c.environment=? AND a.source=? AND a.revision=1 AND a.agreement_id=? AND a.agreement_version=1 AND a.transition='start' AND a.effective_at_us=? AND a.setup_bytes=?)")
        .bind(&seed.customer).bind(&seed.tenant).bind(&seed.environment).bind(&seed.source)
        .bind(&seed.agreement_id).bind(seed.effective_at_us).bind(&seed.setup_bytes)
        .fetch_one(&mut *tx).await?;
    if !seeded {
        return Err(UpgradeError::Refused);
    }
    if from <= 9 {
        let migration = migrator().migrations[9].clone();
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_ref()))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO _sqlx_migrations (version,description,success,checksum,execution_time) VALUES (?,?,TRUE,?,0)")
            .bind(migration.version).bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref()).execute(&mut *tx).await?;
        // The pre-M3 schema this store came from is recorded once, in the same
        // transaction as the migration, so a retry re-derives the identical
        // frozen digest instead of a differently framed one.
        sqlx::query("INSERT INTO billing_m3_upgrade(singleton,source_version) VALUES(1,?)")
            .bind(from)
            .execute(&mut *tx)
            .await?;
        let legacy_owner: Option<String> = sqlx::query_scalar("SELECT a.customer FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1")
            .fetch_optional(&mut *tx).await?;
        for row in &seed.index {
            // Every derived index field is re-proved against the retained row
            // bytes under the write transaction before it becomes durable.
            let retained: Option<(String, String, Vec<u8>, Option<i64>)> = sqlx::query_as(
                "SELECT source,external_id,semantic_key,NULL FROM billing_entries WHERE ordinal=?",
            )
            .bind(row.ordinal)
            .fetch_optional(&mut *tx)
            .await?;
            let scoped: Option<(String, String, String, Vec<u8>, i64)> = sqlx::query_as("SELECT customer,source,external_id,semantic_key,accepted_at_us FROM billing_m2_entries WHERE ordinal=?")
                .bind(row.ordinal).fetch_optional(&mut *tx).await?;
            let (external_id, semantic_key) = match (retained, scoped) {
                (Some((source, external_id, semantic_key, _)), None) => {
                    if legacy_owner.as_deref() != Some(row.customer.as_str())
                        || source != row.source
                    {
                        return Err(UpgradeError::Refused);
                    }
                    (external_id, semantic_key)
                }
                (None, Some((customer, source, external_id, semantic_key, accepted))) => {
                    if customer != row.customer
                        || source != row.source
                        || accepted != row.accepted_at_us
                    {
                        return Err(UpgradeError::Refused);
                    }
                    (external_id, semantic_key)
                }
                _ => return Err(UpgradeError::Refused),
            };
            if external_id != row.external_id || semantic_key != row.semantic_key {
                return Err(UpgradeError::Refused);
            }
            sqlx::query("INSERT INTO billing_m3_index(ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us) VALUES(?,?,?,?,?,?,?,?)")
                .bind(row.ordinal).bind(&row.customer).bind(&row.source)
                .bind(&row.external_id).bind(&row.semantic_key)
                .bind(&row.target).bind(&row.kind).bind(row.accepted_at_us)
                .execute(&mut *tx).await?;
        }
        let retained: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)")
            .fetch_one(&mut *tx).await?;
        if retained != seed.index.len() as i64 {
            return Err(UpgradeError::Refused);
        }
        let (guard, entry_count, entry_bytes, alias_count, alias_bytes): (i64, i64, i64, i64, i64) =
            sqlx::query_as("SELECT guard,entry_count,entry_bytes,alias_count,alias_bytes FROM billing_m3_bounds WHERE singleton=1")
                .fetch_one(&mut *tx).await?;
        if guard != entry_count + alias_count {
            return Err(UpgradeError::Refused);
        }
        let indexed: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m3_index")
            .fetch_one(&mut *tx)
            .await?;
        let indexed_bytes: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(length(semantic_key)),0) FROM billing_m3_index",
        )
        .fetch_one(&mut *tx)
        .await?;
        if indexed != retained
            || entry_count != retained
            || entry_bytes > 268_435_456
            || alias_count > 100_000
            || alias_bytes > 67_108_864
            || indexed_bytes > entry_bytes
        {
            return Err(UpgradeError::Refused);
        }
        super::connect::integrity(&mut tx).await?;
    } else {
        // A schema-10 retry reconciles the exact committed index instead of
        // re-seeding it. A partial or foreign index is refused, never repaired.
        let indexed: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m3_index")
            .fetch_one(&mut *tx)
            .await?;
        if indexed != seed.index.len() as i64 {
            return Err(UpgradeError::Refused);
        }
        for row in &seed.index {
            let exact: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_m3_index WHERE ordinal=? AND customer=? AND source=? AND external_id=? AND semantic_key=? AND target=? AND kind=? AND accepted_at_us=?)")
                .bind(row.ordinal).bind(&row.customer).bind(&row.source)
                .bind(&row.external_id).bind(&row.semantic_key)
                .bind(&row.target).bind(&row.kind).bind(row.accepted_at_us)
                .fetch_one(&mut *tx).await?;
            if !exact {
                return Err(UpgradeError::Refused);
            }
        }
    }
    verify(&mut tx).await?;
    super::connect::integrity(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|_| UpgradeError::OutcomeUnknown)?;
    #[cfg(test)]
    if cut == 2 {
        return Err(UpgradeError::OutcomeUnknown);
    }
    Ok(if from == 10 {
        UpgradeResult::AlreadyCurrent
    } else {
        UpgradeResult::Upgraded
    })
}

#[cfg(test)]
#[path = "billing_upgrade_tests.rs"]
mod billing_upgrade_tests;
