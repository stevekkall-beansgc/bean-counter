//! Fixed binary-owned verifier/descriptor DATA. No executor, origin, admission,
//! capacity or Ready capability is created by these documents.
use super::{CoreMeter,ExactRef,codec,shape};
use super::super::selected_program::{SelectedDocument,finish_owned_document};
use crate::{Result,canonical::{self,audit_arena::{AuditArena,AuditedValue,AuditedBytes,ExternalBufferLease}},validation_work::{self as w,OriginalAuditSession,RepresentationKind}};
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::mem;
#[path="audit_metadata_assets.rs"] mod assets;
const SOURCE:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/app-source-audit-metadata2.json");
const FINANCIAL:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/app-financial-audit-metadata2.json");
const MANIFEST:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/app-audit-contract-profile.json");
const DESCRIPTOR:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/app-compiler6-data.json");

pub struct AuditVerifierDocument{reference:ExactRef,body:AuditedValue,bytes:AuditedBytes,_ownership:ExternalBufferLease}
impl AuditVerifierDocument{
    pub fn reference(&self)->&ExactRef{&self.reference}
    pub fn canonical_value(&self)->&Value{self.body.value()}
    pub fn canonical_bytes(&self)->&[u8]{self.bytes.bytes()}
}
pub struct AppAuditMetadata{source:AuditVerifierDocument,financial:AuditVerifierDocument,descriptor:SelectedDocument,_manifest:AuditedValue,_manifest_bytes:AuditedBytes,_ownership:ExternalBufferLease}
impl AppAuditMetadata{
    pub fn source_verifier(&self)->&AuditVerifierDocument{&self.source}
    pub fn financial_verifier(&self)->&AuditVerifierDocument{&self.financial}
    pub fn compiler_descriptor(&self)->&SelectedDocument{&self.descriptor}
}
fn literal(m:&mut CoreMeter,v:&Value,k:&str,expected:&str)->Result<()>{let actual=shape::text(m,v,k)?;if !shape::same_text(m,actual,expected)?{return Err(shape::fail("binary audit metadata literal"));}Ok(())}
fn parse(m:&mut CoreMeter,a:&AuditArena,raw:&[u8])->Result<(AuditedValue,AuditedBytes)>{
    let v=m.parse_owned_representation(a,raw,RepresentationKind::SourceDefinition)?;
    codec::validate_selected_types(m,v.value())?;
    let bytes=m.canonical_owned_representation(a,v.value(),RepresentationKind::SourceDefinition)?;
    if !shape::same_bytes(m,bytes.bytes(),raw)?{return Err(shape::fail("binary audit metadata is not canonical"));}Ok((v,bytes))
}
fn prefix(m:&mut CoreMeter,p:&str,h:&str)->Result<String>{
    let n=w::usize_add(m,p.len(),h.len())?;
    m.write_bytes(mem::size_of::<String>())?;
    let mut out=String::new();out.try_reserve_exact(n).map_err(|_|shape::fail("audit metadata identity allocation"))?;
    if w::compare(m,out.capacity(),n)?.is_gt(){return Err(shape::fail("audit metadata identity capacity"));}
    m.scan_bytes(p.len())?;m.scan_bytes(h.len())?;m.write_bytes(n)?;out.push_str(p);out.push_str(h);Ok(out)
}
fn raw_hex(m:&mut CoreMeter,raw:&[u8])->Result<String>{
    // Every actual temporary construction is prepaid, including allocation
    // refusals. The caller's retained lease covers this live overlap; it is
    // not a debit credit for repeated per-artifact hashes/hex producers.
    m.write_bytes(mem::size_of::<Sha256>()+32+mem::size_of::<String>())?;
    m.hash_bytes(raw.len())?;canonical::hex_with_work(&Sha256::digest(raw),m)
}
fn raw_asset<'a>(m:&mut CoreMeter,path:&str)->Result<&'a [u8]>{
    for(p,b)in assets::ARTIFACTS{m.visit("audit.metadata_artifact_lookup")?;if shape::same_text(m,path,p)?{return Ok(b);}}
    Err(shape::fail("audit metadata artifact is not binary owned"))
}
fn check_artifacts(m:&mut CoreMeter,rows:&[Value],manifest:bool)->Result<()>{
    if w::compare(m,rows.len(),64usize)?.is_gt()||w::compare(m,rows.len(),0usize)?.is_eq(){return Err(shape::fail("audit metadata artifact count"));}
    for row in rows{m.visit("audit.metadata_artifact")?;shape::closed(m,row,&["path","sha256"],&[])?;
        let path=shape::text(m,row,"path")?;let expected=shape::text(m,row,"sha256")?;
        let raw=raw_asset(m,path)?;let hash=raw_hex(m,raw)?;
        if manifest{let hash=prefix(m,"sha256:",&hash)?;if !shape::same_text(m,expected,&hash)?{return Err(shape::fail("audit contract artifact raw hash"));}}
        else if !shape::same_text(m,expected,&hash)?{return Err(shape::fail("audit verifier artifact raw hash"));}
    }Ok(())
}
fn verifier(m:&mut CoreMeter,a:&AuditArena,raw:&[u8],profile:&str)->Result<AuditVerifierDocument>{
    // Exact Ref payload, retained wrapper and the live SHA/hex/prefix/schema
    // construction overlap. Body trees/bytes have their own arena owners.
    let n=mem::size_of::<AuditVerifierDocument>()+mem::size_of::<Sha256>()+5*mem::size_of::<String>()+"workflow-authority-audit-verifier".len()+69+71+64+71+"ledger-authority-audit-verifier/2".len()+32+5*mem::size_of::<&[u8]>();
    let owner=a.reserve_external_payload(n,m)?;m.write_bytes(mem::size_of::<AuditVerifierDocument>())?;
    let(body,bytes)=parse(m,a,raw)?;
    shape::closed(m,body.value(),&["schema","profile","algorithm","resources","artifacts","origin_recipes"],&[])?;
    literal(m,body.value(),"schema","ledger-authority-audit-verifier/2")?;
    literal(m,body.value(),"profile",profile)?;literal(m,body.value(),"algorithm","verified-introduction-fixed-point/1")?;literal(m,body.value(),"resources","workflow-profile-b-origin-audit/1")?;
    let artifacts=shape::array(m,body.value(),"artifacts")?;check_artifacts(m,artifacts,false)?;
    let rows=shape::array(m,body.value(),"origin_recipes")?;
    if w::compare(m,rows.len(),64usize)?.is_gt()||w::compare(m,rows.len(),0usize)?.is_eq(){return Err(shape::fail("audit metadata recipe count"));}
    for row in rows{m.visit("audit.metadata_origin_recipe")?;shape::closed(m,row,&["registry_digest","operation_kind","payload_schema","normalization_profile","closure_profile","variant"],&[])?;for k in ["registry_digest","operation_kind","payload_schema","normalization_profile","closure_profile","variant"]{shape::text(m,row,k)?;}}
    let schema=a.string_value("ledger-authority-audit-verifier/2",m)?;let schema=m.canonical_owned_representation(a,schema.value(),RepresentationKind::SourceDefinition)?;
    m.write_bytes(mem::size_of::<Sha256>()+5*mem::size_of::<&[u8]>())?;
    let mut h=Sha256::new();let domain=b"bean-counter/workflow-document/1\0";m.hash_bytes(domain.len())?;h.update(domain);
    for part in [b"[".as_slice(),schema.bytes(),b",".as_slice(),bytes.bytes(),b"]".as_slice()]{m.visit("audit.metadata_document_hash_part")?;m.hash_bytes(part.len())?;h.update(part);}
    m.write_bytes(32+mem::size_of::<String>())?;
    let hex=canonical::hex_with_work(&h.finalize(),m)?;let digest=prefix(m,"sha256:",&hex)?;let id=prefix(m,"wdoc_",&hex)?;
    let kind="workflow-authority-audit-verifier";m.scan_bytes(kind.len())?;m.write_bytes(kind.len())?;
    Ok(AuditVerifierDocument{reference:ExactRef{kind:kind.to_owned(),id,digest},body,bytes,_ownership:owner})
}
fn descriptor_asset_digest(m:&mut CoreMeter,a:&AuditArena,descriptor:&Value,field:&str,path:&str)->Result<()>{
    let raw=raw_asset(m,path)?;
    let value=m.parse_owned_representation(a,raw,RepresentationKind::SourceDefinition)?;
    codec::validate_selected_types(m,value.value())?;
    let bytes=m.canonical_owned_representation(a,value.value(),RepresentationKind::SourceDefinition)?;
    let hex=raw_hex(m,bytes.bytes())?;let digest=prefix(m,"sha256:",&hex)?;
    literal(m,descriptor,field,&digest)
}
/// No caller-selected recipes/artifacts, saved descriptor, trusted flag or
/// execution capability is accepted or returned. Fresh and original callers
/// must still establish the complete public-origin and resource qualifications.
pub fn produce_app_audit_metadata(s:&mut OriginalAuditSession)->Result<AppAuditMetadata>{
    let result=(||{
        let a=s.arena().shared_handle();let m=s.meter_mut();m.require_original_data_selection()?;
        let n=mem::size_of::<AppAuditMetadata>()+mem::size_of::<Sha256>()+4*mem::size_of::<String>()+64+71+71+32;
        let owner=a.reserve_external_payload(n,m)?;m.write_bytes(mem::size_of::<AppAuditMetadata>())?;
        let source=verifier(m,&a,SOURCE,"ledger-authority-origin-audit/1")?;
        let financial=verifier(m,&a,FINANCIAL,"ledger-financial-origin-audit/1")?;
        let(manifest,manifest_bytes)=parse(m,&a,MANIFEST)?;
        shape::closed(m,manifest.value(),&["schema","profile","scope","encoding_files"],&[])?;
        literal(m,manifest.value(),"schema","ledger-workflow-builtin-contract-profile/2")?;literal(m,manifest.value(),"profile","app-party-commitment/1")?;literal(m,manifest.value(),"scope","selected-app-economic")?;
        let files=shape::array(m,manifest.value(),"encoding_files")?;
        if !w::compare(m,files.len(),assets::ARTIFACTS.len())?.is_eq(){return Err(shape::fail("audit contract complete binary membership"));}
        check_artifacts(m,files,true)?;
        let(descriptor,_canonical)=parse(m,&a,DESCRIPTOR)?;
        let manifest_hex=raw_hex(m,manifest_bytes.bytes())?;let manifest_digest=prefix(m,"sha256:",&manifest_hex)?;
        literal(m,descriptor.value(),"contract_manifest_digest",&manifest_digest)?;
        descriptor_asset_digest(m,&a,descriptor.value(),"limits_digest","runtime-support/limits.json")?;
        descriptor_asset_digest(m,&a,descriptor.value(),"mode_matrix_digest","party-commitment-first-01/profile.json")?;
        let descriptor=finish_owned_document(m,descriptor,false)?;
        Ok(AppAuditMetadata{source,financial,descriptor,_manifest:manifest,_manifest_bytes:manifest_bytes,_ownership:owner})
    })();if result.is_err(){s.invalidate();}result
}

