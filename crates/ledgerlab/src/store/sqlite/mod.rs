//! Concrete SQLite persistence. `README.md` describes the coordinator integration seam.
mod connect;
mod migrate;
mod owner;
mod read;
#[cfg(test)]
pub(crate) mod tests;
mod tx;
mod write;

use crate::store::{errors::StoreError, ports::AcceptanceStore, records::*};
pub(crate) use connect::Diagnostics;
use sqlx::{Connection, SqlitePool};
use std::{
    fs::OpenOptions,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::Semaphore,
    task::JoinHandle,
    time::{timeout_at, Instant},
};
pub(crate) use tx::SqliteTx;

struct Inner {
    writer: SqlitePool,
    readers: SqlitePool,
    queue: Arc<Semaphore>,
    disabled: AtomicBool,
    commit_task: Mutex<Option<JoinHandle<()>>>,
    _owner: Arc<owner::Owner>,
}
#[derive(Clone)]
pub(crate) struct SqliteStore {
    inner: Arc<Inner>,
    pub diagnostics: Diagnostics,
}
impl SqliteStore {
    /// Explicit empty-store initialization, never implicitly run by open().
    pub async fn create(path: &Path, installation: Installation) -> Result<Self, StoreError> {
        let owner = Arc::new(owner::Owner::acquire(path)?);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&owner.database)?;
        let mut conn = connect::initial(&owner).await?;
        connect::verify(&mut conn, false).await?;
        migrate::create(&mut conn).await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        write::operation(&mut tx, &WriteOp::SeedInstallation(installation)).await?;
        tx.commit().await?;
        conn.close().await?;
        Self::from_owner(owner).await
    }
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        Self::from_owner(Arc::new(owner::Owner::acquire(path)?)).await
    }
    async fn from_owner(owner: Arc<owner::Owner>) -> Result<Self, StoreError> {
        let mut conn = connect::initial(&owner).await?;
        owner.verify_path()?;
        let diagnostics = connect::verify(&mut conn, false).await?;
        migrate::verify(&mut conn).await?;
        connect::integrity(&mut conn).await?;
        read::installation(&mut conn).await?;
        conn.close().await?;
        let writer = connect::pool(Arc::clone(&owner), false).await?;
        let readers = connect::pool(Arc::clone(&owner), true).await?;
        Ok(Self {
            inner: Arc::new(Inner {
                writer,
                readers,
                queue: Arc::new(Semaphore::new(65)),
                disabled: AtomicBool::new(false),
                commit_task: Mutex::new(None),
                _owner: owner,
            }),
            diagnostics,
        })
    }
    /// Drain the registered bounded commit and both pools before releasing ownership.
    pub async fn close(self) {
        self.inner.disabled.store(true, Ordering::Release);
        let task = self
            .inner
            .commit_task
            .lock()
            .expect("commit registry poisoned")
            .take();
        if let Some(task) = task {
            let _ = task.await;
        }
        self.inner.writer.close().await;
        self.inner.readers.close().await;
    }
    #[cfg(test)]
    pub(crate) async fn test_write_locked(&self, path: &Path) -> Result<bool, StoreError> {
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path.join("local.db"))
            .create_if_missing(false)
            .busy_timeout(Duration::ZERO);
        let mut conn = sqlx::SqliteConnection::connect_with(&options).await?;
        let result = match conn.begin_with("BEGIN IMMEDIATE").await {
            Ok(tx) => {
                tx.rollback().await?;
                false
            }
            Err(sqlx::Error::Database(e)) => e.code().is_some_and(|c| c == "5"),
            Err(e) => return Err(e.into()),
        };
        conn.close().await?;
        Ok(result)
    }
    #[cfg(test)]
    pub(crate) async fn test_pool_probe(&self) -> Result<(String, bool), StoreError> {
        let mut conn = self.inner.writer.acquire().await?;
        conn.ping().await?;
        let ptr: String = sqlx::query_scalar("SELECT id FROM temp.test_connection_id")
            .fetch_one(&mut *conn)
            .await?;
        Ok((ptr, conn.is_in_transaction()))
    }
    /// On ambiguity, absence means unresolved; callers must not infer rollback.
    pub async fn lookup_identity(
        &self,
        s: &Scope,
        source: &str,
        external: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        timeout_at(Instant::now() + Duration::from_secs(2), async {
            let mut c = self.inner.readers.acquire().await?;
            read::identity(&mut c, s, source, external).await
        })
        .await
        .map_err(|_| StoreError::Deadline)?
    }
}
impl AcceptanceStore for SqliteStore {
    type Tx = SqliteTx;
    async fn begin(&self, deadline: Instant) -> Result<SqliteTx, StoreError> {
        if self.inner.disabled.load(Ordering::Acquire) {
            return Err(StoreError::WritesDisabled);
        }
        let slot = Arc::clone(&self.inner.queue)
            .try_acquire_owned()
            .map_err(|_| StoreError::Overloaded)?;
        let wait = deadline.min(Instant::now() + Duration::from_millis(500));
        let transaction = timeout_at(wait, self.inner.writer.begin_with("BEGIN IMMEDIATE"))
            .await
            .map_err(|_| StoreError::Deadline)??;
        if self.inner.disabled.load(Ordering::Acquire) {
            transaction.rollback().await?;
            return Err(StoreError::WritesDisabled);
        }
        Ok(SqliteTx {
            transaction: Some(transaction),
            store: Arc::clone(&self.inner),
            slot: Some(slot),
            deadline,
            failed: false,
        })
    }
}
