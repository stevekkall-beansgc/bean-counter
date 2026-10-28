//! Normalize proposed public fullterms INPUT; no store or acceptance authority.
use ledgerlab_core::{
    canonical::{self, audit_arena::AuditArena},
    policy::workflow::{
        AcceptanceTerms, DefinitionRef, FactSchema,
        selected_program::{
            SelectedCompilerSupport, SelectedSourceInput, compile_selected_publication,
        },
    },
    validation_work::{OriginalAuditSession, ResourceLimits},
};
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args_os().skip(1).collect();
    if a.len() != 3 && a.len() != 4 {
        return Err("usage: prepare_local_terms CONTEXT_JSON TERMS_INPUT OUTPUT_DIRECTORY [COMPONENT_PUBLICATION_REQUEST]".into());
    }
    let context = FactSchema::parse(&fs::read(&a[0])?)?;
    let raw = fs::read(&a[1])?;
    let terms = if a.len() == 4 {
        let request: serde_json::Value = canonical::parse(&fs::read(&a[3])?)?;
        let payload = &request["payload"];
        let root: DefinitionRef = serde_json::from_value(payload["root_policy_ref"].clone())?;
        let documents = payload["documents"]
            .as_array()
            .ok_or("publication documents")?
            .iter()
            .map(|value| canonical::CanonicalBytes::from_value(value).map(|bytes| bytes.into_vec()))
            .collect::<Result<Vec<_>, _>>()?;
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited())?;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena)?;
        let support = SelectedCompilerSupport::local_component_lifecycle_execution_with_session(
            &mut session,
        )?;
        let program = compile_selected_publication(
            SelectedSourceInput {
                documents,
                root_policy_ref: root,
            },
            &support,
            &mut session,
        )?;
        if payload["expected_plan_digest"] != program.compiled_plan().reference().digest() {
            return Err("publication plan digest mismatch".into());
        }
        AcceptanceTerms::parse_component(&raw, &context, &program)?
    } else {
        AcceptanceTerms::parse(&raw, &context)?
    };
    let out = PathBuf::from(&a[2]);
    fs::create_dir_all(&out)?;
    fs::write(out.join("acceptance-terms.json"), terms.canonical_bytes())?;
    fs::write(
        out.join("acceptance-terms.identity.json"),
        serde_json::to_vec_pretty(
            &json!({"reference":terms.reference(),"terms_digest":terms.terms_digest(),"classification":"proposed-fullterms-input"}),
        )?,
    )?;
    println!(
        "{}",
        json!({"reference":terms.reference(),"terms_digest":terms.terms_digest()})
    );
    Ok(())
}
