use super::owner::Owner;
use crate::store::errors::StoreError;
use sqlx::{
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqliteLockingMode, SqlitePoolOptions,
        SqliteSynchronous,
    },
    Connection, Row, SqliteConnection, SqlitePool,
};
use std::{path::Path, sync::Arc, time::Duration};

pub(super) const SQLITE_VERSION: &str = "3.51.3";
pub(super) const SQLITE_SOURCE_ID: &str =
    "2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6d618";
#[derive(Clone, Debug)]
#[allow(dead_code)] // Verified startup diagnostics retained for private host integration.
pub(crate) struct Diagnostics {
    pub version: String,
    pub source_id: String,
    pub compile_options: Vec<String>,
}

fn options(path: &Path, reader: bool) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(reader)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .locking_mode(SqliteLockingMode::Normal)
        .busy_timeout(Duration::from_millis(250))
        .shared_cache(false)
        .pragma("query_only", if reader { "ON" } else { "OFF" })
        .pragma("read_uncommitted", "OFF")
        .pragma(
            "fullfsync",
            if cfg!(target_os = "macos") {
                "ON"
            } else {
                "OFF"
            },
        )
}
pub(super) async fn initial(owner: &Owner) -> Result<SqliteConnection, StoreError> {
    Ok(SqliteConnection::connect_with(&options(&owner.database, false)).await?)
}
pub(super) async fn pool(owner: Arc<Owner>, reader: bool) -> Result<SqlitePool, StoreError> {
    let opts = options(&owner.database, reader);
    Ok(SqlitePoolOptions::new()
        .max_connections(if reader { 2 } else { 1 })
        .min_connections(0)
        .acquire_timeout(Duration::from_millis(if reader { 2000 } else { 500 }))
        .after_connect(move |conn, _| {
            let owner = Arc::clone(&owner);
            Box::pin(async move {
                owner
                    .verify_path()
                    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
                verify(conn, reader)
                    .await
                    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
                #[cfg(test)]
                if !reader {
                    static NEXT: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(1);
                    let id = NEXT
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        .to_string();
                    sqlx::query("CREATE TEMP TABLE test_connection_id (id TEXT NOT NULL)")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("INSERT INTO test_connection_id VALUES (?)")
                        .bind(id)
                        .execute(&mut *conn)
                        .await?;
                }
                Ok(())
            })
        })
        .before_acquire(|conn, _| {
            Box::pin(async move {
                // Ping runs after any queued rollback. A stale tracked transaction
                // causes discard, never reuse; SQLx's worker owns the rollback queue.
                conn.ping().await?;
                Ok(!conn.is_in_transaction())
            })
        })
        .connect_with(opts)
        .await?)
}
pub(super) async fn verify(
    conn: &mut SqliteConnection,
    reader: bool,
) -> Result<Diagnostics, StoreError> {
    let (version, source_id): (String, String) =
        sqlx::query_as("SELECT sqlite_version(),sqlite_source_id()")
            .fetch_one(&mut *conn)
            .await?;
    if version != SQLITE_VERSION || source_id != SQLITE_SOURCE_ID {
        return Err(StoreError::InvalidStore("unverified linked SQLite build"));
    }
    let compile_options: Vec<String> = sqlx::query_scalar("PRAGMA compile_options")
        .fetch_all(&mut *conn)
        .await?;
    if !compile_options.iter().any(|s| s == "THREADSAFE=1")
        || compile_options
            .iter()
            .any(|s| matches!(s.as_str(), "OMIT_FOREIGN_KEY" | "OMIT_TRIGGER" | "OMIT_WAL"))
    {
        return Err(StoreError::InvalidStore(
            "SQLite build lacks required features",
        ));
    }
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *conn)
        .await?;
    let locking: String = sqlx::query_scalar("PRAGMA locking_mode")
        .fetch_one(&mut *conn)
        .await?;
    let foreign: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *conn)
        .await?;
    let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(&mut *conn)
        .await?;
    let busy: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *conn)
        .await?;
    let query_only: i64 = sqlx::query_scalar("PRAGMA query_only")
        .fetch_one(&mut *conn)
        .await?;
    let uncommitted: i64 = sqlx::query_scalar("PRAGMA read_uncommitted")
        .fetch_one(&mut *conn)
        .await?;
    let fullfsync: i64 = sqlx::query_scalar("PRAGMA fullfsync")
        .fetch_one(&mut *conn)
        .await?;
    if journal != "wal"
        || locking != "normal"
        || foreign != 1
        || synchronous != 2
        || busy != 250
        || query_only != i64::from(reader)
        || uncommitted != 0
        || fullfsync != i64::from(cfg!(target_os = "macos"))
    {
        return Err(StoreError::InvalidStore(
            "SQLite durability/connection settings mismatch",
        ));
    }
    Ok(Diagnostics {
        version,
        source_id,
        compile_options,
    })
}
pub(super) async fn integrity(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let checks: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut *conn)
        .await?;
    if checks != ["ok"] {
        return Err(StoreError::Integrity("integrity_check failed"));
    }
    if sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(&mut *conn)
        .await?
        .is_some()
    {
        return Err(StoreError::Integrity("foreign_key_check failed"));
    }
    for row in sqlx::query("PRAGMA table_list").fetch_all(conn).await? {
        let name: String = row.try_get("name")?;
        if !name.starts_with("sqlite_")
            && name != "_sqlx_migrations"
            && row.try_get::<i64, _>("strict")? != 1
        {
            return Err(StoreError::Integrity("non-STRICT application table"));
        }
    }
    Ok(())
}
