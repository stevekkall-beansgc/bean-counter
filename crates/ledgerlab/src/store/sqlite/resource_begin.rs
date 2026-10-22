//! Same transaction acquisition with the consuming original resource owner.
//! A cancelled acquisition cannot release native I/O ownership before an ACK.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{OriginalAuditSession, ValidationVisit},
};
use sqlx::Connection;
use workflow::financial::FinancialReadError;
struct BeginIoOwner {
    store: Arc<Inner>,
    lease: Option<ExternalBufferLease>,
    retained_control: Option<ExternalBufferLease>,
    acknowledged: bool,
    cleanup: bool,
    io_started: bool,
}
impl Drop for BeginIoOwner {
    fn drop(&mut self) {
        if !self.io_started {
            self.acknowledged = true;
        }
        if !self.acknowledged && !self.cleanup {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let drain = Self {
                    store: Arc::clone(&self.store),
                    lease: self.lease.take(),
                    retained_control: self.retained_control.take(),
                    acknowledged: false,
                    cleanup: true,
                    io_started: true,
                };
                // The whole Drop guard enters the future, including before its
                // first poll. It cannot drop the lease on task cancellation.
                let mut registered = self
                    .store
                    .commit_task
                    .lock()
                    .expect("commit registry poisoned");
                let previous = registered.take();
                *registered = Some(runtime.spawn(async move {
                    if let Some(previous) = previous {
                        let _ = previous.await;
                    }
                    drain.run().await;
                }));
                self.acknowledged = true;
                return;
            }
        }
        if !self.acknowledged {
            self.store.disabled.store(true, Ordering::Release);
            self.store
                .financial_drain_failed
                .store(true, Ordering::Release);
            // SAME terminal disposition as financial Drain: retain accounting
            // and the OS owner if cleanup acknowledgement is unavailable.
            std::mem::forget(self.lease.take());
            std::mem::forget(self.retained_control.take());
            std::mem::forget(Arc::clone(&self.store));
        }
    }
}
impl BeginIoOwner {
    async fn run(mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = timeout_at(deadline, async {
            let mut connection = self.store.writer.acquire().await?;
            connection.clear_cached_statements().await
        })
        .await;
        self.acknowledged = matches!(result, Ok(Ok(())));
    }
}
impl SqliteStore {
    pub(crate) async fn begin_with_resource_session(
        &self,
        deadline: Instant,
        s: &mut OriginalAuditSession,
    ) -> Result<SqliteTx, FinancialReadError> {
        s.check_cancelled()?;
        const CONTROLS: usize = 2 * std::mem::size_of::<BeginIoOwner>()
            + 2 * std::mem::size_of::<tokio::task::JoinHandle<()>>();
        const RETAINED: usize = std::mem::size_of::<SqliteTx>()
            + std::mem::size_of::<super::tx::WorkflowProofFailureControl>()
            + 2 * std::mem::size_of::<usize>();
        let retained_control = s.reserve_external_payload(RETAINED)?;
        s.semantic_work().write_bytes(RETAINED)?;
        let lease = s.reserve_external_payload(262144 + CONTROLS)?;
        s.semantic_work().write_bytes(CONTROLS)?;
        // Prepay the possible failed-acquisition cleanup lookup while the
        // original meter is still live. Cleanup must also run after S refusal.
        s.semantic_work().visit(ValidationVisit::RefLookup)?;
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        let mut owner = BeginIoOwner {
            store: Arc::clone(&self.inner),
            lease: Some(lease),
            retained_control: Some(retained_control),
            acknowledged: false,
            cleanup: false,
            io_started: false,
        };
        let result = self
            .begin_lane_work(
                deadline,
                false,
                &mut workflow::capacity::SessionWork(s),
                &mut owner.io_started,
                &mut owner.retained_control,
            )
            .await;
        match result {
            Ok(mut tx) => {
                // Retain the acquisition lease through CLEAR and, on refusal,
                // the actual ROLLBACK ACK. A cancelled session cannot release
                // the native workspace while either command is outstanding.
                tx.failed = true;
                let cleared = timeout_at(deadline, tx.conn().clear_cached_statements())
                    .await
                    .map_err(|_| FinancialReadError::Store(StoreError::Deadline))
                    .and_then(|result| result.map_err(FinancialReadError::from));
                let checked =
                    cleared.and_then(|()| s.check_cancelled().map_err(FinancialReadError::from));
                if let Err(error) = checked {
                    let cleanup = crate::store::ports::AcceptanceTx::rollback(tx).await;
                    owner.acknowledged = cleanup.is_ok();
                    owner.cleanup = true;
                    return Err(if cleanup.is_ok() {
                        error
                    } else {
                        StoreError::WritesDisabled.into()
                    });
                }
                tx.failed = false;
                owner.acknowledged = true;
                Ok(tx)
            }
            Err(error) => {
                if !owner.io_started {
                    owner.acknowledged = true;
                    return Err(error);
                }
                // begin_lane may have queued rollback while returning an error.
                // Acquiring that SAME writer waits for SQLx tracked rollback;
                // clear finalizes cached and temporary statements before ACK.
                let cleanup_deadline = Instant::now() + Duration::from_secs(5);
                let cleanup = timeout_at(cleanup_deadline, async {
                    let mut connection = self.inner.writer.acquire().await?;
                    connection.clear_cached_statements().await
                })
                .await;
                owner.acknowledged = matches!(cleanup, Ok(Ok(())));
                owner.cleanup = true;
                if owner.acknowledged {
                    Err(error)
                } else {
                    Err(StoreError::WritesDisabled.into())
                }
            }
        }
    }
}

impl SqliteStore {
    pub(crate) async fn begin_with_operator_session(
        &self,
        deadline: Option<Instant>,
        s: &mut OriginalAuditSession,
    ) -> Result<SqliteTx, FinancialReadError> {
        // Only native acquisition/CLEAR use this fallback, never the request itself.
        let acquisition = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(2));
        let mut tx = self
            .begin_with_resource_session(acquisition, s)
            .await
            .map_err(|error| {
                crate::service::workflow_ci_diagnostic("operator.begin.error");
                error
            })?;
        crate::service::workflow_ci_diagnostic("operator.begin.complete");
        tx.workflow_bind_resources(None, None, deadline);
        Ok(tx)
    }
}
