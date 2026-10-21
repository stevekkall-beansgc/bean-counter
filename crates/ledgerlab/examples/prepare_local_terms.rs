//! Normalize proposed public fullterms INPUT; no store or acceptance authority.
use ledgerlab_core::policy::workflow::{AcceptanceTerms, FactSchema};
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args_os().skip(1).collect();
    if a.len() != 3 {
        return Err("usage: prepare_local_terms CONTEXT_JSON TERMS_INPUT OUTPUT_DIRECTORY".into());
    }
    let context = FactSchema::parse(&fs::read(&a[0])?)?;
    let terms = AcceptanceTerms::parse(&fs::read(&a[1])?, &context)?;
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