/// Exact selected-recipe metadata comparison, not validation of its claimed
/// origin prefix. Old synthetic metadata is deliberately a different identity.
pub(super) fn selected_catalog(m:&mut CoreMeter,program:&super::super::selected_program::SelectedProgram)->Result<bool>{
    match program.execution_registry(){Some(registry)=>shape::same_text(m,registry,"sha256:762b708044ffad44717074a9b3456b91ebbe62d7a12d76cfa80d6a37ecbfe276"),None=>Ok(false)}
}
fn check_exact(m:&mut CoreMeter,value:&Value,expected:&[u8])->Result<()>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning exact audit catalog comparison"))?;
    codec::validate_selected_types(m,value)?;
    let bytes=m.canonical_owned_representation(&arena,value,RepresentationKind::SourceDefinition)?;
    if !shape::same_bytes(m,bytes.bytes(),expected)?{return Err(shape::fail("verifier differs from exact code-owned selected catalog"));}Ok(())
}
pub(super) fn check_source(m:&mut CoreMeter,value:&Value)->Result<()>{check_exact(m,value,SOURCE)}
// Fixed selected binary template for the containing source derivation. This
// is DATA, not a verifier/capability constructor or a caller-supplied bound.
pub(super) fn source_template()->&'static [u8]{SOURCE}

