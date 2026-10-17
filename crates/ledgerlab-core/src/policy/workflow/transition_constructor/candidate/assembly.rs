//! One owning assembly of the actual complete interpreter's outputs. It is
//! private and noncommittable until the full future/resource boundary closes.
use super::super::{CoreMeter,capacity,data::*,shape,working_value::owning_reference};
use crate::{Result,Error,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,ArithmeticOperation,ValidationWork}};
use std::mem;

pub(in crate::policy::workflow::transition_constructor) struct Assembly<'inv,'raw> {
    pub construction:Construction<'inv,'raw>,
    pub schemas:Vec<String>,
    pub result_refs:Vec<ExactRef>,
    pub authority_position:usize,
    pub source_observations:Vec<super::ObservationAssociation>,
    pub actual_members:capacity::ActualMembers,
    _metadata:ExternalBufferLease,
}
fn copy(m:&mut CoreMeter,s:&str)->Result<String>{m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
fn copy_ref(m:&mut CoreMeter,r:&ExactRef)->Result<ExactRef>{Ok(ExactRef{kind:copy(m,&r.kind)?,id:copy(m,&r.id)?,digest:copy(m,&r.digest)?})}
fn product(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;a.checked_mul(b).ok_or_else(||shape::fail("candidate assembly metadata product"))}
fn ref_bytes(m:&mut CoreMeter,r:&ExactRef)->Result<usize>{let n=w::usize_add(m,r.kind.len(),r.id.len())?;w::usize_add(m,n,r.digest.len())}
/// Recipe selected by the actual normalized payload. No action-count inference
/// and no supplied receipt/result list participates in this dispatch.
pub(in crate::policy::workflow::transition_constructor) fn operation(m:&mut CoreMeter,p:&serde_json::Value)->Result<&'static str>{
    let schema=shape::text(m,p,"schema")?;
    for(s,kind)in[("ledger-workflow-work/1","work.accept"),("ledger-workflow-outcome/1","outcome.submit"),("ledger-workflow-outcome-correction/1","outcome.correct"),("ledger-workflow-outcome-close/2","outcome.close"),("ledger-workflow-quantity-correction/2","quantity.correct"),("ledger-workflow-work-reverse/2","work.reverse"),("ledger-workflow-cost-finalize/2","cost.finalize"),("ledger-workflow-cost-correct/2","cost.correct")]{
        m.visit("candidate.executed_payload_recipe")?;if shape::same_text(m,schema,s)?{return Ok(kind);}
    }
    Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","candidate payload recipe is not implemented"))
}
impl<'inv,'raw> Assembly<'inv,'raw>{
 pub(in crate::policy::workflow::transition_constructor) fn derive(m:&mut CoreMeter,c:&DraftContextData,mut t:Construction<'inv,'raw>)->Result<Self>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning candidate assembly"))?;
    let payload=m.parse_owned_representation(&arena,&t.normalized_payload,crate::validation_work::RepresentationKind::OriginalRequest)?;
    let kind=operation(m,payload.value())?;
    let mut bytes=product(m,t.new_envelopes.len(),mem::size_of::<String>()+mem::size_of::<ExactRef>()+mem::size_of::<super::CandidateEnvelope>())?;
    bytes=w::usize_add(m,bytes,mem::size_of::<Self>())?;
    for row in &t.new_envelopes{
        m.visit("candidate.assembly_output_dimension")?;
        let value=row.decoded(m,4194304)?;let schema=shape::text(m,&value,"schema")?;
        bytes=w::usize_add(m,bytes,schema.len())?;let reference_bytes=ref_bytes(m,&row.reference)?;bytes=w::usize_add(m,bytes,reference_bytes)?;
    }
    for value in [&c.key.logical_store_id,&c.key.scope[0],&c.key.scope[1],&c.key.source,&c.key.operation_id]{bytes=w::usize_add(m,bytes,value.len())?;}
    bytes=w::usize_add(m,bytes,4*(64+128+71)+71)?;
    let ordinary=t.candidate_introductions.as_ref().map(|x|x.claims.len()).unwrap_or(0);
    let app=t.app_effects.as_ref().map(|x|x.claims.len()).unwrap_or(0);
    let claim_count=w::usize_add(m,ordinary,app)?;
    let claims_bytes=product(m,claim_count,mem::size_of::<super::ClaimEffect>())?;bytes=w::usize_add(m,bytes,claims_bytes)?;
    let associations=product(m,c.authorities.len(),mem::size_of::<super::ObservationAssociation>()+2*(64+128+71))?;
    bytes=w::usize_add(m,bytes,associations)?;
    let metadata=arena.reserve_external_payload(bytes,m)?;
    let mut schemas=Vec::new();schemas.try_reserve_exact(t.new_envelopes.len()).map_err(|_|shape::fail("candidate schema vector"))?;
    let mut result_refs=Vec::new();result_refs.try_reserve_exact(t.new_envelopes.len()).map_err(|_|shape::fail("candidate result reference vector"))?;
    let mut authority=None;
    for(i,row)in t.new_envelopes.iter().enumerate(){
        m.visit("candidate.assembly_authoritative_result")?;let value=row.decoded(m,4194304)?;
        let schema=shape::text(m,&value,"schema")?;
        if shape::same_text(m,&row.reference.kind,"workflow-economic-authority")?&&shape::same_text(m,schema,"ledger-workflow-economic-authority/7")?{
            let actual=shape::text(m,&value,"operation")?;
            if shape::same_text(m,actual,kind)?{if authority.is_some(){return Err(shape::fail("ambiguous root operation authority result"));}authority=Some(i);}
        }
        schemas.push(copy(m,schema)?);result_refs.push(copy_ref(m,&row.reference)?);
    }
    let authority_position=authority.ok_or_else(||shape::fail("actual root authority result missing"))?;
    let mut source_observations=Vec::new();source_observations.try_reserve_exact(c.authorities.len()).map_err(|_|shape::fail("candidate source association vector"))?;
    for source in &c.authorities{
        m.visit("candidate.assembly_source_observation")?;
        for prior in &source_observations{
            let prior:&super::ObservationAssociation=prior;
            if shape::same_ref(m,&prior.observation_ref,&source.authority_observation_ref)?{return Err(shape::fail("duplicate candidate source observation"));}
        }
        let observation=t.records.get(m,&c.key,&source.authority_observation_ref,"workflow-authority-prefix-observation","ledger-workflow-authority-prefix-observation/1")?;
        let verifier=super::super::working_value::parse_owned_reference(m,&arena,&observation["verifier_ref"])?;
        t.records.get(m,&c.key,verifier.reference(),"workflow-authority-audit-verifier","ledger-authority-audit-verifier/2")?;
        source_observations.push(super::ObservationAssociation{observation_ref:copy_ref(m,&source.authority_observation_ref)?,verifier_ref:copy_ref(m,verifier.reference())?});
    }
    let heads=t.candidate_heads.as_ref().ok_or_else(||shape::fail("actual candidate head effects missing"))?;
    for head in &heads.effects{
        m.visit("candidate.assembly_head_position")?;
        if !w::compare(m,head.introduced_position,t.new_envelopes.len())?.is_lt(){return Err(shape::fail("candidate head output position"));}
        if !shape::same_ref(m,&head.proposed,&t.new_envelopes[head.introduced_position].reference)?{return Err(shape::fail("candidate head output exact association"));}
    }
    t.candidate_introductions.as_ref().ok_or_else(||shape::fail("actual candidate introductions missing"))?;
    let financial=t.financial_observation.as_ref().ok_or_else(||shape::fail("actual financial observation missing"))?;
    let observed=owning_reference(m,&arena,&financial.observation_ref)?;
    // The selected pair has already been checked against the complete held
    // preoperation projections by execute_complete; keep the owner attached.
    let verifier=owning_reference(m,&arena,&financial.verification_ref)?;shape::equal(m,&financial.observation["verifier_ref"],verifier.value())?;
    let _=observed;
    let actual_members=capacity::actual_members(m,&t)?;
    Ok(Self{construction:t,schemas,result_refs,authority_position,source_observations,actual_members,_metadata:metadata})
 }
 /// This is the explicit unfinished eligibility boundary, AFTER the actual
 /// interpreter, head/claim/link/funding/observation assembly and I∪N count.
 /// No profile maximum, caller flag or DraftTransition can satisfy it.
 pub(in crate::policy::workflow::transition_constructor) fn require_complete_capacity_and_resources(&self,m:&mut CoreMeter)->Result<()> {
    m.visit("candidate.complete_future_capacity_boundary")?;
    if let Some(original)=&self.construction.original_reservations{
        let arena=m.representation_arena().ok_or_else(||shape::fail("candidate capacity owner"))?;
        let _union=original.structural_union(&arena,m)?;
        if original.future_records.is_none(){return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","future executor record derivation is incomplete"));}
    }
    Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","complete all-ancestor dependency/capacity and original S/C/bytes/arena proof is not implemented"))
 }
}

