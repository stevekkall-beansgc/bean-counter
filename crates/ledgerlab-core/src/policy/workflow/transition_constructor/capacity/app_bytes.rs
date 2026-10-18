//! App changing-body byte algebra from the SAME closed emitters. This is a
//! canonical BODY and decoded-type-guard contribution, not an arena, whole
//! semantic-work, closure or ProfileB proof.
//! All counts are original-program dimensions; no observed fixture size or
//! registered record maximum is substituted for a constructor contribution.
use super::super::{CoreMeter,shape};
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork,RepresentationKind}};
use serde_json::Value;
// Frozen scalar grammar bounds INCLUDING JSON quotes. Id excludes C0/DEL;
// at most128 codepoints*4UTF8 bytes plus quotes. SourceId permits controls,
// hence256*6 escaped bytes. Name is ASCII and at most128 bytes.
#[derive(Clone,Copy)]
struct Body { bytes:usize,guard:usize,depth:usize }
impl Body {
    const fn scalar(bytes:usize)->Self{Self{bytes,guard:5,depth:0}}
    const fn reference(bytes:usize)->Self{Self{bytes,guard:35,depth:1}}
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::codec;
    use crate::{canonical::{self,audit_arena::AuditArena},validation_work::OriginalAuditSession};
    use serde_json::json;
    #[test]
    fn prospective_closed_body_guard_counts_structure_independently_of_string_bytes(){
        let reference=json!({"kind":"workflow-work","id":"work","digest":"digest"});
        for count in [0,1,32]{
            let facts=json!({"units":"100","label":"x".repeat(10000),"flag":true,"scale":18});
            let value=json!({"facts":facts,"refs":vec![reference.clone();count]});
            let arena=AuditArena::new().unwrap();
            let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let m=session.meter_mut();
            let fact_shape=exact(m,&value["facts"]).unwrap();let refs=array(m,count,REF).unwrap();
            let bound=object(m,&[("facts",fact_shape),("refs",refs)]).unwrap();
            let mut actual=CoreMeter::new(65536).unwrap();codec::validate_selected_types(&mut actual,&value).unwrap();
            assert!(actual.usage().semantic_work<=bound.guard as u64);
            assert!(canonical::CanonicalBytes::from_value(&value).unwrap().as_slice().len()<=bound.bytes);
            assert_eq!(bound.depth,if count==0{2}else{3});
        }
        let mut m=CoreMeter::new(65536).unwrap();
        let too_deep=Body{bytes:2,guard:5,depth:32};
        assert!(object(&mut m,&[("extra",too_deep)]).is_err());
    }
}
const ID:Body=Body::scalar(514);const SOURCE:Body=Body::scalar(1538);const NAME:Body=Body::scalar(130);
const REF:Body=Body::reference(102+64+128);const TIME:Body=Body::scalar(29);const VERSION:Body=Body::scalar(21);
// Amount is a closed currency/scale/atoms object, rather than one scalar.
const AMOUNT:Body=Body::reference(71);
const RATIONAL_NUMERATOR:Body=Body::scalar(158);const RATIONAL_DENOMINATOR:Body=Body::scalar(157);
fn add(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{w::usize_add(m,a,b)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;a.checked_mul(b).ok_or_else(||shape::fail("app changing body product"))}
fn largest(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{Ok(if w::compare(m,a,b)?.is_gt(){a}else{b})}
fn maximum(m:&mut CoreMeter,a:Body,b:Body)->Result<Body>{Ok(Body{bytes:largest(m,a.bytes,b.bytes)?,guard:largest(m,a.guard,b.guard)?,depth:largest(m,a.depth,b.depth)?})}
fn sum(m:&mut CoreMeter,a:Body,b:Body)->Result<Body>{Ok(Body{bytes:add(m,a.bytes,b.bytes)?,guard:add(m,a.guard,b.guard)?,depth:largest(m,a.depth,b.depth)?})}
fn array(m:&mut CoreMeter,n:usize,body:Body)->Result<Body>{
    let items=mul(m,n,body.bytes)?;let commas=if w::compare(m,n,0usize)?.is_gt(){w::usize_sub(m,n,1)?}else{0};let bytes=add(m,items,commas)?;let bytes=add(m,2,bytes)?;
    let per=add(m,4,body.guard)?;let guard=mul(m,n,per)?;let guard=add(m,4,guard)?;
    let depth=if w::compare(m,n,0usize)?.is_gt(){add(m,body.depth,1)?}else{0};
    Ok(Body{bytes,guard,depth})
}
fn object(m:&mut CoreMeter,fields:&[(&str,Body)])->Result<Body>{
    let commas=if w::compare(m,fields.len(),0usize)?.is_gt(){w::usize_sub(m,fields.len(),1)?}else{0};let mut bytes=add(m,2,commas)?;let mut guard=4;let mut depth=0;
    for(key,body)in fields{
        m.visit("capacity.app_body_closed_field")?;let key=add(m,key.len(),3)?;let field=add(m,key,body.bytes)?;bytes=add(m,bytes,field)?;
        guard=add(m,guard,4)?;guard=add(m,guard,body.guard)?;let child=add(m,body.depth,1)?;depth=largest(m,depth,child)?;
    }
    if w::compare(m,depth,32usize)?.is_gt(){return Err(shape::fail("prospective app decoded depth"));}
    Ok(Body{bytes,guard,depth})
}
const fn ascii(s:&str)->Body{Body::scalar(s.len()+2)}
// codec::validate_selected_types pays4 per Value,4 per field/element edge,
// and1 more for a JSON integer.5 per scalar is consequently conservative.
// Immutable normalized facts/context may have a different actual shape than
// any literal fixture, so derive these dimensions from their actual bodies.
pub(super) fn immutable_guard(m:&mut CoreMeter,v:&Value,depth:usize)->Result<(usize,usize)>{
    m.visit("capacity.app_immutable_value_dimension")?;
    if w::compare(m,depth,32usize)?.is_gt(){return Err(shape::fail("original app decoded depth"));}
    let mut guard=5;let mut deepest=depth;
    match v {
        Value::Array(values)=>{guard=4;for value in values{m.visit("capacity.app_immutable_element")?;let next=add(m,depth,1)?;let(g,d)=immutable_guard(m,value,next)?;guard=add(m,guard,4)?;guard=add(m,guard,g)?;deepest=largest(m,deepest,d)?;}},
        Value::Object(values)=>{guard=4;for(_,value)in values{m.visit("capacity.app_immutable_field")?;let next=add(m,depth,1)?;let(g,d)=immutable_guard(m,value,next)?;guard=add(m,guard,4)?;guard=add(m,guard,g)?;deepest=largest(m,deepest,d)?;}},
        Value::Null=>return Err(shape::fail("null original app body")),_=>{}
    }
    Ok((guard,deepest))
}
fn exact(m:&mut CoreMeter,v:&Value)->Result<Body>{let arena=m.representation_arena().ok_or_else(||shape::fail("owning app byte bound"))?;let encoded=m.canonical_owned_representation(&arena,v,RepresentationKind::Record)?;let(guard,depth)=immutable_guard(m,v,0)?;Ok(Body{bytes:encoded.bytes().len(),guard,depth})}
pub(in crate::policy::workflow::transition_constructor) struct AppBodyBounds {
    pub action:usize,pub evaluation:usize,pub claim:usize,pub consumption:usize,pub revision:usize,pub decision:usize,
    pub standalone_authority:usize,pub finalized_authority:usize,pub pending_authority:usize,
    pub finalized_parent_check:usize,pub pending_parent_check:usize,pub retirement:usize,
    pub observation:usize,pub final_evidence:usize,pub evidence_verification:usize,
    pub finalize_new_bodies:usize,pub correct_new_bodies:usize,pub finalized_causal_new_bodies:usize,pub pending_causal_new_bodies:usize,
    pub decoded:AppDecodedBounds,
}
/// Named app-body selected-guard contributions. These are neither the
/// economic executor total nor a bound on arbitrary unvalidated JSON.
pub(in crate::policy::workflow::transition_constructor) struct AppDecodedBounds {
    pub finalize_new_bodies:usize,pub correct_new_bodies:usize,
    pub finalized_causal_new_bodies:usize,pub pending_causal_new_bodies:usize,
    pub observation:usize,pub final_evidence:usize,pub evidence_verification:usize,
    pub action:usize,pub evaluation:usize,pub claim:usize,pub consumption:usize,pub revision:usize,pub decision:usize,
    pub standalone_authority:usize,pub finalized_authority:usize,pub pending_authority:usize,
    pub finalized_parent_check:usize,pub pending_parent_check:usize,pub retirement:usize,
}
/// Actual immutable original payload/context remain fixed for standalone app
/// pricing and causal inversion. The caller supplies the SAME validated terms,
/// not an independently supplied maximum or saved capacity assertion.
pub(in crate::policy::workflow::transition_constructor) fn derive(m:&mut CoreMeter,logical_store:&Value,payload:&Value,terms:&super::super::authority::Terms)->Result<AppBodyBounds>{
    // Scope/store/customer/agreement are original immutable values. Use actual
    // JCS lengths so escaping and UTF8 cannot be confused with character count.
    let scope=exact(m,&terms.slot_terms["scope"])?;
    let customer=exact(m,&terms.binding["customer"])?;let agreement=exact(m,&terms.binding["agreement_id"])?;
    let store=exact(m,logical_store)?;
    let key=object(m,&[("logical_store_id",store),("scope",scope),("source",SOURCE),("operation_id",ID)])?;
    let empty=Body{bytes:2,guard:4,depth:0};let one_ref=array(m,1,REF)?;let two_refs=array(m,2,REF)?;let three_refs=array(m,3,REF)?;
    let grants=shape::array(m,&terms.slot_terms,"source_grant_refs")?;
    let original_grants=array(m,grants.len(),REF)?;
    let call=object(m,&[("source_namespace",SOURCE),("call_id",ID)])?;
    let subject=object(m,&[("logical_store_id",store),("scope",scope),("customer",customer),("agreement_id",agreement),("original_work_ref",REF),("original_binding_ref",REF),("component_slot",ID),("component_key",NAME),("app_call",call)])?;
    let provider=object(m,&[("provider",ID),("account",ID),("request_id",ID)])?;
    let material=object(m,&[("media_type",const{ascii("application/octet-stream")}),("content_base64",Body::scalar(5464+2)),("content_sha256",Body::scalar(64+2))])?;
    let observation=object(m,&[("schema",const{ascii("ledger-workflow-app-observation/1")}),("attestation_id",ID),("source",SOURCE),("evidence_kind",const{ascii("app_usage")}),("subject",subject),("usage_field",NAME),("quantity",Body::scalar(64+2)),("unit",NAME),("observed_at",TIME),("provider_key",provider),("material",material)])?;
    let comparison=object(m,&[("kind",const{ascii("comparable")}),("quantity",Body::scalar(64+2)),("unit",NAME)])?;
    let unavailable=object(m,&[("kind",const{ascii("unavailable")}),("reason",const{ascii("not_comparable")})])?;
    let comparison=maximum(m,comparison,unavailable)?;
    let final_evidence=object(m,&[("schema",const{ascii("ledger-workflow-app-final-cost/1")}),("attestation_id",ID),("source",SOURCE),("evidence_kind",const{ascii("app_final_cost")}),("subject",subject),("observation_ref",REF),("cost_revision",VERSION),("prior_final_evidence_ref",REF),("finality",const{ascii("final_attributable_amount")}),("amount",AMOUNT),("quantity_comparison",comparison),("issued_at",TIME),("material",material)])?;
    let evidence_verification=object(m,&[("schema",const{ascii("ledger-workflow-evidence-verification/1")}),("attestation_ref",REF),("attestation_raw_sha256",Body::scalar(66)),("attestation_digest",Body::scalar(73)),("verified_source",SOURCE),("verification_primitive",const{ascii("authenticated-nominated-source-retention/1")}),("source_grant_ref",REF),("source_grant_revision",VERSION),("authenticated_principal_ref",REF),("trusted_received_at",TIME),("subject_binding_refs",three_refs),("verification_evidence_refs",empty)])?;
    let claim=object(m,&[("schema",const{ascii("ledger-workflow-app-cost-claim/1")}),("subject",subject),("observation_ref",REF),("original_final_evidence_ref",REF),("original_terms_ref",REF),("dependency_ref",REF),("operation_key",key)])?;
    let consumption=object(m,&[("schema",const{ascii("ledger-workflow-app-cost-consumption/1")}),("cost_claim_ref",REF),("observation_ref",REF),("final_evidence_ref",REF),("cost_revision",VERSION),("eligible_amount",AMOUNT),("operation_key",key)])?;
    let revision=object(m,&[("schema",const{ascii("ledger-workflow-app-cost-revision/1")}),("cost_claim_ref",REF),("revision",VERSION),("prior_ref",REF),("current_decision_ref",REF),("current_evidence_ref",REF),("consumption_ref",REF),("status",const{ascii("reversed")}),("operation_key",key)])?;
    let decision=object(m,&[("schema",const{ascii("ledger-workflow-decision-revision/3")}),("decision_id",ID),("revision",VERSION),("prior_ref",REF),("subject_ref",REF),("kind",const{ascii("cost")}),("transition",const{ascii("dependent_reverse")}),("status",const{ascii("reversed")}),("original_terms_ref",REF),("authority_ref",REF),("evaluation_ref",REF),("actions_appended",two_refs),("active_action_refs",one_ref),("dependency_refs",one_ref),("operation_key",key),("accepted_at",TIME)])?;
    let action=object(m,&[("schema",const{ascii("ledger-workflow-action/1")}),("effect_id",ID),("decision_id",ID),("revision",VERSION),("posting_role",const{ascii("replacement")}),("book",const{ascii("retail")}),("component_slot",ID),("root_component_slot",ID),("role_ref",REF),("slot_terms_ref",REF),("funding_leg",NAME),("amount",AMOUNT),("inverse_of",REF),("basis_refs",empty),("operation_key",key)])?;
    let step=object(m,&[("instruction",VERSION),("component_slot",ID),("predicate",const{ascii("true")}),("basis_refs",empty),("rational_numerator",RATIONAL_NUMERATOR),("rational_denominator",RATIONAL_DENOMINATOR),("rounded",AMOUNT),("disposition",const{ascii("inverse")}),("action_ref",REF)])?;
    let steps=array(m,2,step)?;let facts=exact(m,&payload["facts"])?;let context=exact(m,&terms.binding["accepted_context"])?;
    let evaluation=object(m,&[("schema",const{ascii("ledger-workflow-evaluation-proof/1")}),("subject_ref",REF),("policy_ref",REF),("compiled_plan_ref",REF),("compiler_ref",REF),("binding_ref",REF),("fact_schema_ref",REF),("facts",facts),("context",context),("steps",steps)])?;
    let rights=array(m,2,const{ascii("submit")})?;
    let standalone_authority=object(m,&[("schema",const{ascii("ledger-workflow-economic-authority/7")}),("operation",const{ascii("evidence.retain")}),("actor",ID),("source",SOURCE),("scope",scope),("customer",customer),("canonical_economic_namespace",SOURCE),("installation_mode",const{ascii("isolated_synthetic")}),("enrollment_ref",REF),("current_state_ref",REF),("current_grant_ref",REF),("original_binding_ref",REF),("original_accepted_agreement_ref",REF),("original_grant_refs",original_grants),("source_slot",NAME),("rights",rights),("evidence_refs",empty),("verified_fact_refs",empty),("observed_at",TIME),("authority_observation_ref",REF),("causal_closure_checks",empty),("app_observation_refs",one_ref),("app_final_evidence_refs",two_refs),("app_cost_revision_refs",one_ref),("app_cost_closure_checks",empty),("app_pending_cost_closure_checks",empty)])?;
    let clause=object(m,&[("root_relation",const{ascii("original_work_delayed_cost_dependency")}),("mode",const{ascii("exact_inverse_and_permanent_close")}),("standalone_deadline_effect",const{ascii("does_not_expire_preaccepted_causal_reversal")})])?;
    // Bounds include the optional clause; their sum need not itself be a valid
    // branch instance. The exact base fields are overcounted when replaced.
    let evidence_refs=array(m,16,REF)?;
    let extras=object(m,&[("schema",const{ascii("ledger-workflow-app-causal-authority/1")}),("evidence_refs",evidence_refs),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("original_work_ref",REF),("dependency_ref",REF),("component_slot",ID),("original_app_terms_ref",REF),("standalone_correction_until",TIME),("deadline_basis",const{ascii("original_standalone_window")}),("initial_cost_decision_ref",REF),("initial_cost_accepted_at",TIME),("causal_reversal",clause)])?;
    let finalized_authority=sum(m,standalone_authority,extras)?;
    let pending_extras=object(m,&[("schema",const{ascii("ledger-workflow-app-pending-causal-authority/1")}),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("original_work_ref",REF),("dependency_ref",REF),("component_slot",ID),("original_app_terms_ref",REF),("financial_observation_ref",REF),("absence_kind",const{ascii("no_initial_app_cost_claim")}),("causal_reversal",clause)])?;
    let pending_authority=sum(m,standalone_authority,pending_extras)?;
    let finalized_parent_check=object(m,&[("original_work_ref",REF),("original_cost_claim_ref",REF),("current_cost_revision_ref",REF),("current_decision_ref",REF),("dependency_ref",REF),("original_binding_ref",REF),("original_terms_ref",REF),("original_source_grant_refs",original_grants),("current_source_grant_ref",REF),("correction_source_slot",NAME),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("component_slot",ID),("dependent_authority_ref",REF),("deadline_basis",const{ascii("original_standalone_window")}),("initial_cost_decision_ref",REF)])?;
    let pending_parent_check=object(m,&[("original_work_ref",REF),("dependency_ref",REF),("original_binding_ref",REF),("original_terms_ref",REF),("original_source_grant_refs",original_grants),("current_source_grant_ref",REF),("correction_source_slot",NAME),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("component_slot",ID),("dependent_authority_ref",REF),("financial_observation_ref",REF),("absence_kind",const{ascii("no_initial_app_cost_claim")})])?;
    let retirement=object(m,&[("schema",const{ascii("ledger-workflow-capacity-retirement/1")}),("dependency_ref",REF),("closure_state_refs",one_ref),("reason",const{ascii("no_future_or_active_dependency")}),("authority_ref",REF),("operation_key",key)])?;
    let mut finalize_new_bodies=Body{bytes:0,guard:0,depth:0};for n in [claim,standalone_authority,action,evaluation,decision,consumption,revision]{finalize_new_bodies=sum(m,finalize_new_bodies,n)?;}
    let mut correct_new_bodies=Body{bytes:0,guard:0,depth:0};for n in [standalone_authority,action,action,evaluation,decision,consumption,revision]{correct_new_bodies=sum(m,correct_new_bodies,n)?;}
    let mut finalized_causal_new_bodies=Body{bytes:0,guard:0,depth:0};for n in [finalized_authority,action,evaluation,decision,revision]{finalized_causal_new_bodies=sum(m,finalized_causal_new_bodies,n)?;}
    let pending_causal_new_bodies=sum(m,pending_authority,retirement)?;
    let decoded=AppDecodedBounds{action:action.guard,evaluation:evaluation.guard,claim:claim.guard,consumption:consumption.guard,revision:revision.guard,decision:decision.guard,standalone_authority:standalone_authority.guard,finalized_authority:finalized_authority.guard,pending_authority:pending_authority.guard,finalized_parent_check:finalized_parent_check.guard,pending_parent_check:pending_parent_check.guard,retirement:retirement.guard,observation:observation.guard,final_evidence:final_evidence.guard,evidence_verification:evidence_verification.guard,finalize_new_bodies:finalize_new_bodies.guard,correct_new_bodies:correct_new_bodies.guard,finalized_causal_new_bodies:finalized_causal_new_bodies.guard,pending_causal_new_bodies:pending_causal_new_bodies.guard};
    Ok(AppBodyBounds{action:action.bytes,evaluation:evaluation.bytes,claim:claim.bytes,consumption:consumption.bytes,revision:revision.bytes,decision:decision.bytes,standalone_authority:standalone_authority.bytes,finalized_authority:finalized_authority.bytes,pending_authority:pending_authority.bytes,finalized_parent_check:finalized_parent_check.bytes,pending_parent_check:pending_parent_check.bytes,retirement:retirement.bytes,observation:observation.bytes,final_evidence:final_evidence.bytes,evidence_verification:evidence_verification.bytes,finalize_new_bodies:finalize_new_bodies.bytes,correct_new_bodies:correct_new_bodies.bytes,finalized_causal_new_bodies:finalized_causal_new_bodies.bytes,pending_causal_new_bodies:pending_causal_new_bodies.bytes,decoded})
}
