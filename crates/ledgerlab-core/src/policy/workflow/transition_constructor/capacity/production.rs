//! Canonical dependency/capacity output from the original constructor's own
//! reservations and its derived future envelopes. This module owns no meter,
//! caller allowance, saved-maximum parser, or financial admission capability.
use super::super::{CoreMeter,body,cost_postings,data::*,emitted_record::ConstructedEnvelope,shape,working_value::owning_reference};
use super::original::OriginalReservations;
use crate::{Result,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,ArithmeticOperation,RepresentationKind,ValidationWork}};
use std::mem;

/// Internal numerical DATA. Only the source-envelope derivation may supply
/// these numbers; no public entry accepts them and no saved maximum is used.
/// Wire S excludes C, whose complete proof remains independently required.
pub(in crate::policy::workflow::transition_constructor::capacity) struct EnvelopeBound {
 pub records:usize,pub body_bytes:usize,pub semantic_work:usize,
}
pub(in crate::policy::workflow::transition_constructor::capacity) struct ProducedCapacity {
 pub envelopes:Vec<ConstructedEnvelope>,
 pub dependency_refs:Vec<ExactRef>,
 pub capacity_ref:ExactRef,
 _ownership:ExternalBufferLease,
}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;a.checked_mul(b).ok_or_else(||shape::fail("capacity output metadata multiplication"))}
fn copy(m:&mut CoreMeter,s:&str)->Result<String>{m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
fn copy_ref(m:&mut CoreMeter,r:&ExactRef)->Result<ExactRef>{Ok(ExactRef{kind:copy(m,&r.kind)?,id:copy(m,&r.id)?,digest:copy(m,&r.digest)?})}
fn bound(m:&mut CoreMeter,n:usize,maximum:usize)->Result<()>{if w::compare(m,n,maximum)?.is_gt(){return Err(crate::Error::new("CORRECTION_CAPACITY_EXCEEDED","derived future envelope exceeds existing wire domain"));}Ok(())}
fn validate_envelope(m:&mut CoreMeter,b:&EnvelopeBound)->Result<()>{bound(m,b.records,4096)?;bound(m,b.body_bytes,8388608)?;bound(m,b.semantic_work,65536)}
/// Complete ORIGINAL root plus all of its original conditional edges, before
/// any measured-work admission. Bounds are in exactly the reservation order
/// below; a missing, additional or oversized contribution refuses atomically.
pub(in crate::policy::workflow::transition_constructor::capacity) fn initial(
 m:&mut CoreMeter,c:&DraftContextData,original:&OriginalReservations,
 root:&EnvelopeBound,marginals:&[EnvelopeBound],
)->Result<ProducedCapacity>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning capacity record production"))?;
 let n=w::usize_add(m,original.families.len(),original.delayed_costs.len())?;
 let n=w::usize_add(m,n,original.app_costs.len())?;
 if !w::compare(m,n,marginals.len())?.is_eq(){return Err(shape::fail("derived marginal reservation count"));}
 validate_envelope(m,root)?;
 let union=original.structural_union(&arena,m)?;
 let count=w::usize_add(m,n,1)?;
 let envelope_bytes=mul(m,count,mem::size_of::<ConstructedEnvelope>()+64+128+71)?;
 let references=mul(m,n,mem::size_of::<ExactRef>()+64+128+71+mem::size_of::<&ExactRef>())?;
 let bytes=w::usize_add(m,envelope_bytes,references)?;
 let bytes=w::usize_add(m,bytes,mem::size_of::<ProducedCapacity>()+64+128+71)?;
 // The temporary string/BigInt/frame controls overlap every retained output.
 // Canonical bytes/Value graphs have their own same-arena owning artifacts.
 // Live hash/ID strings (including nested hexadecimal formatting), one
 // bounded counter BigInt input, and their control headers. The output Ref
 // payloads themselves were counted separately per authoritative envelope.
 const SCRATCH:usize=8*mem::size_of::<String>()+128+71+64+71+64+mem::size_of::<num_bigint::BigInt>()+8;
 let bytes=w::usize_add(m,bytes,SCRATCH)?;
 let ownership=arena.reserve_external_payload(bytes,m)?;
 let mut envelopes=Vec::new();envelopes.try_reserve_exact(count).map_err(|_|shape::fail("capacity output vector"))?;
 let mut dependency_refs=Vec::new();dependency_refs.try_reserve_exact(n).map_err(|_|shape::fail("capacity reserved Ref vector"))?;
 let mut borrowed=Vec::new();borrowed.try_reserve_exact(n).map_err(|_|shape::fail("capacity sorted Ref workspace"))?;
 let constants=m.parse_owned_representation(&arena,br#"{"dependency":"ledger-workflow-dependency/1","capacity":"ledger-correction-capacity/1","family":"outcome_family","cost":"delayed_cost"}"#,RepresentationKind::SourceDefinition)?;
 let key=cost_postings::operation_key(m,&arena,c)?;
 let terms=owning_reference(m,&arena,&original.original_binding_ref)?;
 let ancestor=arena.string_value(&original.root_decision_id,m)?;
 let mut position=0usize;
 for family in &original.families{
  m.visit("capacity.produce_original_family_dependency")?;
  let row=dependency(m,c,&constants.value()["dependency"],&constants.value()["family"],ancestor.value(),&family.dependent_decision_id,&family.target_ref,Some(&family.family_slot),terms.value(),key.value(),family.maximum_inverse_actions,&marginals[position])?;
  dependency_refs.push(copy_ref(m,&row.reference)?);envelopes.push(row);position=w::usize_add(m,position,1)?;
 }
 for cost in &original.delayed_costs{
  m.visit("capacity.produce_original_supplier_dependency")?;
  let row=dependency(m,c,&constants.value()["dependency"],&constants.value()["cost"],ancestor.value(),&cost.dependent_decision_id,&cost.target_ref,None,terms.value(),key.value(),cost.maximum_inverse_actions,&marginals[position])?;
  dependency_refs.push(copy_ref(m,&row.reference)?);envelopes.push(row);position=w::usize_add(m,position,1)?;
 }
 for cost in &original.app_costs{
  m.visit("capacity.produce_original_app_dependency")?;
  let row=dependency(m,c,&constants.value()["dependency"],&constants.value()["cost"],ancestor.value(),&cost.dependent_decision_id,&cost.target_ref,None,terms.value(),key.value(),cost.maximum_inverse_actions,&marginals[position])?;
  dependency_refs.push(copy_ref(m,&row.reference)?);envelopes.push(row);position=w::usize_add(m,position,1)?;
 }
 for reference in &dependency_refs{m.visit("capacity.sort_reserved_dependency")?;borrowed.push(reference);}
 let reserved=super::super::app_cost::reference_set(m,&arena,&borrowed)?;
 let digest=super::super::records::hash(m,"workflow-dependency-set",reserved.value())?;
 let dependency_root=arena.string_value(&digest,m)?;
 let root_ref=owning_reference(m,&arena,&original.original_root_ref)?;
 let revision=arena.string_value("1",m)?;
 let seed=arena.array_from_values(&[ancestor.value(),revision.value()],m)?;
 let id=body::semantic_id(m,"wcapacity_","workflow-correction-capacity",seed.value())?;
 let affected=cost_postings::scalar(m,&arena,&union.decision_ids.len().into())?;
 let actions=cost_postings::scalar(m,&arena,&union.maximum_appended_actions.into())?;
 let records=cost_postings::scalar(m,&arena,&root.records.into())?;
 let bytes=cost_postings::scalar(m,&arena,&root.body_bytes.into())?;
 let work=cost_postings::scalar(m,&arena,&root.semantic_work.into())?;
 let empty=arena.array_from_values(&[],m)?;
 let value=arena.object_from_fields(&[("schema",&constants.value()["capacity"]),("root_ref",root_ref.value()),("revision",revision.value()),("dependency_root",dependency_root.value()),("affected_decisions",affected.value()),("maximum_actions",actions.value()),("maximum_records",records.value()),("maximum_bytes",bytes.value()),("maximum_work",work.value()),("reserved_dependency_refs",reserved.value()),("retirement_refs",empty.value())],m)?;
 let capacity=body::record_borrowed(m,&c.key,"workflow-correction-capacity",id,value.value())?;
 let capacity_ref=copy_ref(m,&capacity.reference)?;envelopes.push(capacity);
 Ok(ProducedCapacity{envelopes,dependency_refs,capacity_ref,_ownership:ownership})
}
fn dependency(
 m:&mut CoreMeter,c:&DraftContextData,schema:&serde_json::Value,kind:&serde_json::Value,
 ancestor:&serde_json::Value,dependent:&str,target:&ExactRef,family:Option<&str>,terms:&serde_json::Value,key:&serde_json::Value,actions:usize,envelope:&EnvelopeBound,
)->Result<ConstructedEnvelope>{
 validate_envelope(m,envelope)?;bound(m,actions,128)?;
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning original dependency"))?;
 let dependent=arena.string_value(dependent,m)?;let target=owning_reference(m,&arena,target)?;
 let seed=arena.array_from_values(&[ancestor,dependent.value(),terms],m)?;
 let id=body::semantic_id(m,"wdep_","workflow-dependency",seed.value())?;
 let actions=cost_postings::scalar(m,&arena,&actions.into())?;
 let records=cost_postings::scalar(m,&arena,&envelope.records.into())?;
 let bytes=cost_postings::scalar(m,&arena,&envelope.body_bytes.into())?;
 let work=cost_postings::scalar(m,&arena,&envelope.semantic_work.into())?;
 let fields=[("schema",schema),("ancestor_decision_id",ancestor),("dependent_decision_id",dependent.value()),("kind",kind),("target_ref",target.value()),("original_terms_ref",terms),("maximum_actions",actions.value()),("maximum_records",records.value()),("maximum_bytes",bytes.value()),("maximum_work",work.value()),("operation_key",key)];
 // The exact family-only optional field is emitted structurally; delayed_cost
 // keeps the original WorkRecord target and never overloads family_slot.
 let value=if let Some(slot)=family{
  let base=arena.object_from_fields(&fields,m)?;let slot=arena.string_value(slot,m)?;
  super::super::working_value::WorkingValue::Owned(base).with_fields(m,&[("family_slot",slot.value())])?.into_owned()?
 }else{arena.object_from_fields(&fields,m)?};
 body::record_borrowed(m,&c.key,"workflow-dependency",id,value.value())
}
