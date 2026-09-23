use crate::store::errors::StoreError;
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
    if current != 7 {
        return Err(StoreError::InvalidStore("unsupported SQLite write schema"));
    }
    version(conn).await?;
    Ok(())
}
async fn version(conn: &mut SqliteConnection) -> Result<i64, StoreError> {
    let v: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if !(1..=7).contains(&v) {
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
    for migration in migrator().iter().filter(|m| m.version > from) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_ref()))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO _sqlx_migrations (version,description,success,checksum,execution_time) VALUES (?,?,TRUE,?,0)")
            .bind(migration.version).bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref()).execute(&mut *tx).await?;
    }
    verify(&mut tx).await?;
    super::connect::integrity(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|_| UpgradeError::OutcomeUnknown)?;
    #[cfg(test)]
    if lose_ack {
        return Err(UpgradeError::OutcomeUnknown);
    }
    Ok(if from == 7 {
        UpgradeResult::AlreadyCurrent
    } else {
        UpgradeResult::Upgraded
    })
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod upgrade_tests;
