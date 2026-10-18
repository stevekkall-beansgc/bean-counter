//! Original local receipts: saved recipe and intent, exact canonical retained
//! records and committed associations. No rerating or current mutation rights.
use super::*;
use ledgerlab_core::{validation_work::{OriginalAuditSession,RepresentationKind},policy::workflow::transition_constructor::{OperationKey as CoreKey,locate_local_subject_binding,normalize_local_saved_intent}};

pub(super) async fn verify(tx:&mut SqliteTx,original:&prefix::LocatedWrappers,incoming:Option<&ingress::IncomingRequest>,s:&mut OriginalAuditSession)->Result<bool>{
 let w=original.wrappers();if w.class()!=wrappers::WireClass::Financial{return Ok(false);}
 let evidence=request_wrapper::field(w.operation(),"request_evidence",s)?;
 let resolution=request_wrapper::field(evidence,"descriptor_resolution",s)?;
 if request_wrapper::text(resolution,"registry_digest",s)?!=wrappers::LOCAL_REGISTRY{return Ok(false);}
 request_wrapper::same_text(request_wrapper::text(w.operation(),"schema",s)?,"ledger-workflow-economic-operation/10",s)?;
 let key=original.key();let before=w.sequence().checked_add(1).ok_or(ServiceError::IntegrityFailure)?;
 let mut pool=local_inputs::LocalInputs::new(s)?;
 for r in request_wrapper::field(w.manifest(),"members",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?{
  if request_wrapper::text(r,"kind",s)?!="workflow-operation"{pool.add_value(r,s)?;}
 }
 pool.complete(tx,key,before,s).await?;
 let normalized=request_wrapper::field(evidence,"normalized_request",s)?;
 let payload=request_wrapper::field(normalized,"payload",s)?;
 let normalized=s.canonical_representation(payload,RepresentationKind::OriginalRequest).map_err(core_error)?;
 let ck=CoreKey{logical_store_id:key.logical_store_id.clone(),scope:key.scope.clone(),source:key.source.clone(),operation_id:key.operation_id.clone()};
 let subject=locate_local_subject_binding(&ck,normalized.bytes(),&pool.views(),s).map_err(core_error)?;
 let binding=s.parse_representation(pool.body(subject.original_binding_ref())?,RepresentationKind::Record).map_err(core_error)?;
 let authority=request_wrapper::field(w.operation(),"authority_snapshot",s)?;
 let accepted=request_wrapper::field(authority,"original_accepted_agreement_ref",s)?;
 let accepted=current::source::retained_record(tx,key,accepted,"workflow-accepted-agreement","ledger-workflow-accepted-agreement/1",before,s).await?;
 let expected=wire::reference(subject.original_binding_ref(),s)?;
 let actual_binding=request_wrapper::field(accepted.body(),"binding_ref",s)?;
 let actual=s.canonical_representation(actual_binding,RepresentationKind::Record).map_err(core_error)?;
 require(expected.bytes()==actual.bytes(),"INTEGRITY_FAILURE")?;
 let publication=prefix::publication_for(tx,key,request_wrapper::field(accepted.body(),"publication_ref",s)?,before,s).await?;
 require(publication.program().uses_local_execution_recipe(),"UNSUPPORTED_SCHEMA_PROFILE")?;
 let expected=publication.program().compiled_plan().reference();
 let plan=request_wrapper::field(binding.value(),"compiled_plan_ref",s)?;
 request_wrapper::same_text(request_wrapper::text(plan,"id",s)?,expected.id(),s)?;
 request_wrapper::same_text(request_wrapper::text(plan,"digest",s)?,expected.digest(),s)?;
 let saved=ingress::IncomingRequest::parse(w.original_request().raw(),s)?;
 let saved_payload=saved.payload(s)?;
 let raw=s.canonical_representation(saved_payload,RepresentationKind::OriginalRequest).map_err(core_error)?;
 let saved_normalized=normalize_local_saved_intent(publication.program(),normalized.bytes(),raw.bytes(),s).map_err(core_error)?;
 require(saved_normalized.bytes()==normalized.bytes(),"INTEGRITY_FAILURE")?;
 if let Some(incoming)=incoming{
  require(incoming.kind()==saved.kind(),"IDENTITY_CONFLICT")?;
  let incoming_payload=incoming.payload(s)?;
  let raw=s.canonical_representation(incoming_payload,RepresentationKind::OriginalRequest).map_err(core_error)?;
  let actual=normalize_local_saved_intent(publication.program(),normalized.bytes(),raw.bytes(),s).map_err(core_error)?;
  require(actual.bytes()==normalized.bytes(),"IDENTITY_CONFLICT")?;
 }
 for r in request_wrapper::field(w.receipt(),"result_refs",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?{
  let(r,_)=current::source::from_value_local(r,s)?;let _=pool.body(&r)?;
 }
 s.check_cancelled().map_err(core_error)?;Ok(true)
}
