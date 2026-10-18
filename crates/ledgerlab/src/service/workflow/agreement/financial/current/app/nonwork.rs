//! Held, targeted non-work operands. Stable identities come from the same core
//! selectors as execution; projection hits never supply authority or pricing.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{self as core,CostHead,PermanentClaimKey,LocalOperationLocators};
use crate::store::sqlite::workflow::financial::{AppClaimLocator,HeadKey,HeadLocator};

pub(super) struct ExtraState {
 pub(super) assertion:Option<ExactRef>,pub(super) costs:Vec<CostHead>,
 pub(super) observations:Vec<ExactRef>,pub(super) claims:Vec<ExactRef>,pub(super) consumed:Vec<ExactRef>,
 _storage:ExternalBufferLease,
}
fn decoded(b:&[u8])->Result<&str>{std::str::from_utf8(b).map_err(|_|ServiceError::IntegrityFailure)}
fn claim_ref(row:&AppClaimLocator,s:&mut OriginalAuditSession)->Result<ExactRef>{
 let k=wire::string(decoded(row.kind.bytes())?,s)?;let i=wire::string(decoded(row.id.bytes())?,s)?;let d=wire::string(decoded(row.digest.bytes())?,s)?;
 let r=wire::object(&[("digest",d.bytes()),("id",i.bytes()),("kind",k.bytes())],262144,s)?;
 let v=s.parse_representation(r.bytes(),RepresentationKind::Record).map_err(core_error)?;
 let(reference,_)=source::from_value_local(v.value(),s)?;Ok(reference)
}
fn head_ref(row:&HeadLocator,s:&mut OriginalAuditSession)->Result<ExactRef>{
 let v=serde_json::json!({"kind":decoded(row.kind.bytes())?,"id":decoded(row.id.bytes())?,"digest":decoded(row.digest.bytes())?});source::from_value_local(&v,s).map(|v|v.0)
}
pub(in super::super::super) async fn observe(ledger:&LocalWorkflowLedger,tx:&mut SqliteTx,key:&OperationKey,payload:&Value,s:&mut OriginalAuditSession)->Result<ObservedAppBinding>{
 let before=tx.workflow_sequence().await.map_err(crate::service::store_error)?;
 let mut pool=super::super::super::local_inputs::LocalInputs::new(s)?;
 pool.payload_refs(payload,s)?;pool.complete(tx,key,before,s).await?;
 let ck=core::OperationKey{logical_store_id:key.logical_store_id.clone(),scope:key.scope.clone(),source:key.source.clone(),operation_id:key.operation_id.clone()};
 let raw=s.canonical_representation(payload,RepresentationKind::OriginalRequest).map_err(core_error)?;
 let subject=core::locate_local_subject_binding(&ck,raw.bytes(),&pool.views(),s).map_err(core_error)?;
 let binding=serde_json::to_value(subject.original_binding_ref()).map_err(|_|ServiceError::IntegrityFailure)?;
 let mut o=observe_binding(ledger,tx,key,"work.accept",&binding,None,None,s).await?;
 require(o.original.publication.program().uses_local_execution_recipe(),"UNSUPPORTED_SCHEMA_PROFILE")?;
 let index=work_capture::source_index(&o,key,s)?;
 let birth=o.original.source_birth_inputs(tx,key,o.sources[index].enrollment_ref(),o.before,s).await?;o.work_source_birth=Some(birth);
 let root=work_capture::retained_root(&mut o,tx,key,index,s).await?;o.work_root=Some(root);
 let received=crate::local::now().map_err(|_|ServiceError::Unavailable)?;
 let locators=work_capture::with_capture(&o,key,payload,index,&received,s,|capture,_,raw,s|{
  core::select_local_operation_locators(o.original.publication.program(),capture,raw,&pool.views(),s).map_err(core_error)
 })?;
 let mut state=if let Some(heads)=locators.work_heads(){
  let held=work_heads::observe(tx,&mut o.original,key,heads,o.parties.as_ref().ok_or(ServiceError::IntegrityFailure)?,s).await?;
  let state=held.typed_state(heads,o.parties.as_ref().ok_or(ServiceError::IntegrityFailure)?,s)?;
  let seeds=s.parse_representation(held.seeds.bytes(),RepresentationKind::Record).map_err(core_error)?;pool.payload_refs(seeds.value(),s)?;state
 }else{work_heads::unattached_state(o.parties.as_ref().ok_or(ServiceError::IntegrityFailure)?,s)?};
 for identity in locators.record_identities(){
  if let Some(row)=tx.workflow_local_record_identity(&key.logical_store_id,&key.scope,identity.kind(),identity.id(),identity.schema(),s).await.map_err(prefix::read_error)?{pool.add(&claim_ref(&row,s)?,s)?;}
 }
 // Concrete limits are retained. This owns the bounded Ref/head copies until
 // construction and journal framing finish under this same attempt.
 let owner=s.reserve_external_payload(4096*1024).map_err(core_error)?;
 let mut extra=ExtraState{assertion:None,costs:Vec::new(),observations:Vec::new(),claims:Vec::new(),consumed:Vec::new(),_storage:owner};
 for claim in locators.claim_keys(){
  let row=match claim{
   PermanentClaimKey::Assertion{..}=>tx.workflow_assertion_claim(claim,s).await,
   PermanentClaimKey::Work{..}=>tx.workflow_work_claim(claim,s).await,
   PermanentClaimKey::Outcome{..}=>tx.workflow_outcome_claim(&key.logical_store_id,claim,s).await,
   PermanentClaimKey::AppCall{..}|PermanentClaimKey::AppComponentObservation{..}|PermanentClaimKey::AppProviderCall{..}|PermanentClaimKey::AppCostComponent{..}|PermanentClaimKey::AppFinalEvidence{..}|PermanentClaimKey::AppObservationRevision{..}=>tx.workflow_app_claim(claim,s).await,
   _=>return Err(reject("UNSUPPORTED_COMPOSITION")),
  }.map_err(prefix::read_error)?;
  if let Some(row)=row{
   let r=claim_ref(&row,s)?;pool.add(&r,s)?;
   match claim{
    PermanentClaimKey::Assertion{..}=>extra.assertion=Some(r),
    PermanentClaimKey::Work{..}=>{if !state.financial.work_claims.contains(&r){state.financial.work_claims.push(r);}},
    PermanentClaimKey::Outcome{..}=>state.financial.outcome_claims.push(r),
    PermanentClaimKey::AppCostComponent{..}=>{extra.claims.push(r);},
    PermanentClaimKey::AppCall{..}|PermanentClaimKey::AppComponentObservation{..}|PermanentClaimKey::AppProviderCall{..}=>extra.observations.push(r),
    PermanentClaimKey::AppFinalEvidence{..}|PermanentClaimKey::AppObservationRevision{..}=>extra.consumed.push(r),_=>unreachable!(),
   }
  }
 }
 for r in &extra.claims{
  if let Some(h)=tx.workflow_financial_head(&key.logical_store_id,&key.scope,HeadKey::AppCost(r),s).await.map_err(prefix::read_error)?{
   let reference=head_ref(&h,s)?;pool.add(&reference,s)?;
   extra.costs.push(CostHead{cost_claim_ref:r.clone(),revision:decoded(h.revision.bytes())?.to_owned(),reference});
  }
 }
 for r in o.original.publication.required_refs().chain(o.original.acceptance.work_locator_records().map(|r|r.reference))
 .chain(o.sources.iter().flat_map(source::ObservedSource::record_refs)).chain(o.parties.iter().flat_map(|p|p.selected_refs())){pool.add(r,s)?;}
 if let Some(r)=o.work_root.as_ref(){pool.add(r.reference(),s)?;}
 if let Some(b)=o.work_source_birth.as_ref(){pool.add(b.state.reference(),s)?;pool.add(b.grant.reference(),s)?;}
 if let Some(a)=o.activation.as_ref(){pool.add(a.head.reference(),s)?;pool.add(a.activation.reference(),s)?;}
 pool.complete(tx,key,o.before,s).await?;
 o.work_state=Some(state);o.local_input_bodies=Some(pool);o.work_source_index=Some(index);o.work_received_at=Some(received);o.local_extra=Some(extra);
 Ok(o)
}
