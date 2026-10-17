//! App changing-body byte algebra from the SAME closed emitters. This is a
//! canonical BODY bound, not an arena, semantic-work, closure or ProfileB proof.
//! All counts are original-program dimensions; no observed fixture size or
//! registered record maximum is substituted for a constructor contribution.
use super::super::{CoreMeter,shape};
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork,RepresentationKind}};
use serde_json::Value;
// Frozen scalar grammar bounds INCLUDING JSON quotes. Id excludes C0/DEL;
// at most128 codepoints*4UTF8 bytes plus quotes. SourceId permits controls,
// hence256*6 escaped bytes. Name is ASCII and at most128 bytes.
const ID:usize=514;const SOURCE:usize=1538;const NAME:usize=130;
const REF:usize=102+64+128;const TIME:usize=29;const VERSION:usize=21;
const AMOUNT:usize=71;const RATIONAL_NUMERATOR:usize=158;const RATIONAL_DENOMINATOR:usize=157;
fn add(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{w::usize_add(m,a,b)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;a.checked_mul(b).ok_or_else(||shape::fail("app changing body product"))}
fn maximum(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{Ok(if w::compare(m,a,b)?.is_gt(){a}else{b})}
fn array(m:&mut CoreMeter,n:usize,body:usize)->Result<usize>{let items=mul(m,n,body)?;let commas=if w::compare(m,n,0usize)?.is_gt(){w::usize_sub(m,n,1)?}else{0};let n=add(m,items,commas)?;add(m,2,n)}
fn object(m:&mut CoreMeter,fields:&[(&str,usize)])->Result<usize>{
    let commas=if w::compare(m,fields.len(),0usize)?.is_gt(){w::usize_sub(m,fields.len(),1)?}else{0};let mut total=add(m,2,commas)?;
    for(key,body)in fields{m.visit("capacity.app_body_closed_field")?;let key=add(m,key.len(),3)?;let field=add(m,key,*body)?;total=add(m,total,field)?;}Ok(total)
}
const fn ascii(s:&str)->usize{s.len()+2}
fn exact(m:&mut CoreMeter,v:&Value)->Result<usize>{let arena=m.representation_arena().ok_or_else(||shape::fail("owning app byte bound"))?;let encoded=m.canonical_owned_representation(&arena,v,RepresentationKind::Record)?;Ok(encoded.bytes().len())}
pub(in crate::policy::workflow::transition_constructor) struct AppBodyBounds {
    pub action:usize,pub evaluation:usize,pub claim:usize,pub consumption:usize,pub revision:usize,pub decision:usize,
    pub standalone_authority:usize,pub finalized_authority:usize,pub pending_authority:usize,
    pub finalized_parent_check:usize,pub pending_parent_check:usize,pub retirement:usize,
    pub observation:usize,pub final_evidence:usize,pub evidence_verification:usize,
    pub finalize_new_bodies:usize,pub correct_new_bodies:usize,pub finalized_causal_new_bodies:usize,pub pending_causal_new_bodies:usize,
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
    let empty=2;let one_ref=array(m,1,REF)?;let two_refs=array(m,2,REF)?;let three_refs=array(m,3,REF)?;
    let grants=shape::array(m,&terms.slot_terms,"source_grant_refs")?;
    let original_grants=array(m,grants.len(),REF)?;
    let call=object(m,&[("source_namespace",SOURCE),("call_id",ID)])?;
    let subject=object(m,&[("logical_store_id",store),("scope",scope),("customer",customer),("agreement_id",agreement),("original_work_ref",REF),("original_binding_ref",REF),("component_slot",ID),("component_key",NAME),("app_call",call)])?;
    let provider=object(m,&[("provider",ID),("account",ID),("request_id",ID)])?;
    let material=object(m,&[("media_type",const{ascii("application/octet-stream")}),("content_base64",5464+2),("content_sha256",64+2)])?;
    let observation=object(m,&[("schema",const{ascii("ledger-workflow-app-observation/1")}),("attestation_id",ID),("source",SOURCE),("evidence_kind",const{ascii("app_usage")}),("subject",subject),("usage_field",NAME),("quantity",64+2),("unit",NAME),("observed_at",TIME),("provider_key",provider),("material",material)])?;
    let comparison=object(m,&[("kind",const{ascii("comparable")}),("quantity",64+2),("unit",NAME)])?;
    let unavailable=object(m,&[("kind",const{ascii("unavailable")}),("reason",const{ascii("not_comparable")})])?;
    let comparison=maximum(m,comparison,unavailable)?;
    let final_evidence=object(m,&[("schema",const{ascii("ledger-workflow-app-final-cost/1")}),("attestation_id",ID),("source",SOURCE),("evidence_kind",const{ascii("app_final_cost")}),("subject",subject),("observation_ref",REF),("cost_revision",VERSION),("prior_final_evidence_ref",REF),("finality",const{ascii("final_attributable_amount")}),("amount",AMOUNT),("quantity_comparison",comparison),("issued_at",TIME),("material",material)])?;
    let evidence_verification=object(m,&[("schema",const{ascii("ledger-workflow-evidence-verification/1")}),("attestation_ref",REF),("attestation_raw_sha256",66),("attestation_digest",73),("verified_source",SOURCE),("verification_primitive",const{ascii("authenticated-nominated-source-retention/1")}),("source_grant_ref",REF),("source_grant_revision",VERSION),("authenticated_principal_ref",REF),("trusted_received_at",TIME),("subject_binding_refs",three_refs),("verification_evidence_refs",empty)])?;
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
    let finalized_authority=add(m,standalone_authority,extras)?;
    let pending_extras=object(m,&[("schema",const{ascii("ledger-workflow-app-pending-causal-authority/1")}),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("original_work_ref",REF),("dependency_ref",REF),("component_slot",ID),("original_app_terms_ref",REF),("financial_observation_ref",REF),("absence_kind",const{ascii("no_initial_app_cost_claim")}),("causal_reversal",clause)])?;
    let pending_authority=add(m,standalone_authority,pending_extras)?;
    let finalized_parent_check=object(m,&[("original_work_ref",REF),("original_cost_claim_ref",REF),("current_cost_revision_ref",REF),("current_decision_ref",REF),("dependency_ref",REF),("original_binding_ref",REF),("original_terms_ref",REF),("original_source_grant_refs",original_grants),("current_source_grant_ref",REF),("correction_source_slot",NAME),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("component_slot",ID),("dependent_authority_ref",REF),("deadline_basis",const{ascii("original_standalone_window")}),("initial_cost_decision_ref",REF)])?;
    let pending_parent_check=object(m,&[("original_work_ref",REF),("dependency_ref",REF),("original_binding_ref",REF),("original_terms_ref",REF),("original_source_grant_refs",original_grants),("current_source_grant_ref",REF),("correction_source_slot",NAME),("causal_root_ref",REF),("causal_root_operation",const{ascii("quantity.correct")}),("component_slot",ID),("dependent_authority_ref",REF),("financial_observation_ref",REF),("absence_kind",const{ascii("no_initial_app_cost_claim")})])?;
    let retirement=object(m,&[("schema",const{ascii("ledger-workflow-capacity-retirement/1")}),("dependency_ref",REF),("closure_state_refs",one_ref),("reason",const{ascii("no_future_or_active_dependency")}),("authority_ref",REF),("operation_key",key)])?;
    let mut finalize_new_bodies=0;for n in [claim,standalone_authority,action,evaluation,decision,consumption,revision]{finalize_new_bodies=add(m,finalize_new_bodies,n)?;}
    let mut correct_new_bodies=0;for n in [standalone_authority,action,action,evaluation,decision,consumption,revision]{correct_new_bodies=add(m,correct_new_bodies,n)?;}
    let mut finalized_causal_new_bodies=0;for n in [finalized_authority,action,evaluation,decision,revision]{finalized_causal_new_bodies=add(m,finalized_causal_new_bodies,n)?;}
    let pending_causal_new_bodies=add(m,pending_authority,retirement)?;
    Ok(AppBodyBounds{action,evaluation,claim,consumption,revision,decision,standalone_authority,finalized_authority,pending_authority,finalized_parent_check,pending_parent_check,retirement,observation,final_evidence,evidence_verification,finalize_new_bodies,correct_new_bodies,finalized_causal_new_bodies,pending_causal_new_bodies})
}
