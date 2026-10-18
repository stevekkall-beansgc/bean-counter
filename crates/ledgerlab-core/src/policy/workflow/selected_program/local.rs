//! Exact local recipe document DATA. This producer grants no execution or
//! financial preparation capability and accepts no caller-selected catalog.
use super::{compiler::finish_owned_document,SelectedDocument};
use crate::{Result,validation_work::{OriginalAuditSession,RepresentationKind}};
use crate::policy::workflow::transition_constructor::{codec,shape};
use sha2::{Digest,Sha256};
#[path="local_assets.rs"] mod assets;
pub(super) const REGISTRY:&str="sha256:d0a21becb08150a781770cfef80b5521eeb93c3b45aa9ebdab23c800ef44ee4a";
pub(super) const DESCRIPTOR:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-compiler6-data.json");
const MANIFEST:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-contract-profile.json");

pub fn local_compiler_descriptor_with_session(s:&mut OriginalAuditSession)->Result<SelectedDocument>{
 let result=(||{
  let a=s.arena().shared_handle();let m=s.meter_mut();
  let manifest=m.parse_owned_representation(&a,MANIFEST,RepresentationKind::SourceDefinition)?;
  codec::validate_selected_types(m,manifest.value())?;
  let bytes=m.canonical_owned_representation(&a,manifest.value(),RepresentationKind::SourceDefinition)?;
  if !super::support::same_bytes(m,bytes.bytes(),MANIFEST)?{return Err(shape::fail("local defining manifest canonicality"));}
  let rows=shape::array(m,manifest.value(),"encoding_files")?;
  if !crate::validation_work::compare(m,rows.len(),assets::ARTIFACTS.len())?.is_eq(){return Err(shape::fail("local defining asset membership"));}
  for (row,(path,raw)) in rows.iter().zip(assets::ARTIFACTS){
   m.visit("local.descriptor_defining_asset")?;
   let actual_path=shape::text(m,row,"path")?;if !shape::same_text(m,actual_path,path)?{return Err(shape::fail("local defining artifact path"));}
   m.hash_bytes(raw.len())?;let digest=format!("sha256:{}",crate::canonical::hex_with_work(&Sha256::digest(raw),m)?);
   let actual_hash=shape::text(m,row,"sha256")?;if !shape::same_text(m,actual_hash,&digest)?{return Err(shape::fail("local defining artifact bytes"));}
  }
  let body=m.parse_owned_representation(&a,DESCRIPTOR,RepresentationKind::SourceDefinition)?;
  codec::validate_selected_types(m,body.value())?;
  m.hash_bytes(bytes.bytes().len())?;let digest=format!("sha256:{}",crate::canonical::hex_with_work(&Sha256::digest(bytes.bytes()),m)?);
  let actual_manifest=shape::text(m,body.value(),"contract_manifest_digest")?;if !shape::same_text(m,actual_manifest,&digest)?{return Err(shape::fail("local descriptor defining manifest"));}
  finish_owned_document(m,body,false)
 })();if result.is_err(){s.invalidate();}result
}
