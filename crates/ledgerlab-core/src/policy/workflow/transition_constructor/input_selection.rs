//! Coordinator locator DATA before content-addressed observations exist.
//! This is not authority, a completed immediate closure, or an economic result.
//! The economic child repeats the same selector and all semantic checks.
use super::{CoreMeter,data::{EconomicRecordView,ExactRef,OperationKey},records::{Records,OwnedConsumed},shape};
use super::super::selected_program::SelectedProgram;
use crate::{Result,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,OriginalAuditSession,RepresentationKind}};
use serde_json::Value;
use std::mem;

/// Actual held source operands, without an invented observation Ref. Bodies and
/// original/current custody stay in the host's existing owning inventory.
pub struct SourceAuthorityCapture<'a> {
    pub source_slot:&'a str,
    pub source:&'a str,
    pub principal:&'a str,
    pub canonical_economic_namespace:&'a str,
    pub enrollment_ref:&'a ExactRef,
    pub current_state_ref:&'a ExactRef,
    pub current_grant_ref:&'a ExactRef,
    pub original_grant_refs:&'a [ExactRef],
    pub binding_ref:&'a ExactRef,
    pub accepted_agreement_ref:&'a ExactRef,
    pub activation_ref:&'a ExactRef,
}
/// Borrowed real capture. No current observation, caller-complete boolean,
/// economic child, or new resource allowance is supplied by this view.
pub struct EconomicCapture<'a> {
    pub key:&'a OperationKey,
    pub actor:&'a str,
    pub canonical_economic_namespace:&'a str,
    pub installation_mode:&'a str,
    pub received_at:&'a str,
    pub accepted_at:&'a str,
    pub sources:&'a [SourceAuthorityCapture<'a>],
}
pub struct WorkSourceLocator<'a> {
    source_slot:&'a str,
    frame:&'a SourceAuthorityCapture<'a>,
    _ownership:ExternalBufferLease,
}
impl<'a> WorkSourceLocator<'a> {
    pub fn source_slot(&self)->&str{self.source_slot}
    pub fn captured_source(&self)->&SourceAuthorityCapture<'a>{self.frame}
}
/// Direct original/current locator seeds only. The coordinator must expand the
/// exact recipe closure, implicit locators and held heads before making its F
/// header. These rows cannot stand in for that closure or authorize admission.
pub struct WorkInputSeeds<'a> {
    source:WorkSourceLocator<'a>,
    records:OwnedConsumed,
    _ownership:ExternalBufferLease,
}
impl<'a> WorkInputSeeds<'a> {
    pub fn source(&self)->&WorkSourceLocator<'a>{&self.source}
    pub fn direct_refs(&self)->&[ExactRef]{&self.records.references}
}

/// SAME product/activity/source-slot locator used again inside the economic
/// work constructor. Each invocation pays its own actual lookup/comparisons.
pub(super) fn work_activity<'a>(program:&'a SelectedProgram,p:&Value,m:&mut CoreMeter)->Result<(&'a Value,&'a Value)> {
    let product_ref=shape::field(m,p,"product_ref")?;
    shape::closed(m,product_ref,&["kind","id","digest"],&[])?;
    let kind=shape::text(m,product_ref,"kind")?;
    let id=shape::text(m,product_ref,"id")?;
    let digest=shape::text(m,product_ref,"digest")?;
    super::records::reference_parts(m,kind,id,digest)?;
    let mut selected=None;
    for document in program.source_documents(){
        m.visit("work.selected_product_locator")?;
        let reference=document.reference();
        if shape::same_text(m,kind,reference.kind())?&&shape::same_text(m,id,reference.id())?&&shape::same_text(m,digest,reference.digest())?{
            if selected.is_some(){return Err(shape::fail("ambiguous selected original product"));}
            selected=Some(document.canonical_value());
        }
    }
    let product=selected.ok_or_else(||shape::fail("original product substitution"))?;
    let name=shape::text(m,p,"activity")?;
    let activities=shape::array(m,product,"activities")?;
    let activity=shape::named(m,activities,"name",name)?;
    let primitive=shape::text(m,activity,"primitive")?;
    if !shape::same_text(m,primitive,"work")?{return Err(shape::fail("work primitive"));}
    let actual_schema=shape::field(m,p,"fact_schema_ref")?;
    let selected_schema=shape::field(m,activity,"fact_schema")?;
    shape::equal(m,actual_schema,selected_schema)?;
    // Validate the selector now; the borrowed value remains program-owned.
    shape::text(m,activity,"source_slot")?;
    Ok((product,activity))
}

