use crate::store::errors::StoreError;
use sqlx::{
    migrate::{Migration, MigrationType, Migrator},
    SqlSafeStr, SqliteConnection,
};
const SQL: &str = include_str!("../../../migrations/sqlite/0001_first_slice.sql");
const OUTBOX: &str = include_str!("../../../migrations/sqlite/0002_outbox.sql");
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
    ])
}
#[allow(dead_code)] // Explicit migration-owner provisioning; never run by open.
pub(super) async fn create(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    migrator().run(conn).await?;
    Ok(())
}
pub(super) async fn verify(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let v: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if v != 2 {
        return Err(StoreError::InvalidStore("unsupported SQLite write schema"));
    }
    let migrations = migrator();
    let rows: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(conn)
            .await?;
    if rows.len() != 2
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
    Ok(())
}
