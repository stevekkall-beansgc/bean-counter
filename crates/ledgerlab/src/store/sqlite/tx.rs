use super::{Inner, read, write};
use crate::store::{
    errors::{CommitError, StoreError},
    ports::AcceptanceTx,
    records::*,
};
use sqlx::{Connection as _, Sqlite, SqliteConnection, Transaction};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{
    sync::{OwnedRwLockWriteGuard, OwnedSemaphorePermit, oneshot},
    time::{Instant, timeout_at},
};

// Set the poison bit before every database await. If its future is dropped the
// bit stays set until rollback; successful reads preserve any previous failure.
macro_rules! checked_read {
    ($this:ident, $read:expr) => {{
        let failed = $this.failed;
        $this.failed = true;
        let result = timeout_at(
            $this.workflow_deadline().map(|d|d.min(Instant::now()+Duration::from_secs(2))).unwrap_or_else(||Instant::now()+Duration::from_secs(2)),
            $read,
        )
        .await
        .map_err(|_| $this.workflow_timeout_error())?;
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

/// Private shared poison identity. Its owning lease follows every clone;
/// the handle conveys no completed-origin or append authority.
#[derive(Clone)]
pub(crate) struct WorkflowProofFailureHandle(Arc<WorkflowProofFailureControl>);
pub(super) struct WorkflowProofFailureControl {
    failed:std::sync::atomic::AtomicBool,
    _storage:Option<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>,
}
impl WorkflowProofFailureHandle {
    pub(super) fn new(storage:Option<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>)->Self {
        Self(Arc::new(WorkflowProofFailureControl{failed:std::sync::atomic::AtomicBool::new(false),_storage:storage}))
    }
    pub(crate) fn same_instance(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}
    pub(crate) fn identity(&self)->usize {Arc::as_ptr(&self.0) as usize}
}
impl std::ops::Deref for WorkflowProofFailureHandle {
    type Target=std::sync::atomic::AtomicBool;
    fn deref(&self)->&Self::Target {&self.0.failed}
}
/// Drop queues SQLx's tracked rollback; before_acquire verifies tracked cleanup.
/// A failed or cancelled write poisons this handle so a partial plan cannot commit.
pub(crate) struct SqliteTx {
    pub(super) transaction: Option<Transaction<'static, Sqlite>>,
    pub(super) store: Arc<Inner>,
    pub(super) slot: Option<OwnedSemaphorePermit>,
    pub(super) deadline: Instant,
    pub(super) workflow_operator:Option<super::workflow::OperatorStorage>,
    pub(crate) workflow_snapshot:Option<crate::service::workflow::resources::Snapshot>,
    pub(crate) workflow_read_session:Option<ledgerlab_core::validation_work::OriginalAuditSession>,
    pub(crate) workflow_read_cancellation:Option<crate::service::workflow::AttemptCancellation>,
    pub(crate) workflow_read_active:bool,
    pub(crate) workflow_main_session:bool,
    pub(super) failed: bool,
    pub(super) financial_io: Option<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>,
    // Service proof scopes can poison this transaction on pure failure/cancel.
    pub(super) workflow_proof_failed: WorkflowProofFailureHandle,
    pub(super) outcome_locks: Vec<crate::store::outcomes::OutcomeLock>,
    pub(super) physical_lane: Option<OwnedRwLockWriteGuard<()>>,
    pub(super) adjudication: Option<super::adjudication::tx::Context>,
    pub(super) physical_start_pages: Option<u32>,
    pub(super) fence_start_changes: Option<i64>,
    pub(super) start_changes: Option<i64>,
    #[cfg(test)]
    pub(super) financial_drain_pause: Option<Arc<super::workflow::financial::io::DrainPause>>,
    #[cfg(test)]
    pub(super) outcome_fault: Option<std::sync::Arc<super::outcomes::Fault>>,
}
impl SqliteTx {
    pub(crate) fn workflow_deadline(&self) -> Option<Instant> {
        self.workflow_operator.map(|s|s.deadline).unwrap_or(Some(self.deadline))
    }
    pub(crate) fn workflow_arena(&self)->ledgerlab_core::Result<ledgerlab_core::canonical::audit_arena::AuditArena>{
        if let Some(snapshot)=&self.workflow_snapshot{snapshot.core_arena()}else{ledgerlab_core::canonical::audit_arena::AuditArena::new()}
    }
    pub(crate) fn workflow_bind_resources(&mut self,operations:Option<i64>,bytes:Option<i64>,deadline:Option<Instant>){self.workflow_operator=Some(super::workflow::OperatorStorage{operations,bytes,deadline});}
    pub(super) fn workflow_timeout_error(&self)->StoreError {self.workflow_snapshot.as_ref().and_then(|s|s.timeout_error()).unwrap_or(StoreError::Deadline)}
    pub(super) fn workflow_in_time(&self)->bool{self.workflow_deadline().is_none_or(|d|Instant::now()<d)}
    pub(crate) fn workflow_proof_failure_handle(&self) -> WorkflowProofFailureHandle {
        self.workflow_proof_failed.clone()
    }
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
    pub(crate) async fn m5_period_close_statement(
        &mut self,
        customer: &str,
        statement_hash: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        super::m5::period_close_statement(self.conn(), customer, statement_hash).await
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
    pub(crate) async fn m5_cumulative_sequences(
        &mut self,
    ) -> Result<super::m5::CumulativeSequences, StoreError> {
        super::m5::cumulative_sequences(self.conn()).await
    }
    pub(crate) async fn m5_cumulative_last_accepted(&mut self) -> Result<Option<i64>, StoreError> {
        super::m5::cumulative_last_accepted(self.conn()).await
    }
    pub(crate) async fn m5_cumulative_basis_revision(
        &mut self,
        customer: &str,
        source: &str,
        agreement_id: &str,
        agreement_version: i64,
    ) -> Result<i64, StoreError> {
        super::m5::cumulative_basis_revision(
            self.conn(),
            customer,
            source,
            agreement_id,
            agreement_version,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_latest_basis_effective(
        &mut self,
        customer: &str,
        source: &str,
        agreement_id: &str,
        agreement_version: i64,
    ) -> Result<Option<i64>, StoreError> {
        super::m5::cumulative_latest_basis_effective(
            self.conn(),
            customer,
            source,
            agreement_id,
            agreement_version,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_basis_at(
        &mut self,
        customer: &str,
        source: &str,
        agreement_id: &str,
        agreement_version: i64,
        at_us: i64,
    ) -> Result<Option<super::m5::RetainedBasis>, StoreError> {
        super::m5::cumulative_basis_at(
            self.conn(),
            customer,
            source,
            agreement_id,
            agreement_version,
            at_us,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_basis_version(
        &mut self,
        customer: &str,
        source: &str,
        agreement_id: &str,
        agreement_version: i64,
        basis_version: i64,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        super::m5::cumulative_basis_version(
            self.conn(),
            customer,
            source,
            agreement_id,
            agreement_version,
            basis_version,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_period_at(
        &mut self,
        customer: &str,
        at_us: i64,
    ) -> Result<(i64, i64), StoreError> {
        super::m5::cumulative_period_at(self.conn(), customer, at_us).await
    }
    pub(crate) async fn m5_append_cumulative_basis(
        &mut self,
        command: &super::m5::Command<'_>,
        agreement_id: &str,
        agreement_version: i64,
        basis_version: i64,
        effective_at_us: i64,
        basis_bytes: &[u8],
    ) -> Result<(), StoreError> {
        super::m5::append_cumulative_basis(
            self.conn(),
            command,
            agreement_id,
            agreement_version,
            basis_version,
            effective_at_us,
            basis_bytes,
        )
        .await
    }
    pub(crate) async fn m5_activity_delivery(
        &mut self,
        customer: &str,
        source: &str,
        id: &str,
    ) -> Result<Option<super::m5::ActivityIdentity>, StoreError> {
        super::m5::activity_delivery(self.conn(), customer, source, id).await
    }
    pub(crate) async fn m5_activity_semantic(
        &mut self,
        customer: &str,
        source: &str,
        operation_id: &str,
    ) -> Result<Option<super::m5::ActivityIdentity>, StoreError> {
        super::m5::activity_semantic(self.conn(), customer, source, operation_id).await
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn m5_append_activity(
        &mut self,
        command: &super::m5::Command<'_>,
        operation_id: &str,
        external_id: &str,
        facts: &[u8],
        ingress: &[u8],
        term_version: i64,
        period_index: i64,
    ) -> Result<(), StoreError> {
        super::m5::append_activity(
            self.conn(),
            command,
            operation_id,
            external_id,
            facts,
            ingress,
            term_version,
            period_index,
        )
        .await
    }
    pub(crate) async fn m5_append_activity_alias(
        &mut self,
        customer: &str,
        source: &str,
        external_id: &str,
        ingress: &[u8],
        identity_key: &[u8],
    ) -> Result<(), StoreError> {
        super::m5::append_activity_alias(
            self.conn(),
            customer,
            source,
            external_id,
            ingress,
            identity_key,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_period_records(
        &mut self,
        customer: &str,
        term_version: i64,
        period_index: i64,
        high_water: i64,
    ) -> Result<Vec<super::m5::CumulativeAssignedRecord>, StoreError> {
        super::m5::cumulative_period_records(
            self.conn(),
            customer,
            term_version,
            period_index,
            high_water,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_original_period_records(
        &mut self,
        customer: &str,
        term_version: i64,
        period_index: i64,
        high_water: i64,
    ) -> Result<Vec<super::m5::CumulativeAssignedRecord>, StoreError> {
        super::m5::cumulative_original_period_records(
            self.conn(),
            customer,
            term_version,
            period_index,
            high_water,
        )
        .await
    }
    pub(crate) async fn m5_cumulative_activity_target(
        &mut self,
        customer: &str,
        source: &str,
        target_id: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        super::m5::cumulative_activity_target(self.conn(), customer, source, target_id).await
    }
    pub(crate) async fn m5_cumulative_target_deltas(
        &mut self,
        customer: &str,
        source: &str,
        activity_id: &str,
    ) -> Result<Vec<Vec<u8>>, StoreError> {
        super::m5::cumulative_target_deltas(self.conn(), customer, source, activity_id).await
    }
    pub(crate) async fn m5_cumulative_period_closed(
        &mut self,
        customer: &str,
        term_version: i64,
        period_index: i64,
    ) -> Result<bool, StoreError> {
        super::m5::cumulative_period_closed(self.conn(), customer, term_version, period_index).await
    }
    pub(crate) async fn m5_per_work_target(
        &mut self,
        customer: &str,
        source: &str,
        target: &str,
    ) -> Result<Option<super::m5::PerWorkTarget>, StoreError> {
        super::m5::per_work_target(self.conn(), customer, source, target).await
    }
    pub(crate) async fn m5_per_work_target_deltas(
        &mut self,
        customer: &str,
        source: &str,
        target: &str,
    ) -> Result<Vec<Vec<u8>>, StoreError> {
        super::m5::per_work_target_deltas(self.conn(), customer, source, target).await
    }
    pub(crate) async fn m5_append_quantity_correction(
        &mut self,
        command: &super::m5::Command<'_>,
        projection: &super::m5::QuantityCorrectionProjection<'_>,
    ) -> Result<(), StoreError> {
        super::m5::append_quantity_correction(self.conn(), command, projection).await
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
        if self.failed || self.workflow_proof_failed.load(Ordering::Acquire) {
            return Err(StoreError::Integrity(
                "transaction already failed or was cancelled",
            ));
        }
        self.failed = true;
        let deadline = self.workflow_deadline();
        let result = super::workflow::until(deadline, write::operation(self.conn(), op))
            .await
            .map_err(|_| self.workflow_timeout_error())?;
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
        if self.financial_io.is_some() {
            return self
                .financial_drain()
                .await
                .unwrap_or(Err(StoreError::WritesDisabled));
        }
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
    async fn commit(self) -> Result<(), CommitError> {
        self.commit_internal(None,&mut None).await
    }
}
#[derive(Debug)]
pub(crate) enum FinancialCommitError {
    Commit(CommitError),
    Resource(ledgerlab_core::Error),
}
/// Existing commit worker ownership plus the SAME original session. An aborted
/// worker with live resource I/O terminally retains its writer/physical lane and
/// OS owner, using the same fail-closed disposition as financial Drain.
struct CommitResourceOwner {
    resource:Option<ledgerlab_core::validation_work::OriginalAuditSession>,
    lease:Option<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>,
    slot:Option<OwnedSemaphorePermit>,
    physical_lane:Option<OwnedRwLockWriteGuard<()>>,
    store:Arc<Inner>,
    completed:bool,
}
impl Drop for CommitResourceOwner {
    fn drop(&mut self) {
        if !self.completed && self.resource.is_some() {
            self.store.disabled.store(true,Ordering::Release);
            self.store.financial_drain_failed.store(true,Ordering::Release);
            std::mem::forget(self.resource.take());
            std::mem::forget(self.lease.take());
            std::mem::forget(self.slot.take());
            std::mem::forget(self.physical_lane.take());
            std::mem::forget(Arc::clone(&self.store));
        }
    }
}
impl SqliteTx {
    /// Resource ownership only; does not qualify a journal or permit append.
    pub(crate) async fn commit_with_resource_session(self,session:ledgerlab_core::validation_work::OriginalAuditSession)->Result<(),FinancialCommitError> {
        let mut resource_error=None;
        let result=self.commit_internal(Some(session),&mut resource_error).await;
        match (result,resource_error) {
            (Err(CommitError::RolledBack(_)),Some(error))=>Err(FinancialCommitError::Resource(error)),
            (result,_)=>result.map_err(FinancialCommitError::Commit),
        }
    }
    async fn commit_internal(mut self,mut resource:Option<ledgerlab_core::validation_work::OriginalAuditSession>,resource_error:&mut Option<ledgerlab_core::Error>) -> Result<(), CommitError> {
        if let Some(session)=resource.as_mut() {
            if let Err(error)=session.check_cancelled() {
                *resource_error=Some(error);
                self.failed=true;
            }
        }
        self.failed |=
            self.workflow_proof_failed.load(Ordering::Acquire) || self.financial_io.is_some();
        if let Some(snapshot)=&self.workflow_snapshot {self.failed|=snapshot.recheck().is_err();}
        if !self.failed && self.workflow_in_time() && self.adjudication.is_none() {
            if let Some(before) = self.physical_start_pages {
                self.failed = true;
                let result = super::workflow::until(
                    self.workflow_deadline(),
                    async {
                        if let Some(session)=resource.as_mut() {
                            let paid=async {
                                self.financial_reserve_io(session,super::adjudication::LEGACY_COMMIT_IO_BYTES)?;
                                super::adjudication::charge_legacy_with_session(self.conn(),before,session).await?;
                                self.financial_finish_io(session).await
                            }.await;
                            return paid.map_err(|error|match error {
                                super::workflow::financial::FinancialReadError::Store(error)=>error,
                                super::workflow::financial::FinancialReadError::Resource(error)=>{*resource_error=Some(error);StoreError::ReadBudgetExhausted},
                            });
                        }
                        super::adjudication::charge_legacy(self.conn(),before).await
                    },
                )
                .await
                .map_err(|_| self.workflow_timeout_error())
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
        if !self.failed
            && self.workflow_in_time()
            && self.store.workflow_guard_enabled.load(Ordering::Acquire)
        {
            self.failed = true;
            let before = self.start_changes;
            let result = super::workflow::until(self.workflow_deadline(), async {
                if let Some(session)=resource.as_mut() {
                    return self.workflow_management_commit_guard(before,session).await.map_err(|error|match error {
                        super::workflow::financial::FinancialReadError::Store(error)=>error,
                        super::workflow::financial::FinancialReadError::Resource(error)=>{
                            *resource_error=Some(error);
                            StoreError::ReadBudgetExhausted
                        },
                    });
                }
                let after: i64 = sqlx::query_scalar("SELECT total_changes()")
                    .fetch_one(self.conn())
                    .await?;
                let before =
                    before.ok_or(StoreError::Integrity("workflow mutation origin missing"))?;
                if after != before {
                    match self.billing_schema_version().await? {
                        12 | 13 | 14 | 15 | 16 | 17 => {
                            super::workflow::capacity::check(self.conn(), 0, 0).await?;
                        }
                        11 => {} // A verified opt-in attempt rolled back.
                        _ => {
                            return Err(StoreError::InvalidStore(
                                "unsupported guarded workflow schema",
                            ));
                        }
                    }
                }
                Ok::<_, StoreError>(())
            })
            .await
            .map_err(|_| self.workflow_timeout_error())
            .and_then(|r| r);
            if let Err(error) = result {
                return match self.rollback().await {
                    Ok(()) => Err(CommitError::RolledBack(error)),
                    Err(_) => Err(CommitError::OutcomeUnknown),
                };
            }
            self.failed = false;
        }
        if self.failed || !self.workflow_in_time() {
            let error=if !self.workflow_in_time(){self.workflow_timeout_error()}else{StoreError::Integrity("failed or cancelled transaction")};
            return match self.rollback().await {
                Ok(()) => Err(CommitError::RolledBack(error)),
                Err(_) => Err(CommitError::OutcomeUnknown),
            };
        }
        if let Some(session)=resource.as_mut() {
            const CONTROLS:usize=std::mem::size_of::<CommitResourceOwner>()+2*std::mem::size_of::<ledgerlab_core::validation_work::OriginalAuditSession>();
            let reserve=self.financial_reserve_io(session,262144+CONTROLS)
                .and_then(|()|session.semantic_work().write_bytes(CONTROLS).map_err(super::workflow::financial::FinancialReadError::Resource))
                .map_err(|error|match error {
                super::workflow::financial::FinancialReadError::Store(error)=>error,
                super::workflow::financial::FinancialReadError::Resource(error)=>{
                    *resource_error=Some(error);StoreError::ReadBudgetExhausted
                },
            });
            if let Err(error)=reserve {
                return match self.rollback().await {
                    Ok(())=>Err(CommitError::RolledBack(error)),
                    Err(_)=>Err(CommitError::OutcomeUnknown),
                };
            }
        }
        let resource_lease=self.financial_io.take();
        let mut tx = self.transaction.take().expect("live transaction");
        let store = Arc::clone(&self.store);
        let fence_start_changes = self.fence_start_changes;
        let slot = self.slot.take();
        let physical_lane = self.physical_lane.take();
        let (send, receive) = oneshot::channel();
        // No await between spawn and registration: caller cancellation cannot orphan
        // the task. The task retains the writer permit and ownership until cleanup.
        // Construct the Drop guard BEFORE spawn. A task aborted before its
        // first poll must retain the complete writer/resource ownership too.
        let owner=CommitResourceOwner {
            resource,lease:resource_lease,slot,physical_lane,
            store:Arc::clone(&store),completed:false,
        };
        let handle = tokio::spawn(async move {
            let mut owner=owner;
            let drain_deadline = Instant::now() + Duration::from_secs(5);
            let publication = async {
                let new = if let (Some(fence), Some(before)) =
                    (&store._owner.fence, fence_start_changes)
                {
                    if let Some(s)=owner.resource.as_mut() {
                        s.semantic_work().write_bytes(std::mem::size_of::<i64>()).map_err(|_|StoreError::ReadBudgetExhausted)?;
                        s.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::RefLookup).map_err(|_|StoreError::ReadBudgetExhausted)?;
                    }
                    let after: i64 = sqlx::query_scalar("SELECT total_changes()")
                        .fetch_one(&mut *tx)
                        .await?;
                    if let Some(s)=owner.resource.as_mut() {
                        s.semantic_work().arithmetic(ledgerlab_core::validation_work::ArithmeticOperation::Compare,&[&after.into(),&before.into()]).map_err(|_|StoreError::ReadBudgetExhausted)?;
                    }
                    if after != before {
                        if let Some(s)=owner.resource.as_mut() {
                            Some(Ok(fence.prepare_with_session(&mut tx,s).await.map_err(|error|match error {
                                super::workflow::financial::FinancialReadError::Store(error)=>error,
                                super::workflow::financial::FinancialReadError::Resource(_)=>StoreError::ReadBudgetExhausted,
                            })?))
                        } else {Some(Err(fence.prepare(&mut tx).await?))}
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(s)=owner.resource.as_mut() {
                    // Finalize the actual fence/total_changes statements before
                    // their worker workspace can be released at COMMIT ACK.
                    s.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::RefLookup).map_err(|_|StoreError::ReadBudgetExhausted)?;
                    tx.clear_cached_statements().await?;
                    s.check_cancelled().map_err(|_|StoreError::ReadBudgetExhausted)?;
                }
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
                    match new {
                        Ok(new)=>fence.finish_with_session(new,owner.resource.as_mut().expect("same consuming owner")).map_err(|error|match error {
                            super::workflow::financial::FinancialReadError::Store(error)=>error,
                            super::workflow::financial::FinancialReadError::Resource(_)=>StoreError::ReadBudgetExhausted,
                        })?,
                        Err(new)=>fence.finish(new)?,
                    }
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
                Ok(Ok(())) => {owner.completed=true;Ok(())},
                _ => {
                    store.disabled.store(true, Ordering::Release);
                    // An expired/aborted close is not a worker ACK. The owner
                    // retains all live resources terminally on that path.
                    if timeout_at(drain_deadline, store.writer.close()).await.is_ok() {
                        owner.completed=true;
                    }
                    Err(CommitError::OutcomeUnknown)
                }
            };
            // Caller cancellation never takes the worker's original resource.
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
