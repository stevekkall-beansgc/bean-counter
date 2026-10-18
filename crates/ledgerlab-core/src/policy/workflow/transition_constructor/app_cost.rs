//! App-owned final attributable costs. These executors share original customer
//! pricing/posting and actual source validation; no AP record is an operand.
mod original;
mod binding;
mod consent;
pub(super) use consent::current as check_current_consent;
pub(super) use binding::work as work_operands;
pub(super) use binding::resolved as binding_resolved;

mod evidence;

mod retention;
pub(super) use retention::execute as retain;
pub(super) use retention::execute_complete as retain_complete;
pub(super) use retention::refs as reference_set;

mod records;

mod plan;

mod authority;

mod evaluation;
mod timing;
pub(super) mod causal;

/// One actual app finalize/correct producer; financial preparation still needs
/// the complete original/current authority, reservation and resource factory.
pub(super) struct AppConstruction<'program,'raw>{
 pub plan:plan::AppPlan<'program>,
 pub normalized_payload:super::emitted_record::EncodedBody,
 pub authority:super::emitted_record::ConstructedEnvelope,
 pub source:super::source_authority::SourceSelection,
 pub current_capacity:super::capacity::current::CurrentCapacity,
 pub monetary:evaluation::AppEvaluation,
 pub records:super::records::Records<'raw,'raw>,
}
pub(super) fn produce<'program,'raw>(program:&'program super::super::selected_program::SelectedProgram,c:&'raw super::data::DraftContextData,payload:&[u8],inputs:&'raw [super::data::EconomicRecordView<'raw>],state:&super::data::EconomicStateData<'_>,m:&mut super::CoreMeter)->crate::Result<AppConstruction<'program,'raw>>{
 m.begin_request(payload)?;
 let request=super::working_value::WorkingValue::parse(m,payload)?;
 produce_parsed(program,c,request,inputs,state,m)
}
pub(super) fn produce_parsed<'program,'raw>(program:&'program super::super::selected_program::SelectedProgram,c:&'raw super::data::DraftContextData,request:super::working_value::WorkingValue,inputs:&'raw [super::data::EconomicRecordView<'raw>],state:&super::data::EconomicStateData<'_>,m:&mut super::CoreMeter)->crate::Result<AppConstruction<'program,'raw>>{
 let arena=m.representation_arena().ok_or_else(||super::shape::fail("owning app execution"))?;
 let mut records=super::records::Records::borrowed_app(m,&arena,&c.key,inputs,&c.source_observation_inputs)?;
 records.install_initial_source_outputs(m,state.initial_source_correspondences)?;
 let plan=plan::prepare(m,&mut records,c,program,request,state)?;
 let current_capacity=super::capacity::current::retained_root_head(m,&mut records,c,&plan.original.original_decision_ref,&plan.original.original_decision_ref,state)?;
 let mut held=false;for reference in &current_capacity.reserved{m.visit("app.current_dependency_hold")?;if super::shape::same_ref(m,reference,&plan.dependency_ref)?{held=true;}}
 if !held{return Err(super::shape::fail("app original dependency capacity is not held"));}
 let _clocks=timing::standalone(m,c,program,&plan)?;
 let normalized_payload=plan.request.bytes(m)?;
 let(authority,source)=authority::for_plan(m,&mut records,c,program,&plan)?;
 consent::current(program,m,&mut records,c,&plan.original.terms.binding,&plan.original.terms.accepted,state)?;
 let monetary=evaluation::evaluate(m,&mut records,c,program,&plan,&authority)?;
 Ok(AppConstruction{plan,normalized_payload,authority,source,current_capacity,monetary,records})
}

/// Same normalization used by fresh execution and original retry. This only
/// normalizes request DATA; it never re-evaluates money/current permissions.
pub(super) fn normalize_intent(program:&super::super::selected_program::SelectedProgram,value:&mut crate::canonical::audit_arena::AuditedValue,m:&mut super::CoreMeter)->crate::Result<()> {
 if !program.uses_app_owned_cost_recipe(){return Err(super::shape::fail("app original intent recipe"));}
 let schema=super::shape::text(m,value.value(),"schema")?;
 if super::shape::same_text(m,schema,"ledger-workflow-cost-finalize/2")?||super::shape::same_text(m,schema,"ledger-workflow-cost-correct/2")?{
   let mut working=super::working_value::WorkingValue::copy(m,value.value())?;
   plan::normalize_request(m,&mut working)?;*value=working.into_owned()?;
 }else{
   super::shape::closed(m,value.value(),&["schema","attestation"],&[])?;
   if !super::shape::same_text(m,schema,"ledger-workflow-app-observe/1")?&&!super::shape::same_text(m,schema,"ledger-workflow-app-cost-attest/1")?{return Err(super::shape::fail("app retained intent schema"));}
   let normalized=evidence::normalize(m,&value.value()["attestation"])?;
   value.replace_existing_owned("attestation",normalized,m)?;
 }
 Ok(())
}

