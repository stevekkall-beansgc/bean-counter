//! App dependent closure uses the SAME original price verifier and exact
//! inverse primitive. Root operation identity is never changed to impersonate
//! a dependent source. These private outputs still require the complete root
//! closure, all-ancestor capacity and authenticated observation factory.
use super::{original::OriginalApp,plan::CurrentApp};
use super::super::{CoreMeter,authority::EconomicMode,arithmetic,body,cost_postings,data::*,emitted_record::ConstructedEnvelope,meter,records::Records,shape,working_value::{WorkingValue,owning_reference}};
use super::super::super::selected_program::SelectedProgram;
use crate::{Result,canonical::audit_arena::{AuditedValue,ExternalBufferLease},validation_work::{self as w,RepresentationKind}};
use serde_json::Value;

pub(in crate::policy::workflow::transition_constructor) struct FinalizedClosure {
 pub authority:ConstructedEnvelope,
 pub check:AuditedValue,
 pub dependency_ref:ExactRef,
 pub inverse:Option<ConstructedEnvelope>,
 pub evaluation:ConstructedEnvelope,
 pub decision:ConstructedEnvelope,
 pub revision:ConstructedEnvelope,
 pub source_slot:String,
 _ownership:ExternalBufferLease,
}
fn literal(m:&mut CoreMeter,value:&Value,key:&str,wanted:&str)->Result<()> {
 let actual=shape::text(m,value,key)?;if !shape::same_text(m,actual,wanted)?{return Err(shape::fail("app causal literal"));}Ok(())
}
/// Only validated original root identity and exact per-kind operands determine
/// the retained edge. No future digest, caller bound or saved expected output.
pub(in crate::policy::workflow::transition_constructor) fn dependency(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,original:&OriginalApp,dependent_id:&str)->Result<(ExactRef,super::super::record_body::RecordBody)> {
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning app causal dependency"))?;
 let root=r.get(m,&c.key,&original.original_decision_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
 let binding=owning_reference(m,&arena,&original.binding_ref)?;
 let dependent=arena.string_value(dependent_id,m)?;
 let seed=arena.array_from_values(&[&root["decision_id"],dependent.value(),binding.value()],m)?;
 let id=body::semantic_id(m,"wdep_","workflow-dependency",seed.value())?;
 let local=c.authorities.first().is_some_and(|a|a.authority_observation_ref.is_none());
 let(reference,value)=r.by_identity(m,&c.key,"workflow-dependency",&id,if local{"ledger-workflow-dependency/2"}else{"ledger-workflow-dependency/1"})?;
 if local{shape::closed(m,&value,&["schema","ancestor_decision_id","dependent_decision_id","kind","target_ref","original_terms_ref","maximum_actions","operation_key"],&[])?;}else{shape::closed(m,&value,&["schema","ancestor_decision_id","dependent_decision_id","kind","target_ref","original_terms_ref","maximum_actions","maximum_records","maximum_bytes","maximum_work","operation_key"],&[])?;}
 literal(m,&value,"kind","delayed_cost")?;
 shape::equal(m,&value["ancestor_decision_id"],&root["decision_id"])?;
 shape::equal(m,&value["dependent_decision_id"],dependent.value())?;
 shape::equal(m,&value["original_terms_ref"],binding.value())?;
 let work=owning_reference(m,&arena,&original.work_ref)?;shape::equal(m,&value["target_ref"],work.value())?;
 for key in ["maximum_actions","maximum_records","maximum_bytes","maximum_work"]{if local&&key!="maximum_actions"{continue;}let raw=shape::text(m,&value,key)?;crate::policy::workflow::facts::counter_with_work(raw,m)?;}
 Ok((reference,value))
}
/// Select the immutable original dependent edge, then consume an actual
/// expected current decision/revision from the normalized root request.
pub(in crate::policy::workflow::transition_constructor) fn finalized(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,original:&OriginalApp,current:CurrentApp,root_ref:&ExactRef,root_operation:&str,reversal:&Value,state:&EconomicStateData)->Result<FinalizedClosure>{
 if !program.uses_app_cost_causal_recipe(){return Err(shape::fail("app causal recipe required"));}
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning app causal closure"))?;
 let metadata=w::usize_add(m,std::mem::size_of::<FinalizedClosure>(),20*(64+128+71)+128)?;
 let ownership=arena.reserve_external_payload(metadata,m)?;
 shape::closed(m,reversal,&["decision_ref","expected_revision","authority_evidence_refs"],&[])?;
 let current_ref=owning_reference(m,&arena,&current.decision_ref)?;
 shape::equal(m,&reversal["decision_ref"],current_ref.value())?;
 shape::equal(m,&reversal["expected_revision"],&current.revision["revision"])?;
 let decision_id=shape::text(m,&current.decision,"decision_id")?;
 let work_id=arena.string_value(&original.work_ref.id,m)?;let component=arena.string_value(&original.component_slot,m)?;let app_tag=arena.string_value("app_cost",m)?;
 let decision_seed=arena.array_from_values(&[&original.work["key"]["scope"],work_id.value(),component.value(),app_tag.value()],m)?;
 let expected_id=body::semantic_id(m,"wdecision_","workflow-decision",decision_seed.value())?;
 if !shape::same_text(m,decision_id,&expected_id)?{return Err(shape::fail("app causal original decision identity"));}
 let(dependency_ref,_dependency)=dependency(m,r,c,original,decision_id)?;
 let dependency_value=owning_reference(m,&arena,&dependency_ref)?;
 shape::equal(m,&current.claim["dependency_ref"],dependency_value.value())?;
 let binding=owning_reference(m,&arena,&original.binding_ref)?;shape::equal(m,&current.claim["original_terms_ref"],binding.value())?;
 let observation_proof=super::plan::proof_for(m,r,c,state,&current.claim["observation_ref"])?;
 let observation=super::evidence::resolve_observation(m,r,c,program,original,observation_proof.value())?;
 shape::equal(m,&current.claim["subject"],&observation.pair.attestation["subject"])?;
 let final_proof=super::plan::proof_for(m,r,c,state,&current.revision["current_evidence_ref"])?;
 let final_cost=super::evidence::resolve_final(m,r,c,program,original,&observation,final_proof.value())?;
 let consumed=cost_postings::amount(m,&arena,&final_cost.eligible)?;
 shape::equal(m,&current.consumption["eligible_amount"],consumed.value())?;
 shape::equal(m,&current.consumption["cost_revision"],&final_cost.pair.attestation["cost_revision"])?;
 let slot=shape::text(m,original.declaration,"correction_source_slot")?;
 let source=original.source(m,r,c,program,slot,EconomicMode::CostCorrect)?;
 super::consent::current(program,m,r,c,&original.terms.binding,&original.terms.accepted,state)?;
 let(until,late)=super::timing::causal(m,c,original,&current)?;
 if !shape::same_text(m,root_operation,"quantity.correct")?&&!shape::same_text(m,root_operation,"work.reverse")?{return Err(shape::fail("app causal root operation"));}
 let root=r.get(m,&c.key,root_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
 let original_root=r.get(m,&c.key,&original.original_decision_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
 shape::equal(m,&root["decision_id"],&original_root["decision_id"])?;
 let work=owning_reference(m,&arena,&original.work_ref)?;shape::equal(m,&root["subject_ref"],work.value())?;
 // The root constructor separately checks its current head/CAS and window.
 // Every supplemental retained reference is provenance, never a new authority.
 let evidence=shape::array(m,reversal,"authority_evidence_refs")?;
 if w::compare(m,evidence.len(),1usize)?.is_lt()||w::compare(m,evidence.len(),16usize)?.is_gt(){return Err(shape::fail("app causal supporting evidence count"));}
 for value in evidence{m.visit("app.causal_supporting_provenance")?;let reference=shape::reference(m,value)?;r.provenance(m,&c.key,&reference)?;}
 let evidence=arena.clone_value(&reversal["authority_evidence_refs"],m)?;
 let mut evidence=arena.object_from_fields(&[("refs",evidence.value())],m)?;
 super::super::intent::sort_owned_set(&mut evidence,"refs",false,m,&arena)?;
 let base=super::authority::value(m,c,program,original,&source,EconomicMode::CostCorrect,Some(&observation.pair.attestation_ref),&[&final_cost.pair.attestation_ref],Some(&current.revision_ref))?;
 let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-app-causal-authority/1","window":"original_standalone_window","clause":"accepted_app_causal_clause","decision":"ledger-workflow-decision-revision/3","evaluation":"ledger-workflow-evaluation-proof/1","lineage":"ledger-workflow-app-cost-revision/1","kind":"cost","transition":"dependent_reverse","status":"reversed","empty":[]}"#,RepresentationKind::SourceDefinition)?;
 let fields_size=w::usize_add(m,48*std::mem::size_of::<(&str,&Value)>(),0)?;
 let _fields=arena.reserve_external_payload(fields_size,m)?;
 let mut fields=Vec::new();fields.try_reserve_exact(48).map_err(|_|shape::fail("app causal authority fields"))?;
 // Different closed authority schema: do not carry the parent's pending list.
 for(key,value)in base.value().as_object().ok_or_else(||shape::fail("app authority object"))?{m.visit("app.causal_authority_field")?;if !shape::same_text(m,key,"schema")?&&!shape::same_text(m,key,"app_pending_cost_closure_checks")?&&!shape::same_text(m,key,"evidence_refs")?{fields.push((key.as_str(),value));}}
 let root_value=owning_reference(m,&arena,root_ref)?;let operation=arena.string_value(root_operation,m)?;
 let slot_value=arena.string_value(&original.component_slot,m)?;let terms=owning_reference(m,&arena,&original.terms.slot_terms_ref)?;
 let deadline=arena.string_value(until.as_str(),m)?;
 let initial=owning_reference(m,&arena,&current.initial_decision_ref)?;
 let basis=if late{&constants.value()["clause"]}else{&constants.value()["window"]};
 let selected_schema=arena.string_value(if program.uses_local_execution_recipe(){"ledger-workflow-app-causal-authority/2"}else{"ledger-workflow-app-causal-authority/1"},m)?;
 fields.extend_from_slice(&[("schema",selected_schema.value()),("evidence_refs",&evidence.value()["refs"]),("causal_root_ref",root_value.value()),("causal_root_operation",operation.value()),("original_work_ref",work.value()),("dependency_ref",dependency_value.value()),("component_slot",slot_value.value()),("original_app_terms_ref",terms.value()),("standalone_correction_until",deadline.value()),("deadline_basis",basis),("initial_cost_decision_ref",initial.value()),("initial_cost_accepted_at",&current.initial_decision["accepted_at"])]);
 if original.declaration.get("causal_reversal").is_some(){super::timing::clause(m,original.declaration)?;fields.push(("causal_reversal",&original.declaration["causal_reversal"]));}
 let value=arena.object_from_fields(&fields,m)?;
 let authority=body::document_borrowed(m,"workflow-economic-authority",value.value())?;
 let authority_value=owning_reference(m,&arena,&authority.reference)?;
 let active=super::evaluation::verify_active(m,r,c,program,original,&current,&final_cost)?;
 let old_revision_raw=shape::text(m,&current.revision,"revision")?;
 let old_revision=arithmetic::integer(m,old_revision_raw,false)?;
 let next=meter::add(m,&old_revision,&1.into())?;
 let posting=match active{Some((reference,action,price))=>Some(cost_postings::exact_inverse(m,c,&reference,&action,&price,&next)?),None=>None};
 let steps=match &posting{Some(posting)=>arena.array_from_values(&[posting.step.value()],m)?,None=>arena.array_from_values(&[],m)?};
 let payload=&original.work["payload"];let claim=owning_reference(m,&arena,&current.claim_ref)?;
 let value=arena.object_from_fields(&[("schema",&constants.value()["evaluation"]),("subject_ref",claim.value()),("policy_ref",&payload["policy_ref"]),("compiled_plan_ref",&original.terms.binding["compiled_plan_ref"]),("compiler_ref",&program.compiled_plan().canonical_value()["compiler_ref"]),("binding_ref",binding.value()),("fact_schema_ref",&payload["fact_schema_ref"]),("facts",&payload["facts"]),("context",&original.terms.binding["accepted_context"]),("steps",steps.value())],m)?;
 let evaluation=body::document_borrowed(m,"workflow-evaluation",value.value())?;
 let inverse=posting.and_then(|posting|posting.action);
 let appended=match &inverse{Some(action)=>super::retention::refs(m,&arena,&[&action.reference])?,None=>super::retention::refs(m,&arena,&[])?};
 let eval=owning_reference(m,&arena,&evaluation.reference)?;
 let key=cost_postings::operation_key(m,&arena,c)?;let at=arena.string_value(&c.accepted_at,m)?;
 let revision=cost_postings::scalar(m,&arena,&next)?;
 let value=arena.object_from_fields(&[("schema",&constants.value()["decision"]),("decision_id",&current.decision["decision_id"]),("revision",revision.value()),("prior_ref",current_ref.value()),("subject_ref",claim.value()),("kind",&constants.value()["kind"]),("transition",&constants.value()["transition"]),("status",&constants.value()["status"]),("original_terms_ref",binding.value()),("authority_ref",authority_value.value()),("evaluation_ref",eval.value()),("actions_appended",appended.value()),("active_action_refs",&constants.value()["empty"]),("dependency_refs",&current.decision["dependency_refs"]),("operation_key",key.value()),("accepted_at",at.value())],m)?;
 let seed=arena.array_from_values(&[&current.decision["decision_id"],revision.value()],m)?;let id=body::semantic_id(m,"wdrev_","workflow-decision-revision",seed.value())?;
 let decision=body::record_borrowed(m,&c.key,"workflow-decision",id,value.value())?;
 let decision_value=owning_reference(m,&arena,&decision.reference)?;let prior=owning_reference(m,&arena,&current.revision_ref)?;let consumption=owning_reference(m,&arena,&current.consumption_ref)?;
 // Keep the actual current evidence and permanent consumption. A full inverse
 // is no fake zero observation, no replacement consumption, and no release.
 let value=arena.object_from_fields(&[("schema",&constants.value()["lineage"]),("cost_claim_ref",claim.value()),("revision",revision.value()),("prior_ref",prior.value()),("current_decision_ref",decision_value.value()),("current_evidence_ref",&current.revision["current_evidence_ref"]),("consumption_ref",consumption.value()),("status",&constants.value()["status"]),("operation_key",key.value())],m)?;
 let revision=body::document_borrowed(m,"workflow-cost",value.value())?;
 let original_grants=super::authority::grant_refs(m,&source.original_grant_refs)?;let current_grant=owning_reference(m,&arena,&c.authorities[0].current_grant_ref)?;let source_slot=arena.string_value(slot,m)?;
 let check=arena.object_from_fields(&[("original_work_ref",work.value()),("original_cost_claim_ref",claim.value()),("current_cost_revision_ref",prior.value()),("current_decision_ref",current_ref.value()),("dependency_ref",dependency_value.value()),("original_binding_ref",binding.value()),("original_terms_ref",terms.value()),("original_source_grant_refs",original_grants.value()),("current_source_grant_ref",current_grant.value()),("correction_source_slot",source_slot.value()),("causal_root_ref",root_value.value()),("causal_root_operation",operation.value()),("component_slot",slot_value.value()),("dependent_authority_ref",authority_value.value()),("deadline_basis",basis),("initial_cost_decision_ref",initial.value())],m)?;
 use crate::validation_work::ValidationWork;m.scan_bytes(slot.len())?;m.write_bytes(slot.len())?;let source_slot=slot.to_owned();
 Ok(FinalizedClosure{authority,check,dependency_ref,inverse,evaluation,decision,revision,source_slot,_ownership:ownership})
}

/// Waiting is a permanent-claim absence case, never a missing-head fallback.
/// The host authenticates this exact preoperation observation through complete
/// chronological replay; these fields contain no caller 'absent' capability.
pub(in crate::policy::workflow::transition_constructor) struct PendingClosure {
 pub authority:ConstructedEnvelope,
 pub check:AuditedValue,
 pub dependency_ref:ExactRef,
 pub source_slot:String,
 original_root:super::super::record_body::RecordBody,
 original_work_ref:ExactRef,
 _ownership:ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) fn pending(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,original:&OriginalApp,dependent_id:&str,root_ref:&ExactRef,root_operation:&str,state:&EconomicStateData)->Result<PendingClosure>{
 if !program.uses_app_cost_causal_recipe(){return Err(shape::fail("app pending causal recipe"));}
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning app pending causal"))?;
 let size=w::usize_add(m,std::mem::size_of::<PendingClosure>(),8*(64+128+71)+128)?;let ownership=arena.reserve_external_payload(size,m)?;
 let work=owning_reference(m,&arena,&original.work_ref)?;let component=arena.string_value(&original.component_slot,m)?;
 let app=state.app.ok_or_else(||shape::fail("pending permanent app keys missing"))?;
 if w::compare(m,app.cost_claim_refs.len(),4096usize)?.is_gt()||w::compare(m,app.cost_heads.len(),4096usize)?.is_gt(){return Err(shape::fail("pending selected app keys bound"));}
 // Inspect permanent claim hits even when terminal/zero or with no live head.
 // A provided cost head also demands its actual permanent claim association.
 for reference in app.cost_claim_refs.iter().chain(app.cost_heads.iter().map(|head|&head.cost_claim_ref)){
   m.visit("app.pending_permanent_claim_hit")?;
   let claim=r.get(m,&c.key,reference,"workflow-cost","ledger-workflow-app-cost-claim/1")?;
   if shape::same(m,&claim["subject"]["original_work_ref"],work.value())?&&shape::same(m,&claim["subject"]["component_slot"],component.value())?{return Err(shape::fail("existing permanent app claim cannot be pending"));}
 }
 let(dependency_ref,_)=dependency(m,r,c,original,dependent_id)?;
 let dependency=owning_reference(m,&arena,&dependency_ref)?;
 let root=r.get(m,&c.key,root_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
 let original_root=r.get(m,&c.key,&original.original_decision_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
 shape::equal(m,&root["decision_id"],&original_root["decision_id"])?;shape::equal(m,&root["subject_ref"],work.value())?;
 if !shape::same_text(m,root_operation,"quantity.correct")?&&!shape::same_text(m,root_operation,"work.reverse")?{return Err(shape::fail("app pending root operation"));}
 let slot=shape::text(m,original.declaration,"correction_source_slot")?;
 let source=original.source(m,r,c,program,slot,EconomicMode::CostCorrect)?;
 super::consent::current(program,m,r,c,&original.terms.binding,&original.terms.accepted,state)?;
 let observed=if program.uses_local_execution_recipe(){
  for head in &state.financial.decision_heads{m.visit("app.local_pending_decision_absence")?;if shape::same_text(m,&head.decision_id,dependent_id)?{return Err(shape::fail("existing dependent decision cannot be pending"));}}
  None
 }else{
 let pair=state.observation.ok_or_else(||shape::fail("pending app requires actual financial observation pair"))?;
 let pair_inputs=[pair.observation,pair.verification];
 let mut observation_records=Records::borrowed_app(m,&arena,&c.key,&pair_inputs,&[])?;
 let observation=observation_records.get(m,&c.key,pair.observation.reference,"workflow-financial-history-observation","ledger-workflow-financial-history-observation/3")?;
 shape::closed(m,&observation,&["schema","logical_store_id","scope","installation_mode","root_ref","pending_operation_key","snapshot_through_sequence","preexisting_input_refs","history_edges","head_pins","verifier_ref","prefix_digest","origin_count","document_count","audited_retained_bytes","observed_at"],&[])?;
 let verifier=shape::reference(m,&observation["verifier_ref"])?;
 if !shape::same_ref(m,&verifier,pair.verification.reference)?{return Err(shape::fail("pending financial verifier pair mismatch"));}
 let verifier=observation_records.get(m,&c.key,pair.verification.reference,"workflow-authority-audit-verifier","ledger-authority-audit-verifier/2")?;
 shape::closed(m,&verifier,&["schema","profile","algorithm","resources","artifacts","origin_recipes"],&[])?;
 literal(m,&verifier,"profile","ledger-financial-origin-audit/1")?;literal(m,&verifier,"algorithm","verified-introduction-fixed-point/1")?;literal(m,&verifier,"resources","workflow-profile-b-origin-audit/1")?;
 // Artifact/recipe source eligibility and prefix digest are rederived by the
 // original host audit, never accepted because these serialized claims match.
 let key=cost_postings::operation_key(m,&arena,c)?;
 shape::equal(m,&observation["pending_operation_key"],key.value())?;
 for name in ["logical_store_id","scope"]{shape::equal(m,&observation[name],&key.value()[name])?;}
 shape::equal(m,&observation["root_ref"],&source.required_observation()?["root_ref"])?;
 let mode=arena.string_value(&c.installation_mode,m)?;let at=arena.string_value(&c.accepted_at,m)?;
 shape::equal(m,&observation["installation_mode"],mode.value())?;shape::equal(m,&observation["observed_at"],at.value())?;
 for key in ["snapshot_through_sequence","origin_count","document_count","audited_retained_bytes"]{let raw=shape::text(m,&observation,key)?;crate::policy::workflow::facts::counter_with_work(raw,m)?;}
 for key in ["preexisting_input_refs","history_edges","head_pins"]{let values=shape::array(m,&observation,key)?;if w::compare(m,values.len(),4096usize)?.is_gt(){return Err(shape::fail("pending observation set bound"));}}
 // No cost head for this dependent may be hidden behind another claim key.
 for pin in shape::array(m,&observation,"head_pins")?{m.visit("app.pending_observation_head")?;shape::closed(m,pin,&["kind","ref","revision"],&[])?;let kind=shape::text(m,pin,"kind")?;if shape::same_text(m,kind,"decision")?{let reference=shape::reference(m,&pin["ref"])?;let selected=r.get_schemas(m,&c.key,&reference,"workflow-decision",&["ledger-workflow-decision-revision/1","ledger-workflow-decision-revision/3"])?;let id=shape::text(m,&selected,"decision_id")?;if shape::same_text(m,id,dependent_id)?{return Err(shape::fail("initial app decision cannot be pending"));}}}
 let observed=owning_reference(m,&arena,pair.observation.reference)?;
 Some(observed)
 };
 let base=super::authority::value(m,c,program,original,&source,EconomicMode::CostCorrect,None,&[],None)?;
 let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-app-pending-causal-authority/1","absence":"no_initial_app_cost_claim"}"#,RepresentationKind::SourceDefinition)?;
 let fields_size=w::usize_add(m,44*std::mem::size_of::<(&str,&Value)>(),0)?;let _fields=arena.reserve_external_payload(fields_size,m)?;
 let mut fields=Vec::new();fields.try_reserve_exact(44).map_err(|_|shape::fail("app pending authority fields"))?;
 for(key,value)in base.value().as_object().ok_or_else(||shape::fail("app pending authority object"))?{m.visit("app.pending_authority_field")?;if !shape::same_text(m,key,"schema")?&&!shape::same_text(m,key,"app_pending_cost_closure_checks")?{fields.push((key.as_str(),value));}}
 let root_value=owning_reference(m,&arena,root_ref)?;let operation=arena.string_value(root_operation,m)?;let terms=owning_reference(m,&arena,&original.terms.slot_terms_ref)?;
 let selected_schema=arena.string_value(if program.uses_local_execution_recipe(){"ledger-workflow-app-pending-causal-authority/2"}else{"ledger-workflow-app-pending-causal-authority/1"},m)?;
 fields.extend_from_slice(&[("schema",selected_schema.value()),("causal_root_ref",root_value.value()),("causal_root_operation",operation.value()),("original_work_ref",work.value()),("dependency_ref",dependency.value()),("component_slot",component.value()),("original_app_terms_ref",terms.value()),("absence_kind",&constants.value()["absence"])]);
 if let Some(observed)=&observed{fields.push(("financial_observation_ref",observed.value()));}
 if original.declaration.get("causal_reversal").is_some(){super::timing::clause(m,original.declaration)?;fields.push(("causal_reversal",&original.declaration["causal_reversal"]));}
 let value=arena.object_from_fields(&fields,m)?;let authority=body::document_borrowed(m,"workflow-economic-authority",value.value())?;
 let authority_value=owning_reference(m,&arena,&authority.reference)?;let binding=owning_reference(m,&arena,&original.binding_ref)?;let grants=super::authority::grant_refs(m,&source.original_grant_refs)?;let current=owning_reference(m,&arena,&c.authorities[0].current_grant_ref)?;let source_slot=arena.string_value(slot,m)?;
 let check=arena.object_from_fields(&[("original_work_ref",work.value()),("dependency_ref",dependency.value()),("original_binding_ref",binding.value()),("original_terms_ref",terms.value()),("original_source_grant_refs",grants.value()),("current_source_grant_ref",current.value()),("correction_source_slot",source_slot.value()),("causal_root_ref",root_value.value()),("causal_root_operation",operation.value()),("component_slot",component.value()),("dependent_authority_ref",authority_value.value()),("absence_kind",&constants.value()["absence"])],m)?;
 let check=if let Some(observed)=&observed{WorkingValue::Owned(check).with_fields(m,&[("financial_observation_ref",observed.value())])?.into_owned()?}else{check};
 use crate::validation_work::ValidationWork;m.scan_bytes(slot.len())?;m.write_bytes(slot.len())?;let source_slot=slot.to_owned();
 let original_work_ref=ExactRef{kind:copy(m,&original.work_ref.kind)?,id:copy(m,&original.work_ref.id)?,digest:copy(m,&original.work_ref.digest)?};
 Ok(PendingClosure{authority,check,dependency_ref,source_slot,original_root,original_work_ref,_ownership:ownership})
}
fn copy(m:&mut CoreMeter,s:&str)->Result<String>{use crate::validation_work::ValidationWork;m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
impl PendingClosure {
 /// Only after the actual root result permanently excludes this precise edge.
 /// The complete capacity pass additionally checks every original ancestor.
 pub(in crate::policy::workflow::transition_constructor) fn retire(&self,m:&mut CoreMeter,c:&DraftContextData,new_root:&ConstructedEnvelope)->Result<ConstructedEnvelope>{
   let arena=m.representation_arena().ok_or_else(||shape::fail("owning pending retirement"))?;
   let root=new_root.decoded(m,4194304)?;
   shape::equal(m,&root["decision_id"],&self.original_root["decision_id"])?;
   let work=owning_reference(m,&arena,&self.original_work_ref)?;shape::equal(m,&root["subject_ref"],work.value())?;
   let root_operation=cost_postings::operation_key(m,&arena,c)?;shape::equal(m,&root["operation_key"],root_operation.value())?;
   for value in shape::array(m,&root,"dependency_refs")?{m.visit("app.pending_new_root_dependency")?;let reference=shape::reference(m,value)?;if shape::same_ref(m,&reference,&self.dependency_ref)?{return Err(shape::fail("new root retains pending dependency"));}}
   let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-capacity-retirement/1","reason":"no_future_or_active_dependency"}"#,RepresentationKind::SourceDefinition)?;
   let dependency=owning_reference(m,&arena,&self.dependency_ref)?;let authority=owning_reference(m,&arena,&self.authority.reference)?;let closure=super::retention::refs(m,&arena,&[&new_root.reference])?;
   let value=arena.object_from_fields(&[("schema",&constants.value()["schema"]),("dependency_ref",dependency.value()),("closure_state_refs",closure.value()),("reason",&constants.value()["reason"]),("authority_ref",authority.value()),("operation_key",root_operation.value())],m)?;
   let seed=arena.array_from_values(&[dependency.value(),root_operation.value()],m)?;let id=body::semantic_id(m,"wretire_","workflow-capacity-retirement",seed.value())?;
   body::record_borrowed(m,&c.key,"workflow-capacity-retirement",id,value.value())
 }
}

pub(in crate::policy::workflow::transition_constructor) struct CausalBatch {
 pub finalized:Vec<FinalizedClosure>,
 pub pending:Vec<PendingClosure>,
 pub closed_dependencies:Vec<ExactRef>,
 pub used_reversals:Vec<usize>,
 _ownership:ExternalBufferLease,
}
/// Derive the complete app part of the original work decision's dependency
/// union. Permanent claims, not availability of a current head, partition it.
/// Families consume the remaining nominations in the same parent constructor.
pub(in crate::policy::workflow::transition_constructor) fn prepare(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,work_ref:&ExactRef,root_ref:&ExactRef,root_operation:&str,original_payload:&Value,terms:&super::super::authority::Terms,request:&mut WorkingValue,state:&EconomicStateData,current_capacity:&super::super::capacity::current::CurrentCapacity)->Result<CausalBatch>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning app causal batch"))?;
 let declarations=shape::array(m,program.original_policy_value(),"app_delayed_costs")?;
 let count=declarations.len();if w::compare(m,count,31usize)?.is_gt(){return Err(shape::fail("app causal dependent count"));}
 use crate::validation_work::{ValidationWork,ArithmeticOperation};
 let per=std::mem::size_of::<FinalizedClosure>()+std::mem::size_of::<PendingClosure>()+std::mem::size_of::<usize>()+std::mem::size_of::<ExactRef>()+64+128+71;
 ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
 let size=count.checked_mul(per).ok_or_else(||shape::fail("app causal batch metadata"))?;let ownership=arena.reserve_external_payload(size,m)?;
 let mut batch=CausalBatch{finalized:Vec::new(),pending:Vec::new(),closed_dependencies:Vec::new(),used_reversals:Vec::new(),_ownership:ownership};
 batch.finalized.try_reserve_exact(count).map_err(|_|shape::fail("app finalized closure vector"))?;batch.pending.try_reserve_exact(count).map_err(|_|shape::fail("app pending closure vector"))?;batch.closed_dependencies.try_reserve_exact(count).map_err(|_|shape::fail("app closed dependency vector"))?;batch.used_reversals.try_reserve_exact(count).map_err(|_|shape::fail("app used reversal vector"))?;
 let app=state.app.ok_or_else(||shape::fail("app causal permanent-key state missing"))?;
 let work=owning_reference(m,&arena,work_ref)?;
 for declaration in declarations{
   m.visit("app.causal_original_declaration")?;
   if !shape::same(m,&declaration["activity"],&original_payload["activity"])?{continue;}
   let component=shape::text(m,declaration,"component_key")?;
   let mapping=shape::array(m,&terms.instantiation,"component_map")?;let mapping=shape::named(m,mapping,"template_key",component)?;let slot=shape::text(m,mapping,"slot_id")?;
   let original=super::original::select_for_root(m,r,c,program,work_ref,slot,state)?;
   let component=arena.string_value(slot,m)?;
   let discriminator=arena.string_value("app_cost",m)?;let work_id=arena.string_value(&work_ref.id,m)?;
   let decision_seed=arena.array_from_values(&[&original.work["key"]["scope"],work_id.value(),component.value(),discriminator.value()],m)?;
   let dependent_id=body::semantic_id(m,"wdecision_","workflow-decision",decision_seed.value())?;
   let mut claim_hit=None;
   for reference in app.cost_claim_refs{
     m.visit("app.causal_permanent_claim_selection")?;
     let claim=r.get(m,&c.key,reference,"workflow-cost","ledger-workflow-app-cost-claim/1")?;
     if shape::same(m,&claim["subject"]["original_work_ref"],work.value())?&&shape::same(m,&claim["subject"]["component_slot"],component.value())?{if claim_hit.is_some(){return Err(shape::fail("duplicate permanent app component claim"));}claim_hit=Some(reference);}
   }
   if let Some(claim_ref)=claim_hit{
     let mut head=None;
     for candidate in app.cost_heads{m.visit("app.causal_claim_head")?;if shape::same_ref(m,&candidate.cost_claim_ref,claim_ref)?{if head.is_some(){return Err(shape::fail("duplicate app claim head"));}head=Some(candidate);}}
     let head=head.ok_or_else(||shape::fail("existing app claim lacks current head"))?;
     let revision=r.get(m,&c.key,&head.reference,"workflow-cost","ledger-workflow-app-cost-revision/1")?;
     let claim=owning_reference(m,&arena,claim_ref)?;shape::equal(m,&revision["cost_claim_ref"],claim.value())?;
     let status=shape::text(m,&revision,"status")?;
     if shape::same_text(m,status,"reversed")?{
       // A closed permanent claim never becomes pending. Validate its actual
       // terminal decision and preserve its immutable dependency identity.
       let ref_value=shape::reference(m,&revision["current_decision_ref"])?;
       let decision=r.get(m,&c.key,&ref_value,"workflow-decision","ledger-workflow-decision-revision/3")?;
       literal(m,&decision,"status","reversed")?;literal(m,&decision,"transition","dependent_reverse")?;literal(m,&decision,"decision_id",&dependent_id)?;
       super::plan::decision_head(m,state,&ref_value,&decision)?;
       shape::equal(m,&decision["subject_ref"],claim.value())?;shape::equal(m,&decision["revision"],&revision["revision"])?;
       let values=shape::array(m,&decision,"active_action_refs")?;if !w::compare(m,values.len(),0usize)?.is_eq(){return Err(shape::fail("closed app decision retains active actions"));}
       let(dependency_ref,_)=dependency(m,r,c,&original,&dependent_id)?;
       batch.closed_dependencies.push(dependency_ref);
       continue;
     }
     literal(m,&revision,"status","active")?;
     let reversals=shape::array(m,request,"dependent_reversals")?;
     let decision=shape::reference(m,&revision["current_decision_ref"])?;
     let mut nomination=None;
     for(index,item)in reversals.iter().enumerate(){m.visit("app.causal_current_nomination")?;let reference=shape::reference(m,&item["decision_ref"])?;if shape::same_ref(m,&reference,&decision)?{if nomination.is_some(){return Err(shape::fail("duplicate app reversal nomination"));}nomination=Some(index);}}
     let index=nomination.ok_or_else(||shape::fail("required finalized app reversal missing"))?;
     let expected=shape::text(m,&reversals[index],"expected_revision")?;
     let current=super::plan::current(m,r,c,claim.value(),expected,state)?;
     let selected=WorkingValue::copy(m,&reversals[index])?;
     let mut normalized=selected;
     normalized.normalize_set(m,"authority_evidence_refs",false)?;
     let closed=finalized(m,r,c,program,&original,current,root_ref,root_operation,&normalized,state)?;
     let values=shape::array(m,&normalized,"authority_evidence_refs")?;
     // Keep the caller's exact object, replacing only the named Ref set with
     // the same normalized set that the authority consumed.
     ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&values.len().into(),&(std::mem::size_of::<ExactRef>()+64+128+71).into()])?;
     let metadata=values.len().checked_mul(std::mem::size_of::<ExactRef>()+64+128+71).ok_or_else(||shape::fail("app supporting ref metadata product"))?;let _evidence=arena.reserve_external_payload(metadata,m)?;
     let mut evidence_refs=Vec::new();
     evidence_refs.try_reserve_exact(values.len()).map_err(|_|shape::fail("app normalized supporting refs"))?;
     for value in values{m.visit("app.causal_normalized_supporting_ref")?;evidence_refs.push(shape::reference(m,value)?);}
     request.replace_reference_array_field(m,"dependent_reversals",index,"authority_evidence_refs",&evidence_refs)?;
     batch.used_reversals.push(index);batch.finalized.push(closed);
   }else{
     if let Some(retired)=prior_retired(m,r,c,&original,&dependent_id,root_ref,current_capacity)?{batch.closed_dependencies.push(retired);}
     else{batch.pending.push(pending(m,r,c,program,&original,&dependent_id,root_ref,root_operation,state)?);}
   }
 }
 let total=w::usize_add(m,batch.finalized.len(),batch.pending.len())?;if w::compare(m,total,31usize)?.is_gt(){return Err(shape::fail("combined app causal count"));}
 Ok(batch)
}

impl CausalBatch {
 pub(in crate::policy::workflow::transition_constructor) fn excludes(&self,m:&mut CoreMeter,reference:&ExactRef)->Result<bool>{
   for closed in &self.finalized{m.visit("app.closed_finalized_dependency")?;if shape::same_ref(m,&closed.dependency_ref,reference)?{return Ok(true)}}
   for closed in &self.pending{m.visit("app.closed_pending_dependency")?;if shape::same_ref(m,&closed.dependency_ref,reference)?{return Ok(true)}}
   for closed in &self.closed_dependencies{m.visit("app.previously_closed_dependency")?;if shape::same_ref(m,closed,reference)?{return Ok(true)}}
   Ok(false)
 }
}

/// A prior permanent retirement is an authenticated earlier result to replay,
/// not fresh dependent authority. Requiring today's dependent grant here would
/// resurrect an already closed obligation and change accepted root rights.
fn prior_retired(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,original:&OriginalApp,dependent_id:&str,current_root:&ExactRef,capacity:&super::super::capacity::current::CurrentCapacity)->Result<Option<ExactRef>>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning prior app retirement"))?;
 let(dependency_ref,_)=dependency(m,r,c,original,dependent_id)?;let dependency=owning_reference(m,&arena,&dependency_ref)?;
 let work=owning_reference(m,&arena,&original.work_ref)?;let binding=owning_reference(m,&arena,&original.binding_ref)?;let terms=owning_reference(m,&arena,&original.terms.slot_terms_ref)?;let slot=arena.string_value(&original.component_slot,m)?;
 let original_root=r.get(m,&c.key,&original.original_decision_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
 let current=r.get(m,&c.key,current_root,"workflow-decision","ledger-workflow-decision-revision/1")?;
 let mut found=false;
 for reference in &capacity.retirements{
   m.visit("app.prior_retirement_selection")?;
   let retired=r.get(m,&c.key,reference,"workflow-capacity-retirement","ledger-workflow-capacity-retirement/1")?;
   if !shape::same(m,&retired["dependency_ref"],dependency.value())?{continue;}
   if found{return Err(shape::fail("duplicate permanent app retirement"));}found=true;
   shape::closed(m,&retired,&["schema","dependency_ref","closure_state_refs","reason","authority_ref","operation_key"],&[])?;
   literal(m,&retired,"reason","no_future_or_active_dependency")?;
   let authority_ref=shape::reference(m,&retired["authority_ref"])?;
   let authority=r.get(m,&c.key,&authority_ref,"workflow-economic-authority",if c.authorities.first().is_some_and(|a|a.authority_observation_ref.is_none()){"ledger-workflow-app-pending-causal-authority/2"}else{"ledger-workflow-app-pending-causal-authority/1"})?;
   for(key,expected)in[("dependency_ref",dependency.value()),("original_work_ref",work.value()),("original_binding_ref",binding.value()),("original_app_terms_ref",terms.value()),("component_slot",slot.value())]{shape::equal(m,&authority[key],expected)?;}
   literal(m,&authority,"absence_kind","no_initial_app_cost_claim")?;literal(m,&authority,"operation","cost.correct")?;
   let key=arena.array_from_values(&[dependency.value(),&retired["operation_key"]],m)?;
   let id=body::semantic_id(m,"wretire_","workflow-capacity-retirement",key.value())?;if !shape::same_text(m,&reference.id,&id)?{return Err(shape::fail("original app retirement identity"));}
   let states=shape::array(m,&retired,"closure_state_refs")?;
   if w::compare(m,states.len(),1usize)?.is_lt()||w::compare(m,states.len(),128usize)?.is_gt(){return Err(shape::fail("prior app closure states bound"));}
   let mut closure_found=false;
   for state in states{
     m.visit("app.prior_root_closure_state")?;let state_ref=shape::reference(m,state)?;
     if !shape::same_text(m,&state_ref.kind,"workflow-decision")?{r.provenance(m,&c.key,&state_ref)?;continue;}
     let closed=r.get_schemas(m,&c.key,&state_ref,"workflow-decision",&["ledger-workflow-decision-revision/1","ledger-workflow-decision-revision/3"])?;
     if !shape::same(m,&closed["decision_id"],&original_root["decision_id"])?{continue;}
     if closure_found{return Err(shape::fail("ambiguous original app root closure"));}closure_found=true;
     shape::equal(m,&closed["subject_ref"],work.value())?;shape::equal(m,&closed["original_terms_ref"],binding.value())?;
     shape::equal(m,&closed["operation_key"],&retired["operation_key"])?;shape::equal(m,&closed["prior_ref"],&authority["causal_root_ref"])?;
     let operation=shape::text(m,&authority,"causal_root_operation")?;
     let transition=if shape::same_text(m,operation,"quantity.correct")?{"quantity_correct"}else if shape::same_text(m,operation,"work.reverse")?{"work_reverse"}else{return Err(shape::fail("prior app closure root operation"))};
     literal(m,&closed,"transition",transition)?;
     for edge in shape::array(m,&closed,"dependency_refs")?{let edge=shape::reference(m,edge)?;if shape::same_ref(m,&edge,&dependency_ref)?{return Err(shape::fail("retired app dependency survives root closure"));}}
     let prior_revision=shape::text(m,&closed,"revision")?;let prior_revision=arithmetic::integer(m,prior_revision,false)?;
     let current_revision=shape::text(m,&current,"revision")?;let current_revision=arithmetic::integer(m,current_revision,false)?;
     let order=meter::cmp(m,&current_revision,&prior_revision)?;if order.is_lt()||(order.is_eq()&&!shape::same_ref(m,current_root,&state_ref)?){return Err(shape::fail("current root precedes permanent app closure"));}
     let parent_ref=shape::reference(m,&closed["authority_ref"])?;let parent=r.get(m,&c.key,&parent_ref,"workflow-economic-authority",if c.authorities.first().is_some_and(|a|a.authority_observation_ref.is_none()){"ledger-workflow-economic-authority/8"}else{"ledger-workflow-economic-authority/7"})?;
     let authority_value=owning_reference(m,&arena,&authority_ref)?;
     let mut check_found=false;
     for check in shape::array(m,&parent,"app_pending_cost_closure_checks")?{m.visit("app.prior_parent_pending_join")?;if shape::same(m,&check["dependency_ref"],dependency.value())?{if check_found{return Err(shape::fail("duplicate prior pending parent check"));}check_found=true;shape::equal(m,&check["dependent_authority_ref"],authority_value.value())?;shape::equal(m,&check["original_work_ref"],work.value())?;shape::equal(m,&check["original_binding_ref"],binding.value())?;shape::equal(m,&check["original_terms_ref"],terms.value())?;shape::equal(m,&check["component_slot"],slot.value())?;}}
     if !check_found{return Err(shape::fail("prior root authority omits app retirement"));}
   }
   if !closure_found{return Err(shape::fail("prior retirement lacks actual excluding root result"));}
 }
 if found{for reserved in &capacity.reserved{m.visit("app.retired_reservation_disjoint")?;if shape::same_ref(m,reserved,&dependency_ref)?{return Err(shape::fail("app dependency both held and retired"));}}Ok(Some(dependency_ref))}else{Ok(None)}
}
impl FinalizedClosure {
 pub(in crate::policy::workflow::transition_constructor) fn head_proposal(&self,m:&mut CoreMeter)->Result<AppHeadProposal>{
   let claim_ref=shape::reference(m,&self.check.value()["original_cost_claim_ref"])?;
   let expected=shape::reference(m,&self.check.value()["current_cost_revision_ref"])?;
   let body=self.revision.decoded(m,4194304)?;
   let revision=shape::text(m,&body,"revision")?;let revision=copy(m,revision)?;
   let proposed=ExactRef{kind:copy(m,&self.revision.reference.kind)?,id:copy(m,&self.revision.reference.id)?,digest:copy(m,&self.revision.reference.digest)?};
   Ok(AppHeadProposal{claim_ref,expected:Some(expected),proposed,revision})
 }
}
