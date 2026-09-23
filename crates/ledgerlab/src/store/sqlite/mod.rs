//! Concrete SQLite persistence. `README.md` describes the coordinator integration seam.
mod adjudication;
mod comparison;
mod connect;
mod fence;
mod inspect;
pub(crate) mod migrate;
mod outbox;
mod outcomes;
mod owner;
mod read;
#[cfg(test)]
pub(crate) mod tests;
mod tx;
mod write;

use crate::store::{errors::StoreError, ports::AcceptanceStore, records::*};
pub(crate) use adjudication::tx::SqliteAdjudicationStore;
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
    sync::{RwLock, Semaphore},
    task::JoinHandle,
    time::{timeout_at, Instant},
};
pub(crate) use tx::SqliteTx;

struct Inner {
    writer: SqlitePool,
    readers: SqlitePool,
    queue: Arc<Semaphore>,
    comparison_queue: Arc<Semaphore>,
    disabled: AtomicBool,
    commit_task: Mutex<Option<JoinHandle<()>>>,
    _owner: Arc<owner::Owner>,
    adjudication_enabled: Arc<AtomicBool>,
    adjudication_gate: Arc<RwLock<()>>,
    #[cfg(test)]
    fail_after_original_base: AtomicBool,
    #[cfg(test)]
    fence_cut: std::sync::atomic::AtomicU8,
}
#[derive(Clone)]
pub(crate) struct SqliteStore {
    inner: Arc<Inner>,
    #[allow(dead_code)] // Retained verified diagnostics for private host integration.
    pub diagnostics: Diagnostics,
}
impl SqliteStore {
    /// One separately accounted optional CPU/session workspace. It never owns
    /// the mandatory lane or a SQL snapshot while the client is idle.
    pub(crate) fn reserve_comparison(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, StoreError> {
        Arc::clone(&self.inner.comparison_queue)
            .try_acquire_owned()
            .map_err(|_| StoreError::Overloaded)
    }
    fn require_published(&self) -> Result<(), StoreError> {
        if let Some(fence) = &self.inner._owner.fence {
            if self.inner.disabled.load(Ordering::Acquire) || !fence.is_stable() {
                return Err(StoreError::WritesDisabled);
            }
        }
        Ok(())
    }
    /// Explicit empty-store initialization, never implicitly run by open().
    #[allow(dead_code)] // Explicit provisioning; opening never invokes it.
    pub async fn create(path: &Path, installation: Installation) -> Result<Self, StoreError> {
        Self::create_with_anchor(path, installation, None).await
    }
    pub(crate) async fn create_fenced(
        path: &Path,
        installation: Installation,
        anchor: &Path,
    ) -> Result<Self, StoreError> {
        Self::create_with_anchor(path, installation, Some(anchor)).await
    }
    async fn create_with_anchor(
        path: &Path,
        installation: Installation,
        anchor: Option<&Path>,
    ) -> Result<Self, StoreError> {
        let mut owner = owner::Owner::acquire(path)?;
        if let Some(anchor) = anchor {
            owner.fence = Some(Arc::new(fence::Fence::acquire(anchor, path)?));
        }
        let owner = Arc::new(owner);
        let mut file = OpenOptions::new();
        file.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            file.mode(0o600);
        }
        file.open(&owner.database)?;
        let mut conn = connect::initial(&owner).await?;
        connect::verify(&mut conn, false).await?;
        migrate::create(&mut conn).await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        write::operation(&mut tx, &WriteOp::SeedInstallation(installation)).await?;
        tx.commit().await?;
        if let Some(fence) = &owner.fence {
            fence.initialize(&mut conn).await?;
        }
        conn.close().await?;
        Self::from_owner(owner).await
    }
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        Self::from_owner(Arc::new(owner::Owner::acquire(path)?)).await
    }
    pub(crate) async fn open_fenced(path: &Path, anchor: &Path) -> Result<Self, StoreError> {
        let mut owner = owner::Owner::acquire(path)?;
        owner.fence = Some(Arc::new(fence::Fence::acquire(anchor, path)?));
        Self::from_owner(Arc::new(owner)).await
    }
    async fn from_owner(owner: Arc<owner::Owner>) -> Result<Self, StoreError> {
        let mut conn = connect::initial(&owner).await?;
        owner.verify_path()?;
        let diagnostics = connect::verify(&mut conn, false).await?;
        migrate::verify(&mut conn).await?;
        connect::integrity(&mut conn).await?;
        read::installation(&mut conn).await?;
        if let Some(fence) = &owner.fence {
            fence.recover(&mut conn).await?;
            owner.adjudication_enabled.store(true, Ordering::Release);
        } else {
            let anchored: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM r3_commit_witness)")
                    .fetch_one(&mut conn)
                    .await?;
            if anchored {
                return Err(StoreError::InvalidStore(
                    "R3 anchor must be supplied by trusted host",
                ));
            }
        }
        let r3_pages: Option<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT maximum_pages,profile FROM r3_storage_profile WHERE singleton=1",
        )
        .fetch_optional(&mut conn)
        .await?;
        if let Some((pages, profile)) = r3_pages {
            adjudication::verify_stored_profile(&owner, &profile)?;
            owner.adjudication_max_pages.store(
                u32::try_from(pages).map_err(|_| StoreError::InvalidStore("R3 page quota"))?,
                Ordering::Release,
            );
            owner.adjudication_enabled.store(true, Ordering::Release);
        }
        conn.close().await?;
        let writer = connect::pool(Arc::clone(&owner), false).await?;
        let readers = connect::pool(Arc::clone(&owner), true).await?;
        Ok(Self {
            inner: Arc::new(Inner {
                writer,
                readers,
                queue: Arc::new(Semaphore::new(65)),
                comparison_queue: Arc::new(Semaphore::new(1)),
                disabled: AtomicBool::new(false),
                commit_task: Mutex::new(None),
                adjudication_enabled: Arc::clone(&owner.adjudication_enabled),
                adjudication_gate: Arc::clone(&owner.adjudication_gate),
                #[cfg(test)]
                fail_after_original_base: AtomicBool::new(false),
                #[cfg(test)]
                fence_cut: std::sync::atomic::AtomicU8::new(0),
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
    /// A test-only second driver pool under the same OS owner. It exercises
    /// real database contention; normal construction still has one writer pool.
    #[cfg(test)]
    pub(crate) async fn test_contender(&self) -> Result<Self, StoreError> {
        Self::from_owner(self.inner._owner.clone()).await
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
    pub(crate) fn test_writer_closed(&self) -> bool {
        self.inner.writer.is_closed() && self.inner.writer.size() == 0
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
    #[cfg(test)]
    pub async fn lookup_identity(
        &self,
        s: &Scope,
        source: &str,
        external: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        timeout_at(Instant::now() + Duration::from_secs(2), async {
            let _lane = if self.inner.adjudication_enabled.load(Ordering::Acquire) {
                Some(self.inner.adjudication_gate.read().await)
            } else {
                None
            };
            self.require_published()?;
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
        self.begin_lane(deadline, false).await
    }
}
impl SqliteStore {
    /// Mandatory R3 work has a reserved admission slot that optional callers
    /// cannot consume. Both lanes still share the same fair physical writer gate.
    async fn begin_lane(&self, deadline: Instant, mandatory: bool) -> Result<SqliteTx, StoreError> {
        if self.inner.disabled.load(Ordering::Acquire) {
            return Err(StoreError::WritesDisabled);
        }
        let queue = if mandatory {
            &self.inner._owner.mandatory_queue
        } else {
            &self.inner.queue
        };
        let slot = Arc::clone(queue)
            .try_acquire_owned()
            .map_err(|_| StoreError::Overloaded)?;
        let wait = if mandatory {
            deadline
        } else {
            deadline.min(Instant::now() + Duration::from_millis(500))
        };
        let physical_lane = if self.inner.adjudication_enabled.load(Ordering::Acquire) {
            let lane = timeout_at(
                wait,
                Arc::clone(&self.inner.adjudication_gate).write_owned(),
            )
            .await
            .map_err(|_| StoreError::Deadline)?;
            // The gate excludes every admitted profile reader/writer. SQLx first
            // drains a cancelled prior transaction before this acquisition.
            let mut connection = timeout_at(wait, self.inner.writer.acquire())
                .await
                .map_err(|_| StoreError::Deadline)??;
            let (busy, log, checkpointed): (i64, i64, i64) =
                sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
                    .fetch_one(&mut *connection)
                    .await?;
            if busy != 0 || log != 0 || checkpointed != 0 {
                return Err(StoreError::Overloaded);
            }
            Some(lane)
        } else {
            None
        };
        let mut transaction = timeout_at(wait, self.inner.writer.begin_with("BEGIN IMMEDIATE"))
            .await
            .map_err(|_| StoreError::Deadline)??;
        if self.inner.disabled.load(Ordering::Acquire) {
            transaction.rollback().await?;
            return Err(StoreError::WritesDisabled);
        }
        let physical_start_pages = if self
            .inner
            ._owner
            .adjudication_max_pages
            .load(Ordering::Acquire)
            > 0
        {
            Some(adjudication::physical_usage(&mut transaction).await?)
        } else {
            None
        };
        let fence_start_changes = if self.inner._owner.fence.is_some() {
            Some(
                sqlx::query_scalar("SELECT total_changes()")
                    .fetch_one(&mut *transaction)
                    .await?,
            )
        } else {
            None
        };
        Ok(SqliteTx {
            transaction: Some(transaction),
            store: Arc::clone(&self.inner),
            slot: Some(slot),
            deadline,
            failed: false,
            outcome_locks: Vec::new(),
            physical_lane,
            adjudication: None,
            physical_start_pages,
            fence_start_changes,
            #[cfg(test)]
            outcome_fault: None,
        })
    }
}

#[cfg(test)]
mod adjudication_tests;
