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
mod acceptance;
mod activation;
mod current;
mod evidence;
mod ingress;
mod management;
mod local_party;
mod local_publication;
mod local_acceptance;
mod local_activation;
mod local_inputs;
mod local_financial;
mod local_financial_original;
pub(in crate::service::workflow) use management::effective_setup;
use management::ManagementObservation;
mod prefix;
pub(in crate::service::workflow) use prefix::read_error as management_read_error;
mod publication_prepare;
mod recovery;
mod response;
pub use response::WorkflowReply;
mod request_wrapper;
mod wire;
mod wrappers;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Released R1 dependencies may select a root retained by the adopted local
/// recipe. Verify its actual scoped committed frame and membership using that
/// recipe, instead of interpreting it as a released R1 publication certificate.
pub(super) async fn local_selected_origin(
 tx:&mut SqliteTx,key:&OperationKey,reference:&RecordRef,sequence:i64,
)->Result<bool>{
 let mut s=take_read_session(tx)?;
 let result=async {
 let prior=sequence.checked_sub(1).ok_or(ServiceError::IntegrityFailure)?;
 let old=prefix::next(tx,key,prior,sequence,&mut s).await?.ok_or(ServiceError::IntegrityFailure)?;
 // Inspect only the actual retained registry; all local wrapper/domain/receipt
 // checks execute before this association is accepted.
 let old=old.checked_wrappers(tx,&mut s).await?;
 let op=old.wrappers().operation();
 let ev=request_wrapper::field(op,"request_evidence",&mut s)?;
 let resolution=request_wrapper::field(ev,"descriptor_resolution",&mut s)?;
 if request_wrapper::text(resolution,"registry_digest",&mut s)?!=wrappers::LOCAL_REGISTRY{return Ok(false);}
 let value=serde_json::to_value(reference).map_err(|_|ServiceError::IntegrityFailure)?;
 old.require_member(&value,&mut s)?;
 s.check_cancelled().map_err(core_error)?;Ok(true)
 }.await;
 retain_read_session(tx,s);
 result
}

pub(super) async fn local_publication_origin(tx:&mut SqliteTx,key:&OperationKey,sequence:i64)->Result<bool>{
 let mut s=take_read_session(tx)?;
 let result=async {
 let previous=sequence.checked_sub(1).ok_or(ServiceError::IntegrityFailure)?;
 let old=prefix::next(tx,key,previous,sequence,&mut s).await?.ok_or(ServiceError::IntegrityFailure)?
    .checked_wrappers(tx,&mut s).await?;
 Ok(local_publication::verify_original(tx,&old,&mut s).await?.is_some())
 }.await;
 retain_read_session(tx,s);
 result
}

/// Explicit local enable coordinator only. The mechanical migrations establish
/// empty projection storage, never financial eligibility or supplier rights.
/// Keep the completed original owners alive through all three schema steps.
pub(in crate::service::workflow) async fn enable_app_schemas(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
) -> Result<()> {
    proof_session::with_proofs(tx, key, |tx| {
        Box::pin(async move {
            let arena =
                tx.workflow_arena().map_err(core_error)?;
            let _cancellation = AttemptCancellation::new(&arena, tx.workflow_deadline());
            let mut session =
                ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(
                    &arena,
                )
                .map_err(core_error)?;
            let _prefix =
                prefix::authenticate_migration_prefix(ledger, tx, key, &mut session).await?;
            tx.workflow_enable_financial_schema()
                .await
                .map_err(crate::service::store_error)?;
            tx.workflow_enable_supplier_schema()
                .await
                .map_err(crate::service::store_error)?;
            tx.workflow_enable_app_schema()
                .await
                .map_err(crate::service::store_error)?;
            tx.workflow_enable_assertion_schema()
                .await
                .map_err(crate::service::store_error)?;
            session.check_cancelled().map_err(core_error)
        })
    })
    .await
}

impl LocalWorkflowLedger {
 pub async fn local_activation_owned(&self,raw:&[u8])->Result<Option<WorkflowReply>>{
  let snapshot=self.resource_snapshot()?;let arena=snapshot.arena()?;
  let mut session=ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
  let _cancellation=AttemptCancellation::new(&arena,snapshot.deadline()?);
  if local_activation::selected(self,raw,&mut session).await?{if snapshot.settings.is_some(){self.financial_owned_reply_in(raw,snapshot,session).await.map(Some)}else{self.financial_owned_reply(raw).await.map(Some)}}else{Ok(None)}
 }
}

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