/// First parent-paid locator step for work.accept. It deliberately exposes no
/// input-set or prefix digest: complete recipe closure and independent current
/// authority are still required before observation construction/admission.
pub fn select_work_source_frame<'a>(program:&'a SelectedProgram,capture:&'a EconomicCapture<'a>,raw:&[u8],s:&mut OriginalAuditSession)->Result<WorkSourceLocator<'a>> {
    let result=(||{
        s.check_cancelled()?;program.require_attempt_owner(s.meter_mut())?;
        let payload=s.parse_representation(raw,RepresentationKind::OriginalRequest)?;
        let m=s.meter_mut();super::codec::validate_selected_types(m,payload.value())?;
        let schema=shape::text(m,payload.value(),"schema")?;
        if !shape::same_text(m,schema,"ledger-workflow-work/1")?{return Err(shape::fail("work source locator payload"));}
        let(_,activity)=work_activity(program,payload.value(),m)?;
        let slot=shape::text(m,activity,"source_slot")?;
        let binding=shape::field(m,payload.value(),"binding_ref")?;
        shape::closed(m,binding,&["kind","id","digest"],&[])?;
        let k=shape::text(m,binding,"kind")?;let id=shape::text(m,binding,"id")?;let d=shape::text(m,binding,"digest")?;
        super::records::reference_parts(m,k,id,d)?;
        let mut frame=None;
        for source in capture.sources{
            m.visit("work.captured_source_slot_locator")?;
            if shape::same_text(m,source.source_slot,slot)?{
                // This is the actual submitting work source. A dependent's
                // separately captured identity must never rewrite the root key.
                if !shape::same_text(m,source.source,&capture.key.source)?||!shape::same_text(m,source.principal,capture.actor)?||!shape::same_text(m,source.canonical_economic_namespace,capture.canonical_economic_namespace)?{return Err(shape::fail("captured work submitting identity"));}
                if !shape::same_text(m,k,&source.binding_ref.kind)?||!shape::same_text(m,id,&source.binding_ref.id)?||!shape::same_text(m,d,&source.binding_ref.digest)?{return Err(shape::fail("captured source binding locator"));}
                if frame.is_some(){return Err(shape::fail("ambiguous captured source slot"));}
                frame=Some(source);
            }
        }
        let frame=frame.ok_or_else(||shape::fail("actual captured work source slot missing"))?;
        let arena=m.representation_arena().ok_or_else(||shape::fail("owning work source locator"))?;
        let owner=arena.reserve_external_payload(mem::size_of::<WorkSourceLocator>(),m)?;
        m.write_bytes(mem::size_of::<WorkSourceLocator>())?;
        Ok(WorkSourceLocator{source_slot:slot,frame,_ownership:owner})
    })();if result.is_err(){s.invalidate();}result
}

