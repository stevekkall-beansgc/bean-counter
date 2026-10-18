//! The original source.enroll record producer shared with chronological replay.
//! These outputs establish construction correspondence only. They do not prove
//! the ceiling, customer, evidence, root, custody or as-of authority recipes.
use super::{CoreMeter,body,data::{ExactRef,OperationKey},records,shape,working_value::owning_reference};
use crate::{Result,canonical::audit_arena::{AuditArena,AuditedBytes,ExternalBufferLease},validation_work::{OriginalAuditSession,RepresentationKind}};
use serde_json::Value;
use std::mem;

pub struct InitialSourceRecord { reference:ExactRef, body:AuditedBytes, _ownership:ExternalBufferLease }
impl InitialSourceRecord {
    pub fn reference(&self)->&ExactRef{&self.reference}
    pub fn canonical_body(&self)->&[u8]{self.body.bytes()}
}
pub struct InitialSourceRecords {
    enrollment:InitialSourceRecord,grant:InitialSourceRecord,state:InitialSourceRecord,
    _ownership:ExternalBufferLease,
}
// Exact producer output correspondence, not historical authority or custody.
// No Clone/Serde/public constructor; the host associates it with a completed
// original separately. Full bodies and every consumer-specific check remain.
const CORRESPONDENCE_BYTES:usize=3*71+72;
pub struct InitialSourceCorrespondence {
    enrollment_digest:String,grant_digest:String,state_digest:String,state_id:String,
    boxed_once:bool,_ownership:ExternalBufferLease,
}
impl InitialSourceCorrespondence {
    /// The original producer prepaid TWO carrier controls plus its four actual
    /// string payloads: inline controls plus this heap move. No string is copied
    /// during the move. There is no caller-selected allowance,
    /// naked Box allocation, second pool or repeated reboxing credit.
    pub fn into_boxed(mut self,s:&mut OriginalAuditSession)->Result<Box<Self>>{
        let result=(||{
            let a=s.arena().shared_handle();let m=s.meter_mut();
            a.require_external_owner(&self._ownership,m)?;
            m.visit("source.correspondence_single_box_handoff")?;
            if self.boxed_once{return Err(shape::fail("source correspondence already boxed"));}
            self.boxed_once=true;
            m.scan_bytes(mem::size_of::<Self>())?;
            m.write_bytes(mem::size_of::<Self>()+mem::size_of::<Box<Self>>())?;
            Ok(Box::new(self))
        })();if result.is_err(){s.invalidate();}result
    }
    pub(super) fn checked_parts<'a>(&'a self,a:&AuditArena,m:&mut CoreMeter)->Result<[&'a str;4]>{
        a.require_external_owner(&self._ownership,m)?;
        m.write_bytes(4*mem::size_of::<&str>())?;
        Ok([&self.enrollment_digest,&self.grant_digest,&self.state_digest,&self.state_id])
    }
}
impl InitialSourceRecords {
    pub fn enrollment(&self)->&InitialSourceRecord{&self.enrollment}
    pub fn grant(&self)->&InitialSourceRecord{&self.grant}
    pub fn state(&self)->&InitialSourceRecord{&self.state}
    pub fn correspondence(&self,s:&mut OriginalAuditSession)->Result<InitialSourceCorrespondence>{
        let result=(||{
            let a=s.arena().shared_handle();let m=s.meter_mut();
            a.require_external_owner(&self._ownership,m)?;
            let owner=a.reserve_external_payload(2*mem::size_of::<InitialSourceCorrespondence>()+CORRESPONDENCE_BYTES,m)?;
            // Keep the producer's typed strings. Reconstructing a str from a
            // byte array later would run a redundant UTF8 decoder. Their exact
            // lengths follow from SHA256 and the fixed wsource_ prefix, not
            // supplied bounds. This owner also covers the single Box handoff.
            let enrollment_digest=copy(m,&self.enrollment.reference.digest)?;
            let grant_digest=copy(m,&self.grant.reference.digest)?;
            let state_digest=copy(m,&self.state.reference.digest)?;
            let state_id=copy(m,&self.state.reference.id)?;
            m.write_bytes(2*mem::size_of::<InitialSourceCorrespondence>())?;
            Ok(InitialSourceCorrespondence{enrollment_digest,grant_digest,state_digest,state_id,boxed_once:false,_ownership:owner})
        })();if result.is_err(){s.invalidate();}result
    }
    /// Every row carries its own Ref/control owner. Splitting the triple cannot
    /// detach those leases or require another canonical-body copy/hash.
    pub fn into_parts(self)->[InitialSourceRecord;3]{
        let Self{enrollment,grant,state,_ownership}=self;
        [enrollment,grant,state]
    }
}
fn copy(m:&mut CoreMeter,s:&str)->Result<String>{m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
fn row_owner(m:&mut CoreMeter,a:&AuditArena)->Result<ExternalBufferLease>{
    a.reserve_external_payload(mem::size_of::<InitialSourceRecord>()+64+128+71,m)
}
fn document(m:&mut CoreMeter,a:&AuditArena,kind:&str,v:&Value)->Result<InitialSourceRecord>{
    let ownership=row_owner(m,a)?;
    // Match the original host's 262144-byte emitter before allocation, rather
    // than serializing with the generic 4MiB record cap and checking afterward.
    let bytes=m.canonical_new_body_representation(a,v,RepresentationKind::SourceDefinition)?;
    m.new_body(bytes.bytes().len())?;
    let schema=shape::field(m,v,"schema")?;
    let schema=m.canonical_owned_representation(a,schema,RepresentationKind::SourceDefinition)?;
    let digest=records::hash_parts(m,"workflow-document",&[schema.bytes(),bytes.bytes()],true)?;
    m.scan_bytes(64)?;m.write_bytes(69)?;
    let id=format!("wdoc_{}",&digest[7..]);
    Ok(InitialSourceRecord{reference:ExactRef{kind:copy(m,kind)?,id,digest},body:bytes,_ownership:ownership})
}
/// SAME source administration output recipe, with real captured key/actor/time
/// supplied by its original host verifier. No DraftContext/economic authority,
/// saved result list, completed-origin boolean or numerical credit is accepted.
pub fn produce_initial_source_records(
    key:&OperationKey,payload:&Value,ceiling:&Value,installation_mode:&str,actor:&str,accepted_at:&str,
    root_ref:&ExactRef,session:&mut OriginalAuditSession,
)->Result<InitialSourceRecords>{
    let result=produce(key,payload,ceiling,installation_mode,actor,accepted_at,root_ref,session);
    if result.is_err(){session.invalidate();}result
}
fn produce(
    key:&OperationKey,p:&Value,ceiling:&Value,mode:&str,actor:&str,at:&str,root:&ExactRef,
    s:&mut OriginalAuditSession,
)->Result<InitialSourceRecords>{
    let a=s.arena().shared_handle();let m=s.meter_mut();
    // Retained Ref strings, return/move controls and hash/ID temporaries coexist
    // with the three independently owning canonical outputs. No input clone.
    let payload=mem::size_of::<InitialSourceRecords>()
        +8*mem::size_of::<String>()+4*(64+71+128)+mem::size_of::<[&Value;16]>();
    let ownership=a.reserve_external_payload(payload,m)?;
    let constants=m.parse_owned_representation(&a,br#"{"enrollment":"ledger-workflow-source-enrollment/1","grant":"ledger-workflow-source-grant/1","state":"ledger-workflow-source-state/1","one":"1","active":"active"}"#,RepresentationKind::SourceDefinition)?;
    if !shape::same_text(m,mode,"real_local")?&&!shape::same_text(m,mode,"isolated_synthetic")?{return Err(shape::fail("initial source production mode"));}
    let mode=a.string_value(mode,m)?;
    let schema=shape::text(m,p,"schema")?;
    if !shape::same_text(m,schema,"ledger-workflow-source-enroll/1")?{return Err(shape::fail("initial source production payload"));}
    let id=shape::field(m,p,"enrollment_id")?;let customers=shape::field(m,p,"customer_refs")?;let evidence=shape::field(m,p,"evidence_ref")?;
    let actor=a.string_value(actor,m)?;let at=a.string_value(at,m)?;let root=owning_reference(m,&a,root)?;
    let store=a.string_value(&key.logical_store_id,m)?;let source=a.string_value(&key.source,m)?;let opid=a.string_value(&key.operation_id,m)?;
    let left=a.string_value(&key.scope[0],m)?;let right=a.string_value(&key.scope[1],m)?;
    let scope=a.array_from_values(&[left.value(),right.value()],m)?;
    let key_value=a.object_from_fields(&[("logical_store_id",store.value()),("scope",scope.value()),("source",source.value()),("operation_id",opid.value())],m)?;
    let c=constants.value();
    let value=a.object_from_fields(&[("schema",&c["enrollment"]),("scope",scope.value()),("enrollment_id",id),("ceiling",ceiling),("customer_refs",customers),("evidence_ref",evidence),("authority_ref",root.value()),("installation_mode",mode.value()),("enrolled_by",actor.value()),("enrolled_at",at.value())],m)?;
    let enrollment=document(m,&a,"workflow-source-enrollment",value.value())?;
    let enrollment_ref=owning_reference(m,&a,enrollment.reference())?;
    let mut fields=[("",&Value::Null);16];
    // Fixed field selection is the actual original copy recipe. Missing fields
    // fail here; validation/normalization remains the caller's SAME ceiling path.
    for(i,name)in ["activity_selectors","canonical_economic_namespace","customer_scope","ends_at","link_selectors","operations","permissions","principal_id","product_selectors","source","source_slots","starts_at"].iter().enumerate(){
        m.visit("source.produce_original_grant_field")?;fields[i]=(*name,shape::field(m,ceiling,name)?);
    }
    fields[12]=("schema",&c["grant"]);fields[13]=("grant_id",id);fields[14]=("revision",&c["one"]);fields[15]=("original_ceiling_ref",enrollment_ref.value());
    let value=a.object_from_fields(&fields,m)?;let grant=document(m,&a,"workflow-source-grant",value.value())?;
    let grant_ref=owning_reference(m,&a,grant.reference())?;
    let value=a.object_from_fields(&[("schema",&c["state"]),("enrollment_ref",enrollment_ref.value()),("revision",&c["one"]),("status",&c["active"]),("current_grant_ref",grant_ref.value()),("operation_key",key_value.value()),("actor",actor.value()),("accepted_at",at.value())],m)?;
    let tuple=a.array_from_values(&[store.value(),scope.value(),id,&c["one"]],m)?;
    let state_ownership=row_owner(m,&a)?;
    let state_id=body::semantic_id(m,"wsource_","workflow-source-state",tuple.value())?;
    let bytes=m.canonical_new_body_representation(&a,value.value(),RepresentationKind::SourceDefinition)?;m.new_body(bytes.bytes().len())?;
    let kind=a.string_value("workflow-source-state",m)?;
    let kind=m.canonical_owned_representation(&a,kind.value(),RepresentationKind::SourceDefinition)?;
    let encoded_scope=m.canonical_owned_representation(&a,scope.value(),RepresentationKind::SourceDefinition)?;
    let digest=records::hash_parts(m,"workflow-record",&[kind.bytes(),encoded_scope.bytes(),bytes.bytes()],true)?;
    let state=InitialSourceRecord{reference:ExactRef{kind:copy(m,"workflow-source-state")?,id:state_id,digest},body:bytes,_ownership:state_ownership};
    // The owner admission includes the stack-to-result move of all controls.
    m.write_bytes(mem::size_of::<InitialSourceRecords>())?;
    Ok(InitialSourceRecords{enrollment,grant,state,_ownership:ownership})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn shared_original_source_producer_matches_retained_triple_and_refuses_missing_fields(){
        let(_,c,_,records)=super::super::first_work_tests::test_inputs();
        let enrollment=records.iter().find(|r|r.reference==c.authorities[0].enrollment_ref).unwrap();
        let enrollment_value:Value=serde_json::from_slice(&enrollment.canonical_body).unwrap();
        let state=records.iter().find(|r|r.reference==c.authorities[0].current_state_ref).unwrap();
        let state_value:Value=serde_json::from_slice(&state.canonical_body).unwrap();
        let grant=records.iter().find(|r|r.reference==c.authorities[0].current_grant_ref).unwrap();
        let key:OperationKey=serde_json::from_value(state_value["operation_key"].clone()).unwrap();
        let root:ExactRef=serde_json::from_value(enrollment_value["authority_ref"].clone()).unwrap();
        let payload=json!({"schema":"ledger-workflow-source-enroll/1","enrollment_id":enrollment_value["enrollment_id"],"customer_refs":enrollment_value["customer_refs"],"evidence_ref":enrollment_value["evidence_ref"]});
        let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline=arena.usage().unwrap();
        let actual=produce_initial_source_records(&key,&payload,&enrollment_value["ceiling"],enrollment_value["installation_mode"].as_str().unwrap(),enrollment_value["enrolled_by"].as_str().unwrap(),enrollment_value["enrolled_at"].as_str().unwrap(),&root,&mut s).unwrap();
        for(got,want)in [(actual.enrollment(),enrollment),(actual.grant(),grant),(actual.state(),state)]{
            assert_eq!(got.reference(),&want.reference);assert_eq!(got.canonical_body(),want.canonical_body);
        }
        let facts=actual.correspondence(&mut s).unwrap().into_boxed(&mut s).unwrap();
        let views:Vec<_>=records.iter().map(|r|super::super::data::EconomicRecordView{reference:&r.reference,canonical_body:&r.canonical_body}).collect();
        // Exact producer correspondence cannot replace consuming customer,
        // clock, store/scope or installation-mode checks. Each call is a new
        // original owner in the SAME attempt; historical authority is absent.
        for case in 0..5{
            let mut context=c.clone();let mut customer=enrollment_value["ceiling"]["customer_scope"][0].clone();
            if case==1{context.accepted_at="2026-10-12T00:00:00.000000Z".into();}
            if case==2{customer=json!("different-customer");}
            if case==3{context.installation_mode="real_local".into();}
            if case==4{context.key.logical_store_id="different-store".into();}
            let mut consumer=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut meter=consumer.meter_mut().original_economic_meter().unwrap();
            let mut pool=super::super::records::Records::borrowed_app(&mut meter,&arena,&context.key,&views,&[]).unwrap();
            pool.install_initial_source_outputs(&mut meter,&[&facts]).unwrap();
            let e=pool.get(&mut meter,&context.key,&enrollment.reference,"workflow-source-enrollment","ledger-workflow-source-enrollment/1").unwrap();
            let g=pool.get(&mut meter,&context.key,&grant.reference,"workflow-source-grant","ledger-workflow-source-grant/1").unwrap();
            let st=pool.get(&mut meter,&context.key,&state.reference,"workflow-source-state","ledger-workflow-source-state/1").unwrap();
            let invocation=super::super::representation::InvocationToken::new();
            let result=super::super::inventory::source::birth(&mut meter,&mut pool,&context,&invocation,&e,&st,&g,&customer,&enrollment.reference,&state.reference,&grant.reference);
            if case==0{result.unwrap();}else{assert!(result.is_err(),"consumer case {case} was bypassed");}
        }
        let other=AuditArena::new().unwrap();let mut foreign=OriginalAuditSession::for_resource_profile_v2(&other).unwrap();
        assert!(facts.checked_parts(&other,foreign.meter_mut()).is_err());
        let rows=actual.into_parts();
        assert!(arena.usage().unwrap().live_bytes>baseline.live_bytes);
        assert_eq!(rows[0].reference(),&enrollment.reference);
        assert_eq!(rows[2].canonical_body(),state.canonical_body);
        drop(rows);
        let mut rebox=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let moved=*facts;
        assert!(moved.into_boxed(&mut rebox).is_err());
        assert_eq!(arena.usage().unwrap().live_bytes,baseline.live_bytes);
        let mut incomplete=enrollment_value["ceiling"].clone();incomplete.as_object_mut().unwrap().remove("source");
        assert!(produce_initial_source_records(&key,&payload,&incomplete,"isolated_synthetic","fixture-admin","2026-10-13T00:00:05.000000Z",&root,&mut s).is_err());
        assert!(s.meter_mut().visit("test.invalid_source_production_cannot_resume").is_err());
    }
}
