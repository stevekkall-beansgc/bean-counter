//! Request-scaled worker payload ownership. A dropped caller is not a worker
//! acknowledgment; its actual transaction and guards move to bounded cleanup.
use super::*;
use ledgerlab_core::canonical::audit_arena::ExternalBufferLease;
use sqlx::Connection;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{
    sync::{OwnedRwLockWriteGuard, OwnedSemaphorePermit, oneshot},
    time::Instant,
};

pub(crate) struct Drain {
    rollback: Option<Pin<Box<dyn Future<Output = Result<(), sqlx::Error>> + Send>>>,
    slot: Option<OwnedSemaphorePermit>,
    physical_lane: Option<OwnedRwLockWriteGuard<()>>,
    lease: Option<ExternalBufferLease>,
    store: Arc<crate::store::sqlite::Inner>,
    completed: bool,
    #[cfg(test)]
    pause: Option<Arc<DrainPause>>,
}
impl Drop for Drain {
    fn drop(&mut self) {
        if !self.completed {
            self.store.disabled.store(true, Ordering::Release);
            self.store
                .financial_drain_failed
                .store(true, Ordering::Release);
            // Terminal fail-closed retention, also if the runtime aborts this
            // cleanup task. Never release accounted worker memory/ownership on
            // an unobserved timeout/error. This installation requires a process
            // restart; retaining the OS owner prevents a second in-process open.
            std::mem::forget(self.rollback.take());
            std::mem::forget(self.slot.take());
            std::mem::forget(self.physical_lane.take());
            std::mem::forget(self.lease.take());
            std::mem::forget(Arc::clone(&self.store));
        }
    }
}
impl Drain {
    // Consuming the whole guard keeps every owned field INSIDE the async
    // future, including cancellation before its first poll. Disjoint field
    // capture must never leave the retention guard outside the worker task.
    async fn run(mut self) -> Result<(), StoreError> {
        #[cfg(test)]
        if let Some(pause) = self.pause.take() {
            pause.reached.notify_one();
            pause.release.notified().await;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = timeout_at(
            deadline,
            self.rollback.as_mut().expect("owned rollback").as_mut(),
        )
        .await;
        if matches!(result, Ok(Ok(()))) {
            // Actual queued statement-cache CLEAR and ROLLBACK acknowledgements.
            // CLEAR finalizes cached AND temporary handles, including bindings
            // left behind when sqlite3_reset returned an earlier step error.
            // This does not claim that
            // a separately queued COMMIT was undone; that path stays unknown.
            self.rollback.take();
            self.completed = true;
            Ok(())
        } else {
            Err(StoreError::WritesDisabled)
        }
    }
}
impl SqliteTx {
    pub(crate) fn financial_reserve_io(
        &mut self,
        session: &mut OriginalAuditSession,
        bytes: usize,
    ) -> Result<(), FinancialReadError> {
        if self.financial_io.is_some() {
            return Err(StoreError::Integrity("overlapping financial I/O lease").into());
        }
        session.check_cancelled()?;
        self.financial_io = Some(session.reserve_external_payload(bytes)?);
        Ok(())
    }
    pub(crate) async fn financial_finish_io(
        &mut self,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        // Reset can return a prior step error before clearing bindings. The
        // explicit queued ClearCache command finalizes BOTH persistent and
        // temporary handles before its ACK, regardless of reset outcome.
        timeout_at(self.deadline, self.conn().clear_cached_statements())
            .await
            .map_err(|_| FinancialReadError::Store(StoreError::Deadline))??;
        self.financial_io.take();
        session.check_cancelled()?;
        Ok(())
    }
    pub(crate) fn financial_drain(&mut self) -> oneshot::Receiver<Result<(), StoreError>> {
        self.failed = true;
        let mut transaction = self.transaction.take().expect("live transaction");
        let guard = Drain {
            rollback: Some(Box::pin(async move {
                // FIFO on this exact worker: prior execute completes, then
                // all statement bindings are finalized, then ROLLBACK. A
                // failed/cancelled ACK retains this whole future and lease.
                transaction.clear_cached_statements().await?;
                transaction.rollback().await
            })),
            slot: self.slot.take(),
            physical_lane: self.physical_lane.take(),
            lease: self.financial_io.take(),
            store: Arc::clone(&self.store),
            completed: false,
            #[cfg(test)]
            pause: self.financial_drain_pause.take(),
        };
        let (send, receive) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let result = guard.run().await;
            let _ = send.send(result);
        });
        // No await before registering the one existing writer cleanup task.
        *self
            .store
            .commit_task
            .lock()
            .expect("commit registry poisoned") = Some(handle);
        receive
    }
}
impl Drop for SqliteTx {
    fn drop(&mut self) {
        if self.financial_io.is_some() && self.transaction.is_some() {
            if tokio::runtime::Handle::try_current().is_ok() {
                drop(self.financial_drain());
            } else {
                self.store.disabled.store(true, Ordering::Release);
                self.store
                    .financial_drain_failed
                    .store(true, Ordering::Release);
                std::mem::forget(self.transaction.take());
                std::mem::forget(self.financial_io.take());
                std::mem::forget(self.slot.take());
                std::mem::forget(self.physical_lane.take());
                std::mem::forget(Arc::clone(&self.store));
            }
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct DrainPause {
    pub reached: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;
    use crate::{
        billing::BillingLedger,
        store::{
            errors::CommitError,
            ports::{AcceptanceStore, AcceptanceTx},
        },
    };
    use ledgerlab_core::canonical::audit_arena::AuditArena;
    async fn store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        (dir, store)
    }
    #[tokio::test]
    async fn cancelled_owner_holds_lease_and_writer_until_real_rollback_ack() {
        let (_dir, store) = store().await;
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap();
        tx.financial_reserve_io(&mut session, 1048576).unwrap();
        let pause = Arc::new(DrainPause::default());
        tx.financial_drain_pause = Some(Arc::clone(&pause));
        drop(tx);
        timeout_at(
            Instant::now() + Duration::from_secs(2),
            pause.reached.notified(),
        )
        .await
        .unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes, baseline + 1048576);
        assert_eq!(store.inner.queue.available_permits(), 64);
        pause.release.notify_one();
        let task = store.inner.commit_task.lock().unwrap().take().unwrap();
        task.await.unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        assert_eq!(store.inner.queue.available_permits(), 65);
        store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap()
            .rollback()
            .await
            .unwrap();
        store.close().await;
    }
    #[tokio::test]
    async fn aborted_cleanup_retains_resources_and_disables_this_owner() {
        let (_dir, store) = store().await;
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap();
        tx.financial_reserve_io(&mut session, 4096).unwrap();
        let pause = Arc::new(DrainPause::default());
        tx.financial_drain_pause = Some(Arc::clone(&pause));
        drop(tx);
        timeout_at(
            Instant::now() + Duration::from_secs(2),
            pause.reached.notified(),
        )
        .await
        .unwrap();
        let task = store.inner.commit_task.lock().unwrap().take().unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(arena.usage().unwrap().live_bytes, baseline + 4096);
        assert_eq!(store.inner.queue.available_permits(), 64);
        assert!(store.inner.disabled.load(Ordering::Acquire));
        assert!(store.inner.financial_drain_failed.load(Ordering::Acquire));
        store.close().await; // does not falsely release retained failed ownership
    }
    #[tokio::test]
    async fn worker_binding_outlives_cancelled_query_until_cache_clear_and_rollback() {
        let (_dir, store) = store().await;
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap();
        tx.financial_reserve_io(&mut session, 1048576).unwrap();
        let reached = Arc::new(tokio::sync::Notify::new());
        let signal = Arc::clone(&reached);
        let first = std::sync::atomic::AtomicBool::new(true);
        let (release, receive) = std::sync::mpsc::sync_channel(1);
        {
            let mut handle = tx.conn().lock_handle().await.unwrap();
            handle.set_progress_handler(1, move || {
                if first.swap(false, Ordering::AcqRel) {
                    signal.notify_one();
                    receive.recv_timeout(Duration::from_secs(2)).is_ok()
                } else {
                    true
                }
            });
        }
        // Actual SQLite worker has bound the argument before its first VM
        // progress callback. This is not a fabricated financial journal row.
        let payload = vec![b'x'; 262144];
        let mut query = Box::pin(
            sqlx::query("SELECT length(?)")
                .bind(payload.as_slice())
                .fetch_one(tx.conn()),
        );
        tokio::select! {
            _ = reached.notified() => {},
            result = &mut query => panic!("worker completed before pause: {result:?}"),
        }
        drop(query);
        drop(payload);
        drop(tx);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline + 1048576);
        assert_eq!(store.inner.queue.available_permits(), 64);
        release.send(()).unwrap();
        let task = store.inner.commit_task.lock().unwrap().take().unwrap();
        task.await.unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap();
        assert_eq!(tx.conn().cached_statements_size(), 0);
        tx.conn()
            .lock_handle()
            .await
            .unwrap()
            .remove_progress_handler();
        tx.rollback().await.unwrap();
        store.close().await;
    }
    #[tokio::test]
    async fn step_error_bindings_are_finalized_before_normal_lease_release() {
        let (_dir, store) = store().await;
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap();
        tx.financial_reserve_io(&mut session, 1048576).unwrap();
        let payload = vec![b'x'; 262144];
        assert!(
            sqlx::query("SELECT json_extract(CAST(? AS TEXT), '$')")
                .bind(payload.as_slice())
                .fetch_one(tx.conn())
                .await
                .is_err()
        );
        assert!(tx.conn().cached_statements_size() > 0);
        drop(payload);
        tx.financial_finish_io(&mut session).await.unwrap();
        assert_eq!(tx.conn().cached_statements_size(), 0);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        tx.rollback().await.unwrap();
        store.close().await;
    }
    #[tokio::test]
    async fn an_unacknowledged_io_reservation_cannot_reach_commit() {
        let (_dir, store) = store().await;
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(20))
            .await
            .unwrap();
        tx.financial_reserve_io(&mut session, 4096).unwrap();
        assert!(matches!(tx.commit().await, Err(CommitError::RolledBack(_))));
        let task = store.inner.commit_task.lock().unwrap().take().unwrap();
        task.await.unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        store.close().await;
    }
}

