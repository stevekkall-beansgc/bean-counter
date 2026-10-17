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
        &self.introduced[self.assertion_index].inner.decoded.as_ref().expect("owning assertion body")
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
    let enrollment=r.get(m,&c.key,&a.enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
    for reference in a.original_grant_refs.iter().chain(std::iter::once(&a.current_grant_ref)) {
        m.visit("retention.additional_operation_grant")?;
        let grant=r.get(m,&c.key,reference,"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let operations=shape::array(m,&grant,"operations")?;
        if !shape::member(m,operations,&json!(operation))?{return Err(shape::fail("assertion original/current operation"));}
    }
    let operations=shape::array(m,&enrollment["ceiling"],"operations")?;
    if !shape::member(m,operations,&json!(operation))?{return Err(shape::fail("assertion original ceiling operation"));}
    Ok(())
}
fn subject_owner(m:&mut CoreMeter,c:&DraftContextData,subject:&Value)->Result<()> {
    eq(m,&subject["logical_store_id"],&json!(c.key.logical_store_id))?;
    eq(m,&subject["scope"],&json!(c.key.scope))
}
fn field_schema<'p>(m:&mut CoreMeter,program:&'p SelectedProgram,reference:&Value)->Result<&'p crate::policy::workflow::FactSchema>{
    let mut selected=None;
    for (r,schema) in program.fact_schemas(){
        m.visit("retention.original_schema")?;
        if shape::same(m,&json!(r),reference)? {
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
fn execute(program:&SelectedProgram,c:&DraftContextData,raw:&[u8],inputs:&[EconomicRecordView<'_>],state:&EconomicStateData<'_>,arena:&AuditArena,m:&mut CoreMeter)->Result<RetainedAssertionData>{
    m.begin_request(raw)?;
    entry::context(m,c)?;
    let mut parsed=m.parse_owned_representation(arena,raw,RepresentationKind::OriginalRequest)?;
    let p=parsed.value();
    // Generic syntax parsing does not discharge economic closed/type guards.
    codec::validate_selected_value(m,p,262144)?;
    shape::closed(m,p,&["schema","attestation"],&[])?;
    let schema=shape::text(m,p,"schema")?;
    if program.uses_app_owned_cost_recipe() && (shape::same_text(m,schema,"ledger-workflow-app-observe/1")? || shape::same_text(m,schema,"ledger-workflow-app-cost-attest/1")?) {
        return super::app_cost::retain(program,c,parsed,inputs,state,arena,m);
    }

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
        eq(m,&att["schema"],&json!("ledger-workflow-family-close-attestation/1"))?;
        eq(m,&att["purpose"],&json!("family_no_success"))?;
        eq(m,&att["assertion"],&json!("no_success"))?;
        eq(m,&att["evidence_kind"],&json!("outcome"))?;
    } else {
        shape::closed(m,att,&["schema","attestation_id","source","evidence_kind","subject","subject_schema_ref","claims","issued_at","supporting_evidence_refs"],&[])?;
        eq(m,&att["schema"],&json!("ledger-workflow-fact-attestation/1"))?;
    }
    let identifier=shape::text(m,att,"attestation_id")?;
    text_bound(m,identifier,128,true)?;
    let source=shape::text(m,att,"source")?;
    text_bound(m,source,256,false)?;
    eq(m,&att["source"],&json!(c.key.source))?;
    let issued=Timestamp::parse_with_work(shape::text(m,att,"issued_at")?,m)?.micros();
    let received=Timestamp::parse_with_work(&c.received_at,m)?.micros();
    let accepted=Timestamp::parse_with_work(&c.accepted_at,m)?.micros();
    if meter::cmp(m,&issued.into(),&received.into())?.is_gt() || meter::cmp(m,&received.into(),&accepted.into())?.is_gt(){return Err(shape::fail("assertion trusted time"));}
    let supporting=shape::array(m,att,"supporting_evidence_refs")?;
    if meter::cmp(m,&supporting.len().into(),&16usize.into())?.is_gt(){return Err(shape::fail("assertion supporting reference bound"));}
    // Generic supporting provenance is resolved below; never used as authority.
    let mut r=if program.uses_app_owned_cost_recipe(){Records::borrowed_app(m,arena,&c.key,inputs,&c.source_observation_inputs)?}else{Records::borrowed(m,arena,&c.key,inputs,&c.source_observation_inputs)?};
    let subject=shape::field(m,att,"subject")?;
    subject_owner(m,c,subject)?;
    let (resolved,subject_bindings)=if close {
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
        let binding=shape::reference(m,&selected.target["binding_ref"])?;
        (resolved,body::ref_set(m,vec![binding,selected.target_ref])?)
    } else {
        resolve_field(m,&mut r,c,program,att,state)?
    };
    let nominations=shape::array(m,&resolved.binding,"source_slots")?;
    let nomination=shape::named(m,nominations,"name",&c.authorities[0].source_slot)?;
    let kinds=shape::array(m,nomination,"evidence_kinds")?;
    if !shape::member(m,kinds,&att["evidence_kind"])? {return Err(shape::fail("assertion original evidence nomination"));}
    // Supplemental references are immutable provenance only. Closed intrinsic
    // framing and completed retained origin are required; no authorization is
    // inferred from their record kind or caller placement.
    for reference in supporting {let reference=shape::reference(m,reference)?; r.provenance(m,&c.key,&reference)?;}
    let canonical_assertion=m.canonical_owned_representation(arena,att,RepresentationKind::Record)?;
    let assertion=body::document_borrowed(m,"workflow-fact-attestation",att)?;
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
    // Subject Ref set has already been normalized and duplicate-checked from
    // immutable original operands. Build its JSON ownership before copying.
    let mut subject_values=Vec::new();
    let slots=crate::validation_work::usize_add(m,std::mem::size_of::<AuditedValue>(),std::mem::size_of::<&Value>())?;
    crate::validation_work::ValidationWork::arithmetic(m,crate::validation_work::ArithmeticOperation::Multiply,&[&subject_bindings.len().into(),&slots.into()])?;
    let bytes=subject_bindings.len().checked_mul(slots).ok_or_else(||shape::fail("retention subject metadata"))?;
    let _subject_metadata=arena.reserve_external_payload(bytes,m)?;
    subject_values.try_reserve_exact(subject_bindings.len()).map_err(|_|shape::fail("retention subject vector"))?;
    for reference in &subject_bindings{m.visit("retention.subject_binding")?;subject_values.push(owned_ref(m,arena,reference)?);}
    let mut subject_borrows=Vec::new();
    subject_borrows.try_reserve_exact(subject_values.len()).map_err(|_|shape::fail("retention subject borrowed vector"))?;
    for value in &subject_values{m.visit("retention.subject_member")?;subject_borrows.push(value.value());}
    let subjects=arena.array_from_values(&subject_borrows,m)?;
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
    let authority_ref=authority.reference.clone();let assertion_ref=assertion.reference.clone();let verification_ref=verification.reference.clone();
    let mut introduced=resolved.introduced;
    let assertion_index=crate::validation_work::usize_add(m,introduced.len(),1)?;
    introduced.push(authority);introduced.push(assertion);introduced.push(verification);
    Ok(RetainedAssertionData{normalized,introduced:introduced.into_iter().map(|inner|RetainedAssertionEnvelope{inner}).collect(),authority_ref,assertion_ref,assertion_index,verification_ref,consumed:r.into_consumed_owned(),claims:vec![],_output_ownership:None})
}
fn resolve_field(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,att:&Value,state:&EconomicStateData)->Result<(authority::ResolvedOperands,Vec<ExactRef>)>{
    let subject=&att["subject"];
    let kind=shape::text(m,subject,"kind")?;
    let schema=field_schema(m,program,&att["subject_schema_ref"])?;
    let (resolved,binding)=if shape::same_text(m,kind,"outcome")? {
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
        (resolved,shape::reference(m,&selected.target["binding_ref"])?)
    } else if shape::same_text(m,kind,"work")? {
        shape::closed(m,subject,&["kind","logical_store_id","scope","canonical_source_namespace","business_operation_id","customer","activity","product_ref","binding_ref"],&[])?;
        let business=shape::text(m,subject,"business_operation_id")?;
        text_bound(m,business,128,true)?;
        eq(m,&subject["canonical_source_namespace"],&json!(c.canonical_economic_namespace))?;
        let mut product=None;
        for document in program.source_documents(){
            m.visit("retention.subject_product")?;
            if shape::same(m,&json!(document.reference()),&subject["product_ref"])?{product=Some(document.canonical_value());}
        }
        let product=product.ok_or_else(||shape::fail("assertion original product"))?;
        let activities=shape::array(m,product,"activities")?;
        let activity_name=shape::text(m,subject,"activity")?;
        let activity=shape::named(m,activities,"name",activity_name)?;
        eq(m,&activity["primitive"],&json!("work"))?;
        eq(m,&activity["fact_schema"],&att["subject_schema_ref"])?;
        let p=json!({"customer":subject["customer"],"activity":subject["activity"],"product_ref":subject["product_ref"],"policy_ref":program.root_policy_ref(),"fact_schema_ref":att["subject_schema_ref"],"binding_ref":subject["binding_ref"],"evidence_refs":[]});
        let owner=super::representation::InvocationToken::new();
        let times=super::representation::ParsedDraftTimes::parse(m,&owner,&c.received_at,&c.accepted_at)?;
        let resolved=authority::resolve_economic(m,r,c,&p,activity,product,&owner,&times,program,EconomicMode::EvidenceRetain,&[])?;
        additional_operation(m,r,c,"work.accept")?;
        (resolved,shape::reference(m,&subject["binding_ref"])?)
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
        eq(m,&field["provenance"]["kind"],&json!("attested"))?;
        eq(m,&field["provenance"]["source_slot"],&json!(c.authorities[0].source_slot))?;
        eq(m,&field["provenance"]["evidence_kind"],&att["evidence_kind"])?;
        schema.normalize_attested_field_complete_with_work(name,&claim["value"],m)?;
    }
    Ok((resolved,vec![binding]))
}
fn product_claim_schema(m:&mut CoreMeter,program:&SelectedProgram,att:&Value)->Result<()> {
    let activity_name=shape::text(m,&att["subject"],"activity")?;
    let mut found=false;
    for document in program.source_documents(){
        m.visit("retention.original_activity_schema")?;
        if shape::same(m,&json!(document.reference()),&json!(program.product_ref()))? {
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
