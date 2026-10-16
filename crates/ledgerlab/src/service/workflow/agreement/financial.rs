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
mod wire;
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
            | "invocation.authorize"
            | "invocation.close"
            | "cost.finalize"
            | "cost.correct"
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

/// Submission retry retains the actual incoming request until ORIGINAL intent
/// normalization/comparison; key-only status never manufactures that intent.
enum OriginalQuery<'a> {
    Status(&'a [u8]),
    Retry(&'a request::Request),
}

pub(in crate::service::workflow) async fn original_status(
    tx: &mut SqliteTx,
    key: &OperationKey,
    raw_key: &[u8],
) -> Result<Value> {
    original_query(tx, key, OriginalQuery::Status(raw_key)).await
}
async fn original_retry(tx: &mut SqliteTx, incoming: &request::Request) -> Result<Value> {
    original_query(tx, incoming.key(), OriginalQuery::Retry(incoming)).await
}

/// Never returns saved success without full original financial semantics. The
/// incoming request selects only the global identity; saved evidence supplies
/// the ORIGINAL literal request. No current compiler/grant/time is consulted.
async fn original_query(
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: OriginalQuery<'_>,
) -> Result<Value> {
    use ledgerlab_core::validation_work::{OriginalAuditSession, RepresentationKind};
    let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().map_err(core_error)?;
    let _cancellation = AttemptCancellation::new(&arena, tx.workflow_deadline());
    let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
    // Same canonical emitter over borrowed input fields; no naked key.bytes()
    // allocation occurs outside the audit owner in this original branch.
    let (raw, kind) = match &input {
        OriginalQuery::Status(raw) => (*raw, RepresentationKind::Record),
        OriginalQuery::Retry(incoming) => (incoming.raw(), RepresentationKind::OriginalRequest),
    };
    let incoming = session
        .parse_representation(raw, kind)
        .map_err(core_error)?;
    let key_value = match &input {
        OriginalQuery::Status(_) => incoming.value(),
        OriginalQuery::Retry(_) => request_wrapper::field(incoming.value(), "key", &mut session)?,
    };
    request_wrapper::key(key_value, key, &mut session)?;
    let identity = session
        .canonical_representation(key_value, RepresentationKind::Record)
        .map_err(core_error)?;
    let acquired = prefix::target(tx, key, identity, &mut session).await?;
    let original = acquired.checked_wrappers(tx, &mut session).await?;
    // The chronological recipe driver must finish the exact saved program,
    // original prestate, full wrapper/result closure and all capacity/funding
    // obligations first. For Retry, normalize_saved_intent receives this SAME
    // incoming request and that completed candidate's normalized bytes before
    // any receipt can return. No current retry bytes replace saved raw evidence.
    let _unresolved = (original, incoming, input);
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
                    return original_retry(tx, &req).await;
                }
                let management = ManagementObservation::capture(self, tx, key).await?;
                management.recheck(self, tx, key).await?;
                if req.kind() == "work.accept" {
                    let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new()
                        .map_err(core_error)?;
                    let _cancellation = AttemptCancellation::new(&arena, tx.workflow_deadline());
                    let mut session = ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
                        .map_err(core_error)?;
                    let _observed = current::work(self, tx, key, req.payload(), &mut session).await?;
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
