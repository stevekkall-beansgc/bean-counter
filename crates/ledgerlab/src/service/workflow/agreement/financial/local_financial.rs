//! Private local financial preparation. The financial interpreter/effects gate
//! remains distinct from the experimental future-resource certificate.
use super::*;
use ledgerlab_core::{
    policy::workflow::transition_constructor::{EconomicCandidate, LocalFinancialRequestEvidence},
    validation_work::OriginalAuditSession,
};
struct PreparedFinancialCommit {
    observed: current::app::ObservedAppBinding,
    candidate: EconomicCandidate,
    evidence: LocalFinancialRequestEvidence,
}
impl PreparedFinancialCommit {
    async fn work(
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        req: &ingress::IncomingRequest,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let payload = req.payload(s)?;
        let mut observed = if req.kind() == "work.accept" {
            current::app::observe(ledger, tx, req.key(), payload, s).await?
        } else {
            current::app::observe_nonwork(ledger, tx, req.key(), payload, s).await?
        };
        require(
            observed
                .original
                .publication
                .program()
                .uses_local_execution_recipe(),
            "UNSUPPORTED_SCHEMA_PROFILE",
        )?;
        let (candidate, evidence) = if observed.is_work_alias() {
            current::app::construct_work_alias(&mut observed, ledger, tx, req, s).await?
        } else {
            observed.construct_local(req.key(), payload, req.raw(), s)?
        };
        require(
            candidate.result_metadata().is_local_execution(),
            "INTEGRITY_FAILURE",
        )?;
        Ok(Self {
            observed,
            candidate,
            evidence,
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
        let journal = wire::frame_candidate(
            &self.candidate,
            req.key(),
            req.kind(),
            self.evidence.value(),
            self.observed.received_at()?,
            &self.observed.at,
            self.observed.before,
            s,
        )?;
        let reply = WorkflowReply::financial(&journal, &self.evidence, s)?;
        let append = journal.append_view();
        management.recheck(ledger, tx, req.key(), s).await?;
        tx.workflow_financial_append(&append, s)
            .await
            .map_err(prefix::read_error)?;
        Ok(reply)
    }
}
pub(super) async fn prepare_work_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    management: &ManagementObservation,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply> {
    PreparedFinancialCommit::work(ledger, tx, req, s)
        .await?
        .append(ledger, tx, req, management, s)
        .await
}

/// The caller's held local owner does not confer participant authority. Build
/// the full current selected-role capture and pure candidate before deriving its
/// private freshness scope; the same writer and owner then reach append.
pub(super) async fn prepare_registered_participant_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    held: crate::service::workflow::local::OwnedLocalManagement,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply> {
    let prepared = PreparedFinancialCommit::work(ledger, tx, req, s).await?;
    let management = ManagementObservation::capture_registered_participant(
        ledger,
        tx,
        req,
        held,
        &prepared.observed,
        s,
    )
    .await?;
    prepared.append(ledger, tx, req, &management, s).await
}

#[cfg(test)]
pub(super) async fn check_report_prepare_without_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let prepared = PreparedFinancialCommit::work(ledger, tx, req, s).await?;
    use ledgerlab_core::policy::workflow::transition_constructor::FinancialHeadKey;
    assert!(prepared.candidate.introduced().iter().all(|record| {
        !matches!(
            record.reference().kind.as_str(),
            "workflow-action" | "workflow-family-state" | "workflow-capacity-retirement"
        )
    }));
    let decision = prepared
        .candidate
        .introduced()
        .iter()
        .find(|record| record.reference().kind == "workflow-decision")
        .unwrap();
    let decision: Value = serde_json::from_slice(decision.canonical_body()).unwrap();
    assert_eq!(decision["subject_ref"], req.payload(s)?["work_ref"]);
    assert_eq!(decision["status"], "reversed");
    for head in prepared.candidate.heads() {
        match head.key() {
            FinancialHeadKey::Decision { decision_id } => {
                assert_eq!(decision_id, decision["decision_id"].as_str().unwrap())
            }
            FinancialHeadKey::Capacity { root_decision_id } => {
                assert_eq!(root_decision_id, decision["decision_id"].as_str().unwrap())
            }
            FinancialHeadKey::Chain { .. } => {}
            _ => panic!("report reversal affected another root's app or family"),
        }
    }
    let _journal = wire::frame_candidate(
        &prepared.candidate,
        req.key(),
        req.kind(),
        prepared.evidence.value(),
        prepared.observed.received_at()?,
        &prepared.observed.at,
        prepared.observed.before,
        s,
    )?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn check_retention_prepare_without_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let held = ledger
        .admin
        .read_management(&ledger.store, tx, req.key(), s)
        .await?;
    let prepared = PreparedFinancialCommit::work(ledger, tx, req, s).await?;
    let management = ManagementObservation::capture_registered_participant(
        ledger,
        tx,
        req,
        held,
        &prepared.observed,
        s,
    )
    .await?;
    management.recheck(ledger, tx, req.key(), s).await?;
    assert!(prepared
        .candidate
        .introduced()
        .iter()
        .all(|record| !matches!(
            record.reference().kind.as_str(),
            "workflow-action" | "workflow-decision" | "workflow-target" | "workflow-family-state"
        )));
    let _journal = wire::frame_candidate(
        &prepared.candidate,
        req.key(),
        req.kind(),
        prepared.evidence.value(),
        prepared.observed.received_at()?,
        &prepared.observed.at,
        prepared.observed.before,
        s,
    )?;
    Ok(())
}
