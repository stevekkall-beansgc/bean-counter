//! Private initial local activation, retaining actual current source/party
//! checks and the held absent-head prestate through the existing atomic writer.
use super::*;
use ledgerlab_core::validation_work::{OriginalAuditSession,RepresentationKind};
use crate::store::sqlite::workflow::financial::{AppActivationAppend,FinancialDocument};
pub(super) async fn prepare_append(ledger:&LocalWorkflowLedger,tx:&mut SqliteTx,req:&ingress::IncomingRequest,management:&ManagementObservation,s:&mut OriginalAuditSession)->Result<WorkflowReply>{
 let payload=req.payload(s)?;
 request_wrapper::same_text(request_wrapper::text(payload,"schema",s)?,"ledger-workflow-activation/1",s)?;
 let observed=current::app::observe_activation(ledger,tx,req,payload,s).await?;
 require(observed.publication().program().uses_local_execution_recipe(),"UNSUPPORTED_SCHEMA_PROFILE")?;
 let admission=observed.admission_records.as_ref().ok_or(ServiceError::IntegrityFailure)?;
 let parsed=s.parse_representation(observed.normalized_payload.bytes(),RepresentationKind::Record).map_err(core_error)?;
 request_wrapper::same_text(request_wrapper::text(parsed.value(),"expected_revision",s)?,"0",s)?;
 let generated=[&admission.root,&admission.authority,&observed.records.intent,&observed.records.head,&observed.records.activation];
 let original=&observed.binding.original;
 let selected=original.acceptance.work_locator_records().map(|r|r.reference)
   .chain(original.publication.required_refs())
   .chain(observed.binding.sources.iter().flat_map(current::source::ObservedSource::record_refs))
   .chain(observed.binding.parties.iter().flat_map(|p|p.selected_refs()));
 let mut members=prefix::ResolvedMembers::local_selected(tx,req.key(),observed.before(),generated.into_iter(),selected,s).await?;
 let authority=s.parse_representation(admission.authority.bytes(),RepresentationKind::Authority).map_err(core_error)?;
 for r in request_wrapper::field(authority.value(),"app_consent_verification_refs",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?{
  let(r,_owner)=current::source::from_value(r,"workflow-verification",s)?;
  prefix::verify_selected_association(tx,req.key(),&r,observed.before(),s).await?;members.add_record_ref(&r,s)?;
 }
 let evidence=evidence::activation_material(req,observed.normalized_payload.bytes(),observed.publication(),s)?;
 let journal=wire::frame_activation(req,observed.normalized_payload.bytes(),observed.authority()?,observed.at(),observed.before(),observed.result_refs(),&evidence,members,s)?;
 let reply=WorkflowReply::agreement(&journal,&evidence,s)?;
 let _storage=s.reserve_external_payload(5*std::mem::size_of::<FinancialDocument>()+journal.members().len()*std::mem::size_of::<&ledgerlab_core::policy::workflow::transition_constructor::ExactRef>()).map_err(core_error)?;
 let documents:Vec<_>=generated.iter().map(|r|FinancialDocument{reference:r.reference(),schema:r.schema(),canonical_bytes:r.bytes()}).collect();
 let mut refs=Vec::with_capacity(journal.members().len());for i in 0..journal.members().len(){let r=journal.members().at(i,s)?;if r!=journal.operation_reference(){refs.push(r);}}
 let journal_view=journal.storage_view(req,&evidence,&refs,observed.before(),observed.at().micros(),s)?;
 let binding=s.parse_representation(original.acceptance.binding.bytes(),RepresentationKind::Record).map_err(core_error)?;
 let activation=s.parse_representation(observed.records.activation.bytes(),RepresentationKind::Record).map_err(core_error)?;
 let effective=Timestamp::parse_with_work(request_wrapper::text(activation.value(),"effective_at",s)?,s.semantic_work()).map_err(core_error)?;
 let append=AppActivationAppend{logical_store_id:&req.key().logical_store_id,scope:&req.key().scope,source:&req.key().source,operation_id:&req.key().operation_id,
 documents:&documents,journal:journal_view,customer:request_wrapper::text(binding.value(),"customer",s)?,agreement:request_wrapper::text(binding.value(),"agreement_id",s)?,
 expected_revision:0,revision:1,head:observed.records.head.reference(),activation:observed.records.activation.reference(),binding:original.acceptance.binding.reference(),intent:observed.records.intent.reference(),effective_at_us:effective.micros()};
 management.recheck(ledger,tx,req.key(),s).await?;
 tx.workflow_app_activation_append(&append,s).await.map_err(prefix::read_error)?;
 Ok(reply)
}

/// Routing DATA only. The selected writer independently reloads and validates
/// the exact original agreement before preparation; no decision comes from this read.
pub(super) async fn selected(ledger:&LocalWorkflowLedger,raw:&[u8],s:&mut OriginalAuditSession)->Result<bool>{
 use crate::store::ports::{AcceptanceStore,AcceptanceTx};
 let req=super::super::request::Request::parse_agreement(raw)?;
 if req.kind()!="agreement.activate"{return Ok(false);}
 let Some(r)=req.payload().get("accepted_binding_ref") else{return Ok(false)};
 let mut tx=ledger.store.begin(deadline()).await.map_err(crate::service::store_error)?;
 let result=async{
  ledger.admin.read(&ledger.store,&mut tx,req.key()).await?;
  let before=tx.workflow_sequence().await.map_err(crate::service::store_error)?;
  let identity=req.key().bytes()?;
  if tx.workflow_original_identity_exists(&identity,s).await.map_err(prefix::read_error)? {
    let owned=s.copy_representation(&identity,RepresentationKind::Record).map_err(core_error)?;
    let old=prefix::target(&mut tx,req.key(),owned,s).await?.checked_wrappers(&mut tx,s).await?;
    let e=request_wrapper::field(old.wrappers().operation(),"request_evidence",s)?;
    let d=request_wrapper::field(e,"descriptor_resolution",s)?;
    return Ok(request_wrapper::text(d,"registry_digest",s)?==wrappers::LOCAL_REGISTRY);
  }
  let(reference,_lease)=current::source::from_value(r,"workflow-binding",s)?;
  let stored=tx.workflow_financial_document(&reference,&req.key().logical_store_id,&req.key().scope,before,s).await.map_err(prefix::read_error)?.ok_or(ServiceError::IntegrityFailure)?;
  if stored.schema.bytes()!=b"ledger-workflow-binding/2" {return Ok(false);}
  let binding=current::source::retained_record(&mut tx,req.key(),r,"workflow-binding","ledger-workflow-binding/2",before,s).await?;
  let policy_ref=request_wrapper::field(binding.body(),"policy_ref",s)?;
  let policy=current::source::retained_record(&mut tx,req.key(),policy_ref,"workflow-policy","ledger-workflow-policy/4",before,s).await?;
  let descriptor=ledgerlab_core::policy::workflow::selected_program::local_compiler_descriptor_with_session(s).map_err(core_error)?;
  let actual=request_wrapper::field(policy.body(),"compiler_ref",s)?;
  let expected=serde_json::to_value(descriptor.reference()).map_err(|_|ServiceError::IntegrityFailure)?;
  Ok(actual==&expected)
 }.await;
 tx.rollback().await.map_err(crate::service::store_error)?;result
}
