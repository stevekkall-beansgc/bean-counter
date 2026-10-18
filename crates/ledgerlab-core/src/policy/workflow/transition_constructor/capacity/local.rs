//! Local financial reservation production over the SAME original graph.
//! No future machine/resource maxima or chronological certificate is emitted.
use super::{original::FinancialReservations,topology};
use super::super::{CoreMeter,body,cost_postings,data::*,emitted_record::ConstructedEnvelope,shape,working_value::owning_reference};
use super::super::super::selected_program::SelectedProgram;
use crate::{Result,validation_work::{self as w,RepresentationKind}};
use serde_json::Value;
fn same_scalar(m:&mut CoreMeter,v:&Value,field:&str,n:usize)->Result<()>{let a=m.representation_arena().ok_or_else(||shape::fail("local capacity owner"))?;let n=cost_postings::scalar(m,&a,&n.into())?;shape::equal(m,&v[field],n.value())}
fn dependency(m:&mut CoreMeter,c:&DraftContextData,o:&FinancialReservations,dependent:&str,target:&ExactRef,family:Option<&str>,actions:usize,key:&Value)->Result<ConstructedEnvelope>{
 let a=m.representation_arena().ok_or_else(||shape::fail("local dependency owner"))?;
 let ancestor=a.string_value(&o.root_decision_id,m)?;let dependent=a.string_value(dependent,m)?;
 let terms=owning_reference(m,&a,&o.original_binding_ref)?;let target=owning_reference(m,&a,target)?;
 let seed=a.array_from_values(&[ancestor.value(),dependent.value(),terms.value()],m)?;let id=body::semantic_id(m,"wdep_","workflow-dependency",seed.value())?;
 let schema=a.string_value("ledger-workflow-dependency/2",m)?;let kind=a.string_value(if family.is_some(){"outcome_family"}else{"delayed_cost"},m)?;
 let actions=cost_postings::scalar(m,&a,&actions.into())?;
 let v=a.object_from_fields(&[("schema",schema.value()),("ancestor_decision_id",ancestor.value()),("dependent_decision_id",dependent.value()),("kind",kind.value()),("target_ref",target.value()),("original_terms_ref",terms.value()),("maximum_actions",actions.value()),("operation_key",key)],m)?;
 let v=if let Some(slot)=family{let slot=a.string_value(slot,m)?;super::super::working_value::WorkingValue::Owned(v).with_fields(m,&[("family_slot",slot.value())])?.into_owned()?}else{v};
 body::record_borrowed(m,&c.key,"workflow-dependency",id,v.value())
}
/// Selected current capacity / original graph equality precedes any successor.
/// Historical retirement removes an edge only through its exact dependency Ref.
pub(in crate::policy::workflow::transition_constructor) fn produce(m:&mut CoreMeter,c:&DraftContextData,p:&SelectedProgram,state:&EconomicStateData,t:&mut Construction)->Result<()>{
 let a=m.representation_arena().ok_or_else(||shape::fail("local financial graph owner"))?;
 if t.financial_reservations.is_none(){
  if let OriginalWorkPayload::Retained(work)=&t.economic.original_work{
   let work=work.clone();t.financial_reservations=Some(super::original::derive_financial_for_work(&a,m,&mut t.records,c,p,&work,&t.economic.terms)?);
  }
 }
 let Some(original)=t.financial_reservations.as_ref() else{return Ok(());};
 let root=if matches!(t.economic.original_work,OriginalWorkPayload::New(_)){
  let mut selected=None;for row in &t.new_envelopes{m.visit("local.capacity_new_root")?;if shape::same_ref(m,&row.reference,&original.original_root_ref)?{selected=Some(row.decoded(m,4194304)?);}}
  selected.ok_or_else(||shape::fail("local original root output missing"))?
 }else{t.records.get(m,&c.key,&original.original_root_ref,"workflow-decision","ledger-workflow-decision-revision/1")?};
 let key=shape::field(m,&root,"operation_key")?;
 let mut expected=Vec::new();let mut edges=Vec::new();
 for f in &original.families{expected.push(dependency(m,c,original,&f.dependent_decision_id,&f.target_ref,Some(&f.family_slot),f.maximum_inverse_actions,key)?);edges.push(topology::Reservation{ancestor_decision_id:&original.root_decision_id,dependent_decision_id:&f.dependent_decision_id,maximum_inverse_actions:f.maximum_inverse_actions});}
 for f in &original.delayed_costs{expected.push(dependency(m,c,original,&f.dependent_decision_id,&f.target_ref,None,f.maximum_inverse_actions,key)?);edges.push(topology::Reservation{ancestor_decision_id:&original.root_decision_id,dependent_decision_id:&f.dependent_decision_id,maximum_inverse_actions:f.maximum_inverse_actions});}
 for f in &original.app_costs{expected.push(dependency(m,c,original,&f.dependent_decision_id,&f.target_ref,None,f.maximum_inverse_actions,key)?);edges.push(topology::Reservation{ancestor_decision_id:&original.root_decision_id,dependent_decision_id:&f.dependent_decision_id,maximum_inverse_actions:f.maximum_inverse_actions});}
 let current=if matches!(t.economic.original_work,OriginalWorkPayload::Retained(_)){
  let mut head=None;for h in &state.financial.decision_heads{m.visit("local.capacity_held_root")?;if shape::same_text(m,&h.decision_id,&original.root_decision_id)?{if head.is_some(){return Err(shape::fail("ambiguous held local root"));}head=Some(h);}}
  let h=head.ok_or_else(||shape::fail("held local root missing"))?;
  Some(super::current::retained_local_root_head(m,&mut t.records,c,&original.original_root_ref,&h.reference,state)?)
 }else{None};
 let mut retired_dependencies=Vec::new();let mut retirement_refs=Vec::new();
 if let Some(current)=&current{
  for rr in &current.retirements{let v=t.records.get(m,&c.key,rr,"workflow-capacity-retirement","ledger-workflow-capacity-retirement/1")?;retired_dependencies.push(shape::reference(m,&v["dependency_ref"])?);retirement_refs.push(rr.clone());}
 }
 let mut before_edges=Vec::new();
 for (row,edge) in expected.iter().zip(&edges){
  let mut retired=0usize;for r in &retired_dependencies{m.visit("local.capacity_original_retirement")?;if shape::same_ref(m,r,&row.reference)?{retired=w::usize_add(m,retired,1)?;}}
  let mut reserved=0usize;if let Some(current)=&current{for r in &current.reserved{m.visit("local.capacity_original_reservation")?;if shape::same_ref(m,r,&row.reference)?{reserved=w::usize_add(m,reserved,1)?;let raw=t.records.canonical(m,r)?;if !shape::same_bytes(m,raw,&row.canonical_body)?{return Err(shape::fail("local original dependency bytes"));}}}
   let count=w::usize_add(m,retired,reserved)?;if !w::compare(m,count,1usize)?.is_eq(){return Err(shape::fail("local dependency must be exactly reserved or retired"));}
  }
  if retired==0{before_edges.push(topology::Reservation{ancestor_decision_id:edge.ancestor_decision_id,dependent_decision_id:edge.dependent_decision_id,maximum_inverse_actions:edge.maximum_inverse_actions});}
 }
 if let Some(current)=&current{
  let count=w::usize_add(m,current.reserved.len(),retired_dependencies.len())?;if !w::compare(m,count,expected.len())?.is_eq(){return Err(shape::fail("local capacity has unknown dependency"));}
  let before=topology::union(&a,m,&original.root_decision_id,original.maximum_root_actions,&before_edges)?;
  same_scalar(m,&current.body,"affected_decisions",before.decision_ids.len())?;same_scalar(m,&current.body,"maximum_actions",before.maximum_appended_actions)?;
 }
 for row in &t.new_envelopes{m.visit("local.capacity_new_retirement")?;if !shape::same_text(m,&row.reference.kind,"workflow-capacity-retirement")?{continue;}let v=row.decoded(m,4194304)?;let dep=shape::reference(m,&v["dependency_ref"])?;
  let mut found=false;for original in &expected{if shape::same_ref(m,&dep,&original.reference)?{found=true;}}
  if !found{return Err(shape::fail("retirement of unrelated local dependency"));}
  for old in &retired_dependencies{if shape::same_ref(m,old,&dep)?{return Err(shape::fail("local dependency already retired"));}}
  retired_dependencies.push(dep);retirement_refs.push(row.reference.clone());
 }
 let mut after_edges=Vec::new();let mut reserved=Vec::new();
 for (row,edge) in expected.iter().zip(&edges){let mut retired=false;for r in &retired_dependencies{if shape::same_ref(m,r,&row.reference)?{retired=true;}}
  if !retired{reserved.push(&row.reference);after_edges.push(topology::Reservation{ancestor_decision_id:edge.ancestor_decision_id,dependent_decision_id:edge.dependent_decision_id,maximum_inverse_actions:edge.maximum_inverse_actions});}}
 let after=topology::union(&a,m,&original.root_decision_id,original.maximum_root_actions,&after_edges)?;
 let reserved=super::super::app_cost::reference_set(m,&a,&reserved)?;let retired:Vec<_>=retirement_refs.iter().collect();let retired=super::super::app_cost::reference_set(m,&a,&retired)?;
 let digest=super::super::records::hash(m,"workflow-dependency-set",reserved.value())?;let digest=a.string_value(&digest,m)?;
 let next=match &current{Some(c)=>super::super::meter::add(m,&c.revision,&1.into())?,None=>1.into()};
 let revision=cost_postings::scalar(m,&a,&next)?;let root_ref=owning_reference(m,&a,&original.original_root_ref)?;
 let ancestor=a.string_value(&original.root_decision_id,m)?;let seed=a.array_from_values(&[ancestor.value(),revision.value()],m)?;let id=body::semantic_id(m,"wcapacity_","workflow-correction-capacity",seed.value())?;
 let schema=a.string_value("ledger-correction-capacity/2",m)?;let decisions=cost_postings::scalar(m,&a,&after.decision_ids.len().into())?;let actions=cost_postings::scalar(m,&a,&after.maximum_appended_actions.into())?;
 let v=a.object_from_fields(&[("schema",schema.value()),("root_ref",root_ref.value()),("revision",revision.value()),("dependency_root",digest.value()),("affected_decisions",decisions.value()),("maximum_actions",actions.value()),("reserved_dependency_refs",reserved.value()),("retirement_refs",retired.value())],m)?;
 let capacity=body::record_borrowed(m,&c.key,"workflow-correction-capacity",id,v.value())?;
 drop(after);drop(after_edges);drop(before_edges);drop(edges);
 if current.is_none(){t.new_envelopes.extend(expected);}t.new_envelopes.push(capacity);Ok(())
}