/// The actual request lifetime and held-writer deadline feed the same terminal
/// core cancellation flag. This creates no proof state or additional budget.
pub(crate) struct AttemptCancellation {
    flag: ledgerlab_core::canonical::audit_arena::AuditCancellation,
    timer: Option<tokio::task::JoinHandle<()>>,
}
impl AttemptCancellation {
    pub(crate) fn new(
        arena: &ledgerlab_core::canonical::audit_arena::AuditArena,
        deadline: Option<tokio::time::Instant>,
    ) -> Self {
        let flag = arena.cancellation_handle();
        let timed = flag.clone();
        let timer = deadline.map(|deadline|tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            timed.cancel();
        }));
        Self { flag, timer }
    }
}
impl Drop for AttemptCancellation {
    fn drop(&mut self) {
        self.flag.cancel();
        if let Some(timer)=&self.timer{timer.abort();}
    }
}

/// Submission retry retains the actual incoming request until ORIGINAL intent
/// normalization/comparison; key-only status never manufactures that intent.
enum OriginalQuery<'a> {
    Status(&'a [u8]),
    Retry(&'a ingress::IncomingRequest),
}

// This disposition is private to the coordinator. A core DATA result is never
// converted into Appended: that branch is reserved for the actual prepared
// append's successful return under the held proof scope. It has no public
// factory, caller flag or serialization. No present fresh branch produces it.
enum TransactionResult {
    Original(Value),
    OriginalOwned(WorkflowReply),
    Appended(WorkflowReply),
}

#[cfg(test)]
static LOST_REPLY_KEY: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(super) fn inject_committed_reply_loss(key: Vec<u8>) {
    let mut selected = LOST_REPLY_KEY.lock().unwrap();
    assert!(selected.is_none());
    *selected = Some(key);
}

/// Same commit outcome semantics as the administrative coordinator. This does
/// not decide whether a candidate is eligible, nor append anything itself.
/// In particular a timed-out/unknown COMMIT is never followed by a claim that
/// ROLLBACK undid it. The next submission resolves the original identity.
async fn finish_transaction(
    tx: SqliteTx,
    key: &OperationKey,
    attempted: Result<TransactionResult>,
    mut session: ledgerlab_core::validation_work::OriginalAuditSession,
    resources:Option<&crate::service::workflow::resources::Snapshot>,
) -> Result<WorkflowReply> {
    match attempted {
        Err(error) => {
            tx.rollback().await.map_err(crate::service::store_error)?;
            Err(error)
        }
        Ok(TransactionResult::OriginalOwned(response)) => {
            tx.rollback().await.map_err(crate::service::store_error)?;
            Ok(response)
        }
        Ok(TransactionResult::Original(response)) => {
            let response=WorkflowReply::original(&response,&mut session);
            tx.rollback().await.map_err(crate::service::store_error)?;
            response
        }
        Ok(TransactionResult::Appended(response)) => {
            if let Some(snapshot)=resources {if let Err(e)=snapshot.recheck(){tx.rollback().await.map_err(crate::service::store_error)?;return Err(e);}}
            tx.commit_with_resource_session(session).await.map_err(|error| match error {
                crate::store::sqlite::FinancialCommitError::Resource(error)=>core_error(error),
                crate::store::sqlite::FinancialCommitError::Commit(error)=>match error {
                crate::store::errors::CommitError::RolledBack(error) => {
                    crate::service::store_error(error)
                }
                crate::store::errors::CommitError::OutcomeUnknown => ServiceError::OutcomeUnknown {
                    scope: key.scope.clone(),
                    source: key.source.clone(),
                    external_id: key.operation_id.clone(),
                },
                },
            })?;
            #[cfg(test)]
            {
                let mut selected = LOST_REPLY_KEY.lock().unwrap();
                if selected.as_ref() == Some(&key.bytes()?) {
                    *selected = None;
                    return Err(ServiceError::ResponseLost);
                }
            }
            Ok(response)
        }
    }
}

pub(in crate::service::workflow) async fn local_management_status(
    tx:&mut SqliteTx,key:&OperationKey,raw_key:&[u8],
)->Result<Option<Value>> {
    use ledgerlab_core::validation_work::{OriginalAuditSession,RepresentationKind};
 let mut s=take_read_session(tx)?;
 let result=async {
    let parsed=s.parse_representation(raw_key,RepresentationKind::Record).map_err(core_error)?;
    request_wrapper::key(parsed.value(),key,&mut s)?;
    let identity=s.canonical_representation(parsed.value(),RepresentationKind::Record).map_err(core_error)?;
    let acquired=prefix::target(tx,key,identity,&mut s).await?;
    let original=acquired.checked_wrappers(tx,&mut s).await?;
    if !local_party::verify_original(tx,&original,&mut s).await?
        &&local_publication::verify_original(tx,&original,&mut s).await?.is_none() {return Ok(None);}
    WorkflowReply::status(&original,&mut s).map(|r|Some(r.into_legacy_value()))
 }.await;
 retain_read_session(tx,s);
 result
}
pub(in crate::service::workflow) async fn original_status(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    raw_key: &[u8],
) -> Result<Value> {
    original_query(ledger, tx, key, OriginalQuery::Status(raw_key)).await
}
async fn original_retry(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    incoming: &ingress::IncomingRequest,
    session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
) -> Result<Value> {
    original_query_in(
        ledger,
        tx,
        incoming.key(),
        OriginalQuery::Retry(incoming),
        session,
    )
    .await
}

/// Never returns saved success without full original financial semantics. The
/// incoming request selects only the global identity; saved evidence supplies
/// the ORIGINAL literal request. No current compiler/grant/time is consulted.
async fn original_query(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: OriginalQuery<'_>,
) -> Result<Value> {
    use ledgerlab_core::validation_work::OriginalAuditSession;
    let snapshot=ledger.resource_snapshot()?;snapshot.bind(tx).await?;
    let mut session=take_read_session(tx)?;
    let result=original_query_in(ledger, tx, key, input, &mut session).await;
    retain_read_session(tx,session);result
}
async fn original_query_in(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: OriginalQuery<'_>,
    session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
) -> Result<Value> {
    use ledgerlab_core::validation_work::RepresentationKind;
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
        OriginalQuery::Retry(_) => request_wrapper::field(incoming.value(), "key", session)?,
    };
    request_wrapper::key(key_value, key, session)?;
    let identity = session
        .canonical_representation(key_value, RepresentationKind::Record)
        .map_err(core_error)?;
    let acquired = prefix::target(tx, key, identity, session).await?;
    let original = acquired.checked_wrappers(tx, session).await?;
    if let OriginalQuery::Retry(request) = &input {
        let saved_kind = request_wrapper::text(original.wrappers().operation(), "kind", session)?;
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(saved_kind.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(request.kind().len())
            .map_err(core_error)?;
        require(saved_kind == request.kind(), "IDENTITY_CONFLICT")?;
    }
    if local_party::verify_original(tx,&original,session).await? {
        input.compare_party_intent(&original,session)?;
        return WorkflowReply::status(&original,session).map(WorkflowReply::into_legacy_value);
    }
    if let Some(publication)=local_publication::verify_original(tx,&original,session).await? {
        if let OriginalQuery::Retry(req)=&input {local_publication::compare_retry(tx,req,&original,&publication,session).await?;}
        return WorkflowReply::status(&original,session).map(WorkflowReply::into_legacy_value);
    }
    let retry=match &input{OriginalQuery::Retry(req)=>Some(*req),_=>None};
    if local_financial_original::verify(tx,&original,retry,session).await? {
        let reply=recovery::party_reply(&original,session)?;
        return if retry.is_some(){WorkflowReply::original(reply.value(),session).map(WorkflowReply::into_legacy_value)}else{WorkflowReply::status(&original,session).map(WorkflowReply::into_legacy_value)};
    }
    let _prefix = prefix::fold_original_prefix(ledger, tx, key, &original, session).await?;
    // The chronological recipe driver must finish the exact saved program,
    // original prestate, full wrapper/result closure and all capacity/funding
    // obligations first. For Retry, normalize_saved_intent receives this SAME
    // incoming request and that completed candidate's normalized bytes before
    // any receipt can return. No current retry bytes replace saved raw evidence.
    let _unresolved = (original, incoming, input);
    Err(ServiceError::Unavailable)
}

impl LocalWorkflowLedger {
    pub async fn publish_local_owned(&self,raw:&[u8])->Result<WorkflowReply> {
        self.publish_selected_with(raw,
            ledgerlab_core::policy::workflow::selected_program::SelectedCompilerSupport::local_execution_with_session).await
    }
    /// Private binary-selected local publication seam. The caller supplies
    /// only the genuine code-owned selector after the original identity check.
    pub(in crate::service::workflow) async fn publish_selected_with<F>(&self,raw:&[u8],fresh_support:F)->Result<WorkflowReply>
    where F:FnOnce(&mut ledgerlab_core::validation_work::OriginalAuditSession)
        ->ledgerlab_core::Result<ledgerlab_core::policy::workflow::selected_program::SelectedCompilerSupport> + Send {
        use ledgerlab_core::validation_work::OriginalAuditSession;
        let resource_snapshot=self.resource_snapshot()?;
        let arena=resource_snapshot.arena()?;
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
        let req=ingress::IncomingRequest::parse_publication(raw,&mut session)?;
        let mut tx=resource_snapshot.begin(&self.store,&mut session).await?;
        let _cancellation=AttemptCancellation::new(&arena,tx.workflow_deadline());
        tx.workflow_main_session=true;
        let attempted=proof_session::with_proofs(&mut tx,req.key(),|tx|Box::pin(async {
            self.admin.read_management(&self.store,tx,req.key(),&mut session).await?;
            if tx.workflow_original_identity_exists(req.identity(),&mut session).await.map_err(prefix::read_error)? {
                return original_retry(self,tx,&req,&mut session).await.map(TransactionResult::Original);
            }
            local_publication::prepare_append(self,tx,raw,fresh_support,&mut session).await.map(TransactionResult::Appended)
        })).await;
        finish_transaction(tx,req.key(),attempted,session,Some(&resource_snapshot)).await
    }
    /// Financial admission is disabled. This validates lexical ingress and
    /// authenticates local management/identity ordering, but creates no receipt,
    /// financial record, claim, reservation, alias, projection or meter delta.
    pub async fn financial(&self,raw:&[u8])->Result<Value> {
        self.financial_owned_reply(raw).await.map(WorkflowReply::into_legacy_value)
    }
    /// New-profile owning endpoint. Exact same request/prepare/commit route;
    /// the CLI retains its rendered output rather than decoding another tree.
    pub async fn financial_owned_reply(&self, raw: &[u8]) -> Result<WorkflowReply> {
        let resource_snapshot=self.resource_snapshot()?;
        let arena=resource_snapshot.arena()?;
        let mut session =
            ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
                .map_err(core_error)?;
        self.financial_owned_reply_in(raw,resource_snapshot,session).await
    }
    async fn financial_owned_reply_in(&self,raw:&[u8],resource_snapshot:crate::service::workflow::resources::Snapshot,mut session:ledgerlab_core::validation_work::OriginalAuditSession)->Result<WorkflowReply>{
        let req = ingress::IncomingRequest::parse(raw, &mut session)?;
        let key = req.key();
        let mut tx=resource_snapshot.begin(&self.store,&mut session).await?;
        let _cancellation = AttemptCancellation::new(session.arena(), tx.workflow_deadline());
        tx.workflow_main_session=true;
        let attempted = proof_session::with_proofs(&mut tx, key, |tx| {
            Box::pin(async {
                let held_management=self.admin.read_management(&self.store, tx, key, &mut session).await?;
                if tx
                    .workflow_original_identity_exists(req.identity(), &mut session)
                    .await
                    .map_err(prefix::read_error)?
                {
                    // Kind and intent are checked from the SAME owning saved
                    // frame below, never an unleased asserted kind column.
                    // No fresh selection, grant check, compiler or capacity admission
                    // precedes this original identity branch.
                    if let Some(reply)=local_party::retry(tx,&req,&mut session).await? {
                        return Ok(TransactionResult::OriginalOwned(reply));
                    }
                    return original_retry(self, tx, &req, &mut session)
                        .await
                        .map(TransactionResult::Original);
                }
                let key_body = request_wrapper::field(req.value(), "key", &mut session)?;
                let management =
                    ManagementObservation::capture_owned(self, tx, key, key_body, held_management, &mut session).await?;
                management.recheck(self, tx, key, &mut session).await?;
                if req.kind() == "agreement.accept" {
                    if request_wrapper::text(req.payload(&mut session)?,"schema",&mut session)?=="ledger-workflow-agreement-accept/9" {
                        let reply=local_acceptance::prepare_append(self,tx,&req,&management,&mut session).await?;
                        return Ok(TransactionResult::Appended(reply));
                    }
                    // AppAccept8 reaches the actual saved publication and SAME
                    // terms/binding assembly. Its DATA owners do not bypass
                    // complete chronology, consent/private graph or preparation.
                    let mut pending = acceptance::prepare(self, tx, &req, &mut session).await?;
                    let evidence = evidence::acceptance(&req, &pending, &mut session)?;
                    let members = pending.resolve_members(&mut session)?;
                    let journal = wire::frame_acceptance(
                        &req,
                        pending.normalized_payload(),
                        pending.authority(),
                        &pending.at,
                        pending.before,
                        pending.results(),
                        &evidence,
                        members,
                        &mut session,
                    )?;
                    let write_data = acceptance::AcceptanceWriteData::new(
                        &pending,
                        &req,
                        &journal,
                        &evidence,
                        &mut session,
                    )?;
                    let _write_operands = write_data.append_view(&mut session)?;
                    // The real atomic writer now has exact borrowed operands;
                    // full original/current/resource private preparation still
                    // precedes any call to workflow_app_acceptance_append.
                }
                if req.kind()=="agreement.activate" && request_wrapper::text(req.payload(&mut session)?,"schema",&mut session)?=="ledger-workflow-activation/1" {
                    let reply=local_activation::prepare_append(self,tx,&req,&management,&mut session).await?;
                    return Ok(TransactionResult::Appended(reply));
                }
                if matches!(req.kind(), "agreement.activate" | "agreement.revoke") {
                    let payload = req.payload(&mut session)?;
                    let mut observed =
                        current::app::observe_activation(self, tx, &req, payload, &mut session)
                            .await?;
                    if req.kind() == "agreement.activate" {
                        let evidence = evidence::activation_material(
                            &req,
                            observed.normalized_payload.bytes(),
                            observed.publication(),
                            &mut session,
                        )?;
                        let members = observed.resolve_members(&mut session)?;
                        let _journal = wire::frame_activation(
                            &req,
                            observed.normalized_payload.bytes(),
                            observed.authority()?,
                            observed.at(),
                            observed.before(),
                            observed.result_refs(),
                            &evidence,
                            members,
                            &mut session,
                        )?;
                    }
                    // Exact captured head and generated records remain DATA.
                    // Whole historical schedule, original/current consent and
                    // activation rights must finish before private preparation.
                }

                if matches!(req.kind(), "evidence.retain" | "authority.change") {
                    let payload = req.payload(&mut session)?;
                    let schema = request_wrapper::text(payload, "schema", &mut session)?;
                    if matches!(
                        schema,
                        "ledger-workflow-party-evidence-retain/1"
                            | "ledger-workflow-party-evidence-retain/2"
                            | "ledger-workflow-party-authorize/1"
                            | "ledger-workflow-party-revoke/1"
                    ) {
                        let reply=local_party::prepare_append(self,tx,&req,&management,&mut session).await?;
                        return Ok(TransactionResult::Appended(reply));
                    }
                }

                if matches!(request_wrapper::text(req.payload(&mut session)?,"schema",&mut session)?,
                    "ledger-workflow-work/1"|"ledger-workflow-fact-attest/1"|"ledger-workflow-family-close-attest/1"|
                    "ledger-workflow-app-observe/1"|"ledger-workflow-app-cost-attest/1"|"ledger-workflow-cost-finalize/2"|
                    "ledger-workflow-cost-correct/2"|"ledger-workflow-outcome/1"|"ledger-workflow-outcome-correction/1"|
                    "ledger-workflow-outcome-close/2"|"ledger-workflow-quantity-correction/2"|"ledger-workflow-work-reverse/2") {
                    let reply=local_financial::prepare_work_append(self,tx,&req,&management,&mut session).await?;
                    return Ok(TransactionResult::Appended(reply));
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
        // Fresh admission remains unavailable: no current path can construct
        // the Appended disposition. Failure always rolls back. The original
        // path retains its incoming intent and also rolls back read-only work.
        finish_transaction(tx, key, attempted, session,Some(&resource_snapshot)).await
    }
}

#[cfg(test)]
mod tests;

fn take_read_session(tx:&mut SqliteTx)->Result<ledgerlab_core::validation_work::OriginalAuditSession>{
 if tx.workflow_snapshot.as_ref().is_some_and(|v|v.settings.is_some()) {require(!tx.workflow_main_session&&!tx.workflow_read_active,"INTEGRITY_FAILURE")?;}
 tx.workflow_read_active=true;
 if let Some(s)=tx.workflow_read_session.take(){return Ok(s);}
 // Only legacy/R1 origin adapters enter here; the new financial graph passes
 // its existing mutable session explicitly and never invokes these adapters.
 let arena=tx.workflow_arena().map_err(core_error)?;
 let s=ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
 tx.workflow_read_cancellation=Some(AttemptCancellation::new(&arena,tx.workflow_deadline()));Ok(s)
}

fn retain_read_session(tx:&mut SqliteTx,s:ledgerlab_core::validation_work::OriginalAuditSession){
 tx.workflow_read_active=false;
 if tx.workflow_snapshot.as_ref().is_some_and(|v|v.settings.is_some()){tx.workflow_read_session=Some(s);}else{tx.workflow_read_cancellation.take();}
}
