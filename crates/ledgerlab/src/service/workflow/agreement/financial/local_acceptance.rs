//! Adopted local acceptance preparation. The same terms, nominations, original
//! and current source rights and full consent checks precede atomic head CAS.
use super::*;
use ledgerlab_core::validation_work::OriginalAuditSession;
struct PreparedAcceptance {
    pending: acceptance::PendingAppAcceptance,
    evidence: evidence::RequestEvidence,
    journal: wire::AgreementJournalData,
    reply: WorkflowReply,
}
impl PreparedAcceptance {
    async fn prepare(
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        req: &ingress::IncomingRequest,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let mut pending = acceptance::prepare_local(ledger, tx, req, s).await?;
        let evidence = evidence::acceptance(req, &pending, s)?;
        let members = pending.resolve_members(s)?;
        let journal = wire::frame_acceptance(
            req,
            pending.normalized_payload(),
            pending.authority(),
            &pending.at,
            pending.before,
            pending.results(),
            &evidence,
            members,
            s,
        )?;
        let reply = WorkflowReply::agreement(&journal, &evidence, s)?;
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
        let write = acceptance::AcceptanceWriteData::new(
            &self.pending,
            req,
            &self.journal,
            &self.evidence,
            s,
        )?;
        let append = write.append_view(s)?;
        management.recheck(ledger, tx, req.key(), s).await?;
        tx.workflow_app_acceptance_append(&append, s)
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
    PreparedAcceptance::prepare(ledger, tx, req, s)
        .await?
        .append(ledger, tx, req, management, s)
        .await
}

/// Exact local acceptance originals share one complete historical certificate
/// for both status and identity retry. Other recipes retain their old dispatch.
pub(super) async fn verify_original(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    incoming: Option<&ingress::IncomingRequest>,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    let w = origin.wrappers();
    if w.class() != wrappers::WireClass::Agreement
        || request_wrapper::text(w.operation(), "kind", s)? != "agreement.accept"
    {
        return Ok(false);
    }
    let evidence = request_wrapper::field(w.operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    if request_wrapper::text(resolution, "registry_digest", s)? != wrappers::LOCAL_REGISTRY {
        return Ok(false);
    }
    let saved = ingress::IncomingRequest::parse(w.original_request().raw(), s)?;
    if request_wrapper::text(saved.payload(s)?, "schema", s)?
        != "ledger-workflow-agreement-accept/9"
    {
        return Ok(false);
    }
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Add,
            &[&w.sequence().into(), &1i64.into()],
        )
        .map_err(core_error)?;
    let before = w
        .sequence()
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut accepted = None;
    for r in request_wrapper::field(w.receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if request_wrapper::text(r, "kind", s)? == "workflow-accepted-agreement" {
            require(accepted.replace(r).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    let reference = accepted.ok_or(ServiceError::IntegrityFailure)?;
    origin.require_member(reference, s)?;
    let accepted = current::source::retained_record(
        tx,
        origin.key(),
        reference,
        "workflow-accepted-agreement",
        "ledger-workflow-accepted-agreement/1",
        before,
        s,
    )
    .await?;
    let complete = prefix::acceptance_for_original(
        ledger,
        tx,
        origin.key(),
        request_wrapper::field(accepted.body(), "binding_ref", s)?,
        before,
        s,
    )
    .await?;
    complete.require_original_sequence(w.sequence(), s)?;
    if let Some(incoming) = incoming {
        request_wrapper::key(
            request_wrapper::field(incoming.value(), "key", s)?,
            origin.key(),
            s,
        )?;
        complete.compare_original_retry(tx, incoming, s).await?;
    }
    s.check_cancelled().map_err(core_error)?;
    Ok(true)
}
