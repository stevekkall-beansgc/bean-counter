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
    sync::{oneshot, OwnedRwLockWriteGuard, OwnedSemaphorePermit},
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
    pub(super) outcome_locks: Vec<crate::store::outcomes::OutcomeLock>,
    pub(super) physical_lane: Option<OwnedRwLockWriteGuard<()>>,
    pub(super) adjudication: Option<super::adjudication::tx::Context>,
    pub(super) physical_start_pages: Option<u32>,
    pub(super) fence_start_changes: Option<i64>,
    #[cfg(test)]
    pub(super) outcome_fault: Option<std::sync::Arc<super::outcomes::Fault>>,
}
impl SqliteTx {
    pub(super) fn conn(&mut self) -> &mut SqliteConnection {
        self.transaction.as_mut().expect("live transaction")
    }
    pub(crate) async fn m5_term_state(
        &mut self,
        customer: &str,
    ) -> Result<super::m5::TermState, StoreError> {
        super::m5::term_state(self.conn(), customer).await
    }
    pub(crate) async fn m5_fiscal_state(&mut self) -> Result<super::m5::FiscalState, StoreError> {
        super::m5::fiscal_state(self.conn()).await
    }
    pub(crate) async fn m5_fiscal_report_state(
        &mut self,
        calendar_version: i64,
        requested_snapshot: Option<(i64, i64)>,
        start_at_us: i64,
        end_at_us: i64,
    ) -> Result<super::m5::FiscalReportState, StoreError> {
        super::m5::fiscal_report_state(
            self.conn(),
            calendar_version,
            requested_snapshot,
            start_at_us,
            end_at_us,
        )
        .await
    }
    pub(crate) async fn m5_recurrence_state(
        &mut self,
        customer: &str,
        source: &str,
    ) -> Result<super::m5::RecurrenceState, StoreError> {
        super::m5::recurrence_state(self.conn(), customer, source).await
    }
    pub(crate) async fn m5_append_recurrence_versions(
        &mut self,
        command: &super::m5::Command<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_recurrence_versions(self.conn(), command).await
    }
    pub(crate) async fn m5_append_recurrence_cancel(
        &mut self,
        command: &super::m5::Command<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_recurrence_cancel(self.conn(), command).await
    }
    pub(crate) async fn m5_assignment_for_occurrence(
        &mut self,
        plan: &crate::service::billing::ValidatedEntry,
    ) -> Result<Option<super::m5::M3Assignment>, StoreError> {
        super::m5::assignment_for_m3(self.conn(), plan).await
    }
    pub(crate) async fn m5_append_occurrence(
        &mut self,
        command: &super::m5::Command<'_>,
        m3_receipt_id: &str,
    ) -> Result<(), StoreError> {
        super::m5::append_occurrence(self.conn(), command, m3_receipt_id).await
    }
    pub(crate) async fn m5_occurrence_accepted(
        &mut self,
        customer: &str,
        source: &str,
        id: &str,
    ) -> Result<bool, StoreError> {
        super::m5::occurrence_accepted(self.conn(), customer, source, id).await
    }
    pub(crate) async fn m5_term_transition_state(
        &mut self,
        customer: &str,
        accepted_at_us: i64,
    ) -> Result<super::m5::TermTransitionState, StoreError> {
        super::m5::term_transition_state(self.conn(), customer, accepted_at_us).await
    }
    pub(crate) async fn m5_resolution_head(
        &mut self,
        customer: &str,
        term_version: i64,
        period_index: i64,
    ) -> Result<Option<super::m5::ResolutionHead>, StoreError> {
        super::m5::resolution_head(self.conn(), customer, term_version, period_index).await
    }
    pub(crate) async fn m5_period_resolve_state(
        &mut self,
        customer: &str,
        term_version: i64,
        period_index: i64,
    ) -> Result<super::m5::PeriodResolveState, StoreError> {
        super::m5::period_resolve_state(self.conn(), customer, term_version, period_index).await
    }
    pub(crate) async fn m5_period_close_state(
        &mut self,
        customer: &str,
        term_version: i64,
        period_index: i64,
    ) -> Result<super::m5::PeriodCloseState, StoreError> {
        super::m5::period_close_state(self.conn(), customer, term_version, period_index).await
    }
    pub(crate) async fn m5_ad_hoc_state(
        &mut self,
        customer: &str,
        references: &[(String, String)],
    ) -> Result<super::m5::AdHocState, StoreError> {
        super::m5::ad_hoc_state(self.conn(), customer, references).await
    }
    pub(crate) async fn m5_adjustment_claimed(
        &mut self,
        customer: &str,
        source: &str,
        adjustment_id: &str,
    ) -> Result<Option<bool>, StoreError> {
        super::m5::adjustment_claimed(self.conn(), customer, source, adjustment_id).await
    }
    pub(crate) async fn m5_lookup(
        &mut self,
        identity: &[u8],
    ) -> Result<Option<super::m5::StoredCommand>, StoreError> {
        super::m5::lookup(self.conn(), identity).await
    }
    pub(crate) async fn m5_append_initial_term(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::InitialTermProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_initial_term(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_append_fiscal_version(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::FiscalVersionProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_fiscal_version(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_append_fiscal_report(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::FiscalReportProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_fiscal_report(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_append_term_transition(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::TransitionProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_term_transition(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_append_period_resolution(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::PeriodResolutionProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_period_resolution(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_append_period_close(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::PeriodCloseProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_period_close(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_append_ad_hoc(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::AdHocProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_ad_hoc(self.conn(), command, projection).await
    }
    pub(crate) async fn m5_preflight_capacity(
        &mut self,
        additional_commands: i64,
        additional_records: i64,
        additional_bytes: i64,
    ) -> Result<(), StoreError> {
        super::m5::preflight_capacity(
            self.conn(),
            additional_commands,
            additional_records,
            additional_bytes,
        )
        .await
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
        if !self.failed && Instant::now() < self.deadline && self.adjudication.is_none() {
            if let Some(before) = self.physical_start_pages {
                self.failed = true;
                let result = timeout_at(
                    self.deadline,
                    super::adjudication::charge_legacy(self.conn(), before),
                )
                .await
                .map_err(|_| StoreError::Deadline)
                .and_then(|r| r);
                if let Err(error) = result {
                    return match self.rollback().await {
                        Ok(()) => Err(CommitError::RolledBack(error)),
                        Err(_) => Err(CommitError::OutcomeUnknown),
                    };
                }
                self.failed = false;
            }
        }
        if self.failed || Instant::now() >= self.deadline {
            return match self.rollback().await {
                Ok(()) => Err(CommitError::RolledBack(StoreError::Integrity(
                    "failed, cancelled or expired transaction",
                ))),
                Err(_) => Err(CommitError::OutcomeUnknown),
            };
        }
        let mut tx = self.transaction.take().expect("live transaction");
        let store = Arc::clone(&self.store);
        let fence_start_changes = self.fence_start_changes;
        let slot = self.slot.take();
        let physical_lane = self.physical_lane.take();
        let (send, receive) = oneshot::channel();
        // No await between spawn and registration: caller cancellation cannot orphan
        // the task. The task retains the writer permit and ownership until cleanup.
        let handle = tokio::spawn(async move {
            let _slot = slot;
            let _physical_lane = physical_lane;
            let drain_deadline = Instant::now() + Duration::from_secs(5);
            let publication = async {
                let new = if let (Some(fence), Some(before)) =
                    (&store._owner.fence, fence_start_changes)
                {
                    let after: i64 = sqlx::query_scalar("SELECT total_changes()")
                        .fetch_one(&mut *tx)
                        .await?;
                    if after != before {
                        Some(fence.prepare(&mut tx).await?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                #[cfg(test)]
                if store.fence_cut.load(Ordering::Acquire) == 11 {
                    std::process::exit(77);
                }
                #[cfg(test)]
                if store.fence_cut.load(Ordering::Acquire) == 1 {
                    return Err(StoreError::Deadline);
                }
                tx.commit().await?;
                #[cfg(test)]
                if store.fence_cut.load(Ordering::Acquire) == 12 {
                    std::process::exit(77);
                }
                #[cfg(test)]
                if store.fence_cut.load(Ordering::Acquire) == 2 {
                    return Err(StoreError::Deadline);
                }
                if let (Some(fence), Some(new)) = (&store._owner.fence, new) {
                    fence.finish(new)?;
                }
                #[cfg(test)]
                if store.fence_cut.load(Ordering::Acquire) == 13 {
                    std::process::exit(77);
                }
                #[cfg(test)]
                if store.fence_cut.load(Ordering::Acquire) == 3 {
                    return Err(StoreError::Deadline);
                }
                Ok::<(), StoreError>(())
            };
            let result = match timeout_at(drain_deadline, publication).await {
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
