//! Exact retained capacity-head/lineage association. Serialized maxima are
//! DATA to compare with the independently rederived prospective proof; they
//! never grant resource permission and this module cannot mint that proof.
use super::super::{CoreMeter,arithmetic,body,data::*,meter,record_body::RecordBody,records::Records,shape,working_value::owning_reference};
use crate::{Result,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,ArithmeticOperation,ValidationWork}};
use num_bigint::BigInt;

pub(in crate::policy::workflow::transition_constructor) struct CurrentCapacity {
 pub reference:ExactRef,
 pub body:RecordBody,
 pub revision:BigInt,
 pub reserved:Vec<ExactRef>,
 pub retirements:Vec<ExactRef>,
 _ownership:ExternalBufferLease,
}
fn copy(m:&mut CoreMeter,value:&str)->Result<String>{m.scan_bytes(value.len())?;m.write_bytes(value.len())?;Ok(value.to_owned())}
fn copy_ref(m:&mut CoreMeter,value:&ExactRef)->Result<ExactRef>{Ok(ExactRef{kind:copy(m,&value.kind)?,id:copy(m,&value.id)?,digest:copy(m,&value.digest)?})}
/// root_ref is ALWAYS the actual original revision1 root, independently of
/// today's root/head/CAS and independently of the stable capacity counter.
pub(in crate::policy::workflow::transition_constructor) fn retained_root_head(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,original_root:&ExactRef,current_root:&ExactRef,state:&EconomicStateData)->Result<CurrentCapacity>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning capacity head"))?;
 let original=r.get(m,&c.key,original_root,"workflow-decision","ledger-workflow-decision-revision/1")?;
 let first=shape::text(m,&original,"revision")?;if !shape::same_text(m,first,"1")?{return Err(shape::fail("capacity root is not original revision1"));}
 let current=r.get(m,&c.key,current_root,"workflow-decision","ledger-workflow-decision-revision/1")?;
 shape::equal(m,&current["decision_id"],&original["decision_id"])?;shape::equal(m,&current["subject_ref"],&original["subject_ref"])?;
 shape::equal(m,&current["original_terms_ref"],&original["original_terms_ref"])?;
 let root_id=shape::text(m,&original,"decision_id")?;
 let mut found=None;
 for head in state.capacity_heads{m.visit("capacity.current_stable_root_head")?;if shape::same_text(m,&head.root_decision_id,root_id)?{if found.is_some(){return Err(shape::fail("duplicate current capacity root head"));}found=Some(head);}}
 let head=found.ok_or_else(||shape::fail("current root capacity is missing"))?;
 let capacity=r.get(m,&c.key,&head.reference,"workflow-correction-capacity","ledger-correction-capacity/1")?;
 shape::closed(m,&capacity,&["schema","root_ref","revision","dependency_root","affected_decisions","maximum_actions","maximum_records","maximum_bytes","maximum_work","reserved_dependency_refs","retirement_refs"],&[])?;
 let root=owning_reference(m,&arena,original_root)?;shape::equal(m,&capacity["root_ref"],root.value())?;
 let counter=shape::text(m,&capacity,"revision")?;if !shape::same_text(m,counter,&head.revision)?{return Err(shape::fail("capacity current head counter"));}
 let revision=arithmetic::integer(m,counter,false)?;if !meter::cmp(m,&revision,&0.into())?.is_gt(){return Err(shape::fail("positive capacity revision"));}
 let revision_value=super::super::cost_postings::scalar(m,&arena,&revision)?;
 let root_id_value=arena.string_value(root_id,m)?;let seed=arena.array_from_values(&[root_id_value.value(),revision_value.value()],m)?;
 let id=body::semantic_id(m,"wcapacity_","workflow-correction-capacity",seed.value())?;
 if !shape::same_text(m,&id,&head.reference.id)?{return Err(shape::fail("stable capacity identity/counter"));}
 for(name,minimum,maximum)in[("affected_decisions",1u64,32u64),("maximum_actions",0,128),("maximum_records",0,4096),("maximum_bytes",0,8388608),("maximum_work",0,65536)]{
   let raw=shape::text(m,&capacity,name)?;let value=arithmetic::integer(m,raw,false)?;
   if meter::cmp(m,&value,&minimum.into())?.is_lt()||meter::cmp(m,&value,&maximum.into())?.is_gt(){return Err(shape::fail("retained capacity exceeds original domain"));}
 }
 let reserved=shape::array(m,&capacity,"reserved_dependency_refs")?;let retired=shape::array(m,&capacity,"retirement_refs")?;
 if w::compare(m,reserved.len(),128usize)?.is_gt()||w::compare(m,retired.len(),128usize)?.is_gt(){return Err(shape::fail("capacity reservation/retirement count"));}
 let n=w::usize_add(m,reserved.len(),retired.len())?;
 let per=std::mem::size_of::<ExactRef>()+64+128+71+std::mem::size_of::<&ExactRef>();
 ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
 let bytes=n.checked_mul(per).ok_or_else(||shape::fail("capacity reference payload product"))?;
 let bytes=w::usize_add(m,bytes,std::mem::size_of::<CurrentCapacity>()+64+128+71)?;
 let ownership=arena.reserve_external_payload(bytes,m)?;
 let mut reserved_refs=Vec::new();reserved_refs.try_reserve_exact(reserved.len()).map_err(|_|shape::fail("capacity reservation references"))?;
 for value in reserved{m.visit("capacity.retained_reservation_ref")?;let reference=shape::reference(m,value)?;let dependency=r.get(m,&c.key,&reference,"workflow-dependency","ledger-workflow-dependency/1")?;let _=dependency;reserved_refs.push(reference);}
 let mut retired_refs=Vec::new();retired_refs.try_reserve_exact(retired.len()).map_err(|_|shape::fail("capacity retirement references"))?;
 for value in retired{m.visit("capacity.retained_retirement_ref")?;let reference=shape::reference(m,value)?;let retirement=r.get(m,&c.key,&reference,"workflow-capacity-retirement","ledger-workflow-capacity-retirement/1")?;shape::closed(m,&retirement,&["schema","dependency_ref","closure_state_refs","reason","authority_ref","operation_key"],&[])?;let reason=shape::text(m,&retirement,"reason")?;if !shape::same_text(m,reason,"no_future_or_active_dependency")?{return Err(shape::fail("capacity retirement reason"));}retired_refs.push(reference);}
 // The retained complete sets must already equal their canonical sorted form;
 // normalization here must not silently repair an originally invalid record.
 let mut borrowed=Vec::new();borrowed.try_reserve_exact(reserved_refs.len()).map_err(|_|shape::fail("capacity sorted reservation view"))?;
 for value in &reserved_refs{m.visit("capacity.sorted_reservation_view")?;borrowed.push(value);}
 let normalized=super::super::app_cost::reference_set(m,&arena,&borrowed)?;
 shape::equal(m,&capacity["reserved_dependency_refs"],normalized.value())?;
 let digest=super::super::records::hash(m,"workflow-dependency-set",normalized.value())?;
 let stored=shape::text(m,&capacity,"dependency_root")?;if !shape::same_text(m,stored,&digest)?{return Err(shape::fail("capacity dependency root digest"));}
 borrowed.clear();borrowed.try_reserve_exact(retired_refs.len()).map_err(|_|shape::fail("capacity sorted retirement view"))?;for value in &retired_refs{m.visit("capacity.sorted_retirement_view")?;borrowed.push(value);}
 let normalized=super::super::app_cost::reference_set(m,&arena,&borrowed)?;shape::equal(m,&capacity["retirement_refs"],normalized.value())?;
 let reference=copy_ref(m,&head.reference)?;
 Ok(CurrentCapacity{reference,body:capacity,revision,reserved:reserved_refs,retirements:retired_refs,_ownership:ownership})
}

