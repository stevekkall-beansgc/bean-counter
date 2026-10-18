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
