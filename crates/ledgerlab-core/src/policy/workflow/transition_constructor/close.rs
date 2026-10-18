//! Original-family close executor. The explicit proof is consumed directly;
//! discovery through supplemental evidence or fabricated pricing facts is
//! impossible. Completed retained custody and current projection locks remain
//! host obligations, just as for every other constructor operand.
use super::super::selected_program::SelectedProgram;
use super::{CoreMeter,arithmetic,authority::{self,EconomicMode},body,data::*,family_context::{self,FamilyContext},meter,records::{Records,SuppliedRecords},representation::InvocationToken,shape,working_value::{self,WorkingValue}};
use crate::{Result,Error,canonical,domain::Timestamp,validation_work::{self as w,RepresentationKind}};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use num_traits::ToPrimitive;

fn eq(m:&mut CoreMeter,a:&Value,b:&Value)->Result<()> {shape::equal(m,a,b)}
fn time(m:&mut CoreMeter,v:&Value,key:&str)->Result<i64>{
    let raw=shape::text(m,v,key)?;
    Ok(Timestamp::parse_with_work(raw,m)?.micros())
}
fn positive_revision(m:&mut CoreMeter,raw:&str)->Result<u64>{
    let n=super::super::facts::counter_with_work(raw,m)?;
    if !meter::cmp(m,&n.into(),&0u64.into())?.is_gt(){return Err(shape::fail("positive family revision"));}
    Ok(n)
}
fn deadline_grant_allows(m:&mut CoreMeter,g:&Value,c:&DraftContextData,slot:&Value,customer:&Value,activity:&str,product:&str,at:i64)->Result<bool>{
    let mut eligible=true;
    for (field,want) in [("source",json!(c.key.source)),("principal_id",json!(c.actor)),("canonical_economic_namespace",json!(c.canonical_economic_namespace))]{eligible&=shape::same(m,&g[field],&want)?;}
    eligible&=authority::deadline_grant_scope(m,g,slot,customer,activity,product,at)?;
    Ok(eligible)
}
/// Every candidate is checked; finding an earlier name never ends the walk.
/// Slot-dependent nomination/grant/ceiling predicates are independent. The
/// selected authority then validates the common exact target role/terms/source
/// lineage/assent/observation once: those facts cannot differ between slots of
/// this same source, original Binding and target role. Failure returns no result.
fn deadline_slot(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,selected:&FamilyContext<'_>)->Result<crate::canonical::audit_arena::AuditedValue>{
    const REGISTRY:&str="sha256:8ebed26695ae3594d40f91d733839bddcc31b94dcb688b2aa5890af7775af0da";
    let registry=if program.uses_app_cost_causal_recipe(){"sha256:42368b84a652527c58389014fefa69140d6a40c2217e193694f162441699cdc2"}else if program.uses_app_owned_cost_recipe(){"sha256:9a6379b0f0d20005f1406ca6ed3a5b55c40d275cec5435e1648d9b482332509b"}else{REGISTRY};
    if program.execution_registry()!=Some(registry){return Err(Error::new("ORIGINAL_VERIFIER_UNAVAILABLE","exact original deadline-close recipe not enabled"));}
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning deadline profile"))?;
    let a=&c.authorities[0];
    let binding_ref=shape::reference(m,&selected.target["binding_ref"])?;
    if !shape::same_ref(m,&binding_ref,&a.binding_ref)?{return Err(shape::fail("deadline original binding context"));}
    let binding=r.get(m,&c.key,&binding_ref,"workflow-binding",if program.uses_app_owned_cost_recipe(){"ledger-workflow-binding/2"}else{"ledger-workflow-binding/1"})?;
    let accepted=r.get(m,&c.key,&a.accepted_agreement_ref,"workflow-accepted-agreement","ledger-workflow-accepted-agreement/1")?;
    eq(m,&accepted["binding_ref"],&json!(binding_ref))?;
    let terms_ref=shape::reference(m,&accepted["terms_ref"])?;
    let terms=r.get(m,&c.key,&terms_ref,"workflow-acceptance-terms",if program.uses_app_owned_cost_recipe(){"ledger-workflow-acceptance-terms/2"}else{"ledger-workflow-acceptance-terms/1"})?;
    if program.uses_app_owned_cost_recipe(){super::inventory::binding::validate_app(m,&binding,&terms)?;}else{super::inventory::binding::validate(m,&binding,&terms)?;}
    let enrollment=r.get(m,&c.key,&a.enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
    let current=r.get(m,&c.key,&a.current_grant_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
    let product_id=original_product_id(m,program,selected)?;
    let trigger=shape::text(m,selected.family,"trigger_activity")?;
    let at=Timestamp::parse_with_work(&c.accepted_at,m)?.micros();
    let nominations=shape::array(m,&binding,"source_slots")?;
    let mut chosen:Option<(crate::canonical::audit_arena::AuditedBytes,crate::canonical::audit_arena::AuditedValue)>=None;
    for (i,nomination) in nominations.iter().enumerate(){
        m.visit("close.complete_original_deadline_slots")?;
        shape::closed(m,nomination,&["name","sources","evidence_kinds"],&[])?;
        let name=shape::field(m,nomination,"name")?;
        for prior in &nominations[..i]{if shape::same(m,&prior["name"],name)?{return Err(shape::fail("duplicate original deadline slot"));}}
        let sources=shape::array(m,nomination,"sources")?;
        let mut eligible=shape::member(m,sources,&json!(c.key.source))?;
        eligible&=deadline_grant_allows(m,&current,c,name,&selected.work["payload"]["customer"],trigger,product_id,at)?;
        eligible&=deadline_grant_allows(m,&enrollment["ceiling"],c,name,&selected.work["payload"]["customer"],trigger,product_id,at)?;
        if !w::compare(m,a.original_grant_refs.len(),0usize)?.is_gt(){return Err(shape::fail("missing original deadline grant"));}
        for original_ref in &a.original_grant_refs{
            m.visit("close.original_deadline_grant")?;
            let grants=shape::array(m,&binding,"authority_refs")?;
            if !shape::member(m,grants,&json!(original_ref))?{return Err(shape::fail("deadline grant absent original binding"));}
            let original=r.get(m,&c.key,original_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
            eligible&=deadline_grant_allows(m,&original,c,name,&selected.work["payload"]["customer"],trigger,product_id,at)?;
        }
        if eligible{
            let bytes=m.canonical_owned_representation(&arena,name,RepresentationKind::SourceDefinition)?;
            let earlier=match &chosen{None=>true,Some((old,_))=>shape::byte_order(m,bytes.bytes(),old.bytes())?.is_lt()};
            if earlier{chosen=Some((bytes,arena.clone_value(name,m)?));}
        }
    }
    chosen.map(|(_,name)|name).ok_or_else(||Error::new("SOURCE_AUTHORITY_DENIED","no individually sufficient original deadline slot"))
}
fn original_product_id<'p>(m:&mut CoreMeter,program:&'p SelectedProgram,selected:&FamilyContext<'_>)->Result<&'p str>{
    let mut product=None;
    for d in program.source_documents(){m.visit("close.original_product")?;if shape::same(m,&json!(d.reference()),&selected.target["product_ref"])?{product=Some(d.canonical_value());}}
    shape::text(m,product.ok_or_else(||shape::fail("close original product"))?,"product_id")
}
fn deadline_time(m:&mut CoreMeter,selected:&FamilyContext<'_>,accepted:&str)->Result<()> {
    let current=shape::text(m,&selected.family_head,"state")?;
    let deadline=if shape::same_text(m,current,"awaiting_anchor")?{
        if selected.family_head.get("window").is_some(){return Err(shape::fail("awaiting family has frozen window"));}
        let at=time(m,&selected.work,"accepted_at")?;
        let raw=shape::text(m,&selected.family["window"],"activation_deadline_us")?;
        let offset=arithmetic::integer(m,raw,false)?;
        if meter::cmp(m,&offset,&0.into())?.is_lt(){return Err(shape::fail("negative original activation deadline"));}
        meter::add(m,&at.into(),&offset)?
    }else{
        let original_at=shape::text(m,&selected.work,"accepted_at")?;
        let original=super::targets::frozen_window(m,&selected.family["window"],&selected.work_ref,original_at)?.ok_or_else(||Error::new("UNSUPPORTED_COMPOSITION","verified predecessor window replay pending"))?;
        eq(m,&selected.family_head["window"],&original)?;
        eq(m,&selected.family_head["anchor_work_ref"],&json!(selected.work_ref))?;
        time(m,&original,"accepted_by")?.into()
    };
    let accepted=Timestamp::parse_with_work(accepted,m)?.micros();
    if !meter::cmp(m,&accepted.into(),&deadline)?.is_gt(){return Err(Error::new("DEADLINE_NOT_EXPIRED","original close endpoint is inclusive"));}
    Ok(())
}
/// The historical grant is selected by the retained verification, not replaced
/// by today's current grant. Full retention replay/custody is still required.
fn retained_proof(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,selected:&FamilyContext<'_>,p:&Value,source_slot:&str)->Result<()> {
    let proof=shape::field(m,p,"close_proof")?;
    shape::closed(m,proof,&["attestation_ref","verification_ref"],&[])?;
    let att_ref=shape::reference(m,&proof["attestation_ref"])?;
    let verification_ref=shape::reference(m,&proof["verification_ref"])?;
    let att=r.get(m,&c.key,&att_ref,"workflow-fact-attestation","ledger-workflow-family-close-attestation/1")?;
    shape::closed(m,&att,&["schema","attestation_id","source","evidence_kind","purpose","assertion","subject","issued_at","supporting_evidence_refs"],&[])?;
    for (key,want) in [("evidence_kind","outcome"),("purpose","family_no_success"),("assertion","no_success")]{
        let actual=shape::text(m,&att,key)?;
        if !shape::same_text(m,actual,want)?{return Err(shape::fail("family close assertion variant"));}
    }
    eq(m,&att["source"],&json!(c.key.source))?;
    let id=shape::text(m,&att,"attestation_id")?;
    if !w::compare(m,id.len(),0usize)?.is_gt() || w::compare(m,id.len(),128usize)?.is_gt(){return Err(shape::fail("family close assertion id"));}
    for ch in id.chars(){m.scan_bytes(ch.len_utf8())?;if m.check("close.assertion_identifier",||ch<='\u{1f}'||ch=='\u{7f}')?{return Err(shape::fail("family close assertion id"));}}
    let subject=shape::field(m,&att,"subject")?;
    shape::closed(m,subject,&["logical_store_id","scope","original_binding_ref","target_ref","family_slot","expected_family_revision"],&[])?;
    for (key,want) in [
        ("logical_store_id",json!(c.key.logical_store_id)),("scope",json!(c.key.scope)),
        ("original_binding_ref",selected.target["binding_ref"].clone()),("target_ref",p["target_ref"].clone()),
        ("family_slot",p["family_slot"].clone()),("expected_family_revision",p["expected_revision"].clone()),
    ]{eq(m,&subject[key],&want)?;}
    let verification=r.get(m,&c.key,&verification_ref,"workflow-evidence-verification","ledger-workflow-evidence-verification/1")?;
    shape::closed(m,&verification,&["schema","attestation_ref","attestation_raw_sha256","attestation_digest","verified_source","verification_primitive","source_grant_ref","source_grant_revision","authenticated_principal_ref","trusted_received_at","subject_binding_refs","verification_evidence_refs"],&[])?;
    let a=&c.authorities[0];
    eq(m,&verification["attestation_ref"],&proof["attestation_ref"])?;
    eq(m,&verification["attestation_digest"],&json!(att_ref.digest))?;
    eq(m,&verification["verified_source"],&json!(c.key.source))?;
    eq(m,&verification["authenticated_principal_ref"],&json!(a.enrollment_ref))?;
    eq(m,&verification["verification_primitive"],&json!("authenticated-nominated-source-retention/1"))?;
    let binding_ref=shape::reference(m,&selected.target["binding_ref"])?;
    let bindings=body::ref_set(m,vec![binding_ref,selected.target_ref.clone()])?;
    eq(m,&verification["subject_binding_refs"],&json!(bindings))?;
    let raw=r.canonical(m,&att_ref)?;
    m.hash_bytes(raw.len())?;
    let digest=canonical::hex_with_work(&Sha256::digest(raw),m)?;
    eq(m,&verification["attestation_raw_sha256"],&json!(digest))?;
    let grant_ref=shape::reference(m,&verification["source_grant_ref"])?;
    let historical=r.get(m,&c.key,&grant_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
    let enrollment=r.get(m,&c.key,&a.enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
    eq(m,&historical["grant_id"],&enrollment["enrollment_id"])?;
    eq(m,&historical["original_ceiling_ref"],&json!(a.enrollment_ref))?;
    eq(m,&historical["revision"],&verification["source_grant_revision"])?;
    let revision=shape::text(m,&historical,"revision")?;
    positive_revision(m,revision)?;
    let issued=time(m,&att,"issued_at")?;
    let trusted=time(m,&verification,"trusted_received_at")?;
    let accepted=Timestamp::parse_with_work(&c.accepted_at,m)?.micros();
    if meter::cmp(m,&issued.into(),&trusted.into())?.is_gt() || meter::cmp(m,&trusted.into(),&accepted.into())?.is_gt(){return Err(shape::fail("close proof trusted times"));}
    let mut product=None;
    for document in program.source_documents(){m.visit("close.original_product")?;if shape::same(m,&json!(document.reference()),&selected.target["product_ref"])?{product=Some(document.canonical_value());}}
    let product=product.ok_or_else(||shape::fail("close original product"))?;
    let product_id=shape::text(m,product,"product_id")?;
    let trigger=shape::text(m,selected.family,"trigger_activity")?;
    let owner=InvocationToken::new();
    for mode in [EconomicMode::EvidenceRetain,EconomicMode::OutcomeClose]{
        authority::grant(m,&owner,&historical,&enrollment["ceiling"],c,&selected.work["payload"]["customer"],trigger,product_id,source_slot,&trusted.into(),mode)?;
        for original_ref in &a.original_grant_refs{
            m.visit("close.retention_original_grant")?;
            let original=r.get(m,&c.key,original_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
            authority::grant(m,&owner,&original,&enrollment["ceiling"],c,&selected.work["payload"]["customer"],trigger,product_id,source_slot,&trusted.into(),mode)?;
        }
    }
    // These sets establish provenance only. They cannot replace the explicit
    // typed pair or contribute a different source/subject authority.
    for (value,key) in [(&*att,"supporting_evidence_refs"),(&*verification,"verification_evidence_refs")]{
        let values=shape::array(m,value,key)?;
        if w::compare(m,values.len(),16usize)?.is_gt(){return Err(shape::fail("close proof provenance count"));}
        for (i,item) in values.iter().enumerate(){
            m.visit("close.provenance")?;
            let reference=shape::reference(m,item)?;
            for prior in &values[..i]{if shape::same(m,prior,item)?{return Err(shape::fail("duplicate close provenance"));}}
            r.provenance(m,&c.key,&reference)?;
        }
    }
    Ok(())
}

pub(super) fn execute<'inv,'raw>(program:&SelectedProgram,c:&'raw DraftContextData,mut p:WorkingValue,inputs:SuppliedRecords<'raw>,state:&DraftStateData,m:&mut CoreMeter,intrinsic:Option<(&'inv super::intrinsic_record::IntrinsicDataScope,&[&'inv super::intrinsic_record::IntrinsicRecord<'inv>])>)->Result<Construction<'inv,'raw>>{
    super::inventory::entry::context(m,c)?;
    if !program.uses_family_close_recipe(){return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","close original compiler recipe"));}
    shape::closed(m,&p,&["schema","target_ref","family_slot","expected_revision","reason","evidence_refs"],&["close_proof"])?;
    let reason=shape::text(m,&p,"reason")?;
    let no_success=shape::same_text(m,reason,"no_success")?;
    if !no_success {
        if !shape::same_text(m,reason,"deadline")?{return Err(shape::fail("close reason"));}
        if m.check("close.deadline_no_proof",||p.get("close_proof").is_some())?{return Err(shape::fail("deadline close forbids business proof"));}
    }else{shape::field(m,&p,"close_proof")?;}
    let received=Timestamp::parse_with_work(&c.received_at,m)?.micros();
    let accepted=Timestamp::parse_with_work(&c.accepted_at,m)?.micros();
    if meter::cmp(m,&received.into(),&accepted.into())?.is_gt(){return Err(shape::fail("close received after accepted"));}
    let revision=shape::text(m,&p,"expected_revision")?;
    let revision=positive_revision(m,revision)?;
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning close profile"))?;
    let normalized_revision=counter_value(m,&arena,revision)?;
    p.replace(m,"expected_revision",normalized_revision.value())?;
    let evidence=shape::array(m,&p,"evidence_refs")?;
    if w::compare(m,evidence.len(),16usize)?.is_gt(){return Err(shape::fail("close evidence count"));}
    p.normalize_set(m,"evidence_refs",false)?;
    let mut r=inputs.resolve(m,&c.key,&c.source_observation_inputs)?;
    if let Some((owner,evidence))=intrinsic{r=r.with_intrinsic_evidence(m,&c.key,owner,evidence)?;}
    let slot=shape::text(m,&p,"family_slot")?;
    let selected=family_context::select(m,&mut r,c,program,&p["target_ref"],slot,&EconomicStateData{retained_assertion:None,financial:state,capacity_heads:&[],supplier:None,app:None,current_party_authorizations:&[],observation:None,audit:None,initial_source_correspondences:&[]})?;
    eq(m,&selected.family_head["revision"],&p["expected_revision"])?;
    lifecycle(m,&selected.family_head)?;
    let chosen_slot=if no_success{
        let declaration=shape::field(m,selected.family,"no_success_close")?;
        shape::closed(m,declaration,&["close_source_slot"],&[])?;
        let source_slot=shape::field(m,declaration,"close_source_slot")?;
        arena.clone_value(source_slot,m)?
    }else{
        deadline_time(m,&selected,&c.accepted_at)?;
        deadline_slot(m,&mut r,c,program,&selected)?
    };
    let source_slot=chosen_slot.value().as_str().ok_or_else(||shape::fail("original close slot text"))?;
    let mode=if no_success{EconomicMode::OutcomeClose}else{EconomicMode::DeadlineClose};
    let resolved=selected.authority(m,&mut r,c,program,source_slot,mode)?;
    if no_success{retained_proof(m,&mut r,c,program,&selected,&p,source_slot)?;}
    for item in shape::array(m,&p,"evidence_refs")?{let reference=shape::reference(m,item)?;r.provenance(m,&c.key,&reference)?;}
    let next=meter::add(m,&revision.into(),&1u64.into())?;
    if meter::cmp(m,&next,&9_999_999_999_999_999_999u64.into())?.is_gt(){return Err(shape::fail("family revision overflow"));}
    let next_u64=next.to_u64().ok_or_else(||shape::fail("family revision overflow"))?;
    let next_value=counter_value(m,&arena,next_u64)?;
    let next_text=next_value.value().as_str().ok_or_else(||shape::fail("rendered close revision"))?;
    let constants=m.parse_owned_representation(&arena,br#"{"no_success":"closed_no_success","deadline":"expired"}"#,RepresentationKind::SourceDefinition)?;
    let resulting_state=&constants.value()[if no_success{"no_success"}else{"deadline"}];
    let prior=working_value::owning_reference(m,&arena,&selected.family_head_ref)?;
    let key=operation_key(m,&arena,&c.key)?;
    let at=arena.string_value(&c.accepted_at,m)?;
    let original=WorkingValue::copy(m,&selected.family_head)?;
    let new_body=original.with_fields(m,&[("revision",next_value.value()),("state",resulting_state),("closure_evidence_refs",&p["evidence_refs"]),("operation_key",key.value()),("accepted_at",at.value()),("prior_ref",prior.value())])?;
    let id=body::semantic_id(m,"wfamily_","workflow-family-state",&json!([selected.target_ref.id,slot,next_text]))?;
    let family=body::record_borrowed(m,&c.key,"workflow-family-state",id,&new_body)?;
    let authority=resolved.authority_document(m,None)?;
    let normalized_payload=p.bytes(m)?;
    m.scan_bytes(next_text.len())?;m.write_bytes(next_text.len())?;
    let proposed_heads=ProposedHeads{decision:vec![],family:vec![(selected.target_ref,slot.to_owned(),next_text.to_owned(),family.reference.clone())],chain:None};
    let mut introduced=resolved.introduced;introduced.push(authority);introduced.push(family);
    Ok(Construction{economic:EconomicConstruction{terms:resolved.terms,original_work:OriginalWorkPayload::Retained(selected.work)},original_reservations:None,normalized_payload,new_envelopes:introduced,reused_result:None,evaluation_steps:vec![],proposed_heads,claim_introductions:vec![],link_introductions:vec![],prospective_obligations:vec![],records:r,history_edges:vec![],app_effects:None,app_causal:None,current_capacity:None,candidate_heads:None,candidate_introductions:None,financial_observation:None,funding_effects:None,control_owners:[None,None,None,None,None]})
}
pub(super) fn counter_value(m:&mut CoreMeter,arena:&crate::canonical::audit_arena::AuditArena,n:u64)->Result<crate::canonical::audit_arena::AuditedValue>{
    // unsigned_string: requested digit capacities8+16+32, plus the <=20-byte
    // output for u64. Hold this transient lease until that String is destroyed.
    let _format=arena.reserve_external_payload(76,m)?;
    let text=w::unsigned_string(n.into(),m)?;
    arena.string_value(&text,m)
}
fn lifecycle(m:&mut CoreMeter,head:&Value)->Result<()> {
    let state=shape::text(m,head,"state")?;
    if !shape::same_text(m,state,"awaiting_anchor")? && !shape::same_text(m,state,"open")?{return Err(Error::new("FAMILY_CLOSED","close cannot erase a claimed or terminal family"));}
    for name in ["claim_ref","decision_ref"]{if m.check("close.unclaimed_state",||head.get(name).is_some())?{return Err(shape::fail("unclaimed family has claim or decision"));}}
    Ok(())
}
fn operation_key(m:&mut CoreMeter,arena:&crate::canonical::audit_arena::AuditArena,key:&OperationKey)->Result<crate::canonical::audit_arena::AuditedValue>{
    let store=arena.string_value(&key.logical_store_id,m)?;
    let a=arena.string_value(&key.scope[0],m)?;let b=arena.string_value(&key.scope[1],m)?;
    let scope=arena.array_from_values(&[a.value(),b.value()],m)?;
    let source=arena.string_value(&key.source,m)?;let operation=arena.string_value(&key.operation_id,m)?;
    arena.object_from_fields(&[("logical_store_id",store.value()),("scope",scope.value()),("source",source.value()),("operation_id",operation.value())],m)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_grant_requires_its_own_close_read_scope_and_never_submit(){
        let c=DraftContextData{key:OperationKey{logical_store_id:"store".into(),scope:["a".into(),"b".into()],source:"actual-source".into(),operation_id:"close".into()},actor:"actual-principal".into(),canonical_economic_namespace:"actual-namespace".into(),installation_mode:"real_local".into(),received_at:"2026-10-16T01:00:00.000000Z".into(),accepted_at:"2026-10-16T01:00:00.000000Z".into(),authorities:vec![],source_observation_inputs:vec![]};
        let grant=json!({"source":"actual-source","principal_id":"actual-principal","canonical_economic_namespace":"actual-namespace","operations":["outcome.close"],"permissions":["close","read"],"source_slots":["test:closer"],"customer_scope":["customer"],"activity_selectors":[{"names":["test:assessment"]}],"product_selectors":[{"names":["test:product"]}],"starts_at":"2026-10-16T00:00:00.000000Z","ends_at":"2026-10-16T02:00:00.000000Z"});
        let mut m=CoreMeter::new(65536).unwrap();
        let at=Timestamp::parse_with_work(&c.accepted_at,&mut m).unwrap().micros();
        assert!(deadline_grant_allows(&mut m,&grant,&c,&json!("test:closer"),&json!("customer"),"test:assessment","test:product",at).unwrap());
        for (field,value) in [("permissions",json!(["read","submit"])),("permissions",json!(["close"])),("source_slots",json!(["test:assessor"])),("source",json!("other-source")),("customer_scope",json!(["other-customer"]))]{
            let mut wrong=grant.clone();wrong[field]=value;
            assert!(!deadline_grant_allows(&mut m,&wrong,&c,&json!("test:closer"),&json!("customer"),"test:assessment","test:product",at).unwrap());
        }
        let end=time(&mut m,&grant,"ends_at").unwrap();
        assert!(!deadline_grant_allows(&mut m,&grant,&c,&json!("test:closer"),&json!("customer"),"test:assessment","test:product",end).unwrap());
        // This checks the actual paid per-grant predicate, not host custody,
        // role/assent or complete-set selection. It mints no authority record.
    }
    #[test]
    fn close_cannot_erase_claimed_terminal_or_inconsistent_unclaimed_state(){
        for state in ["awaiting_anchor","open"]{
            let mut m=CoreMeter::new(65536).unwrap();
            lifecycle(&mut m,&json!({"state":state})).unwrap();
        }
        for state in ["claimed","expired","closed_no_success","closed_by_root"]{
            let mut m=CoreMeter::new(65536).unwrap();
            assert_eq!(lifecycle(&mut m,&json!({"state":state})).unwrap_err().code,"FAMILY_CLOSED");
        }
        for field in ["claim_ref","decision_ref"]{
            let mut head=json!({"state":"open"});head[field]=json!({"malformed":"still forbidden"});
            let mut m=CoreMeter::new(65536).unwrap();
            assert_eq!(lifecycle(&mut m,&head).unwrap_err().code,"INTEGRITY_FAILURE");
        }
        let mut m=CoreMeter::new(65536).unwrap();
        assert_eq!(positive_revision(&mut m,"0002").unwrap(),2);
        for raw in ["0","-1","10000000000000000000"]{
            assert!(positive_revision(&mut m,raw).is_err());
        }
    }
}