#[cfg(test)]
mod tests {
 use super::*;
 use serde_json::json;
 use crate::{canonical::audit_arena::AuditArena,validation_work::OriginalAuditSession};
 fn key()->OperationKey{OperationKey{logical_store_id:"store".into(),scope:["org".into(),"scope".into()],source:"source".into(),operation_id:"original".into()}}
 fn context()->DraftContextData{DraftContextData{key:key(),actor:"principal".into(),canonical_economic_namespace:"namespace".into(),installation_mode:"real_local".into(),received_at:"2026-10-17T00:00:00Z".into(),accepted_at:"2026-10-17T00:00:00Z".into(),authorities:vec![],source_observation_inputs:vec![]}}
 fn envelope(m:&mut CoreMeter,revision:&str)->super::super::super::emitted_record::ConstructedEnvelope{
   let id=body::semantic_id(m,"wdrev_","workflow-decision-revision",&json!(["decision",revision])).unwrap();
   body::record(m,&key(),"workflow-decision",id,json!({"schema":"ledger-workflow-decision-revision/1","decision_id":"decision","revision":revision,"subject_ref":{"kind":"workflow-work","id":"work","digest":format!("sha256:{}","1".repeat(64))},"original_terms_ref":{"kind":"workflow-binding","id":"binding","digest":format!("sha256:{}","2".repeat(64))}})).unwrap()
 }
 // Association-only unit: no origin/currentness/authentication is asserted.
 // The retained capacity revision belongs to the stable root decision, while
 // its full root Ref stays revision1 even after the financial head advances.
 #[test]
 fn original_root_ref_persists_while_capacity_counter_advances(){
   let mut build=CoreMeter::new(65536).unwrap();let first=envelope(&mut build,"1");let current=envelope(&mut build,"2");
   for wrong in [false,true]{
     let digest=super::super::super::records::hash(&mut build,"workflow-dependency-set",&json!([])).unwrap();
     let id=body::semantic_id(&mut build,"wcapacity_","workflow-correction-capacity",&json!(["decision","3"])).unwrap();
     let capacity=body::record(&mut build,&key(),"workflow-correction-capacity",id,json!({"schema":"ledger-correction-capacity/1","root_ref":if wrong{&current.reference}else{&first.reference},"revision":"3","dependency_root":digest,"affected_decisions":"1","maximum_actions":"0","maximum_records":"0","maximum_bytes":"0","maximum_work":"0","reserved_dependency_refs":[],"retirement_refs":[]})).unwrap();
     let inputs=[EconomicRecordView{reference:&first.reference,canonical_body:&first.canonical_body},EconomicRecordView{reference:&current.reference,canonical_body:&current.canonical_body},EconomicRecordView{reference:&capacity.reference,canonical_body:&capacity.canonical_body}];
     let heads=[CapacityHead{root_decision_id:"decision".into(),revision:"3".into(),reference:capacity.reference.clone()}];
     let financial=DraftStateData::default();let state=EconomicStateData{financial:&financial,capacity_heads:&heads,supplier:None,app:None,current_party_authorizations:&[],observation:None,audit:None};
     let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
     let mut child=session.meter_mut().original_economic_meter().unwrap();let c=context();
     let mut records=Records::borrowed_app(&mut child,&arena,&c.key,&inputs,&[]).unwrap();
     let result=retained_root_head(&mut child,&mut records,&c,&first.reference,&current.reference,&state);
     if wrong{assert!(result.is_err());}else{let result=result.unwrap();assert_eq!(result.revision,BigInt::from(3));assert_eq!(result.body["root_ref"],json!(first.reference));session.meter_mut().finish_original_economic(child).unwrap();}
   }
 }
}
