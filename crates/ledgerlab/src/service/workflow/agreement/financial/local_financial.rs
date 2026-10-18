//! Private local financial preparation. The financial interpreter/effects gate
//! remains distinct from the experimental future-resource certificate.
use super::*;
use ledgerlab_core::{policy::workflow::transition_constructor::{EconomicCandidate,LocalFinancialRequestEvidence},validation_work::OriginalAuditSession};
struct PreparedFinancialCommit{
 observed:current::app::ObservedAppBinding,
 candidate:EconomicCandidate,
 evidence:LocalFinancialRequestEvidence,
}
impl PreparedFinancialCommit{
 async fn work(ledger:&LocalWorkflowLedger,tx:&mut SqliteTx,req:&ingress::IncomingRequest,s:&mut OriginalAuditSession)->Result<Self>{
  let payload=req.payload(s)?;
  let observed=if req.kind()=="work.accept"{current::app::observe(ledger,tx,req.key(),payload,s).await?}else{current::app::observe_nonwork(ledger,tx,req.key(),payload,s).await?};
  require(observed.original.publication.program().uses_local_execution_recipe(),"UNSUPPORTED_SCHEMA_PROFILE")?;
  let(candidate,evidence)=observed.construct_local(req.key(),payload,req.raw(),s)?;
  require(candidate.result_metadata().is_local_execution(),"INTEGRITY_FAILURE")?;
  Ok(Self{observed,candidate,evidence})
 }
 async fn append(self,ledger:&LocalWorkflowLedger,tx:&mut SqliteTx,req:&ingress::IncomingRequest,management:&ManagementObservation,s:&mut OriginalAuditSession)->Result<WorkflowReply>{
  let journal=wire::frame_candidate(&self.candidate,req.key(),req.kind(),self.evidence.value(),self.observed.received_at()?,&self.observed.at,self.observed.before,s)?;
  let reply=WorkflowReply::financial(&journal,&self.evidence,s)?;
  let append=journal.append_view();
  management.recheck(ledger,tx,req.key(),s).await?;
  tx.workflow_financial_append(&append,s).await.map_err(prefix::read_error)?;
  Ok(reply)
 }
}
pub(super) async fn prepare_work_append(ledger:&LocalWorkflowLedger,tx:&mut SqliteTx,req:&ingress::IncomingRequest,management:&ManagementObservation,s:&mut OriginalAuditSession)->Result<WorkflowReply>{
 PreparedFinancialCommit::work(ledger,tx,req,s).await?.append(ledger,tx,req,management,s).await
}
