//! Customer-only attributable cost execution through the shared original-price
//! interpreter and exact-inverse posting primitive. No AP state is produced.
use super::{plan::AppPlan,records};
use super::super::{CoreMeter,body,data::*,emitted_record::ConstructedEnvelope,records::Records,record_body::RecordBody,shape,meter,pricing,working_value::{owning_reference,WorkingValue},cost_postings::{self,OriginalCostPosting}};
use super::super::super::selected_program::SelectedProgram;
use crate::{Result,canonical::audit_arena::{AuditedValue,ExternalBufferLease},validation_work::{self as w,RepresentationKind}};
use serde_json::Value;
macro_rules! call {($f:path,$m:expr,$a:expr,$b:expr)=>{{let(a,b)=($a,$b);$f($m,a,b)}};}
pub(super) struct AppEvaluation {
 pub new_claim:Option<ConstructedEnvelope>,pub claim_ref:ExactRef,
 pub claims:Vec<super::super::candidate::ClaimEffect>,
 pub actions:Vec<ConstructedEnvelope>,pub evaluation:ConstructedEnvelope,
 pub decision:ConstructedEnvelope,pub lineage:records::AppLineage,
 pub(super) _ownership:ExternalBufferLease,
}
fn eq(m:&mut CoreMeter,a:&Value,b:&Value)->Result<()>{shape::equal(m,a,b)}
fn literal(m:&mut CoreMeter,v:&Value,k:&str,s:&str)->Result<()>{let a=shape::text(m,v,k)?;if !shape::same_text(m,a,s)?{return Err(shape::fail("app cost record literal"));}Ok(())}
pub(super) fn price(m:&mut CoreMeter,program:&SelectedProgram,original:&super::original::OriginalApp,eligible:&num_bigint::BigInt)->Result<pricing::PricedInstruction>{
 let activity=shape::text(m,&original.work["payload"],"activity")?;
 let component=shape::text(m,original.declaration,"component_key")?;
 let mut prices=pricing::app_final_cost(program,activity,component,&original.work["payload"]["facts"],eligible,m)?;
 if !w::compare(m,prices.len(),1usize)?.is_eq(){return Err(shape::fail("app cost declaration must select one original instruction"));}
 prices.pop().ok_or_else(||shape::fail("app original price missing"))
}
/// Recompute the current active price from its authenticated retained evidence
/// and original program. Saved payable atoms never enter replacement pricing.
pub(super) fn verify_active(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,original:&super::original::OriginalApp,current:&super::plan::CurrentApp,prior:&super::evidence::AppFinalAmount)->Result<Option<(ExactRef,RecordBody,pricing::PricedInstruction)>>{
 let price=price(m,program,original,&prior.eligible)?;
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning app current price"))?;
 let refs=shape::array(m,&current.decision,"active_action_refs")?;
 let nonzero=!meter::cmp(m,&price.atoms.into(),&0.into())?.is_eq();
 if !w::compare(m,refs.len(),usize::from(nonzero))?.is_eq(){return Err(shape::fail("app current zero/action association"));}
 let evaluation_ref=shape::reference(m,&current.decision["evaluation_ref"])?;
 let evaluation=r.get(m,&c.key,&evaluation_ref,"workflow-evaluation","ledger-workflow-evaluation-proof/1")?;
 shape::closed(m,&evaluation,&["schema","subject_ref","policy_ref","compiled_plan_ref","compiler_ref","binding_ref","fact_schema_ref","facts","context","steps"],&[])?;
 eq(m,&evaluation["subject_ref"],&current.decision["subject_ref"])?;
 let compiled=program.compiled_plan().canonical_value();
 let compiled_ref=program.compiled_plan().reference();
 for(k,s)in[("kind",compiled_ref.kind()),("id",compiled_ref.id()),("digest",compiled_ref.digest())]{let a=shape::text(m,&evaluation["compiled_plan_ref"],k)?;if !shape::same_text(m,a,s)?{return Err(shape::fail("app current original compiled plan"));}}
 eq(m,&evaluation["compiler_ref"],&compiled["compiler_ref"])?;
 let payload=&original.work["payload"];
 for name in ["policy_ref","fact_schema_ref","facts"]{eq(m,&evaluation[name],&payload[name])?;}
 eq(m,&evaluation["context"],&original.terms.binding["accepted_context"])?;
 eq(m,&evaluation["binding_ref"],&payload["binding_ref"])?;
 let steps=shape::array(m,&evaluation,"steps")?;
 if w::compare(m,steps.len(),1usize)?.is_lt()||w::compare(m,steps.len(),2usize)?.is_gt(){return Err(shape::fail("app current evaluation step bound"));}
 let step=steps.last().ok_or_else(||shape::fail("app current evaluation missing"))?;
 shape::closed(m,step,&["instruction","component_slot","predicate","basis_refs","rational_numerator","rational_denominator","rounded","disposition"],&["action_ref"])?;
 literal(m,step,"instruction",&price.ordinal)?;literal(m,step,"component_slot",&original.component_slot)?;literal(m,step,"predicate","true")?;
 let basis=shape::array(m,step,"basis_refs")?.len();
 if !w::compare(m,basis,0usize)?.is_eq(){return Err(shape::fail("app current evaluation basis"));}
 let numerator=cost_postings::scalar(m,&arena,&price.rational.numerator)?;
 let denominator=cost_postings::scalar(m,&arena,&price.rational.denominator)?;
 let amount=cost_postings::amount(m,&arena,&price.atoms.into())?;
 eq(m,&step["rational_numerator"],numerator.value())?;eq(m,&step["rational_denominator"],denominator.value())?;eq(m,&step["rounded"],amount.value())?;
 literal(m,step,"disposition",if nonzero{"posted"}else{"zero"})?;
 if !nonzero{if step.get("action_ref").is_some(){return Err(shape::fail("app zero current step action"));}return Ok(None)}
 let reference=shape::reference(m,&refs[0])?;
 let action=r.get(m,&c.key,&reference,"workflow-action","ledger-workflow-action/1")?;
 shape::closed(m,&action,&["schema","effect_id","decision_id","revision","posting_role","book","component_slot","root_component_slot","role_ref","slot_terms_ref","funding_leg","amount","basis_refs","operation_key"],&[])?;
 let slot=arena.string_value(&original.component_slot,m)?;
 let effect=cost_postings::original_effect(m,&arena,c,&original.terms.binding,&original.claim_ref,slot.value())?;
 literal(m,&action,"effect_id",&effect)?;literal(m,&action,"book","retail")?;
 let transition=shape::text(m,&current.decision,"transition")?;
 let original_posting=shape::same_text(m,transition,"cost_finalize")?;
 if !original_posting&&!shape::same_text(m,transition,"cost_correct")?{return Err(shape::fail("app current cost transition"));}
 literal(m,&action,"posting_role",if original_posting{"original"}else{"replacement"})?;
 for name in ["decision_id","revision","operation_key"]{eq(m,&action[name],&current.decision[name])?;}
 for name in ["component_slot","root_component_slot"]{eq(m,&action[name],slot.value())?;}
 let role=owning_reference(m,&arena,&original.terms.role_ref)?;let terms=owning_reference(m,&arena,&original.terms.slot_terms_ref)?;
 eq(m,&action["role_ref"],role.value())?;eq(m,&action["slot_terms_ref"],terms.value())?;
 eq(m,&action["funding_leg"],&original.terms.slot_terms["funding_leg"])?;eq(m,&action["amount"],amount.value())?;
 let basis=shape::array(m,&action,"basis_refs")?.len();
 if !w::compare(m,basis,0usize)?.is_eq(){return Err(shape::fail("app cost unexpected action basis"));}
 let id=cost_postings::action_id(m,&arena,&action["effect_id"],&action["revision"],&action["posting_role"])?;
 if !shape::same_text(m,&reference.id,&id)?{return Err(shape::fail("app original active action identity"));}
 eq(m,&step["action_ref"],&refs[0])?;
 Ok(Some((reference,action,price)))
}
pub(super) fn evaluate(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,plan:&AppPlan,authority:&ConstructedEnvelope)->Result<AppEvaluation>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning app evaluation"))?;
 // At most one original cost instruction, one current inverse and two steps.
 // Exact Ref/string metadata and all simultaneously live vector buffers are
 // reserved before their producers; owning canonical trees retain own leases.
 let item=w::usize_add(m,std::mem::size_of::<ConstructedEnvelope>(),std::mem::size_of::<AuditedValue>())?;
 let bytes=w::usize_add(m,item,8*(std::mem::size_of::<ExactRef>()+263)+1024)?;
 let bytes=w::usize_add(m,bytes,bytes)?;
 let claim_bytes=w::usize_add(m,3*std::mem::size_of::<super::super::candidate::ClaimEffect>(),6*(64+128+71)+3*c.key.logical_store_id.len()+512)?;
 let bytes=w::usize_add(m,bytes,claim_bytes)?;
 let ownership=arena.reserve_external_payload(bytes,m)?;
 let price=price(m,program,&plan.original,&plan.final_cost.eligible)?;
 let old=match (&plan.current,&plan.old_cost){(Some(current),Some(prior))=>verify_active(m,r,c,program,&plan.original,current,prior)?,(None,None)=>None,_=>return Err(shape::fail("app current/evidence partition"))};
 let new_claim=if plan.current.is_none(){Some(records::claim(m,c,&plan.original,&plan.observation,&plan.final_cost,&plan.dependency_ref)?)}else{None};
 let claim_ref=match (&new_claim,&plan.current){(Some(claim),None)=>claim.reference.clone(),(None,Some(current))=>current.claim_ref.clone(),_=>return Err(shape::fail("app claim introduction partition"))};
 let mut actions=Vec::new();actions.try_reserve_exact(2).map_err(|_|shape::fail("app action vector"))?;
 let mut steps=Vec::new();steps.try_reserve_exact(2).map_err(|_|shape::fail("app evaluation step vector"))?;
 if let Some((reference,action,old_price))=&old{let inverse=cost_postings::exact_inverse(m,c,reference,action,old_price,&plan.revision)?;actions.push(inverse.action.ok_or_else(||shape::fail("app exact inverse missing"))?);steps.push(inverse.step);}
 let slot=arena.string_value(&plan.original.component_slot,m)?;
 let constants=m.parse_owned_representation(&arena,br#"{"book":"retail","evaluation":"ledger-workflow-evaluation-proof/1","decision":"ledger-workflow-decision-revision/3","kind":"cost","active":"active","finalize":"cost_finalize","correct":"cost_correct"}"#,RepresentationKind::SourceDefinition)?;
 let posting=cost_postings::original_cost_posting(m,c,OriginalCostPosting{binding:&plan.original.terms.binding,original_work_claim:&plan.original.claim_ref,component_slot:slot.value(),root_slot:slot.value(),book:&constants.value()["book"],terms_ref:&plan.original.terms.slot_terms_ref,terms:&plan.original.terms.slot_terms,role_ref:&plan.original.terms.role_ref,decision_id:&plan.decision_id,revision:&plan.revision,replacement:plan.current.is_some()},&price)?;
 let active=match &posting.action{Some(action)=>super::retention::refs(m,&arena,&[&action.reference])?,None=>super::retention::refs(m,&arena,&[])?};
 if let Some(action)=posting.action{actions.push(action)}steps.push(posting.step);
 let step_values=if steps.len()==2{arena.array_from_values(&[steps[0].value(),steps[1].value()],m)?}else{arena.array_from_values(&[steps[0].value()],m)?};
 let subject=owning_reference(m,&arena,&claim_ref)?;let binding=owning_reference(m,&arena,&plan.original.binding_ref)?;
 let payload=&plan.original.work["payload"];
 let value=arena.object_from_fields(&[("schema",&constants.value()["evaluation"]),("subject_ref",subject.value()),("policy_ref",&payload["policy_ref"]),("compiled_plan_ref",&plan.original.terms.binding["compiled_plan_ref"]),("compiler_ref",&program.compiled_plan().canonical_value()["compiler_ref"]),("binding_ref",binding.value()),("fact_schema_ref",&payload["fact_schema_ref"]),("facts",&payload["facts"]),("context",&plan.original.terms.binding["accepted_context"]),("steps",step_values.value())],m)?;
 let evaluation=body::document_borrowed(m,"workflow-evaluation",value.value())?;
 let appended=if actions.len()==2{super::retention::refs(m,&arena,&[&actions[0].reference,&actions[1].reference])?}else if actions.len()==1{super::retention::refs(m,&arena,&[&actions[0].reference])?}else{super::retention::refs(m,&arena,&[])?};
 let dependencies=super::retention::refs(m,&arena,&[&plan.dependency_ref])?;
 let authority_ref=owning_reference(m,&arena,&authority.reference)?;let evaluation_ref=owning_reference(m,&arena,&evaluation.reference)?;
 let key=cost_postings::operation_key(m,&arena,c)?;let accepted=arena.string_value(&c.accepted_at,m)?;let decision_id=arena.string_value(&plan.decision_id,m)?;let revision=cost_postings::scalar(m,&arena,&plan.revision)?;
 let value=arena.object_from_fields(&[("schema",&constants.value()["decision"]),("decision_id",decision_id.value()),("revision",revision.value()),("subject_ref",subject.value()),("kind",&constants.value()["kind"]),("transition",if plan.current.is_some(){&constants.value()["correct"]}else{&constants.value()["finalize"]}),("status",&constants.value()["active"]),("original_terms_ref",binding.value()),("authority_ref",authority_ref.value()),("evaluation_ref",evaluation_ref.value()),("actions_appended",appended.value()),("active_action_refs",active.value()),("dependency_refs",dependencies.value()),("operation_key",key.value()),("accepted_at",accepted.value())],m)?;
 let value=if let Some(old)=&plan.current{let prior=owning_reference(m,&arena,&old.decision_ref)?;WorkingValue::Owned(value).with_fields(m,&[("prior_ref",prior.value())])?.into_owned()?}else{value};
 let seed=arena.array_from_values(&[decision_id.value(),revision.value()],m)?;let id=body::semantic_id(m,"wdrev_","workflow-decision-revision",seed.value())?;
 let decision=body::record_borrowed(m,&c.key,"workflow-decision",id,value.value())?;
 let lineage=records::advance(m,c,&claim_ref,&plan.observation.pair.attestation_ref,&plan.final_cost,&decision.reference,&plan.revision,plan.current.as_ref().map(|old|&old.revision_ref))?;
 use super::super::candidate::{ClaimEffect,PermanentClaimKey};
 fn copy(m:&mut CoreMeter,s:&str)->Result<String>{use crate::validation_work::ValidationWork;m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
 fn copy_ref(m:&mut CoreMeter,r:&ExactRef)->Result<ExactRef>{Ok(ExactRef{kind:copy(m,&r.kind)?,id:copy(m,&r.id)?,digest:copy(m,&r.digest)?})}
 let mut claims=Vec::new();claims.try_reserve_exact(3).map_err(|_|shape::fail("app permanent claim effects"))?;
 if new_claim.is_some(){claims.push(ClaimEffect::derived(PermanentClaimKey::AppCostComponent{logical_store_id:copy(m,&c.key.logical_store_id)?,original_work_ref:copy_ref(m,&plan.original.work_ref)?,component_slot:copy(m,&plan.original.component_slot)?},copy_ref(m,&claim_ref)?));}
 claims.push(ClaimEffect::derived(PermanentClaimKey::AppFinalEvidence{logical_store_id:copy(m,&c.key.logical_store_id)?,final_evidence_ref:copy_ref(m,&plan.final_cost.pair.attestation_ref)?},copy_ref(m,&lineage.consumption.reference)?));
 let cost_revision=shape::text(m,&plan.final_cost.pair.attestation,"cost_revision")?;
 claims.push(ClaimEffect::derived(PermanentClaimKey::AppObservationRevision{logical_store_id:copy(m,&c.key.logical_store_id)?,observation_ref:copy_ref(m,&plan.observation.pair.attestation_ref)?,cost_revision:copy(m,cost_revision)?},copy_ref(m,&lineage.consumption.reference)?));
 Ok(AppEvaluation{new_claim,claim_ref,claims,actions,evaluation,decision,lineage,_ownership:ownership})
}
