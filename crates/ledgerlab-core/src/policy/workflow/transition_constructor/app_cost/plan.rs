//! Original app obligation/current exact head selection, shared by fresh cost
//! execution and saved reconstruction. Raw caller state remains DATA.
use super::{original::{self,OriginalApp},evidence::{self,AppObservation,AppFinalAmount}};
use super::super::{CoreMeter,arithmetic,body,data::*,meter,record_body::RecordBody,records::Records,shape,working_value::{WorkingValue,owning_reference}};
use super::super::super::selected_program::SelectedProgram;
use crate::{Error,Result,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,RepresentationKind,ValidationWork}};
use num_bigint::BigInt;
use serde_json::Value;
macro_rules! call {
 ($f:path,$m:expr,$a:expr)=>{{let a=$a;$f($m,a)}};
 ($f:path,$m:expr,$a:expr,$b:expr)=>{{let (a,b)=($a,$b);$f($m,a,b)}};
 ($f:path,$m:expr,$a:expr,$b:expr,$c:expr)=>{{let (a,b,c)=($a,$b,$c);$f($m,a,b,c)}};
}
fn eq(m:&mut CoreMeter,a:&Value,b:&Value)->Result<()>{shape::equal(m,a,b)}
fn load(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,v:&Value,kind:&str,schema:&str)->Result<(ExactRef,RecordBody)>{let reference=shape::reference(m,v)?;let body=r.get(m,&c.key,&reference,kind,schema)?;Ok((reference,body))}
fn literal(m:&mut CoreMeter,v:&Value,key:&str,want:&str)->Result<()> {let actual=shape::text(m,v,key)?;if !shape::same_text(m,actual,want)?{return Err(shape::fail("app current record literal"));}Ok(())}
pub(super) struct CurrentApp {
    pub claim_ref:ExactRef,
    pub claim:RecordBody,
    pub revision_ref:ExactRef,
    pub revision:RecordBody,
    pub decision_ref:ExactRef,
    pub decision:RecordBody,
    pub consumption_ref:ExactRef,
    pub consumption:RecordBody,
    pub initial_decision_ref:ExactRef,
    pub initial_decision:RecordBody,
}
pub(super) struct AppPlan<'p>{
    pub request:WorkingValue,
    pub original:OriginalApp<'p>,
    pub observation:AppObservation,
    pub final_cost:AppFinalAmount,
    pub old_cost:Option<AppFinalAmount>,
    pub current:Option<CurrentApp>,
    pub dependency_ref:ExactRef,
    pub dependency:RecordBody,
    pub decision_id:String,
    pub revision:BigInt,
    pub(super) _ownership:ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) fn normalize_request(m:&mut CoreMeter,p:&mut WorkingValue)->Result<bool>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning app request"))?;
    let schema=shape::text(m,p,"schema")?;
    let correct=shape::same_text(m,schema,"ledger-workflow-cost-correct/2")?;
    if correct{
        shape::closed(m,p,&["schema","original_cost_claim_ref","expected_revision","replacement_final_cost_proof","dependent_reversals","evidence_refs"],&[])?;
        shape::reference(m,&p["original_cost_claim_ref"])?;
        let revision=crate::policy::workflow::facts::counter_with_work(shape::text(m,p,"expected_revision")?,m)?;
        if !w::compare(m,revision,0u64)?.is_gt(){return Err(shape::fail("positive app correction revision"));}
        let revision=super::super::close::counter_value(m,&arena,revision)?;
        p.replace(m,"expected_revision",revision.value())?;
        shape::closed(m,&p["replacement_final_cost_proof"],&["attestation_ref","verification_ref"],&[])?;
        for field in ["attestation_ref","verification_ref"]{shape::reference(m,&p["replacement_final_cost_proof"][field])?;}
        let dependents=shape::array(m,p,"dependent_reversals")?;
        if w::compare(m,dependents.len(),31usize)?.is_gt(){return Err(shape::fail("app dependent reversal bound"));}
        // Full closure objects are normalized by the same existing correction
        // routine once that actual dependent executor is selected. A terminal
        // app component accepts exactly the derived empty closure below.
        if !w::compare(m,dependents.len(),0usize)?.is_eq(){return Err(Error::new("UNSUPPORTED_COMPOSITION","app dependent correction executor must be selected"));}
        p.normalize_set(m,"dependent_reversals",false)?;
    }else{
        if !shape::same_text(m,schema,"ledger-workflow-cost-finalize/2")?{return Err(shape::fail("app cost request tuple"));}
        shape::closed(m,p,&["schema","original_work_ref","component_slot","observation_proof","final_cost_proof","evidence_refs"],&[])?;
        shape::reference(m,&p["original_work_ref"])?;
        let slot=shape::text(m,p,"component_slot")?;
        if w::compare(m,slot.len(),1usize)?.is_lt()||w::compare(m,slot.len(),128usize)?.is_gt(){return Err(shape::fail("app cost component slot bound"));}
        for name in ["observation_proof","final_cost_proof"]{shape::closed(m,&p[name],&["attestation_ref","verification_ref"],&[])?;for field in ["attestation_ref","verification_ref"]{shape::reference(m,&p[name][field])?;}}
    }
    let evidence=shape::array(m,p,"evidence_refs")?;
    if w::compare(m,evidence.len(),16usize)?.is_gt(){return Err(shape::fail("app supplemental evidence bound"));}
    for reference in evidence{shape::reference(m,reference)?;}
    p.normalize_set(m,"evidence_refs",false)?;
    Ok(correct)
}
/// The app lineage head and generic decision head must identify the SAME
/// current immutable revision under the held snapshot; neither substitutes for
/// the other CAS operand.
pub(super) fn decision_head(m:&mut CoreMeter,state:&EconomicStateData,reference:&ExactRef,value:&Value)->Result<()> {
 let id=shape::text(m,value,"decision_id")?;let revision=shape::text(m,value,"revision")?;
 let mut selected=None;
 for head in &state.financial.decision_heads{m.visit("app.current_decision_head")?;if shape::same_text(m,&head.decision_id,id)?{if selected.is_some(){return Err(shape::fail("duplicate app decision head"));}selected=Some(head);}}
 let head=selected.ok_or_else(||shape::fail("app current decision head missing"))?;
 if !shape::same_ref(m,&head.reference,reference)?||!shape::same_text(m,&head.revision,revision)?{return Err(Error::new("REVISION_CONFLICT","app lineage and decision head differ"));}
 Ok(())
}
pub(super) fn current(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,claim_value:&Value,expected_revision:&str,state:&EconomicStateData)->Result<CurrentApp>{
    let (claim_ref,claim)=load(m,r,c,claim_value,"workflow-cost","ledger-workflow-app-cost-claim/1")?;
    shape::closed(m,&claim,&["schema","subject","observation_ref","original_final_evidence_ref","original_terms_ref","dependency_ref","operation_key"],&[])?;
    let app=state.app.ok_or_else(||shape::fail("app current state missing"))?;
    let mut selected=None;
    for head in app.cost_heads{m.visit("app.current_claim_head")?;if shape::same_ref(m,&head.cost_claim_ref,&claim_ref)?{if selected.is_some(){return Err(shape::fail("duplicate app cost head"));}selected=Some(head);}}
    let selected=selected.ok_or_else(||shape::fail("app cost head missing"))?;
    let revision=r.get(m,&c.key,&selected.reference,"workflow-cost","ledger-workflow-app-cost-revision/1")?;
    shape::closed(m,&revision,&["schema","cost_claim_ref","revision","current_decision_ref","current_evidence_ref","consumption_ref","status","operation_key"],&["prior_ref"])?;
    literal(m,&revision,"status","active")?;
    eq(m,&revision["cost_claim_ref"],claim_value)?;
    let actual=shape::text(m,&revision,"revision")?;
    if !shape::same_text(m,actual,&selected.revision)?||!shape::same_text(m,expected_revision,actual)?{return Err(Error::new("REVISION_CONFLICT","app cost expected revision"));}
    let counter=crate::policy::workflow::facts::counter_with_work(actual,m)?;
    if !w::compare(m,counter,0u64)?.is_gt(){return Err(shape::fail("app current positive revision"));}
    if w::compare(m,counter,1u64)?.is_eq(){if revision.get("prior_ref").is_some(){return Err(shape::fail("app initial revision predecessor"));}}
    else{call!(shape::reference,m,shape::field(m,&revision,"prior_ref")?)?;}
    let (decision_ref,decision)=load(m,r,c,&revision["current_decision_ref"],"workflow-decision","ledger-workflow-decision-revision/3")?;
    shape::closed(m,&decision,&["schema","decision_id","revision","subject_ref","kind","transition","status","original_terms_ref","authority_ref","evaluation_ref","actions_appended","active_action_refs","dependency_refs","operation_key","accepted_at"],&["prior_ref"])?;
    literal(m,&decision,"kind","cost")?;literal(m,&decision,"status","active")?;
    decision_head(m,state,&decision_ref,&decision)?;
    eq(m,&decision["subject_ref"],claim_value)?;eq(m,&decision["revision"],&revision["revision"])?;eq(m,&decision["original_terms_ref"],&claim["original_terms_ref"])?;
    let (consumption_ref,consumption)=load(m,r,c,&revision["consumption_ref"],"workflow-cost","ledger-workflow-app-cost-consumption/1")?;
    shape::closed(m,&consumption,&["schema","cost_claim_ref","observation_ref","final_evidence_ref","cost_revision","eligible_amount","operation_key"],&[])?;
    eq(m,&consumption["cost_claim_ref"],claim_value)?;eq(m,&consumption["observation_ref"],&claim["observation_ref"])?;eq(m,&consumption["final_evidence_ref"],&revision["current_evidence_ref"])?;
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning app current selection"))?;
    let constants=m.parse_owned_representation(&arena,br#""1""#,RepresentationKind::SourceDefinition)?;
    let initial_key=arena.array_from_values(&[&decision["decision_id"],constants.value()],m)?;
    let initial_id=body::semantic_id(m,"wdrev_","workflow-decision-revision",initial_key.value())?;
    let (initial_decision_ref,initial_decision)=r.by_identity(m,&c.key,"workflow-decision",&initial_id,"ledger-workflow-decision-revision/3")?;
    literal(m,&initial_decision,"revision","1")?;literal(m,&initial_decision,"kind","cost")?;literal(m,&initial_decision,"transition","cost_finalize")?;literal(m,&initial_decision,"status","active")?;
    eq(m,&initial_decision["decision_id"],&decision["decision_id"])?;eq(m,&initial_decision["subject_ref"],claim_value)?;eq(m,&initial_decision["original_terms_ref"],&claim["original_terms_ref"])?;
    // The immutable initial decision is retained separately: later corrections
    // never move its trusted accepted_at clock or replace it with today's head.
    Ok(CurrentApp{claim_ref,claim,revision_ref:selected.reference.clone(),revision,decision_ref,decision,consumption_ref,consumption,initial_decision_ref,initial_decision})
}
pub(super) fn proof_for(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,state:&EconomicStateData,attestation:&Value)->Result<crate::canonical::audit_arena::AuditedValue>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning retained app proof locator"))?;
    let mut selected=None;
    for reference in &state.financial.evidence_verifications{
        m.visit("app.retained_verification_locator")?;
        let body=r.get(m,&c.key,reference,"workflow-evidence-verification","ledger-workflow-evidence-verification/1")?;
        if shape::same(m,&body["attestation_ref"],attestation)?{if selected.is_some(){return Err(shape::fail("ambiguous retained app verification"));}selected=Some(reference);}
    }
    let selected=selected.ok_or_else(||shape::fail("app original proof verifier missing"))?;
    let selected=owning_reference(m,&arena,selected)?;
    arena.object_from_fields(&[("attestation_ref",attestation),("verification_ref",selected.value())],m)
}
pub(super) fn prepare<'p>(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&'p SelectedProgram,mut request:WorkingValue,state:&EconomicStateData)->Result<AppPlan<'p>>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning app cost plan"))?;
    let metadata=w::usize_add(m,std::mem::size_of::<AppPlan>(),12*(64+128+71)+512)?;
    let ownership=arena.reserve_external_payload(metadata,m)?;
    let correct=normalize_request(m,&mut request)?;
    let current=if correct{let expected=shape::text(m,&request,"expected_revision")?;Some(current(m,r,c,&request["original_cost_claim_ref"],expected,state)?)}else{None};
    let work=match &current{Some(old)=>&old.claim["subject"]["original_work_ref"],None=>&request["original_work_ref"]};
    let slot=match &current{Some(old)=>shape::text(m,&old.claim["subject"],"component_slot")?,None=>shape::text(m,&request,"component_slot")?};
    let work_ref=shape::reference(m,work)?;
    let original=original::select(m,r,c,program,&work_ref,slot,state)?;
    let observed_proof=match &current{Some(old)=>proof_for(m,r,c,state,&old.claim["observation_ref"])?,None=>arena.clone_value(&request["observation_proof"],m)?};
    let observation=evidence::resolve_observation(m,r,c,program,&original,observed_proof.value())?;
    let final_proof=if correct{&request["replacement_final_cost_proof"]}else{&request["final_cost_proof"]};
    let final_cost=evidence::resolve_final(m,r,c,program,&original,&observation,final_proof)?;
    let old_cost=if let Some(old)=&current{
        let old_proof=proof_for(m,r,c,state,&old.revision["current_evidence_ref"])?;
        Some(evidence::resolve_final(m,r,c,program,&original,&observation,old_proof.value())?)
    }else{None};
    let app=state.app.ok_or_else(||shape::fail("app permanent-key selection missing"))?;
    if w::compare(m,app.cost_claim_refs.len(),4096usize)?.is_gt()||w::compare(m,app.evidence_consumption_refs.len(),4096usize)?.is_gt(){return Err(shape::fail("app selected permanent-key operand bound"));}
    let work_value=owning_reference(m,&arena,&original.work_ref)?;
    let slot_value=arena.string_value(&original.component_slot,m)?;
    for reference in app.cost_claim_refs{
        m.visit("app.permanent_component_claim_hit")?;
        let claim=r.get(m,&c.key,reference,"workflow-cost","ledger-workflow-app-cost-claim/1")?;
        shape::closed(m,&claim,&["schema","subject","observation_ref","original_final_evidence_ref","original_terms_ref","dependency_ref","operation_key"],&[])?;
        if shape::same(m,&claim["subject"]["original_work_ref"],work_value.value())?&&shape::same(m,&claim["subject"]["component_slot"],slot_value.value())?{
            match &current{
                None=>return Err(Error::new("IDENTITY_CONFLICT","app work/component permanently claimed")),
                Some(current)=>{if !shape::same_ref(m,reference,&current.claim_ref)?{return Err(shape::fail("app current claim differs from permanent component index"));}}
            }
        }
    }
    let final_ref=owning_reference(m,&arena,&final_cost.pair.attestation_ref)?;
    let observation_ref=owning_reference(m,&arena,&observation.pair.attestation_ref)?;
    for reference in app.evidence_consumption_refs{
        m.visit("app.permanent_evidence_consumption_hit")?;
        let consumption=r.get(m,&c.key,reference,"workflow-cost","ledger-workflow-app-cost-consumption/1")?;
        shape::closed(m,&consumption,&["schema","cost_claim_ref","observation_ref","final_evidence_ref","cost_revision","eligible_amount","operation_key"],&[])?;
        let same_evidence=shape::same(m,&consumption["final_evidence_ref"],final_ref.value())?;
        let same_revision=shape::same(m,&consumption["observation_ref"],observation_ref.value())?&&shape::same(m,&consumption["cost_revision"],&final_cost.pair.attestation["cost_revision"])?;
        if same_evidence||same_revision{return Err(Error::new("IDENTITY_CONFLICT","app evidence/revision permanently consumed"));}
    }
    let key=super::super::cost_postings::operation_key(m,&arena,c)?;
    let work_id=arena.string_value(&original.work_ref.id,m)?;
    let component=arena.string_value(&original.component_slot,m)?;
    let constants=m.parse_owned_representation(&arena,br#"["app_cost","1"]"#,RepresentationKind::SourceDefinition)?;
    let seed=arena.array_from_values(&[&key.value()["scope"],work_id.value(),component.value(),&constants.value()[0]],m)?;
    let decision_id=body::semantic_id(m,"wdecision_","workflow-decision",seed.value())?;
    let revision=if let Some(old)=&current{
        if !call!(shape::same_text,m,shape::text(m,&old.decision,"decision_id")?,&decision_id)?{return Err(shape::fail("app stable original decision identity"));}
        let old_revision=call!(arithmetic::integer,m,shape::text(m,&old.revision,"revision")?,false)?;
        eq(m,&old.claim["subject"],&observation.pair.attestation["subject"])?;
        let binding=owning_reference(m,&arena,&original.binding_ref)?;
        eq(m,&old.claim["original_terms_ref"],binding.value())?;
        let old_cost=old_cost.as_ref().unwrap();
        let old_ref=owning_reference(m,&arena,&old_cost.pair.attestation_ref)?;
        eq(m,&final_cost.pair.attestation["prior_final_evidence_ref"],old_ref.value())?;
        let old_cost_revision=call!(arithmetic::integer,m,shape::text(m,&old_cost.pair.attestation,"cost_revision")?,false)?;
        let new_cost_revision=call!(arithmetic::integer,m,shape::text(m,&final_cost.pair.attestation,"cost_revision")?,false)?;
        let next=meter::add(m,&old_cost_revision,&1.into())?;
        if !meter::cmp(m,&next,&new_cost_revision)?.is_eq(){return Err(shape::fail("app correction next final evidence"));}
        let amount=super::super::cost_postings::amount(m,&arena,&old_cost.eligible)?;
        eq(m,&old.consumption["eligible_amount"],amount.value())?;
        eq(m,&old.consumption["cost_revision"],&old_cost.pair.attestation["cost_revision"])?;
        meter::add(m,&old_revision,&1.into())?
    }else{
        literal(m,&final_cost.pair.attestation,"cost_revision","1")?;
        1.into()
    };
    let original_decision=r.get(m,&c.key,&original.original_decision_ref,"workflow-decision","ledger-workflow-decision-revision/1")?;
    let dependent=arena.string_value(&decision_id,m)?;let binding=owning_reference(m,&arena,&original.binding_ref)?;
    let dependency_key=arena.array_from_values(&[&original_decision["decision_id"],dependent.value(),binding.value()],m)?;
    let dependency_id=body::semantic_id(m,"wdep_","workflow-dependency",dependency_key.value())?;
    let (dependency_ref,dependency)=r.by_identity(m,&c.key,"workflow-dependency",&dependency_id,"ledger-workflow-dependency/1")?;
    shape::closed(m,&dependency,&["schema","ancestor_decision_id","dependent_decision_id","kind","target_ref","original_terms_ref","maximum_actions","maximum_records","maximum_bytes","maximum_work","operation_key"],&[])?;
    literal(m,&dependency,"kind","delayed_cost")?;
    eq(m,&dependency["ancestor_decision_id"],&original_decision["decision_id"])?;eq(m,&dependency["dependent_decision_id"],dependent.value())?;eq(m,&dependency["original_terms_ref"],binding.value())?;
    let work=owning_reference(m,&arena,&original.work_ref)?;eq(m,&dependency["target_ref"],work.value())?;
    for name in ["maximum_actions","maximum_records","maximum_bytes","maximum_work"]{crate::policy::workflow::facts::counter_with_work(shape::text(m,&dependency,name)?,m)?;}
    if let Some(old)=&current{let dep=owning_reference(m,&arena,&dependency_ref)?;eq(m,&old.claim["dependency_ref"],dep.value())?;}
    for reference in shape::array(m,&request,"evidence_refs")?{let reference=shape::reference(m,reference)?;r.provenance(m,&c.key,&reference)?;}
    Ok(AppPlan{request,original,observation,final_cost,old_cost,current,dependency_ref,dependency,decision_id,revision,_ownership:ownership})
}