// Existing component leases move with their actual payloads. None can be
// detached by a public setter, serialization or a DraftTransition conversion.
pub(in crate::policy::workflow::transition_constructor) struct CandidateOwnership {
    heads:super::heads::OwnedHeads,
    introductions:super::introductions::OwnedIntroductions,
    funding:Option<super::super::funding::OwnedFundingEffects>,
    consumed:super::super::records::OwnedConsumed,
    controls:[Option<ExternalBufferLease>;4],
    metadata:ExternalBufferLease,
}
impl Assembly<'_, '_>{
 pub(in crate::policy::workflow::transition_constructor) fn into_complete(self,c:&DraftContextData,digest:&str,m:&mut CoreMeter)->Result<super::EconomicCandidate>{
    // Called only AFTER require_complete_capacity_and_resources succeeds and
    // the SAME attempt's closed AUTH03 digest has been bound to c/financial.
    let Self{mut construction,schemas,result_refs,authority_position,source_observations,actual_members:_,_metadata}=self;
    let financial=construction.financial_observation.as_ref().ok_or_else(||shape::fail("candidate financial owner"))?;
    let financial_observation=super::ObservationAssociation{observation_ref:copy_ref(m,&financial.observation_ref)?,verifier_ref:copy_ref(m,&financial.verification_ref)?};
    let authority_ref=copy_ref(m,&construction.new_envelopes[authority_position].reference)?;
    let mut introduced=Vec::new();introduced.try_reserve_exact(construction.new_envelopes.len()).map_err(|_|shape::fail("candidate owning envelope vector"))?;
    for(row,schema)in construction.new_envelopes.drain(..).zip(schemas){
        m.visit("candidate.move_owning_envelope")?;
        if shape::same_text(m,&row.reference.kind,"workflow-dependency")?||shape::same_text(m,&row.reference.kind,"workflow-correction-capacity")?{
            return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","complete derived dependency/capacity effect projection is not implemented"));
        }
        introduced.push(super::CandidateEnvelope{reference:row.reference,schema,body:row.canonical_body});
    }
    let mut heads=construction.candidate_heads.take().ok_or_else(||shape::fail("candidate held head owner"))?;
    let mut introductions=construction.candidate_introductions.take().ok_or_else(||shape::fail("candidate introduction owner"))?;
    let app_claims=construction.app_effects.as_mut().map(|x|&mut x.claims);
    let total=w::usize_add(m,introductions.claims.len(),app_claims.as_ref().map(|x|x.len()).unwrap_or(0))?;
    let mut claims=Vec::new();claims.try_reserve_exact(total).map_err(|_|shape::fail("candidate claim ownership vector"))?;
    claims.append(&mut introductions.claims);if let Some(app)=app_claims{claims.append(app);}
    let links=std::mem::take(&mut introductions.links);let head_effects=std::mem::take(&mut heads.effects);
    let mut funding=construction.funding_effects.take();let funding_effects=match &mut funding{Some(v)=>std::mem::take(&mut v.effects),None=>Vec::new()};
    let mut consumed=construction.records.into_consumed_owned();let consumed_refs=std::mem::take(&mut consumed.references);
    let key=OperationKey{logical_store_id:copy(m,&c.key.logical_store_id)?,scope:[copy(m,&c.key.scope[0])?,copy(m,&c.key.scope[1])?],source:copy(m,&c.key.source)?,operation_id:copy(m,&c.key.operation_id)?};
    let result=super::CandidateResultMetadata{kind:super::CandidateResultKind::Economic,result_refs,authority_ref,source_observations,financial_observation,combined_audit_digest:copy(m,digest)?};
    Ok(super::EconomicCandidate{key,normalized_payload:construction.normalized_payload,introduced,consumed_refs,heads:head_effects,claims,links,dependencies:Vec::new(),funding:funding_effects,account_enrollments:Vec::new(),result,_ownership:CandidateOwnership{heads,introductions,funding,consumed,controls:construction.control_owners,metadata:_metadata}})
 }
}
