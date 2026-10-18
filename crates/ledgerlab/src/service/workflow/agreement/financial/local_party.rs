//! The adopted local party recipe. Only this private coordinator combines the
//! actual held authority, selected committed inputs and prepared journal with
//! the existing atomic writer. Historical recipes keep their original path.
use super::*;
use ledgerlab_core::validation_work::{OriginalAuditSession, RepresentationKind};

struct PreparedPartyCommit {
    pending: current::party::produce::PendingPartyOperation,
    evidence: evidence::RequestEvidence,
    journal: wire::PartyJournalData,
    reply: WorkflowReply,
}
impl PreparedPartyCommit {
    async fn prepare(
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        req: &ingress::IncomingRequest,
        management: &ManagementObservation,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let mut pending =
            current::party::produce::prepare_local_observed(ledger, tx, req, management.held(), s)
                .await?;
        let evidence = evidence::local_party(req, &pending, s)?;
        let members = pending.resolve_members(s)?;
        let journal = wire::frame_party(req, &pending, &evidence, pending.before, members, s)?;
        let reply = WorkflowReply::party(&journal, &evidence, s)?;
        Ok(Self {
            pending,
            evidence,
            journal,
            reply,
        })
    }
    async fn append(
        self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        req: &ingress::IncomingRequest,
        management: &ManagementObservation,
        s: &mut OriginalAuditSession,
    ) -> Result<WorkflowReply> {
        let write = current::party::write::PartyWriteData::new(
            &self.pending,
            req,
            &self.evidence,
            &self.journal,
            s,
        )?;
        let operands = write.append_view(s)?;
        // Recheck the same transaction, owner, literal key, unchanged database
        // baseline and live filesystem immediately before our first mutation.
        management.recheck(ledger, tx, req.key(), s).await?;
        tx.workflow_party_append(&operands, s)
            .await
            .map_err(prefix::read_error)?;
        Ok(self.reply)
    }
}
pub(super) async fn prepare_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    management: &ManagementObservation,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply> {
    PreparedPartyCommit::prepare(ledger, tx, req, management, s)
        .await?
        .append(ledger, tx, req, management, s)
        .await
}

/// Saved local receipts retain their original literal request, normalization,
/// canonical results and exact selected dependency associations. No current
/// mapping/rights/policy or generic chronological replay is used on this path.
pub(super) async fn verify_original(
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    let operation = original.wrappers().operation();
    let evidence = request_wrapper::field(operation, "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    let registry = request_wrapper::text(resolution, "registry_digest", s)?;
    if registry != wrappers::LOCAL_REGISTRY {
        return Ok(false);
    }
    let normalized = request_wrapper::field(evidence, "normalized_request", s)?;
    let payload = request_wrapper::field(normalized, "payload", s)?;
    let schema = request_wrapper::text(payload, "schema", s)?;
    if !matches!(
        schema,
        "ledger-workflow-party-authorize/1"
            | "ledger-workflow-party-revoke/1"
            | "ledger-workflow-party-evidence-retain/1"
            | "ledger-workflow-party-evidence-retain/2"
    ) {
        return Ok(false);
    }
    let pending = current::party::produce::reconstruct(tx, original, s).await?;
    let evidence = evidence::original_party(original, &pending, s)?;
    let incoming =
        ingress::IncomingRequest::parse(original.wrappers().original_request().raw(), s)?;
    let members = prefix::ResolvedMembers::resolve_local_party(
        tx,
        original.key(),
        pending.before,
        &[&pending.root, &pending.authority, &pending.result],
        pending.dependency_refs(),
        s,
    )
    .await?;
    wire::frame_party(&incoming, &pending, &evidence, pending.before, members, s)?
        .compare_original(original, s)?;
    // All retained result bytes and full Refs were compared by the producer;
    // the complete saved wrapper/manifest/receipt is compared above.
    s.check_cancelled().map_err(core_error)?;
    Ok(true)
}
pub(super) async fn retry(
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    s: &mut OriginalAuditSession,
) -> Result<Option<WorkflowReply>> {
    let key_body = request_wrapper::field(req.value(), "key", s)?;
    let identity = s
        .canonical_representation(key_body, RepresentationKind::Record)
        .map_err(core_error)?;
    let acquired = prefix::target(tx, req.key(), identity, s).await?;
    let original = acquired.checked_wrappers(tx, s).await?;
    if !verify_original(tx, &original, s).await? {
        return Ok(None);
    }
    OriginalQuery::Retry(req).compare_party_intent(&original, s)?;
    let reply = recovery::party_reply(&original, s)?;
    WorkflowReply::original(reply.value(), s).map(Some)
}
