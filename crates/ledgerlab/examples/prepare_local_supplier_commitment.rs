//! Derive proposed supplier commitment DATA with the production selected compiler.
//! No store, provider assent, account authority, receipt, or billing is created.
use ledgerlab_core::{
    canonical::{self, audit_arena::AuditArena},
    policy::workflow::{
        DefinitionRef,
        selected_program::{
            SelectedCompilerSupport, SelectedSourceInput, compile_selected_publication,
        },
        transition_constructor::{
            EconomicRecordView, ExactRef, OperationKey, derive_local_supplier_commitment,
        },
    },
    validation_work::OriginalAuditSession,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 && !(args.len() == 3 && args[2] == "--operator-resources") {
        return Err("usage: prepare_local_supplier_commitment INPUT_JSON OUTPUT_DIRECTORY [--operator-resources]".into());
    }
    let input: Value = canonical::parse(&fs::read(&args[0])?)?;
    let arena = if args.len() == 3 {
        AuditArena::with_resource_limits(
            ledgerlab_core::validation_work::ResourceLimits::unlimited(),
        )?
    } else {
        AuditArena::new()?
    };
    let mut session = OriginalAuditSession::for_resource_profile_v2(&arena)?;
    let publication = &input["publication_payload"];
    let root: DefinitionRef = serde_json::from_value(publication["root_policy_ref"].clone())?;
    let policies = publication["documents"]
        .as_array()
        .ok_or("publication documents")?
        .iter()
        .filter(|d| d["schema"] == "ledger-workflow-policy/4")
        .collect::<Vec<_>>();
    if policies.len() != 1 {
        return Err("exactly one root policy required".into());
    }
    let compiler = &policies[0]["compiler_ref"];
    let old = SelectedCompilerSupport::local_stage_lifecycle_execution_with_session(&mut session)?;
    let support = if compiler == &serde_json::to_value(old.reference())? {
        old
    } else {
        drop(old);
        let next =
            SelectedCompilerSupport::local_stage_lifecycle_v2_execution_with_session(&mut session)?;
        if compiler != &serde_json::to_value(next.reference())? {
            return Err("unsupported supplier compiler".into());
        }
        next
    };
    let documents = publication["documents"]
        .as_array()
        .ok_or("publication documents")?
        .iter()
        .map(|v| canonical::CanonicalBytes::from_value(v).map(|b| b.into_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    let program = compile_selected_publication(
        SelectedSourceInput {
            documents,
            root_policy_ref: root,
        },
        &support,
        &mut session,
    )?;
    if publication["expected_plan_digest"] != program.compiled_plan().reference().digest() {
        return Err("publication plan digest differs from actual source compilation".into());
    }
    let key: OperationKey = serde_json::from_value(input["key"].clone())?;
    let records = input["records"]
        .as_array()
        .ok_or("records")?
        .iter()
        .map(|row| {
            Ok((
                serde_json::from_value::<ExactRef>(row["reference"].clone())?,
                canonical::CanonicalBytes::from_value(&row["body"])?,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let views = records
        .iter()
        .map(|(reference, body)| EconomicRecordView {
            reference,
            canonical_body: body.as_slice(),
        })
        .collect::<Vec<_>>();
    // Compilation and commitment derivation are separate audit attempts.
    // Retain the actual Program/support owners in the same paid arena.
    let mut derivation = OriginalAuditSession::for_resource_profile_v2(&arena)?;
    let commitment = derive_local_supplier_commitment(
        &program,
        &key,
        &input["binding_inputs"],
        &input["supplier_terms_projection"],
        &views,
        &mut derivation,
    )?;
    let output = PathBuf::from(&args[1]);
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("supplier-commitment.json"),
        commitment.canonical_bytes(),
    )?;
    let identity = json!({"classification":"proposed-supplier-commitment-data-only","reference":commitment.reference()});
    fs::write(
        output.join("supplier-commitment.identity.json"),
        canonical::CanonicalBytes::from_value(&identity)?.as_slice(),
    )?;
    println!("{identity}");
    Ok(())
}
