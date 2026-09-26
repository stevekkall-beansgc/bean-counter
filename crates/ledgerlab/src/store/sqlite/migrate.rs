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
    if current != 9 {
        return Err(StoreError::InvalidStore("unsupported SQLite write schema"));
    }
    version(conn).await?;
    Ok(())
}
async fn version(conn: &mut SqliteConnection) -> Result<i64, StoreError> {
    let v: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if !(1..=9).contains(&v) {
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
}

/// The exact schema-8 history that the billing coordinator must validate before
/// allowing the first schema-9 write. The digest includes row identities,
/// ordering, and original BLOB bytes, not merely the interpreted billing facts.
pub(crate) struct BillingUpgradePreflight {
    pub snapshot: super::BillingSnapshot,
    pub digest: [u8; 32],
}

/// Read the legacy billing history under the exclusive directory owner. This
/// never runs migration SQL. The coordinator validates `snapshot` as a complete
/// v0.4.3 history, then supplies `digest` with the upgrade seed.
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
        if version(&mut conn).await? != 8 {
            return Err(UpgradeError::Refused);
        }
        super::connect::integrity(&mut conn).await?;
        let mut tx = conn.begin().await?;
        let snapshot = legacy_billing_snapshot(&mut tx).await?;
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
        },
        digest,
    })
}

/// Explicit local billing schema-8 to schema-9 transition. Caller cancellation
/// leaves the supervised operation running with its exclusive directory owner.
/// Repeating with the same commercial seed reconciles an unknown commit result.
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
    if from != 8 && from != 9 {
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
    if legacy_billing_snapshot(&mut tx).await?.digest != seed.snapshot_digest {
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
    verify(&mut tx).await?;
    super::connect::integrity(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|_| UpgradeError::OutcomeUnknown)?;
    #[cfg(test)]
    if cut == 2 {
        return Err(UpgradeError::OutcomeUnknown);
    }
    Ok(if from == 9 {
        UpgradeResult::AlreadyCurrent
    } else {
        UpgradeResult::Upgraded
    })
}

#[cfg(test)]
#[path = "billing_upgrade_tests.rs"]
mod billing_upgrade_tests;