impl<'program,'raw> AppConstruction<'program,'raw> {
 /// The same interpreter output carrier used by work/outcome/root operations.
 /// This is not a candidate promotion: complete prospective capacity/resource,
 /// observation/original proof and funding assembly still run afterward.
 pub(super) fn into_construction(self,c:&'raw super::data::DraftContextData,m:&mut super::CoreMeter)->crate::Result<super::data::Construction<'raw,'raw>> {
  use super::{body,data::*,emitted_record::ConstructedEnvelope,shape,working_value::owning_reference};
  use crate::validation_work::{self as w,ArithmeticOperation,RepresentationKind,ValidationWork};
  let arena=m.representation_arena().ok_or_else(||shape::fail("owning app constructor output"))?;
  let Self{plan,normalized_payload,authority,source,current_capacity,mut monetary,records}=self;
  let mut count=w::usize_add(m,monetary.actions.len(),5)?; // authority/eval/decision/consumption/revision
  if monetary.new_claim.is_some(){count=w::usize_add(m,count,1)?;}
  count=w::usize_add(m,count,c.source_observation_inputs.len())?;
  let per=std::mem::size_of::<ConstructedEnvelope>()+64+128+71;
  ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
  let bytes=count.checked_mul(per).ok_or_else(||shape::fail("app output vector payload product"))?;
  let bytes=w::usize_add(m,bytes,4*(64+128+71)+4*std::mem::size_of::<ExactRef>()+std::mem::size_of::<(String,String,ExactRef)>()+std::mem::size_of::<AppHeadProposal>()+128+155)?;
  let output_owner=arena.reserve_external_payload(bytes,m)?;
  let mut output=Vec::new();output.try_reserve_exact(count).map_err(|_|shape::fail("app constructor output vector"))?;
  for input in &c.source_observation_inputs{
    m.visit("app.monetary_pending_source_result")?;
    if !shape::same_ref(m,&input.reference,&c.authorities[0].authority_observation_ref)?&&!shape::same_ref(m,&input.reference,&source.verifier_ref)?{return Err(shape::fail("extra app monetary source observation input"));}
    let raw=records.canonical(m,&input.reference)?;m.new_body(raw.len())?;
    let bytes=m.copy_owned_representation(&arena,raw,RepresentationKind::SourceClosure)?;
    let reference=ExactRef{kind:copy_output(m,&input.reference.kind)?,id:copy_output(m,&input.reference.id)?,digest:copy_output(m,&input.reference.digest)?};
    output.push(ConstructedEnvelope::from_owned(reference,bytes,&arena,m,RepresentationKind::SourceClosure)?);
  }
  if let Some(claim)=monetary.new_claim.take(){output.push(claim);}
  output.push(authority);output.append(&mut monetary.actions);output.push(monetary.evaluation);
  let decision=monetary.decision.decoded(m,4194304)?;
  let id=shape::text(m,&decision,"decision_id")?;let revision=shape::text(m,&decision,"revision")?;
  let mut decision_heads=Vec::new();decision_heads.try_reserve_exact(1).map_err(|_|shape::fail("app decision head proposal vector"))?;
  decision_heads.push((copy_output(m,id)?,copy_output(m,revision)?,copy_output_ref(m,&monetary.decision.reference)?));
  let expected=match &plan.current{Some(old)=>Some(copy_output_ref(m,&old.revision_ref)?),None=>None};
  let app_head=AppHeadProposal{claim_ref:copy_output_ref(m,&monetary.claim_ref)?,expected,proposed:copy_output_ref(m,&monetary.lineage.revision.reference)?,revision:copy_output(m,revision)?};
  output.push(monetary.decision);output.push(monetary.lineage.consumption);output.push(monetary.lineage.revision);
  let mut app_heads=Vec::new();app_heads.try_reserve_exact(1).map_err(|_|shape::fail("app cost head proposal vector"))?;app_heads.push(app_head);
  // All other plan/evidence selection owners drop with the unused operands;
  // copied output/control payloads retain their own original producer leases.
  Ok(Construction{
    economic:EconomicConstruction{terms:plan.original.terms,original_work:OriginalWorkPayload::Retained(plan.original.work)},
    original_reservations:None,normalized_payload,new_envelopes:output,reused_result:None,evaluation_steps:Vec::new(),
    proposed_heads:ProposedHeads{decision:decision_heads,family:Vec::new(),chain:None},
    claim_introductions:Vec::new(),link_introductions:Vec::new(),prospective_obligations:Vec::new(),
    records,history_edges:Vec::new(),app_effects:Some(AppEffects{claims:monetary.claims,heads:app_heads}),app_causal:None,current_capacity:Some(current_capacity),
    candidate_heads:None,candidate_introductions:None,financial_observation:None,funding_effects:None,control_owners:[Some(output_owner),Some(monetary._ownership),Some(plan._ownership),Some(plan.original._ownership),None],
  })
 }
}
fn copy_output(m:&mut super::CoreMeter,value:&str)->crate::Result<String>{use crate::validation_work::ValidationWork;m.scan_bytes(value.len())?;m.write_bytes(value.len())?;Ok(value.to_owned())}
fn copy_output_ref(m:&mut super::CoreMeter,value:&super::data::ExactRef)->crate::Result<super::data::ExactRef>{Ok(super::data::ExactRef{kind:copy_output(m,&value.kind)?,id:copy_output(m,&value.id)?,digest:copy_output(m,&value.digest)?})}
