//! One assertion/verification DATA constructor for fresh retention and saved
//! replay. Authentication, completed origins and permanent assertion uniqueness
//! belong to the held host transaction, never to caller-supplied JSON flags.
use super::super::selected_program::SelectedProgram;
use super::{CoreMeter, authority::{self,EconomicMode},body,codec,data::*,emitted_record::ConstructedEnvelope,family_context,inventory::entry,meter,records::{Records,OwnedConsumed},shape};
use crate::{Result,Error,canonical,canonical::audit_arena::{AuditedBytes,AuditedValue,AuditArena},domain::Timestamp,validation_work::{OriginalAuditSession,RepresentationKind}};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};

pub struct RetainedAssertionEnvelope { pub(super) inner:ConstructedEnvelope }
impl RetainedAssertionEnvelope {
    pub fn reference(&self)->&ExactRef {&self.inner.reference}
    pub fn canonical_body(&self)->&[u8] {&self.inner.canonical_body}
    pub fn schema(&self)->&str {
        // The only factory establishes and retains this decoded body.
        self.inner.decoded.as_ref().expect("owning retention framing")["schema"].as_str().expect("checked retention schema")
    }
}
pub struct RetainedAssertionData {
    pub(super) normalized:AuditedBytes,
    pub(super) introduced:Vec<RetainedAssertionEnvelope>,
    pub(super) authority_ref:ExactRef,
    pub(super) assertion_ref:ExactRef,
    pub(super) assertion_index:usize,
    pub(super) reused_assertion:Option<RetainedAssertionEnvelope>,
    pub(super) verification_ref:ExactRef,
    pub(super) consumed:OwnedConsumed,
    pub(super) claims:Vec<super::candidate::ClaimEffect>,
    pub(super) _output_ownership:Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl RetainedAssertionData {
    pub fn normalized_payload(&self)->&[u8] {self.normalized.bytes()}
    pub fn claims(&self)->&[super::candidate::ClaimEffect] {&self.claims}
    pub fn introduced(&self)->&[RetainedAssertionEnvelope] {&self.introduced}
    pub fn authority_ref(&self)->&ExactRef {&self.authority_ref}
    pub fn assertion_ref(&self)->&ExactRef {&self.assertion_ref}
    pub fn verification_ref(&self)->&ExactRef {&self.verification_ref}
    pub fn source(&self)->&str {self.assertion_body()["source"].as_str().expect("checked assertion source")}
    pub fn attestation_id(&self)->&str {self.assertion_body()["attestation_id"].as_str().expect("checked assertion identity")}
    fn assertion_body(&self)->&Value {
        &self.reused_assertion.as_ref().unwrap_or_else(|| &self.introduced[self.assertion_index]).inner.decoded.as_ref().expect("owning assertion body")
    }
}
fn eq(m:&mut CoreMeter,a:&Value,b:&Value)->Result<()> {
    if shape::same(m,a,b)? {Ok(())}else{Err(shape::fail("retained assertion relationship"))}
}
fn text_bound(m:&mut CoreMeter,s:&str,max:usize,id:bool)->Result<()> {
    if !meter::cmp(m,&s.len().into(),&0usize.into())?.is_gt() || meter::cmp(m,&s.len().into(),&max.into())?.is_gt(){return Err(shape::fail("assertion text bound"));}
    if id {for ch in s.chars(){m.scan_bytes(ch.len_utf8())?;if m.check("retention.identifier",||ch<='\u{1f}'||ch=='\u{7f}')?{return Err(shape::fail("assertion identifier"));}}}
    Ok(())
}
fn additional_operation(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,operation:&str)->Result<()> {
    // resolve_economic just validated these exact original/current snapshots
    // in this invocation. Only the additional required operation differs.
    let a=&c.authorities[0];
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning retention operation"))?;
    let operation=arena.string_value(operation,m)?;
    let enrollment=r.get(m,&c.key,&a.enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
    for reference in a.original_grant_refs.iter().chain(std::iter::once(&a.current_grant_ref)) {
        m.visit("retention.additional_operation_grant")?;
        let grant=r.get(m,&c.key,reference,"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let operations=shape::array(m,&grant,"operations")?;
        if !shape::member(m,operations,operation.value())?{return Err(shape::fail("assertion original/current operation"));}
    }
    let operations=shape::array(m,&enrollment["ceiling"],"operations")?;
    if !shape::member(m,operations,operation.value())?{return Err(shape::fail("assertion original ceiling operation"));}
    Ok(())
}
fn text_eq(m:&mut CoreMeter,value:&Value,want:&str)->Result<()> {
    m.visit("retention.string_operand")?;
    let actual=value.as_str().ok_or_else(||shape::fail("retention string operand"))?;
    if shape::same_text(m,actual,want)?{Ok(())}else{Err(shape::fail("retention string relationship"))}
}
fn definition(m:&mut CoreMeter,reference:&crate::policy::workflow::compiler::DefinitionRef)->Result<AuditedValue>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning retention definition"))?;
    let kind=arena.string_value(reference.kind(),m)?;let id=arena.string_value(reference.id(),m)?;let digest=arena.string_value(reference.digest(),m)?;
    arena.object_from_fields(&[("kind",kind.value()),("id",id.value()),("digest",digest.value())],m)
}
fn subject_owner(m:&mut CoreMeter,c:&DraftContextData,subject:&Value)->Result<()> {
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning retention subject"))?;
    let store=arena.string_value(&c.key.logical_store_id,m)?;
    let a=arena.string_value(&c.key.scope[0],m)?;let b=arena.string_value(&c.key.scope[1],m)?;
    let scope=arena.array_from_values(&[a.value(),b.value()],m)?;
    eq(m,&subject["logical_store_id"],store.value())?;eq(m,&subject["scope"],scope.value())
}
fn subject_set(m:&mut CoreMeter,arena:&AuditArena,values:&[&Value])->Result<AuditedValue>{
    let array=arena.array_from_values(values,m)?;
    let mut object=arena.object_from_fields(&[("refs",array.value())],m)?;
    super::intent::sort_owned_set(&mut object,"refs",false,m,arena)?;
    arena.clone_value(&object.value()["refs"],m)
}
fn field_schema<'p>(m:&mut CoreMeter,program:&'p SelectedProgram,reference:&Value)->Result<&'p crate::policy::workflow::FactSchema>{
    let mut selected=None;
    for (r,schema) in program.fact_schemas(){
        m.visit("retention.original_schema")?;
        let expected=definition(m,r)?;
        if shape::same(m,expected.value(),reference)? {
            if selected.is_some(){return Err(shape::fail("ambiguous assertion schema"));}
            selected=Some(schema);
        }
    }
    selected.ok_or_else(||shape::fail("assertion original schema"))
}
/// Called by BOTH actual evidence.retain and original semantic replay. The
/// supplied source/context/prestate remain DATA requiring host provenance.
pub fn retain_attestation(
    program:&SelectedProgram,c:&DraftContextData,raw:&[u8],inputs:&[EconomicRecordView<'_>],state:&EconomicStateData<'_>,session:&mut OriginalAuditSession,
)->Result<RetainedAssertionData>{
    let arena=session.arena().shared_handle();
    let result=(||{
        let mut m=session.meter_mut().original_economic_meter()?;
        let output=execute(program,c,raw,inputs,state,&arena,&mut m)?;
        session.meter_mut().finish_original_economic(m)?;
        Ok(output)
    })();
    if result.is_err(){session.invalidate();}
    result
}
fn execute<'raw>(program:&SelectedProgram,c:&'raw DraftContextData,raw:&[u8],inputs:&'raw [EconomicRecordView<'raw>],state:&EconomicStateData<'_>,arena:&AuditArena,m:&mut CoreMeter)->Result<RetainedAssertionData>{
    m.begin_request(raw)?;
    entry::context_selected(m,c,program)?;
    let mut parsed=m.parse_owned_representation(arena,raw,RepresentationKind::OriginalRequest)?;
    let p=parsed.value();
    // Generic syntax parsing does not discharge economic closed/type guards.
    codec::validate_selected_value(m,p,262144)?;
    shape::closed(m,p,&["schema","attestation"],&[])?;
    let schema=shape::text(m,p,"schema")?;
    if program.uses_app_owned_cost_recipe() && (shape::same_text(m,schema,"ledger-workflow-app-observe/1")? || shape::same_text(m,schema,"ledger-workflow-app-cost-attest/1")?) {
        return super::app_cost::retain(program,c,parsed,inputs,state,arena,m);
    }
    produce(program,c,parsed,inputs,state,arena,m)?.into_data()
}
struct GenericRetention<'raw>{
    normalized:AuditedBytes,introduced:Vec<RetainedAssertionEnvelope>,
    authority_ref:ExactRef,assertion_ref:ExactRef,assertion_index:usize,verification_ref:ExactRef,
    records:Records<'raw,'raw>,terms:authority::Terms,work:Option<super::record_body::RecordBody>,
    claims:Vec<super::candidate::ClaimEffect>,reused_assertion:Option<RetainedAssertionEnvelope>,
    ownership:crate::canonical::audit_arena::ExternalBufferLease,
}
impl GenericRetention<'_>{
    fn into_data(self)->Result<RetainedAssertionData>{
        let Self{normalized,introduced,authority_ref,assertion_ref,assertion_index,verification_ref,records,claims,reused_assertion,ownership,..}=self;
        Ok(RetainedAssertionData{normalized,introduced,authority_ref,assertion_ref,assertion_index,reused_assertion,verification_ref,consumed:records.into_consumed_owned(),claims,_output_ownership:Some(ownership)})
    }
}
/// SAME retention producer inside the complete factory's already-open child;
/// the real unique Records owner moves with its outputs, as for app retention.
pub(super) fn execute_complete<'raw>(program:&SelectedProgram,c:&'raw DraftContextData,parsed:AuditedValue,inputs:&'raw [EconomicRecordView<'raw>],state:&EconomicStateData<'_>,arena:&AuditArena,m:&mut CoreMeter)->Result<Construction<'raw,'raw>>{
    entry::context_selected(m,c,program)?;
    let output=produce(program,c,parsed,inputs,state,arena,m)?;
    let GenericRetention{normalized,mut introduced,records,terms,work,claims,reused_assertion,ownership,..}=output;
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    let per=std::mem::size_of::<ConstructedEnvelope>();
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&introduced.len().into(),&per.into()])?;
    let n=introduced.len().checked_mul(per).ok_or_else(||shape::fail("complete retention output transfer"))?;
    let n=w::usize_add(m,n,std::mem::size_of::<Construction>())?;
    let transfer=arena.reserve_external_payload(n,m)?;m.write_bytes(n)?;
    let mut envelopes=Vec::new();envelopes.try_reserve_exact(introduced.len()).map_err(|_|shape::fail("complete retention envelope vector"))?;
    for row in introduced.drain(..){m.visit("retention.move_same_owning_envelope")?;m.scan_bytes(per)?;envelopes.push(row.inner);}
    let normalized_payload=super::emitted_record::EncodedBody::owned(arena,normalized,m)?;
    let original_work=match work{Some(work)=>OriginalWorkPayload::Retained(work),None=>OriginalWorkPayload::Unattached};
    Ok(Construction{economic:EconomicConstruction{terms,original_work},original_reservations:None,financial_reservations:None,normalized_payload,new_envelopes:envelopes,reused_result:reused_assertion.map(|x|x.inner.reference),
        evaluation_steps:Vec::new(),proposed_heads:ProposedHeads{decision:Vec::new(),family:Vec::new(),chain:None},claim_introductions:Vec::new(),link_introductions:Vec::new(),prospective_obligations:Vec::new(),records,history_edges:Vec::new(),app_effects:Some(AppEffects{claims,heads:Vec::new()}),app_causal:None,current_capacity:None,candidate_heads:None,candidate_introductions:None,financial_observation:None,funding_effects:None,control_owners:[Some(ownership),Some(transfer),None,None,None]})
}
fn produce<'raw>(program:&SelectedProgram,c:&'raw DraftContextData,mut parsed:AuditedValue,inputs:&'raw [EconomicRecordView<'raw>],state:&EconomicStateData<'_>,arena:&AuditArena,m:&mut CoreMeter)->Result<GenericRetention<'raw>>{
    let p=parsed.value();
    shape::closed(m,p,&["schema","attestation"],&[])?;
    let schema=shape::text(m,p,"schema")?;
    let close=shape::same_text(m,schema,"ledger-workflow-family-close-attest/1")?;
    if !close && !shape::same_text(m,schema,"ledger-workflow-fact-attest/1")?{return Err(shape::fail("assertion retention payload tuple"));}
    if close && !program.uses_family_close_recipe(){return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","family close original compiler recipe"));}
    let att=shape::field(m,p,"attestation")?;
    let normalized_att=normalize_assertion(m,arena,program,att,close)?;
    parsed.replace_existing_owned("attestation",normalized_att,m)?;
    let p=parsed.value();
    let att=shape::field(m,p,"attestation")?;
    if close {
        shape::closed(m,att,&["schema","attestation_id","source","evidence_kind","purpose","assertion","subject","issued_at","supporting_evidence_refs"],&[])?;
        text_eq(m,&att["schema"],"ledger-workflow-family-close-attestation/1")?;
        text_eq(m,&att["purpose"],"family_no_success")?;
        text_eq(m,&att["assertion"],"no_success")?;
        text_eq(m,&att["evidence_kind"],"outcome")?;
    } else {
        shape::closed(m,att,&["schema","attestation_id","source","evidence_kind","subject","subject_schema_ref","claims","issued_at","supporting_evidence_refs"],&[])?;
        text_eq(m,&att["schema"],"ledger-workflow-fact-attestation/1")?;
    }
    let identifier=shape::text(m,att,"attestation_id")?;
    text_bound(m,identifier,128,true)?;
    let source=shape::text(m,att,"source")?;
    text_bound(m,source,256,false)?;
    text_eq(m,&att["source"],&c.key.source)?;
    let issued=Timestamp::parse_with_work(shape::text(m,att,"issued_at")?,m)?.micros();
    let received=Timestamp::parse_with_work(&c.received_at,m)?.micros();
    let accepted=Timestamp::parse_with_work(&c.accepted_at,m)?.micros();
    if meter::cmp(m,&issued.into(),&received.into())?.is_gt() || meter::cmp(m,&received.into(),&accepted.into())?.is_gt(){return Err(shape::fail("assertion trusted time"));}
    let supporting=shape::array(m,att,"supporting_evidence_refs")?;
    if meter::cmp(m,&supporting.len().into(),&16usize.into())?.is_gt(){return Err(shape::fail("assertion supporting reference bound"));}
    // Generic supporting provenance is resolved below; never used as authority.
    let mut r=if program.uses_app_owned_cost_recipe(){Records::borrowed_app(m,arena,&c.key,inputs,&c.source_observation_inputs)?}else{Records::borrowed(m,arena,&c.key,inputs,&c.source_observation_inputs)?};
    r.install_initial_source_outputs(m,state.initial_source_correspondences)?;
    let subject=shape::field(m,att,"subject")?;
    subject_owner(m,c,subject)?;
    let (resolved,subject_bindings,work)=if close {
        shape::closed(m,subject,&["logical_store_id","scope","original_binding_ref","target_ref","family_slot","expected_family_revision"],&[])?;
        let slot=shape::text(m,subject,"family_slot")?;
        text_bound(m,slot,128,true)?;
        let selected=family_context::select(m,&mut r,c,program,&subject["target_ref"],slot,state)?;
        eq(m,&subject["original_binding_ref"],&selected.target["binding_ref"])?;
        eq(m,&subject["expected_family_revision"],&selected.family_head["revision"])?;
        let status=shape::text(m,&selected.family_head,"state")?;
        if !shape::same_text(m,status,"awaiting_anchor")? && !shape::same_text(m,status,"open")? {return Err(Error::new("FAMILY_CLOSED","assertion current family state"));}
        let declaration=shape::field(m,selected.family,"no_success_close")?;
        shape::closed(m,declaration,&["close_source_slot"],&[])?;
        let source_slot=shape::text(m,declaration,"close_source_slot")?;
        let resolved=selected.authority(m,&mut r,c,program,source_slot,EconomicMode::EvidenceRetain)?;
        additional_operation(m,&mut r,c,"outcome.close")?;
        let binding=super::working_value::parse_owned_reference(m,arena,&selected.target["binding_ref"])?;
        let binding=owned_ref(m,arena,binding.reference())?;let target=owned_ref(m,arena,&selected.target_ref)?;
        (resolved,subject_set(m,arena,&[binding.value(),target.value()])?,Some(selected.work))
    } else {
        resolve_field(m,&mut r,c,program,att,state)?
    };
    if program.uses_app_owned_cost_recipe(){super::app_cost::check_current_consent(program,m,&mut r,c,&resolved.binding,&resolved.accepted,state)?;}
    let nominations=shape::array(m,&resolved.binding,"source_slots")?;
    let nomination=shape::named(m,nominations,"name",&c.authorities[0].source_slot)?;
    let kinds=shape::array(m,nomination,"evidence_kinds")?;
    if !shape::member(m,kinds,&att["evidence_kind"])? {return Err(shape::fail("assertion original evidence nomination"));}
    // Supplemental references are immutable provenance only. Closed intrinsic
    // framing and completed retained origin are required; no authorization is
    // inferred from their record kind or caller placement.
    for reference in supporting {let reference=shape::reference(m,reference)?; r.provenance(m,&c.key,&reference)?;}
    // Result/native-control ownership begins before the first emitted Ref or
    // SHA String is constructed. Upstream subject/source selection ownership
    // remains independently required by the containing eligibility proof.
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    let count=w::usize_add(m,resolved.introduced.len(),3)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&2usize.into()])?;
    let slots=count.checked_mul(2).ok_or_else(||shape::fail("retention output overlap"))?;
    let slots=w::usize_add(m,resolved.introduced.capacity(),slots)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&slots.into(),&std::mem::size_of::<RetainedAssertionEnvelope>().into()])?;
    let bytes=slots.checked_mul(std::mem::size_of::<RetainedAssertionEnvelope>()).ok_or_else(||shape::fail("retention output controls"))?;
    let refs=w::usize_add(m,count,6)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&refs.into(),&(std::mem::size_of::<ExactRef>()+64+128+71).into()])?;
    let refs=refs.checked_mul(std::mem::size_of::<ExactRef>()+64+128+71).ok_or_else(||shape::fail("retention Ref outputs"))?;
    let bytes=w::usize_add(m,bytes,refs)?;
    let fixed=std::mem::size_of::<GenericRetention>()+std::mem::size_of::<super::candidate::ClaimEffect>()+std::mem::size_of::<Vec<super::candidate::ClaimEffect>>()
        +8*std::mem::size_of::<String>()+128+71+64+71;
    let bytes=w::usize_add(m,bytes,fixed)?;
    let mut bytes=w::usize_add(m,bytes,c.key.logical_store_id.len())?;
    bytes=w::usize_add(m,bytes,c.key.scope[0].len())?;bytes=w::usize_add(m,bytes,c.key.scope[1].len())?;
    bytes=w::usize_add(m,bytes,c.key.source.len())?;bytes=w::usize_add(m,bytes,identifier.len())?;
    let ownership=arena.reserve_external_payload(bytes,m)?;m.write_bytes(bytes)?;
    let canonical_assertion=m.canonical_owned_representation(arena,att,RepresentationKind::Record)?;
    let assertion=body::document_borrowed(m,"workflow-fact-attestation",att)?;
    let reuse=existing_assertion(m,&mut r,c,state,&assertion)?;
    m.hash_bytes(canonical_assertion.bytes().len())?;
    let raw_sha=canonical::hex_with_work(&Sha256::digest(canonical_assertion.bytes()),m)?;
    let current=r.get(m,&c.key,&c.authorities[0].current_grant_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
    let starts=Timestamp::parse_with_work(shape::text(m,&current,"starts_at")?,m)?.micros();
    let ends=Timestamp::parse_with_work(shape::text(m,&current,"ends_at")?,m)?.micros();
    if meter::cmp(m,&received.into(),&starts.into())?.is_lt() || !meter::cmp(m,&received.into(),&ends.into())?.is_lt(){return Err(shape::fail("retention grant at trusted receipt"));}
    let authority=resolved.authority_document(m,None)?;
    let authority_value=authority.decoded(m,65536)?;
    let constants=m.parse_owned_representation(arena,br#"{"schema":"ledger-workflow-evidence-verification/1","verification_primitive":"authenticated-nominated-source-retention/1","verification_evidence_refs":[]}"#,RepresentationKind::SourceDefinition)?;
    let reference=owned_ref(m,arena,&assertion.reference)?;
    let raw_sha=arena.string_value(&raw_sha,m)?;
    let digest=arena.string_value(&assertion.reference.digest,m)?;
    // SAME preadmitted canonical subject set; no intermediate native Ref Vec.
    let subjects=subject_bindings;
    let trusted=arena.string_value(&c.received_at,m)?;
    let verification=arena.object_from_fields(&[
        ("schema",&constants.value()["schema"]),
        ("attestation_ref",reference.value()),
        ("attestation_raw_sha256",raw_sha.value()),
        ("attestation_digest",digest.value()),
        ("verified_source",&authority_value["source"]),
        ("verification_primitive",&constants.value()["verification_primitive"]),
        ("source_grant_ref",&authority_value["current_grant_ref"]),
        ("source_grant_revision",&current["revision"]),
        ("authenticated_principal_ref",&authority_value["enrollment_ref"]),
        ("trusted_received_at",trusted.value()),
        ("subject_binding_refs",subjects.value()),
        ("verification_evidence_refs",&constants.value()["verification_evidence_refs"]),
    ],m)?;
    let verification=body::document_borrowed(m,"workflow-evidence-verification",verification.value())?;
    let normalized=m.canonical_owned_representation(arena,p,RepresentationKind::OriginalRequest)?;
    fn copied(m:&mut CoreMeter,r:&ExactRef)->Result<ExactRef>{
        fn text(m:&mut CoreMeter,s:&str)->Result<String>{m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
        Ok(ExactRef{kind:text(m,&r.kind)?,id:text(m,&r.id)?,digest:text(m,&r.digest)?})
    }
    let authority_ref=copied(m,&authority.reference)?;let assertion_ref=copied(m,&assertion.reference)?;let verification_ref=copied(m,&verification.reference)?;
    fn copied_text(m:&mut CoreMeter,s:&str)->Result<String>{m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
    let mut claims=Vec::new();claims.try_reserve_exact(1).map_err(|_|shape::fail("assertion identity effect vector"))?;
    // The source was checked against the authenticated nomination above;
    // the produced verifier independently uses this same authority source.
    let verified_source=shape::text(m,&authority_value,"source")?;
    claims.push(super::candidate::ClaimEffect::derived(super::candidate::PermanentClaimKey::Assertion{logical_store_id:copied_text(m,&c.key.logical_store_id)?,scope:[copied_text(m,&c.key.scope[0])?,copied_text(m,&c.key.scope[1])?],verified_source:copied_text(m,verified_source)?,attestation_id:copied_text(m,identifier)?},copied(m,&assertion.reference)?));
    let terms=resolved.terms;
    let mut introduced=resolved.introduced;
    introduced.try_reserve_exact(3).map_err(|_|shape::fail("retention result vector"))?;
    let assertion_index=crate::validation_work::usize_add(m,introduced.len(),1)?;
    introduced.push(authority);
    let reused_assertion=if reuse{Some(RetainedAssertionEnvelope{inner:assertion})}else{introduced.push(assertion);None};
    introduced.push(verification);
    Ok(GenericRetention{normalized,introduced:introduced.into_iter().map(|inner|RetainedAssertionEnvelope{inner}).collect(),authority_ref,assertion_ref,assertion_index,verification_ref,records:r,terms,work,claims,reused_assertion,ownership})
}
fn resolve_field(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,att:&Value,state:&EconomicStateData)->Result<(authority::ResolvedOperands,AuditedValue,Option<super::record_body::RecordBody>)>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning retention field subject"))?;
    let subject=&att["subject"];
    let kind=shape::text(m,subject,"kind")?;
    let schema=field_schema(m,program,&att["subject_schema_ref"])?;
    let (resolved,binding,work)=if shape::same_text(m,kind,"outcome")? {
        shape::closed(m,subject,&["kind","logical_store_id","scope","target_ref","family_slot","business_outcome_token","activity","binding_ref"],&[])?;
        let token=shape::text(m,subject,"business_outcome_token")?;
        text_bound(m,token,128,true)?;
        let slot=shape::text(m,subject,"family_slot")?;
        let selected=family_context::select(m,r,c,program,&subject["target_ref"],slot,state)?;
        eq(m,&subject["binding_ref"],&selected.target["binding_ref"])?;
        eq(m,&subject["activity"],&selected.family["trigger_activity"])?;
        let source_slot=shape::text(m,selected.family,"source_slot")?;
        let resolved=selected.authority(m,r,c,program,source_slot,EconomicMode::EvidenceRetain)?;
        additional_operation(m,r,c,"outcome.submit")?;
        {let binding=super::working_value::parse_owned_reference(m,&arena,&selected.target["binding_ref"])?;
        let value=owned_ref(m,&arena,binding.reference())?;(resolved,subject_set(m,&arena,&[value.value()])?,Some(selected.work))}
    } else if shape::same_text(m,kind,"work")? {
        shape::closed(m,subject,&["kind","logical_store_id","scope","canonical_source_namespace","business_operation_id","customer","activity","product_ref","binding_ref"],&[])?;
        let business=shape::text(m,subject,"business_operation_id")?;
        text_bound(m,business,128,true)?;
        text_eq(m,&subject["canonical_source_namespace"],&c.canonical_economic_namespace)?;
        let mut product=None;
        for document in program.source_documents(){
            m.visit("retention.subject_product")?;
            let reference=definition(m,document.reference())?;
            if shape::same(m,reference.value(),&subject["product_ref"])?{product=Some(document.canonical_value());}
        }
        let product=product.ok_or_else(||shape::fail("assertion original product"))?;
        let activities=shape::array(m,product,"activities")?;
        let activity_name=shape::text(m,subject,"activity")?;
        let activity=shape::named(m,activities,"name",activity_name)?;
        text_eq(m,&activity["primitive"],"work")?;
        eq(m,&activity["fact_schema"],&att["subject_schema_ref"])?;
        let policy=definition(m,program.root_policy_ref())?;let empty=arena.array_from_values(&[],m)?;
        let p=arena.object_from_fields(&[("customer",&subject["customer"]),("activity",&subject["activity"]),("product_ref",&subject["product_ref"]),("policy_ref",policy.value()),("fact_schema_ref",&att["subject_schema_ref"]),("binding_ref",&subject["binding_ref"]),("evidence_refs",empty.value())],m)?;
        let owner=super::representation::InvocationToken::new();
        let times=super::representation::ParsedDraftTimes::parse(m,&owner,&c.received_at,&c.accepted_at)?;
        let resolved=authority::resolve_economic(m,r,c,p.value(),activity,product,&owner,&times,program,EconomicMode::EvidenceRetain,&[])?;
        additional_operation(m,r,c,"work.accept")?;
        {let binding=super::working_value::parse_owned_reference(m,&arena,&subject["binding_ref"])?;
        let value=owned_ref(m,&arena,binding.reference())?;(resolved,subject_set(m,&arena,&[value.value()])?,None)}
    }else{return Err(shape::fail("assertion closed subject kind"));};
    let claims=shape::array(m,att,"claims")?;
    if !meter::cmp(m,&claims.len().into(),&0usize.into())?.is_gt() || meter::cmp(m,&claims.len().into(),&64usize.into())?.is_gt(){return Err(shape::fail("assertion claim count"));}
    let fields=shape::array(m,schema.canonical_value(),"fields")?;
    for (i,claim) in claims.iter().enumerate(){
        m.visit("retention.assertion_claim")?;
        shape::closed(m,claim,&["field","value"],&[])?;
        let name=shape::text(m,claim,"field")?;
        for prior in &claims[..i] {let prior=shape::text(m,prior,"field")?;if shape::same_text(m,prior,name)?{return Err(shape::fail("duplicate assertion field"));}}
        let field=shape::named(m,fields,"name",name)?;
        text_eq(m,&field["provenance"]["kind"],"attested")?;
        text_eq(m,&field["provenance"]["source_slot"],&c.authorities[0].source_slot)?;
        eq(m,&field["provenance"]["evidence_kind"],&att["evidence_kind"])?;
        schema.normalize_attested_field_complete_with_work(name,&claim["value"],m)?;
    }
    Ok((resolved,binding,work))
}
fn product_claim_schema(m:&mut CoreMeter,program:&SelectedProgram,att:&Value)->Result<()> {
    let activity_name=shape::text(m,&att["subject"],"activity")?;
    let mut found=false;
    for document in program.source_documents(){
        m.visit("retention.original_activity_schema")?;
        let reference=definition(m,document.reference())?;let expected=definition(m,program.product_ref())?;
        if shape::same(m,reference.value(),expected.value())? {
            let activities=shape::array(m,document.canonical_value(),"activities")?;
            let activity=shape::named(m,activities,"name",activity_name)?;
            eq(m,&activity["fact_schema"],&att["subject_schema_ref"])?;
            found=true;
        }
    }
    if !found{return Err(shape::fail("assertion original product schema"));}Ok(())
}
pub(super) fn normalize_assertion(m:&mut CoreMeter,arena:&AuditArena,program:&SelectedProgram,att:&Value,close:bool)->Result<AuditedValue>{
    let mut owned=arena.clone_value(att,m)?;
    super::intent::sort_owned_set(&mut owned,"supporting_evidence_refs",false,m,arena)?;
    if close {
        let subject=shape::field(m,owned.value(),"subject")?;
        let raw=shape::text(m,subject,"expected_family_revision")?;
        let revision=crate::policy::workflow::facts::counter_with_work(raw,m)?;
        if !meter::cmp(m,&revision.into(),&0usize.into())?.is_gt(){return Err(shape::fail("positive family revision"));}
        // u64 decimal render is at most20bytes; Version permits at most19.
        if meter::cmp(m,&revision.into(),&9_999_999_999_999_999_999u64.into())?.is_gt(){return Err(shape::fail("family revision bound"));}
        let value=super::close::counter_value(m,arena,revision)?;
        let mut subject=arena.clone_value(subject,m)?;
        subject.replace_existing_owned("expected_family_revision",value,m)?;
        owned.replace_existing_owned("subject",subject,m)?;
    }else{
        let schema=field_schema(m,program,&att["subject_schema_ref"])?;
        product_claim_schema(m,program,att)?;
        let claims=shape::array(m,owned.value(),"claims")?;
        if !meter::cmp(m,&claims.len().into(),&0usize.into())?.is_gt() || meter::cmp(m,&claims.len().into(),&64usize.into())?.is_gt(){return Err(shape::fail("assertion claim count"));}
        let n=claims.len();
        use std::mem;
        use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
        let per=mem::size_of::<AuditedValue>()+mem::size_of::<&Value>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
        let size=n.checked_mul(per).ok_or_else(||shape::fail("assertion normalization metadata"))?;
        let _metadata=arena.reserve_external_payload(size,m)?;
        let mut normalized=Vec::new();
        normalized.try_reserve_exact(n).map_err(|_|shape::fail("assertion normalized allocation"))?;
        for claim in claims {
            m.visit("retention.normalize_claim")?;
            shape::closed(m,claim,&["field","value"],&[])?;
            let name=shape::text(m,claim,"field")?;
            let raw=shape::field(m,claim,"value")?;
            // String/enum copies cannot exceed the input string. Numeric and
            // UTC render scratch follow owned_facts' pinned scalar inventory:
            // <=1024 requested bytes, including the returned typed scalar.
            let raw_bytes=raw.as_str().map(str::len).unwrap_or(0);
            let scratch=w::usize_add(m,raw_bytes,1024+mem::size_of::<crate::policy::workflow::FactValue>())?;
            let _scalar=arena.reserve_external_payload(scratch,m)?;
            let value=schema.normalize_attested_field_complete_with_work(name,raw,m)?;
            use crate::policy::workflow::FactValue;
            let value=match &value {
                FactValue::Boolean(value)=>arena.clone_value(&Value::Bool(*value),m)?,
                FactValue::String(value)|FactValue::Enum(value)=>arena.string_value(value,m)?,
                FactValue::Integer(value)|FactValue::Decimal(value)=>arena.string_value(value.as_str(),m)?,
                FactValue::Timestamp(value)=>arena.string_value(value.as_str(),m)?,
                FactValue::Missing=>return Err(shape::fail("missing attested claim value")),
            };
            let field=shape::field(m,claim,"field")?;
            let claim=arena.object_from_fields(&[("field",field),("value",value.value())],m)?;
            normalized.push(claim);
        }
        let mut borrowed=Vec::new();
        borrowed.try_reserve_exact(n).map_err(|_|shape::fail("assertion borrowed normalization allocation"))?;
        for claim in &normalized {m.visit("retention.normalized_claim_member")?;borrowed.push(claim.value());}
        let values=arena.array_from_values(&borrowed,m)?;
        owned.replace_existing_owned("claims",values,m)?;
        super::intent::sort_owned_set(&mut owned,"claims",true,m,arena)?;
    }
    Ok(owned)
}

fn owned_ref(m:&mut CoreMeter,arena:&AuditArena,r:&ExactRef)->Result<AuditedValue>{
    let kind=arena.string_value(&r.kind,m)?;
    let id=arena.string_value(&r.id,m)?;
    let digest=arena.string_value(&r.digest,m)?;
    arena.object_from_fields(&[("kind",kind.value()),("id",id.value()),("digest",digest.value())],m)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::workflow::selected_program::{SelectedCompilerSupport,SelectedSourceInput,compile_selected_program};
    fn program()->SelectedProgram{
        let source:Value=canonical::parse(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let input=SelectedSourceInput{documents:source["documents"].as_array().unwrap().iter().map(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect(),root_policy_ref:serde_json::from_value(source["root_policy_ref"].clone()).unwrap()};
        let mut m=CoreMeter::for_compile(1_048_576).unwrap();
        compile_selected_program(input,&SelectedCompilerSupport::builtin().unwrap(),&mut m).unwrap()
    }
    #[test]
    fn retained_claim_normalization_uses_original_subset_and_owned_sets(){
        let program=program();
        let (schema_ref,schema)=program.fact_schemas().iter().find(|(_,s)|s.canonical_value()["fields"].as_array().unwrap().iter().any(|f|f["provenance"]["kind"]=="attested")).unwrap();
        let product=program.source_documents().iter().find(|d|d.reference()==program.product_ref()).unwrap().canonical_value();
        let activity=product["activities"].as_array().unwrap().iter().find(|v|v["fact_schema"]==json!(schema_ref)).unwrap();
        let field=&schema.canonical_value()["fields"][0];
        let value=field["type"]["values"][0].clone();
        // Other required attested fields are intentionally absent: retaining a
        // subset must not invent their facts or require a full outcome payload.
        let assertion=json!({"schema":"ledger-workflow-fact-attestation/1","subject_schema_ref":schema_ref,"subject":{"activity":activity["name"]},"claims":[{"field":field["name"],"value":value}],"supporting_evidence_refs":[]});
        let arena=AuditArena::new().unwrap();let baseline=arena.usage().unwrap().live_bytes;
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=session.meter_mut().original_economic_meter().unwrap();
        let normalized=normalize_assertion(&mut child,&arena,&program,&assertion,false).unwrap();
        assert_eq!(normalized.value(),&assertion);
        drop(normalized);
        let mut invalid=assertion.clone();invalid["claims"][0]["value"]=json!(true);
        assert!(normalize_assertion(&mut child,&arena,&program,&invalid,false).is_err());
        let mut duplicate=assertion.clone();duplicate["claims"].as_array_mut().unwrap().push(assertion["claims"][0].clone());
        assert!(normalize_assertion(&mut child,&arena,&program,&duplicate,false).is_err());
        assert_eq!(arena.usage().unwrap().live_bytes,baseline);
        // This exercises normalization DATA only, not source authentication or
        // a financial constructor/qualification.
    }
    #[test]
    fn complete_retention_rejects_missing_source_context_before_indexing(){
        let(program,mut c,_,_)=super::super::first_work_tests::test_inputs();c.authorities.clear();
        let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m=session.meter_mut().original_economic_meter().unwrap();
        let parsed=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-fact-attest/1","attestation":{}}"#,RepresentationKind::OriginalRequest).unwrap();
        let financial=DraftStateData::default();let state=EconomicStateData{retained_assertion:None,financial:&financial,capacity_heads:&[],supplier:None,app:None,current_party_authorizations:&[],observation:None,audit:None,initial_source_correspondences:&[]};
        assert!(execute_complete(&program,&c,parsed,&[],&state,&arena,&mut m).is_err());
    }
    #[test]
    fn retained_assertion_alias_consumes_exact_old_body_and_conflicts_on_change(){
        // This exercises the actual alias/old-I boundary only; the host must
        // independently prove completed origin and exact locked-key selection.
        let c=DraftContextData{key:OperationKey{logical_store_id:"store".into(),scope:["scope".into(),"test".into()],source:"source".into(),operation_id:"operation".into()},actor:"actor".into(),canonical_economic_namespace:"namespace".into(),installation_mode:"real_local".into(),received_at:"2026-10-18T00:00:00.000000Z".into(),accepted_at:"2026-10-18T00:00:00.000000Z".into(),authorities:vec![],source_observation_inputs:vec![]};
        let original=json!({"schema":"ledger-workflow-fact-attestation/1","source":"source","attestation_id":"assertion","claims":[{"field":"code","value":"ok"}]});
        let mut build=CoreMeter::new(65536).unwrap();let old=body::document_borrowed(&mut build,"workflow-fact-attestation",&original).unwrap();
        for case in 0..4 {
            let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();let mut m=session.meter_mut().original_economic_meter().unwrap();
            let views=[EconomicRecordView{reference:&old.reference,canonical_body:&old.canonical_body}];
            let mut records=Records::borrowed_app(&mut m,&arena,&c.key,&views,&[]).unwrap();
            let financial=DraftStateData::default();
            let state=EconomicStateData{retained_assertion:if case==0{None}else{Some(&old.reference)},financial:&financial,capacity_heads:&[],supplier:None,app:None,current_party_authorizations:&[],observation:None,audit:None,initial_source_correspondences:&[]};
            let mut value=original.clone();if case==2{value["claims"][0]["value"]=json!("changed");}if case==3{value["attestation_id"]=json!("other");}
            let produced=body::document_borrowed(&mut m,"workflow-fact-attestation",&value).unwrap();
            match case{0=>{assert!(!existing_assertion(&mut m,&mut records,&c,&state,&produced).unwrap());assert!(records.consumed().is_empty());},1=>{assert!(existing_assertion(&mut m,&mut records,&c,&state,&produced).unwrap());assert_eq!(records.consumed(),&[old.reference.clone()]);assert!(matches!(records.used_canonical(&mut m,&old.reference).unwrap(),super::super::records::UsedCanonical::Preexisting(_)));},2=>assert_eq!(existing_assertion(&mut m,&mut records,&c,&state,&produced).unwrap_err().code,"IDENTITY_CONFLICT"),_=>assert!(existing_assertion(&mut m,&mut records,&c,&state,&produced).is_err())}
        }
    }
    #[test]
    fn close_subject_version_normalizes_without_synthesizing_facts(){
        let program=program();let arena=AuditArena::new().unwrap();let baseline=arena.usage().unwrap().live_bytes;
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();let mut child=session.meter_mut().original_economic_meter().unwrap();
        let a=json!({"subject":{"expected_family_revision":"0002"},"supporting_evidence_refs":[]});
        let normalized=normalize_assertion(&mut child,&arena,&program,&a,true).unwrap();
        assert_eq!(normalized.value()["subject"]["expected_family_revision"],"2");
        assert!(normalized.value().get("facts").is_none());drop(normalized);
        for revision in ["0","10000000000000000000","-1"] {
            let mut invalid=a.clone();invalid["subject"]["expected_family_revision"]=json!(revision);
            assert!(normalize_assertion(&mut child,&arena,&program,&invalid,true).is_err());
            assert_eq!(arena.usage().unwrap().live_bytes,baseline);
        }
    }
}

/// Exact-key point-hit DATA is reconciled by the host against its completed
/// prefix. The child independently reads and compares the old assertion; it
/// never treats the held projection as body/origin or authority proof.
pub(super) fn existing_assertion(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,state:&EconomicStateData,produced:&ConstructedEnvelope)->Result<bool>{
    if !m.check("retention.permanent_assertion_hit",||state.retained_assertion.is_some())?{return Ok(false);}
    let old=state.retained_assertion.expect("checked permanent assertion hit");
    let body=r.provenance(m,&c.key,old)?;
    let value=produced.decoded(m,4194304)?;
    if !shape::same(m,&body["source"],&value["source"])? || !shape::same(m,&body["attestation_id"],&value["attestation_id"])?{return Err(shape::fail("held assertion exact permanent key"));}
    let canonical=r.canonical(m,old)?;
    if !shape::same_ref(m,old,&produced.reference)? || !shape::same_bytes(m,canonical,&produced.canonical_body)?{return Err(Error::new("IDENTITY_CONFLICT","permanent assertion identity has different canonical body"));}
    if !matches!(r.used_canonical(m,old)?,super::records::UsedCanonical::Preexisting(_)){return Err(shape::fail("held assertion must be retained old I"));}
    Ok(true)
}