/// Actual original Binding/Terms source group and initial-state locator. No
/// authority predicate or original receipt is inferred from this selection.
/// In particular the matched original group is DERIVED from accepted Terms,
/// rather than accepting a possibly incomplete capture slice as its proof.
pub fn select_work_input_seeds<'a>(program:&'a SelectedProgram,capture:&'a EconomicCapture<'a>,raw:&[u8],inputs:&[EconomicRecordView<'_>],s:&mut OriginalAuditSession)->Result<WorkInputSeeds<'a>>{
    let result=(||{
        s.check_cancelled()?;program.require_attempt_owner(s.meter_mut())?;
        s.meter_mut().visit("work.parent_original_app_recipe")?;
        if !program.uses_app_owned_cost_recipe(){return Err(crate::Error::new("UNSUPPORTED_SCHEMA_PROFILE","app work seed selector requires its exact original app compiler recipe"));}
        let source=select_work_source_frame(program,capture,raw,s)?;
        let frame=source.captured_source();let arena=s.arena().shared_handle();
        let p=s.parse_representation(raw,RepresentationKind::OriginalRequest)?;
        let m=s.meter_mut();let mut r=Records::borrowed_app(m,&arena,capture.key,inputs,&[])?;
        let binding=r.get(m,capture.key,frame.binding_ref,"workflow-binding","ledger-workflow-binding/2")?;
        let accepted=r.get(m,capture.key,frame.accepted_agreement_ref,"workflow-accepted-agreement","ledger-workflow-accepted-agreement/1")?;
        let activation=r.get(m,capture.key,frame.activation_ref,"workflow-activation","ledger-workflow-activation-transition/1")?;
        for body in [&*accepted,&*activation]{
            m.visit("work.parent_binding_edge")?;
            let actual=shape::field(m,body,"binding_ref")?;let expected=shape::field(m,p.value(),"binding_ref")?;
            shape::equal(m,actual,expected)?;
        }
        let rows=shape::array(m,&binding,"bindings")?;
        if !w::compare(m,rows.len(),1usize)?.is_eq(){return Err(crate::Error::new("UNSUPPORTED_COMPOSITION","work parent app binding selector requires the same implemented single retail binding"));}
        let terms_value=shape::field(m,&rows[0],"binding_ref")?;
        let terms_ref=super::working_value::parse_owned_reference(m,&arena,terms_value)?;
        let schema=if program.uses_app_cost_causal_recipe(){"ledger-workflow-slot-binding-terms/7"}else{"ledger-workflow-slot-binding-terms/6"};
        let terms=r.get(m,capture.key,terms_ref.reference(),"workflow-slot-terms",schema)?;
        let originals=shape::array(m,&terms,"source_grant_refs")?;
        let binding_grants=shape::array(m,&binding,"authority_refs")?;
        let mut count=0usize;
        for value in originals{
            m.visit("work.parent_original_grant_locator")?;
            if !shape::member(m,binding_grants,value)?{return Err(shape::fail("work parent grant absent original Binding"));}
            let reference=super::working_value::parse_owned_reference(m,&arena,value)?;
            let grant=r.get(m,capture.key,reference.reference(),"workflow-source-grant","ledger-workflow-source-grant/1")?;
            if !source_grant_matches(m,&grant,frame.source,frame.principal,frame.enrollment_ref)?{continue;}
            let mut matches=0usize;
            for captured in frame.original_grant_refs{
                m.visit("work.parent_exact_original_group")?;
                if shape::same_ref(m,reference.reference(),captured)?{matches=w::usize_add(m,matches,1)?;}
            }
            if !w::compare(m,matches,1usize)?.is_eq(){return Err(shape::fail("work parent original source group mismatch"));}
            count=w::usize_add(m,count,1)?;
        }
        if !w::compare(m,count,frame.original_grant_refs.len())?.is_eq()||w::compare(m,count,0usize)?.is_eq(){return Err(shape::fail("work parent original source group omitted or extra"));}
        let enrollment=r.get(m,capture.key,frame.enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
        r.get(m,capture.key,frame.current_state_ref,"workflow-source-state","ledger-workflow-source-state/1")?;
        r.get(m,capture.key,frame.current_grant_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let enrollment_raw=shape::text(m,&enrollment,"enrollment_id")?;
        let enrollment_id=arena.string_value(enrollment_raw,m)?;
        let store=arena.string_value(&capture.key.logical_store_id,m)?;
        let left=arena.string_value(&capture.key.scope[0],m)?;let right=arena.string_value(&capture.key.scope[1],m)?;
        let scope=arena.array_from_values(&[left.value(),right.value()],m)?;
        let revision=arena.string_value("1",m)?;
        let tuple=arena.array_from_values(&[store.value(),scope.value(),enrollment_id.value(),revision.value()],m)?;
        let initial_id=super::body::semantic_id(m,"wsource_","workflow-source-state",tuple.value())?;
        let(_,initial)=r.by_identity(m,capture.key,"workflow-source-state",&initial_id,"ledger-workflow-source-state/1")?;
        let initial_value=shape::field(m,&initial,"current_grant_ref")?;
        let initial_grant=super::working_value::parse_owned_reference(m,&arena,initial_value)?;
        r.get(m,capture.key,initial_grant.reference(),"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let owner=arena.reserve_external_payload(mem::size_of::<WorkInputSeeds>(),m)?;
        m.write_bytes(mem::size_of::<WorkInputSeeds>())?;
        Ok(WorkInputSeeds{source,records:r.into_consumed_owned(),_ownership:owner})
    })();if result.is_err(){s.invalidate();}result
}

/// SAME original grant grouping in the parent DATA selector and the consuming
/// source validator. The latter still performs every grant/ceiling/right check.
pub(super) fn source_grant_matches(m:&mut CoreMeter,grant:&Value,source:&str,actor:&str,enrollment:&ExactRef)->Result<bool>{
    let actual_source=shape::text(m,grant,"source")?;
    let principal=shape::text(m,grant,"principal_id")?;
    if !shape::same_text(m,actual_source,source)?||!shape::same_text(m,principal,actor)?{return Ok(false);}
    let value=shape::field(m,grant,"original_ceiling_ref")?;
    shape::closed(m,value,&["kind","id","digest"],&[])?;
    for (field,expected) in [("kind",&enrollment.kind),("id",&enrollment.id),("digest",&enrollment.digest)]{
        let actual=shape::text(m,value,field)?;if !shape::same_text(m,actual,expected)?{return Ok(false);}
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::selected_program::{SelectedCompilerSupport,SelectedSourceInput,compile_selected_publication};
    use crate::canonical::{self,audit_arena::AuditArena};
    #[test]
    fn parent_work_locator_uses_same_compiled_activity_without_observation_refs(){
        let(_,c,raw,_)=super::super::first_work_tests::test_inputs();
        let source:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let arena=AuditArena::new().unwrap();let before=arena.usage().unwrap().live_bytes;
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support=SelectedCompilerSupport::builtin().unwrap();
        let program=compile_selected_publication(SelectedSourceInput{documents:source["documents"].as_array().unwrap().iter().map(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect(),root_policy_ref:serde_json::from_value(source["root_policy_ref"].clone()).unwrap()},&support,&mut session).unwrap();
        let a=&c.authorities[0];
        let frame=||SourceAuthorityCapture{source_slot:&a.source_slot,source:&c.key.source,principal:&c.actor,canonical_economic_namespace:&c.canonical_economic_namespace,enrollment_ref:&a.enrollment_ref,current_state_ref:&a.current_state_ref,current_grant_ref:&a.current_grant_ref,original_grant_refs:&a.original_grant_refs,binding_ref:&a.binding_ref,accepted_agreement_ref:&a.accepted_agreement_ref,activation_ref:&a.activation_ref};
        let sources=[frame()];
        let capture=EconomicCapture{key:&c.key,actor:&c.actor,canonical_economic_namespace:&c.canonical_economic_namespace,installation_mode:&c.installation_mode,received_at:&c.received_at,accepted_at:&c.accepted_at,sources:&sources};
        let locator=select_work_source_frame(&program,&capture,&raw,&mut session).unwrap();
        assert_eq!(locator.source_slot(),a.source_slot);
        assert!(std::ptr::eq(locator.captured_source(),&sources[0]));
        let payload:Value=serde_json::from_slice(&raw).unwrap();
        let(_,activity)=work_activity(&program,&payload,session.meter_mut()).unwrap();
        assert_eq!(locator.source_slot(),activity["source_slot"].as_str().unwrap());
        // Neither capture nor returned locator has an observation Ref, I set,
        // digest, or origin/admission bit. The draft support stays nonadmitting.
        assert_eq!(program.execution_registry(),None);
        drop(locator);
        let mut other=OriginalAuditSession::for_resource_profile_v2(&AuditArena::new().unwrap()).unwrap();
        assert!(select_work_source_frame(&program,&capture,&raw,&mut other).is_err());
        let duplicate=[frame(),frame()];let duplicate_capture=EconomicCapture{sources:&duplicate,..capture};
        assert!(select_work_source_frame(&program,&duplicate_capture,&raw,&mut session).is_err());
        drop(program);drop(session);
        assert_eq!(arena.usage().unwrap().live_bytes,before);
    }
}