pub(super) fn check_financial(m:&mut CoreMeter,value:&Value)->Result<()>{check_exact(m,value,FINANCIAL)}

#[cfg(test)] mod tests{
    use super::*;
    #[test] fn owning_catalogs_match_existing_document_identity_and_release_all_storage(){
        let arena=AuditArena::new().unwrap();let baseline=arena.usage().unwrap().live_bytes;
        let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let metadata=produce_app_audit_metadata(&mut s).unwrap();
        for document in [metadata.source_verifier(),metadata.financial_verifier()]{
            let digest=crate::policy::workflow::document_digest("ledger-authority-audit-verifier/2",document.canonical_value()).unwrap();
            assert_eq!(document.reference().digest,digest);
            assert_eq!(document.canonical_bytes(),canonical::CanonicalBytes::from_value(document.canonical_value()).unwrap().as_slice());
        }
        check_source(s.meter_mut(),metadata.source_verifier().canonical_value()).unwrap();
        check_financial(s.meter_mut(),metadata.financial_verifier().canonical_value()).unwrap();
        let mut wrong=metadata.financial_verifier().canonical_value().clone();wrong["origin_recipes"][0]["closure_profile"]=Value::String("ledger-economic-resolved-inputs/8".into());
        assert!(check_financial(s.meter_mut(),&wrong).is_err());
        let descriptor=metadata.compiler_descriptor();
        assert_eq!(descriptor.reference().digest(),crate::policy::workflow::document_digest("ledger-workflow-compiler-descriptor/6",descriptor.canonical_value()).unwrap());
        assert_ne!(descriptor.reference().digest(),"sha256:f232fc8619efbde13546b3cf6f514ac98e495f0f4de77d1ffc70dd1f79fd057b");
        assert!(arena.usage().unwrap().live_bytes>baseline);
        drop(metadata);assert_eq!(arena.usage().unwrap().live_bytes,baseline);
        s.invalidate();assert!(produce_app_audit_metadata(&mut s).is_err());
    }
}
