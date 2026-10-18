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

#[cfg(test)]mod tests{
 use super::*;
 use crate::{canonical::{self,audit_arena::AuditArena},policy::workflow::{DefinitionRef,document_digest,selected_program::*}};
 use serde_json::{Value,json};
 #[test]
 fn actual_first_policy_local_compile_load_and_incoming_intent(){
  let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
  let support=SelectedCompilerSupport::local_execution_with_session(&mut s).unwrap();
  let data:Value=serde_json::from_slice(include_bytes!("fixtures/local-first-policy-input.json")).unwrap();
  let mut docs=data["documents"].as_array().unwrap().clone();
  let policy=docs.iter_mut().find(|v|v["schema"]=="ledger-workflow-policy/4").unwrap();
  policy["compiler_ref"]=serde_json::to_value(support.reference()).unwrap();
  let digest=document_digest("ledger-workflow-policy/4",policy).unwrap();
  let root=DefinitionRef::new("workflow-policy",&format!("wdoc_{}",&digest[7..]),&digest).unwrap();
  docs.push(support.canonical_value().clone());
  let input=||SelectedSourceInput{documents:docs.iter().map(|d|canonical::CanonicalBytes::from_value(d).unwrap().into_vec()).collect(),root_policy_ref:root.clone()};
  let program=compile_selected_publication(input(),&support,&mut s).unwrap();
  assert!(program.uses_local_execution_recipe());
  let plan=program.compiled_plan().canonical_bytes().to_vec();let plan_ref=program.compiled_plan().reference().clone();
  let source=SelectedSourceInput{documents:program.source_documents().iter().map(|d|d.canonical_bytes().to_vec()).collect(),root_policy_ref:program.root_policy_ref().clone()};
  let loaded=load_local_selected_execution(source,&plan,&plan_ref,&mut s).unwrap();
  assert_eq!(loaded.compiled_plan().canonical_bytes(),program.compiled_plan().canonical_bytes());
  assert_eq!(loaded.static_analysis().compile_work,None);
  let normalized=normalize_local_publication_input(input(),&loaded,&mut s).unwrap();
  assert_eq!(normalized.root_policy_ref(),program.root_policy_ref());
  assert_eq!(normalized.source_documents().len(),program.source_documents().len());
  for actual in normalized.source_documents(){assert!(program.source_documents().iter().any(|p|p.reference()==actual.reference()&&p.canonical_bytes()==actual.canonical_bytes()));}
  let mut changed=input();let mut value:Value=canonical::parse(&changed.documents[5]).unwrap();value["version"]=json!("2");changed.documents[5]=canonical::CanonicalBytes::from_value(&value).unwrap().into_vec();
  assert!(normalize_local_publication_input(changed,&loaded,&mut s).is_err());
 }
}
