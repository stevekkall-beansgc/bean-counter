use super::{read, write, Inner};
use crate::store::{
    errors::{CommitError, StoreError},
    ports::AcceptanceTx,
    records::*,
};
use sqlx::{Sqlite, SqliteConnection, Transaction};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    sync::{oneshot, OwnedSemaphorePermit},
    time::{timeout_at, Instant},
};

// Set the poison bit before every database await. If its future is dropped the
// bit stays set until rollback; successful reads preserve any previous failure.
macro_rules! checked_read {
    ($this:ident, $read:expr) => {{
        let failed = $this.failed;
        $this.failed = true;
        let result = timeout_at(
            $this.deadline.min(Instant::now() + Duration::from_secs(2)),
            $read,
        )
        .await
        .map_err(|_| StoreError::Deadline)?;
        match result {
            Ok(value) => {
                $this.failed = failed;
                Ok(value)
            }
            Err(error) => {
                if error.disables_writes() {
                    $this.store.disabled.store(true, Ordering::Release);
                }
                Err(error)
            }
        }
    }};
}

/// Drop queues SQLx's tracked rollback; before_acquire verifies tracked cleanup.
/// A failed or cancelled write poisons this handle so a partial plan cannot commit.
pub(crate) struct SqliteTx {
    pub(super) transaction: Option<Transaction<'static, Sqlite>>,
    pub(super) store: Arc<Inner>,
    pub(super) slot: Option<OwnedSemaphorePermit>,
    pub(super) deadline: Instant,
    pub(super) failed: bool,
}
impl SqliteTx {
    fn conn(&mut self) -> &mut SqliteConnection {
        self.transaction.as_mut().expect("live transaction")
    }
    #[cfg(test)]
    pub async fn load_delivery(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<StoredDelivery>, StoreError> {
        checked_read!(self, read::delivery(self.conn(), s, id))
    }
}
impl AcceptanceTx for SqliteTx {
    async fn load_outbox(
        &mut self,
        query: crate::outbox::Query,
    ) -> Result<crate::outbox::Snapshot, StoreError> {
        checked_read!(self, super::outbox::read(self.conn(), query))
    }
    async fn load_installation(&mut self) -> Result<Installation, StoreError> {
        checked_read!(self, read::installation(self.conn()))
    }
    async fn load_chain(&mut self, s: &Scope, id: &str) -> Result<Option<Chain>, StoreError> {
        checked_read!(self, read::chain(self.conn(), s, id))
    }
    async fn load_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<(String, CanonicalRecord)>, StoreError> {
        checked_read!(self, read::document(self.conn(), s, id))
    }
    async fn load_authority(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<AuthorityHead>, StoreError> {
        checked_read!(self, read::authority(self.conn(), s, id))
    }
    async fn load_binding(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<BindingHead>, StoreError> {
        checked_read!(self, read::binding(self.conn(), s, id))
    }
    async fn load_grant_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<String>, StoreError> {
        checked_read!(self, read::grant_document(self.conn(), s, id))
    }

    async fn write(&mut self, op: &WriteOp) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::Integrity(
                "transaction already failed or was cancelled",
            ));
        }
        self.failed = true;
        let deadline = self.deadline;
        let result = timeout_at(deadline, write::operation(self.conn(), op))
            .await
            .map_err(|_| StoreError::Deadline)?;
        match result {
            Ok(()) => {
                self.failed = false;
                Ok(())
            }
            Err(e) => {
                if e.disables_writes() {
                    self.store.disabled.store(true, Ordering::Release);
                }
                Err(e)
            }
        }
    }
    async fn load_identity(
        &mut self,
        s: &Scope,
        source: &str,
        external: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        checked_read!(self, read::identity(self.conn(), s, source, external))
    }
    async fn load_claim(
        &mut self,
        s: &Scope,
        source: &str,
        operation: &str,
        kind: &str,
        token: &str,
    ) -> Result<Option<StoredClaim>, StoreError> {
        checked_read!(
            self,
            read::claim(self.conn(), s, source, operation, kind, token)
        )
    }
    async fn rollback(mut self) -> Result<(), StoreError> {
        let tx = self.transaction.take().expect("live transaction");
        let drain_deadline = Instant::now() + Duration::from_secs(5);
        match timeout_at(drain_deadline, tx.rollback()).await {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.store.disabled.store(true, Ordering::Release);
                let _ = timeout_at(drain_deadline, self.store.writer.close()).await;
                Err(StoreError::WritesDisabled)
            }
        }
    }
    async fn commit(mut self) -> Result<(), CommitError> {
        if self.failed || Instant::now() >= self.deadline {
            return match self.rollback().await {
                Ok(()) => Err(CommitError::RolledBack(StoreError::Integrity(
                    "failed, cancelled or expired transaction",
                ))),
                Err(_) => Err(CommitError::OutcomeUnknown),
            };
        }
        let tx = self.transaction.take().expect("live transaction");
        let store = Arc::clone(&self.store);
        let slot = self.slot.take();
        let (send, receive) = oneshot::channel();
        // No await between spawn and registration: caller cancellation cannot orphan
        // the task. The task retains the writer permit and ownership until cleanup.
        let handle = tokio::spawn(async move {
            let _slot = slot;
            let drain_deadline = Instant::now() + Duration::from_secs(5);
            let result = match timeout_at(drain_deadline, tx.commit()).await {
                Ok(Ok(())) => Ok(()),
                _ => {
                    store.disabled.store(true, Ordering::Release);
                    // Close the writer pool: an uncertain connection is never reused.
                    let _ = timeout_at(drain_deadline, store.writer.close()).await;
                    Err(CommitError::OutcomeUnknown)
                }
            };
            let _ = send.send(result);
        });
        *self
            .store
            .commit_task
            .lock()
            .expect("commit registry poisoned") = Some(handle);
        receive.await.unwrap_or_else(|_| {
            self.store.disabled.store(true, Ordering::Release);
            Err(CommitError::OutcomeUnknown)
        })
    }
}