#[cfg(test)]
mod statement_source_tests {
    use super::super::append::{INSERT_DOCUMENT, INSERT_OPERATION, MATCH_DOCUMENT};
    use sqlx::{Connection, Executor, Row, SqliteConnection, sqlite::SqliteConnectOptions};
    #[tokio::test]
    async fn direct_document_values_keep_preflight_and_check_failures() {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        conn.execute(sqlx::AssertSqlSafe(concat!(
            include_str!("../schema.sql"),
            "\n",
            include_str!("../agreement/schema.sql"),
            "\n",
            include_str!("../agreement/transport/schema.sql"),
            "\n",
            include_str!("../agreement/acceptance/schema.sql")
        )))
        .await
        .unwrap();
        conn.execute(sqlx::AssertSqlSafe(include_str!("schema.sql")))
            .await
            .unwrap();
        // Opaque mechanical table witness, never a valid Product or origin.
        let append = sqlx::query(INSERT_DOCUMENT)
            .bind("workflow-product")
            .bind("id")
            .bind("digest")
            .bind("opaque")
            .bind(&[1u8][..])
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(append.rows_affected(), 1);
        let reused = sqlx::query(INSERT_DOCUMENT)
            .bind("workflow-product")
            .bind("id")
            .bind("digest")
            .bind("opaque")
            .bind(&[1u8][..])
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(reused.rows_affected(), 0);
        let different: Option<bool> = sqlx::query_scalar(MATCH_DOCUMENT)
            .bind("workflow-product")
            .bind("digest")
            .bind("opaque")
            .bind(&[2u8][..])
            .bind("id")
            .fetch_optional(&mut conn)
            .await
            .unwrap();
        assert_eq!(different, Some(false)); // caller's immutable preflight refuses
        // ON CONFLICT(id) must not suppress kind or byte-size CHECK failures,
        // including when that id already exists.
        for (kind, bytes) in [
            ("workflow-unknown", &[1u8][..]),
            ("workflow-product", &[][..]),
        ] {
            assert!(
                sqlx::query(INSERT_DOCUMENT)
                    .bind(kind)
                    .bind("id")
                    .bind("digest")
                    .bind("opaque")
                    .bind(bytes)
                    .execute(&mut conn)
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn pinned_large_payload_statements_expose_exact_virtual_machine() {
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(version, crate::store::sqlite::connect::SQLITE_VERSION);
        conn.execute(sqlx::AssertSqlSafe(concat!(
            include_str!("../schema.sql"),
            "\n",
            include_str!("../agreement/schema.sql"),
            "\n",
            include_str!("../agreement/transport/schema.sql"),
            "\n",
            include_str!("../agreement/acceptance/schema.sql")
        )))
        .await
        .unwrap();
        conn.execute(sqlx::AssertSqlSafe(include_str!("schema.sql")))
            .await
            .unwrap();
        // EXPLAIN compiles actual production statements; it performs no
        // financial INSERT and supplies no economic result/authority operand.
        for (name, sql) in [
            ("match_document", MATCH_DOCUMENT),
            ("insert_document", INSERT_DOCUMENT),
            ("insert_operation", INSERT_OPERATION),
        ] {
            let explain = format!("EXPLAIN {sql}");
            let mut query = sqlx::query(sqlx::AssertSqlSafe(explain));
            for _ in 0..sql.bytes().filter(|b| *b == b'?').count() {
                query = query.bind(Option::<String>::None);
            }
            let rows = query.fetch_all(&mut conn).await.unwrap();
            if name == "insert_document" {
                assert!(
                    !rows
                        .iter()
                        .any(|row| row.get::<&str, _>("opcode") == "InitCoroutine"),
                    "full-body INSERT SELECT spool must stay absent"
                );
            }
            println!("SQL-VM-BEGIN {name}");
            for row in rows {
                println!(
                    "{} {} {} {} {} {} {}",
                    row.get::<i64, _>("addr"),
                    row.get::<&str, _>("opcode"),
                    row.get::<i64, _>("p1"),
                    row.get::<i64, _>("p2"),
                    row.get::<i64, _>("p3"),
                    row.get::<Option<&str>, _>("p4").unwrap_or(""),
                    row.get::<i64, _>("p5")
                );
            }
            println!("SQL-VM-END {name}");
        }
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_operations")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(rows, 0);
    }
}
