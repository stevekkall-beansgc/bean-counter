//! Exact resolved-inputs/9 explicit Ref edges. Outputs are locator DATA only;
//! original/current authority, implicit selection and origin custody are separate.
use super::{CoreMeter,data::{ExactRef,EconomicRecordView,OperationKey},records::Records,shape,working_value::{OwnedExactRef,parse_owned_reference}};
use crate::{Result,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,OriginalAuditSession,ValidationWork,ArithmeticOperation}};
use serde_json::Value;
use std::mem;

/// Explicit closure edges for ONE actual canonical record, not a complete I
/// set, authority proof or completed origin. The host owns its fixed point and
/// implicit/head selection. SourceState1 lineage remains the source witness.
pub struct ResolvedInputEdges9 {
    direct:Vec<OwnedExactRef>,
    history:Vec<(&'static str,OwnedExactRef)>,
    _ownership:ExternalBufferLease,
}
impl ResolvedInputEdges9 {
    pub fn direct_refs(&self)->impl Iterator<Item=&ExactRef>{self.direct.iter().map(|r|r.reference())}
    pub fn history_edges(&self)->impl Iterator<Item=(&'static str,&ExactRef)>{self.history.iter().map(|(field,r)|(*field,r.reference()))}
}
fn product(m:&mut CoreMeter,n:usize,size:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&size.into()])?;
    n.checked_mul(size).ok_or_else(||shape::fail("input edge metadata product"))
}
/// Parent-paid DATA classification for the exact named /9 recipe. This uses
/// the SAME field/cut walker as after-evaluation containment; it never replaces
/// that child's independent parse/type/resolve/authority/graph work. Every
/// origin/head association must still come from the held chronological owner.
pub fn select_resolved_inputs9_edges(key:&OperationKey,input:EconomicRecordView<'_>,s:&mut OriginalAuditSession)->Result<ResolvedInputEdges9>{
    let result=(||{
        s.check_cancelled()?;let arena=s.arena().shared_handle();let m=s.meter_mut();
        let inputs=[input];let mut records=Records::borrowed_parent_app(m,&arena,key,&inputs)?;
        let value=records.provenance(m,key,input.reference)?;
        // Two real parent semantic passes. The first derives ACTUAL capacities
        // before allocating either vector; the second validates and owns Refs.
        // Neither pass supplies credit or checked semantics to the child.
        let(mut direct_count,mut history_count)=(0usize,0usize);
        body(&value,&input.reference.kind,true,m,&mut |_,m|{direct_count=w::usize_add(m,direct_count,1)?;Ok(())},&mut |_,_,m|{history_count=w::usize_add(m,history_count,1)?;Ok(())})?;
        let direct_bytes=product(m,direct_count,mem::size_of::<OwnedExactRef>())?;
        let history_bytes=product(m,history_count,mem::size_of::<(&'static str,OwnedExactRef)>())?;
        let bytes=w::usize_add(m,direct_bytes,history_bytes)?;
        let bytes=w::usize_add(m,bytes,2*mem::size_of::<ResolvedInputEdges9>())?;
        let ownership=arena.reserve_external_payload(bytes,m)?;
        // Prepay controls and elements before either Vec initialization or
        // allocation, covering allocation and semantic refusal prefixes.
        m.write_bytes(bytes)?;
        let mut direct=Vec::new();direct.try_reserve_exact(direct_count).map_err(|_|shape::fail("input direct edge allocation"))?;
        let mut history=Vec::new();history.try_reserve_exact(history_count).map_err(|_|shape::fail("input history edge allocation"))?;
        body(&value,&input.reference.kind,true,m,&mut |v,m|{direct.push(parse_owned_reference(m,&arena,v)?);Ok(())},&mut |field,v,m|{history.push((field,parse_owned_reference(m,&arena,v)?));Ok(())})?;
        Ok(ResolvedInputEdges9{direct,history,_ownership:ownership})
    })();if result.is_err(){s.invalidate();}result
}

type Emit<'a> = dyn FnMut(&Value,&mut CoreMeter)->Result<()>+'a;
type Cut<'a> = dyn FnMut(&'static str,&Value,&mut CoreMeter)->Result<()>+'a;

fn generic(v:&Value,depth:usize,m:&mut CoreMeter,emit:&mut Emit<'_>)->Result<()> {
    m.visit("containment.result_value")?;
    if w::compare(m,depth,32usize)?.is_gt(){return Err(shape::fail("result Ref containment depth"));}
    match v{
        Value::Object(fields)=>{
            if w::compare(m,fields.len(),3usize)?.is_eq(){
                let mut exact=true;
                for name in ["kind","id","digest"]{
                    if !m.check("containment.reference_field",||fields.contains_key(name))?{exact=false;break;}
                }
                if exact{return emit(v,m);}
            }
            for value in fields.values(){m.visit("containment.result_field")?;let next=w::usize_add(m,depth,1)?;generic(value,next,m,emit)?;}
        },
        Value::Array(values)=>for value in values{m.visit("containment.result_element")?;let next=w::usize_add(m,depth,1)?;generic(value,next,m,emit)?;},
        _=>{},
    }
    Ok(())
}
fn context(v:&Value,m:&mut CoreMeter,emit:&mut Emit<'_>)->Result<()> {
    // Accepted fact VALUES are business data, including when their field names
    // happen to be kind/id/digest. Only its typed schema relation is a Ref.
    let schema=shape::field(m,v,"fact_schema_ref")?;emit(schema,m)
}
fn work_payload(v:&Value,m:&mut CoreMeter,emit:&mut Emit<'_>)->Result<()> {
    let fields=v.as_object().ok_or_else(||shape::fail("constructed work payload object"))?;
    for (name,value) in fields{
        m.visit("containment.work_payload_field")?;
        if shape::same_text(m,name,"facts")?{continue;}
        generic(value,1,m,emit)?;
    }
    Ok(())
}
fn binding(v:&Value,m:&mut CoreMeter,emit:&mut Emit<'_>)->Result<()> {
    for(name,value)in v.as_object().ok_or_else(||shape::fail("closure binding object"))?{
        m.visit("containment.binding_field")?;
        if shape::same_text(m,name,"accepted_context")?{context(value,m,emit)?;}else{generic(value,1,m,emit)?;}
    }
    Ok(())
}
fn intent_payload(v:&Value,m:&mut CoreMeter,emit:&mut Emit<'_>)->Result<()> {
    for(name,value)in v.as_object().ok_or_else(||shape::fail("closure intent payload"))?{
        m.visit("containment.intent_payload_field")?;
        if shape::same_text(m,name,"binding_inputs")?{binding(value,m,emit)?;}
        else if shape::same_text(m,name,"documents")?{
            for document in value.as_array().ok_or_else(||shape::fail("closure inline documents"))?{
                m.visit("containment.inline_document")?;
                // These are old inline definitions, not old history owners.
                // Their normalized schema defines business/commitment fields.
                body(document,"",false,m,emit,&mut |_,_,_|Err(shape::fail("inline history owner")))?;
            }
        }else{generic(value,1,m,emit)?;}
    }
    Ok(())
}
pub(super) fn body(v:&Value,owner_kind:&str,old:bool,m:&mut CoreMeter,emit:&mut Emit<'_>,cut:&mut Cut<'_>)->Result<()> {
    let schema=shape::text(m,v,"schema")?;
    for (candidate,kind) in [
        ("ledger-workflow-source-state/1","workflow-source-state"),
        ("ledger-workflow-party-evidence/2","workflow-party-evidence"),
        ("ledger-authority-audit-verifier/2","workflow-authority-audit-verifier"),
        ("ledger-workflow-financial-history-observation/3","workflow-financial-history-observation"),
        ("ledger-workflow-binding/1","workflow-binding"),("ledger-workflow-binding/2","workflow-binding"),
        ("ledger-workflow-acceptance-terms/1","workflow-acceptance-terms"),("ledger-workflow-acceptance-terms/2","workflow-acceptance-terms"),
        ("ledger-workflow-agreement-intent/1","workflow-agreement-intent"),
        ("ledger-workflow-work-record/1","workflow-work"),("ledger-workflow-outcome-claim/1","workflow-outcome-claim"),
        ("ledger-workflow-evaluation-proof/1","workflow-evaluation"),("ledger-workflow-verified-fact/1","workflow-verified-fact"),
        ("ledger-workflow-decision-revision/1","workflow-decision"),("ledger-workflow-decision-revision/2","workflow-decision"),("ledger-workflow-decision-revision/3","workflow-decision"),
        ("ledger-workflow-family-state/1","workflow-family-state"),("ledger-workflow-family-state/2","workflow-family-state"),
        ("ledger-workflow-chain-state/1","workflow-chain-state"),("ledger-workflow-invocation/1","workflow-invocation"),
        ("ledger-workflow-cost-revision/1","workflow-cost"),("ledger-cost-obligation-consumption/1","workflow-cost"),("ledger-workflow-app-cost-revision/1","workflow-cost"),
        ("ledger-workflow-supplier-final-cost/1","workflow-fact-attestation"),("ledger-workflow-app-final-cost/1","workflow-fact-attestation"),
    ]{
        m.visit("input_edges.schema_owner_kind")?;
        if shape::same_text(m,schema,candidate)?{
            if !shape::same_text(m,owner_kind,kind)?{return Err(shape::fail("input schema/owner kind mismatch"));}
            break;
        }
    }
    if !old&&shape::same_text(m,schema,"ledger-workflow-financial-history-observation/3")?{
        // Its history target set is an AUDIT seed, intentionally not all I.
        // complete::select checks framing/pins; the final closure check must
        // independently derive its complete I/history equality. Do not turn
        // the serialized history metadata into ordinary new-result Ref edges.
        for name in ["root_ref","verifier_ref"]{let value=shape::field(m,v,name)?;emit(value,m)?;}
        return Ok(());
    }
    if shape::same_text(m,schema,"ledger-authority-audit-verifier/2")?{
        // Closed code-owned verifier metadata, never retained business input.
        shape::closed(m,v,&["schema","profile","algorithm","resources","artifacts","origin_recipes"],&[])?;
        return Ok(());
    }
    if shape::same_text(m,schema,"ledger-workflow-party-evidence/2")?{
        // Exactly the commitment-first schema/purpose exception. All other
        // required party relations stay immediate, including Assent2 basis.
        shape::closed(m,v,&["schema","subject","media_type","content_base64","content_sha256","metadata","scope","retained_by","retained_at","installation_mode","authority_ref"],&[])?;
        emit(shape::field(m,v,"authority_ref")?,m)?;
        let metadata=shape::field(m,v,"metadata")?;
        let purpose=shape::text(m,metadata,"purpose")?;
        if shape::same_text(m,purpose,"party_consent")?{
            shape::closed(m,metadata,&["purpose","consent"],&[])?;
            let consent=shape::field(m,metadata,"consent")?;
            shape::closed(m,consent,&["scope","customer","agreement_id","party","acceptor","roles","authorization_ref","basis","accepted_at"],&[])?;
            let basis=shape::field(m,consent,"basis")?;
            shape::closed(m,basis,&["kind","terms_ref","terms_digest"],&[])?;
            let kind=shape::text(m,basis,"kind")?;
            if !shape::same_text(m,kind,"acceptance_terms")?{return Err(shape::fail("unknown party commitment basis"));}
            // Validate the exact commitment Ref; deliberately do not emit its
            // dependency. Assent2 and Verification2 are separate immediate
            // edges and receive no exception from this record.
            let arena=m.representation_arena().ok_or_else(||shape::fail("owning party commitment"))?;
            let committed=shape::field(m,basis,"terms_ref")?;
            let committed=parse_owned_reference(m,&arena,committed)?;
            let digest=shape::text(m,basis,"terms_digest")?;
            super::records::reference_parts(m,&committed.reference().kind,&committed.reference().id,digest)?;
            emit(shape::field(m,consent,"authorization_ref")?,m)?;
        }else if shape::same_text(m,purpose,"party_operator_attestation")?{
            shape::closed(m,metadata,&["purpose","target_evidence_ref","statement"],&[])?;
            emit(shape::field(m,metadata,"target_evidence_ref")?,m)?;
        }else{return Err(shape::fail("unknown commitment-first evidence purpose"));}
        return Ok(());
    }
    if shape::same_text(m,schema,"ledger-workflow-binding/1")?||shape::same_text(m,schema,"ledger-workflow-binding/2")?{return binding(v,m,emit);}
    let acceptance=shape::same_text(m,schema,"ledger-workflow-acceptance-terms/1")?||shape::same_text(m,schema,"ledger-workflow-acceptance-terms/2")?;
    let intent=shape::same_text(m,schema,"ledger-workflow-agreement-intent/1")?;
    let source=shape::same_text(m,schema,"ledger-workflow-source-state/1")?;
    let mut cut_field=None;
    if old{
        for candidate in ["ledger-workflow-decision-revision/1","ledger-workflow-decision-revision/2","ledger-workflow-decision-revision/3","ledger-workflow-family-state/1","ledger-workflow-family-state/2","ledger-workflow-chain-state/1","ledger-workflow-invocation/1","ledger-workflow-cost-revision/1","ledger-cost-obligation-consumption/1","ledger-workflow-app-cost-revision/1"]{
            m.visit("containment.qualified_history_schema")?;
            if shape::same_text(m,schema,candidate)?{cut_field=Some("prior_ref");break;}
        }
        if shape::same_text(m,schema,"ledger-workflow-supplier-final-cost/1")?||shape::same_text(m,schema,"ledger-workflow-app-final-cost/1")?{cut_field=Some("prior_final_evidence_ref");}
    }
    let work=shape::same_text(m,schema,"ledger-workflow-work-record/1")?;
    let outcome=shape::same_text(m,schema,"ledger-workflow-outcome-claim/1")?;
    let evaluation=shape::same_text(m,schema,"ledger-workflow-evaluation-proof/1")?;
    let fact=shape::same_text(m,schema,"ledger-workflow-verified-fact/1")?;
    let fields=v.as_object().ok_or_else(||shape::fail("constructed result dictionary"))?;
    for(name,value)in fields{
        m.visit("containment.constructed_result_field")?;
        if let Some(field)=cut_field{if shape::same_text(m,name,field)?{cut(field,value,m)?;continue;}}
        if old&&source&&shape::same_text(m,name,"prior_state_ref")?{
            // Exact SourceState1 lineage is the separate source witness's
            // cut, never an extra FinancialHistoryEdge or generic prior skip.
            continue;
        }
        if acceptance&&shape::same_text(m,name,"binding_inputs")?{binding(value,m,emit)?;}
        else if intent&&shape::same_text(m,name,"payload")?{intent_payload(value,m,emit)?;}
        else if work&&shape::same_text(m,name,"payload")?{work_payload(value,m,emit)?;}
        else if outcome&&shape::same_text(m,name,"original_payload")?{work_payload(value,m,emit)?;}
        else if evaluation&&shape::same_text(m,name,"facts")?{continue;}
        else if evaluation&&shape::same_text(m,name,"context")?{context(value,m,emit)?;}
        else if fact&&shape::same_text(m,name,"value")?{continue;}
        else{generic(value,1,m,emit)?;}
    }
    Ok(())
}

