//! Shared original family selection for submit, retention and close. Inputs are
//! DATA selected by the host; no origin/current-index certificate is created.
use super::{CoreMeter, authority, body, data::*, meter, outcome, record_body::RecordBody, records::Records, representation::{InvocationToken,ParsedDraftTimes}, shape};
use super::super::selected_program::SelectedProgram;
use crate::{Result, Error,domain::Timestamp};
use serde_json::{Value,json};

pub(super) struct FamilyContext<'p> {
    pub target_ref:ExactRef,
    pub target:RecordBody,
    pub work_ref:ExactRef,
    pub work:RecordBody,
    pub claim_ref:ExactRef,
    pub claim:RecordBody,
    pub decision_ref:ExactRef,
    pub decision:RecordBody,
    pub family_head_ref:ExactRef,
    pub family_head:RecordBody,
    pub family:&'p Value,
}
fn eq(m:&mut CoreMeter,a:&Value,b:&Value)->Result<()> {
    if shape::same(m,a,b)? {Ok(())}else{Err(shape::fail("original family association"))}
}
fn load(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,v:&Value,kind:&str,schema:&str)->Result<(ExactRef,RecordBody)>{
    let reference=shape::reference(m,v)?;
    let body=r.get(m,&c.key,&reference,kind,schema)?;
    Ok((reference,body))
}
pub(super) fn select<'p>(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&'p SelectedProgram,target_input:&Value,slot:&str,state:&EconomicStateData)->Result<FamilyContext<'p>>{
    let (target_ref,target)=load(m,r,c,target_input,"workflow-target","ledger-workflow-component-target/1")?;
    shape::closed(m,&target,&["schema","scope","agreement_id","work_claim_ref","work_ref","root_component_slot","original_decision_ref","original_action_refs","original_net","policy_ref","product_ref","binding_ref","compiled_plan_ref","families","original_work_accepted_at"],&[])?;
    eq(m,&target["scope"],&json!(c.key.scope))?;
    eq(m,&target["compiled_plan_ref"],&json!(program.compiled_plan().reference()))?;
    eq(m,&target["policy_ref"],&json!(program.root_policy_ref()))?;
    eq(m,&target["product_ref"],&json!(program.product_ref()))?;
    let (work_ref,work)=load(m,r,c,&target["work_ref"],"workflow-work","ledger-workflow-work-record/1")?;
    let (claim_ref,claim)=load(m,r,c,&target["work_claim_ref"],"workflow-work-claim","ledger-workflow-work-claim/1")?;
    eq(m,&claim["work_ref"],&json!(work_ref))?;
    super::inventory::entry::historical_work_claim(m,c,&claim_ref,&claim,&work)?;
    outcome::original_claim_association(m,&claim,&work)?;
    eq(m,&work["payload"]["status"],&json!("succeeded"))?;
    eq(m,&work["payload"]["binding_ref"],&target["binding_ref"])?;
    eq(m,&work["payload"]["product_ref"],&target["product_ref"])?;
    eq(m,&work["payload"]["policy_ref"],&target["policy_ref"])?;
    eq(m,&work["accepted_at"],&target["original_work_accepted_at"])?;
    let root=shape::text(m,&target,"root_component_slot")?;
    let expected=body::semantic_id(m,"wtarget_","workflow-target",&json!([c.key.scope,target["agreement_id"],claim_ref.id,root]))?;
    if !shape::same_text(m,&expected,&target_ref.id)? {return Err(shape::fail("original target identity"));}
    let (decision_ref,decision)=load(m,r,c,&target["original_decision_ref"],"workflow-decision","ledger-workflow-decision-revision/1")?;
    let id=shape::text(m,&decision,"decision_id")?;
    let mut current_root=None;
    for head in &state.financial.decision_heads {
        m.visit("family_context.root_head")?;
        if shape::same_text(m,&head.decision_id,id)? {
            if current_root.is_some(){return Err(shape::fail("duplicate original root head"));}
            current_root=Some(head);
        }
    }
    let root_head=current_root.ok_or_else(||shape::fail("missing original root head"))?;
    if !shape::same_ref(m,&root_head.reference,&decision_ref)? || !shape::same_text(m,&root_head.revision,"1")? {
        return Err(Error::new("TARGET_CLOSED","original family root was corrected"));
    }
    let members=shape::array(m,&target,"families")?;
    let original=shape::named(m,members,"family_slot",slot)?;
    shape::closed(m,original,&["family_key","family_slot","prospective_decision_id","window_terms","maximum_premium","maximum_discount"],&[])?;
    let key=shape::text(m,original,"family_key")?;
    let mut selected=None;
    for instruction in shape::array(m,program.compiled_plan().canonical_value(),"typed_instructions")? {
        m.visit("family_context.original_instruction")?;
        let opcode=shape::text(m,instruction,"opcode")?;
        if shape::same_text(m,opcode,"evaluate_family")? {
            let candidate=shape::text(m,&instruction["family"],"family_key")?;
            if !shape::same_text(m,candidate,key)? {continue;}
            if selected.is_some(){return Err(shape::fail("duplicate original family instruction"));}
            selected=Some(&instruction["family"]);
        }
    }
    let family=selected.ok_or_else(||shape::fail("missing original family instruction"))?;
    eq(m,&family["selector"]["target_activity"],&work["payload"]["activity"])?;
    let expected=super::targets::prospective_decision(m,&target_ref.id,slot)?;
    // Existing producer uses wdecision_ for this permanent prospective identity.
    eq(m,&original["prospective_decision_id"],&json!(expected))?;
    let mut current=None;
    for head in &state.financial.family_heads {
        m.visit("family_context.family_head")?;
        if shape::same_ref(m,&head.target_ref,&target_ref)? && shape::same_text(m,&head.family_slot,slot)? {
            if current.is_some(){return Err(shape::fail("duplicate selected family head"));}
            current=Some(head);
        }
    }
    let head=current.ok_or_else(||shape::fail("missing selected family head"))?;
    let family_head=r.get(m,&c.key,&head.reference,"workflow-family-state","ledger-workflow-family-state/1")?;
    shape::closed(m,&family_head,&["schema","target_ref","family_slot","revision","state","closure_evidence_refs","operation_key","accepted_at"],&["window","anchor_work_ref","claim_ref","decision_ref","prior_ref"])?;
    eq(m,&family_head["target_ref"],&json!(target_ref))?;
    eq(m,&family_head["family_slot"],&json!(slot))?;
    eq(m,&family_head["revision"],&json!(head.revision))?;
    let id=body::semantic_id(m,"wfamily_","workflow-family-state",&json!([target_ref.id,slot,head.revision]))?;
    if !shape::same_text(m,&id,&head.reference.id)? {return Err(shape::fail("selected family identity"));}
    let at=Timestamp::parse_with_work(&c.accepted_at,m)?.micros();
    let prior=Timestamp::parse_with_work(shape::text(m,&family_head,"accepted_at")?,m)?.micros();
    if meter::cmp(m,&at.into(),&prior.into())?.is_lt(){return Err(Error::new("CLOCK_REGRESSION","family trusted time"));}
    Ok(FamilyContext{target_ref,target,work_ref,work,claim_ref,claim,decision_ref,decision,family_head_ref:head.reference.clone(),family_head,family})
}
impl FamilyContext<'_> {
    /// The operation's source-slot choice comes from an ORIGINAL declaration,
    /// never a submitted selector or current-policy replacement.
    pub(super) fn authority(&self,m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,source_slot:&str,mode:authority::EconomicMode)->Result<authority::ResolvedOperands>{
        let mut product=None;
        for document in program.source_documents(){
            m.visit("family_context.product")?;
            if shape::same(m,&json!(document.reference()),&self.target["product_ref"])? {product=Some(document.canonical_value());}
        }
        let product=product.ok_or_else(||shape::fail("original family product"))?;
        let trigger=shape::text(m,self.family,"trigger_activity")?;
        let activities=shape::array(m,product,"activities")?;
        let activity=shape::named(m,activities,"name",trigger)?;
        let arena=m.representation_arena().ok_or_else(||shape::fail("owning family operation profile"))?;
        let mut activity=arena.clone_value(activity,m)?;
        let slot=arena.string_value(source_slot,m)?;
        activity.replace_existing_owned("source_slot",slot,m)?;
        let empty=Value::Array(Vec::new());
        let p=arena.object_from_fields(&[
            ("customer",&self.work["payload"]["customer"]),
            ("activity",&self.family["trigger_activity"]),
            ("product_ref",&self.target["product_ref"]),
            ("policy_ref",&self.target["policy_ref"]),
            ("fact_schema_ref",&activity.value()["fact_schema"]),
            ("binding_ref",&self.target["binding_ref"]),
            ("evidence_refs",&empty),
        ],m)?;
        let owner=InvocationToken::new();
        let times=ParsedDraftTimes::parse(m,&owner,&c.received_at,&c.accepted_at)?;
        let namespace=shape::text(m,&self.work,"source_namespace")?;
        let resolved=authority::resolve_economic_original(m,r,c,p.value(),activity.value(),product,&owner,&times,program,mode,&[],namespace)?;
        let originals=shape::array(m,&self.target,"families")?;
        let slot=shape::text(m,&self.family_head,"family_slot")?;
        let original=shape::named(m,originals,"family_slot",slot)?;
        outcome::original_target_association(m,&resolved.terms,&self.target,original,self.family)?;
        outcome::verify_original_actions(m,r,c,program,&resolved.terms,&self.work_ref,&self.work,&self.claim_ref,&self.decision_ref,&self.decision)?;
        Ok(resolved)
    }
}
