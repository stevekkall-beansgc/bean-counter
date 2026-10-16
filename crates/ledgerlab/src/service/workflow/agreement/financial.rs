//! Closed financial host shell. Local-management observation is not economic
//! eligibility. No pure DATA draft, caller flag, hash-only certificate or demo
//! acknowledgement can enter an append/commit path through this module.
use super::*;
use crate::service::workflow::core_error;
use crate::{
    service::workflow::{
        local::{LocalWorkflowLedger, deadline},
        request::OperationKey,
    },
    store::ports::{AcceptanceStore, AcceptanceTx},
};
mod current;
mod prefix;
mod request_wrapper;
mod wrappers;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(in crate::service::workflow) fn is_kind(kind: &str) -> bool {
    matches!(
        kind,
        "work.accept"
            | "work.reverse"
            | "quantity.correct"
            | "outcome.submit"
            | "outcome.correct"
            | "outcome.close"
            | "link.assert"
    )
}

/// Actual private-owner/setup observation bound to one held transaction/key.
/// This excludes workflow SourceGrant/assent/funding eligibility and financial
/// heads, reservations or absence proofs. It is not serialized or exported.
struct ManagementObservation {
    owner: Arc<AtomicBool>,
    key: OperationKey,
    fingerprint: Vec<u8>,
}
impl ManagementObservation {
    async fn capture(
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
    ) -> Result<Self> {
        require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
        Ok(Self {
            owner: tx.workflow_proof_failure_handle(),
            key: key.clone(),
            fingerprint: management_fingerprint(ledger, tx, key).await?,
        })
    }
    async fn recheck(
        &self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
    ) -> Result<()> {
        let provided = tx.workflow_proof_failure_handle();
        if !Arc::ptr_eq(&self.owner, &provided) || &self.key != key {
            self.owner.store(true, Ordering::Release);
            provided.store(true, Ordering::Release);
            return Err(ServiceError::IntegrityFailure);
        }
        require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
        let fresh = management_fingerprint(ledger, tx, key).await?;
        require(fresh == self.fingerprint, "STALE_REVISION")
    }
}
async fn management_fingerprint(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
) -> Result<Vec<u8>> {
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let installation = tx
        .load_installation()
        .await
        .map_err(crate::service::store_error)?;
    // Setup serde intentionally skips grant_revision for legacy canonical
    // records. Bind it explicitly here; this is private comparison data only.
    records::bytes(&json!([
        key,
        setup.grant_revision.to_string(),
        setup,
        installation.logical_store_id,
        installation.scope.tenant,
        installation.scope.environment,
        installation.mode,
        installation.admission,
        installation.dispatch_hold,
        installation.dispatch_enabled,
        installation.generation.to_string()
    ]))
}

/// The actual request lifetime and held-writer deadline feed the same terminal
/// core cancellation flag. This creates no proof state or additional budget.
struct AttemptCancellation {
    flag: ledgerlab_core::canonical::audit_arena::AuditCancellation,
    timer: tokio::task::JoinHandle<()>,
}
impl AttemptCancellation {
    fn new(
        arena: &ledgerlab_core::canonical::audit_arena::AuditArena,
        deadline: tokio::time::Instant,
    ) -> Self {
        let flag = arena.cancellation_handle();
        let timed = flag.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            timed.cancel();
        });
        Self { flag, timer }
    }
}
impl Drop for AttemptCancellation {
    fn drop(&mut self) {
        self.flag.cancel();
        self.timer.abort();
    }
}

/// Never returns saved financial success without the complete version-aware
/// original financial auditor. Current compiler/grant/time are not consulted.
pub(in crate::service::workflow) async fn original_status(
    tx: &mut SqliteTx,
    key: &OperationKey,
) -> Result<Value> {
    // One owner for this entire audit attempt. Every per-origin session in the
    // chronological driver will share this same arena, not create another pool.
    let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().map_err(core_error)?;
    let _cancellation = AttemptCancellation::new(&arena, tx.workflow_deadline());
    let mut session =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .map_err(core_error)?;
    let identity = key.bytes()?;
    let original = wrappers::load(tx, key, &identity, &mut session).await?;
    // Mechanical wrappers are unresolved DATA. The full prefix/descriptor and
    // same-constructor semantic/capacity/funding completion is still mandatory;
    // no receipt or CompletedOrigin is returned from this partial path.
    let _unresolved = original;

    Err(ServiceError::Unavailable)
}

impl LocalWorkflowLedger {
    /// Financial admission is disabled. This validates lexical ingress and
    /// authenticates local management/identity ordering, but creates no receipt,
    /// financial record, claim, reservation, alias, projection or meter delta.
    pub async fn financial(&self, raw: &[u8]) -> Result<Value> {
        let req = request::Request::parse_financial(raw)?;
        let key = req.key();
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(crate::service::store_error)?;
        let attempted = proof_session::with_proofs(&mut tx, key, |tx| {
            Box::pin(async {
                self.admin.read(&self.store, tx, key).await?;
                if let Some(kind) = tx
                    .workflow_operation_kind(&key.bytes()?)
                    .await
                    .map_err(crate::service::store_error)?
                {
                    require(kind == req.kind(), "IDENTITY_CONFLICT")?;
                    // No fresh selection, grant check, compiler or capacity admission
                    // precedes this original identity branch.
                    return original_status(tx, key).await;
                }
                let management = ManagementObservation::capture(self, tx, key).await?;
                management.recheck(self, tx, key).await?;
                if req.kind() == "work.accept" {
                    let _observed = current::work(self, tx, key, req.payload()).await?;
                    // Actual observed inputs remain noncommittable until full
                    // original audit, core candidate and capacity gates close.
                }

                // No normalized economic selection is manufactured here. The current
                // draft DATA layer is noncommittable; typed intrinsic/audit inputs,
                // full contextual authority, funding/exposure, all-ancestor future
                // reservation, financial CAS and Profile B prepared-delta proofs must
                // all precede the first financial append. There is no bypass factory.
                Err(ServiceError::Unavailable)
            })
        })
        .await;
        // Always rollback. Failed/cancelled proof scopes poison the same held
        // transaction; existing SQLx drop/cleanup and commit-unknown machinery
        // remains unchanged. No financial commit branch exists while gates remain closed.
        tx.rollback().await.map_err(crate::service::store_error)?;
        attempted
    }
}

#[cfg(test)]
mod tests;
