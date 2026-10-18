//! Prepare public local-publication INPUT with the production source compiler.
//! This does not access a store, retain authority, or construct a bill/receipt.
use ledgerlab_core::{canonical::{self,audit_arena::AuditArena},policy::workflow::{
    DefinitionRef,selected_program::{SelectedCompilerSupport,SelectedSourceInput,compile_selected_publication}},
    validation_work::OriginalAuditSession};
use serde_json::{Value,json};
use std::{fs,path::PathBuf};
fn main()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args_os().skip(1).collect();
    if args.len()!=3 && !(args.len()==4 && args[3]=="--operator-resources") {return Err("usage: prepare_local_publication INPUT_DIRECTORY KEY_JSON OUTPUT_JSON".into());}
    let input=PathBuf::from(&args[0]);let key:Value=canonical::parse(&fs::read(&args[1])?)?;
    let arena=if args.len()==4{AuditArena::with_resource_limits(ledgerlab_core::validation_work::ResourceLimits::unlimited())?}else{AuditArena::new()?};let mut session=OriginalAuditSession::for_resource_profile_v2(&arena)?;
    let support=SelectedCompilerSupport::local_execution_with_session(&mut session)?;
    let mut documents=Vec::new();
    for name in ["context-facts.json","work-facts.json","outcome-facts.json","product.json","slot-template.json","policy.json"] {
        documents.push(canonical::parse(&fs::read(input.join(name))?)?);
    }
    let policy=documents.iter().find(|d|d["schema"]=="ledger-workflow-policy/4").ok_or("missing policy")?;
    if policy["compiler_ref"]!=serde_json::to_value(support.reference())? {return Err("input compiler_ref differs from actual local compiler".into());}
    use sha2::{Digest,Sha256};
    let mut hash=Sha256::new();hash.update(b"bean-counter/workflow-document/1\0");
    hash.update(canonical::CanonicalBytes::from_value(&json!(["ledger-workflow-policy/4",policy]))?.as_slice());
    let digest=format!("sha256:{}",canonical::hex(&hash.finalize()));
    let root=DefinitionRef::new("workflow-policy",&format!("wdoc_{}",&digest[7..]),&digest)?;
    documents.push(support.canonical_value().clone());
    let program=compile_selected_publication(SelectedSourceInput{documents:documents.iter()
        .map(|d|canonical::CanonicalBytes::from_value(d).map(|b|b.into_vec())).collect::<Result<_,_>>()?,
        root_policy_ref:root.clone()},&support,&mut session)?;
    let request=json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"policy.publish","key":key,
        "payload":{"schema":"ledger-workflow-policy-publication/6","mode":"inline","documents":documents,
            "root_policy_ref":root,"expected_plan_digest":program.compiled_plan().reference().digest()}});
    let raw=canonical::CanonicalBytes::from_value(&request)?;
    if raw.as_slice().len()>262144{return Err("public request exceeds existing ingress bound".into());}
    fs::write(&args[2],raw.as_slice())?;
    println!("{}",json!({"classification":"public-input-only","root_policy_ref":program.root_policy_ref(),
        "compiled_plan_ref":program.compiled_plan().reference(),"request_bytes":raw.as_slice().len()}));
    Ok(())
}
