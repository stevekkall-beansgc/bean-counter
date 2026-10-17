//! Source-derived NEW result dimensions of the existing future executors.
//! These are not manifest sizes or a whole resource proof: old input closure,
//! operation, exact capacities, body bounds and all phase work remain separate.
use super::super::{CoreMeter,shape};
use super::super::super::selected_program::SelectedProgram;
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork}};
pub(in crate::policy::workflow::transition_constructor) struct FutureRecords {
    pub root_correction:usize,
    pub family_submit:usize,
    pub family_correction:usize,
    pub family_close:usize,
    pub app_finalize:usize,
    pub app_correction:usize,
    pub app_observation_retain:usize,
    pub app_final_evidence_retain:usize,
    pub maximum_attested_family_fields:usize,
    pub maximum_source_frames:usize,
}
fn add(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{w::usize_add(m,a,b)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;a.checked_mul(b).ok_or_else(||shape::fail("future result dimension multiplication"))}
fn proof_fields(m:&mut CoreMeter,p:&SelectedProgram,family:&serde_json::Value)->Result<usize>{
    let trigger=shape::text(m,family,"trigger_activity")?;
    let activities=shape::array(m,p.compiled_plan().canonical_value(),"activity_table")?;
    let selected=shape::named(m,activities,"activity",trigger)?;
    let mut result=0usize;
    for slot in shape::array(m,p.compiled_plan().canonical_value(),"fact_slots")?{
        m.visit("capacity.future_family_attested_field")?;
        if !shape::same(m,&slot["schema_ref"],&selected["fact_schema_ref"])?{continue;}
        let kind=shape::text(m,&slot["field"]["provenance"],"kind")?;
        if shape::same_text(m,kind,"attested")?{result=add(m,result,1)?;}
    }Ok(result)
}
pub(in crate::policy::workflow::transition_constructor) fn derive(m:&mut CoreMeter,p:&SelectedProgram,payload:&serde_json::Value,root_actions:usize,family_count:usize,app_count:usize)->Result<FutureRecords>{
    let mut maximum_proofs=0usize;
    for instruction in shape::array(m,p.compiled_plan().canonical_value(),"typed_instructions")?{
        m.visit("capacity.future_result_instruction")?;
        let op=shape::text(m,instruction,"opcode")?;
        if !shape::same_text(m,op,"evaluate_family")?{continue;}
        let family=&instruction["family"];
        if !shape::same(m,&family["selector"]["target_activity"],&payload["activity"])?{continue;}
        let proofs=proof_fields(m,p,family)?;
        if w::compare(m,proofs,maximum_proofs)?.is_gt(){maximum_proofs=proofs;}
    }
    // Root producer: four common records (authority/evaluation/decision/chain),
    // actual root inverse+replacement bound, <=5 records per claimed family,
    // <=5 per finalized app. Pending app is two and already terminal is zero,
    // so five is a conservative maximum of mutually exclusive app branches.
    // One spare fact result is conservative; it does not implement the
    // separately unfinished attested quantity-correction executor.
    let dependents=add(m,family_count,app_count)?;
    let source_frames=add(m,dependents,1)?;
    let new_source_records=mul(m,source_frames,2)?;
    let root=add(m,4,root_actions)?;let dependent_records=mul(m,dependents,5)?;
    let root=add(m,root,dependent_records)?;let root=add(m,root,1)?;
    let root=add(m,root,new_source_records)?;
    // One COMMON financial observation/verifier pair for the root, not one
    // per dependent. Capacity revisions are added by the actual ancestor pass.
    let root=add(m,root,2)?;
    // Family submit: claim/link/authority/evaluation/decision/family/chain,
    // one optional nonzero posting, and <=one new fact per attested field.
    // Correction has authority/inverse/replacement/evaluation/decision/family/
    // chain; none/zero branches introduce fewer, never erase the decision.
    let submit=add(m,8,maximum_proofs)?;let submit=add(m,submit,4)?;
    let correction=add(m,7,maximum_proofs)?;let correction=add(m,correction,4)?;
    // Family close emits authority+family; the explicit no-success proof pair
    // is retained input, not invented output. Two source and two financial
    // witness records may be new. All distinctness is resolved at assembly.
    let close=6usize;
    // App finalize6+z and correct5+z_old+z_new are each<=7 monetary records;
    // add the source and common financial pairs. Retention's two source
    // records+authority+assertion+verification are<=5, with no capacity change
    // merely because evidence has been retained but not yet associated.
    Ok(FutureRecords{root_correction:root,family_submit:submit,family_correction:correction,family_close:close,app_finalize:11,app_correction:11,app_observation_retain:5,app_final_evidence_retain:5,maximum_attested_family_fields:maximum_proofs,maximum_source_frames:source_frames})
}
