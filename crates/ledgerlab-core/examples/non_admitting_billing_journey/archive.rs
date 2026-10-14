//! Example-local durable artifact transport. Digests detect accidental changes,
//! not a hostile editor rewriting the complete synthetic archive.
use super::{CLASSIFICATION, Result, engine, input, require};
use ledgerlab_core::canonical;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
const FILE_LIMIT: u64 = 4_194_304;
const ARCHIVE_LIMIT: u64 = 12_582_912;
const FILES: [&str; 7] = [
    "input.json",
    "original-input.raw.json",
    "policy-terms.json",
    "acknowledgement.json",
    "evidence.json",
    "base.evaluation.json",
    "computed.json",
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    classification: String,
    financial_admission: bool,
    policy_terms_ref: String,
    acknowledgement_ref: String,
    files: Vec<Entry>,
}
pub fn hash(raw: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(raw)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    require(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "artifact must be a regular file",
    )?;
    let f = File::open(path)?;
    require(f.metadata()?.len() <= maximum, "artifact byte limit")?;
    let mut raw = Vec::new();
    f.take(maximum + 1).read_to_end(&mut raw)?;
    require(raw.len() as u64 <= maximum, "artifact grew beyond limit")?;
    Ok(raw)
}
fn write(dir: &Path, name: &str, raw: &[u8]) -> Result<()> {
    require(raw.len() as u64 <= FILE_LIMIT, "artifact byte limit")?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))?;
    f.write_all(raw)?;
    f.sync_all()?;
    Ok(())
}
fn exact(a: &[u8], b: &[u8]) -> Result<()> {
    require(a == b, "DEMO_REPLAY: reconstructed output differs")
}
fn verify_entry(entry: &Entry, raw: &[u8]) -> Result<()> {
    require(
        entry.bytes == raw.len() as u64 && entry.sha256 == hash(raw),
        "DEMO_DIGEST: changed original artifact",
    )
}
fn prepared_files(
    raw: &[u8],
    i: &input::Input,
    p: &input::Prepared,
) -> Result<BTreeMap<String, Vec<u8>>> {
    Ok(BTreeMap::from([
        ("input.json".into(), input::bytes(i)?),
        ("original-input.raw.json".into(), raw.to_vec()),
        ("policy-terms.json".into(), input::bytes(&p.terms)?),
        (
            "acknowledgement.json".into(),
            input::bytes(&p.acknowledgement)?,
        ),
        ("evidence.json".into(), input::bytes(&p.evidence)?),
    ]))
}
fn archive_negatives(computed: &mut Value, entry: &Entry, raw: &[u8]) -> Result<()> {
    let mut altered = raw.to_vec();
    require(!altered.is_empty(), "empty original input")?;
    altered[0] ^= 1;
    require(
        verify_entry(entry, &altered).is_err(),
        "changed original unexpectedly accepted",
    )?;
    let mut changed = computed.clone();
    changed["base"]["retail_total_atoms"] = json!("changed-retained-output");
    require(
        exact(&input::bytes(computed)?, &input::bytes(&changed)?).is_err(),
        "changed output unexpectedly accepted",
    )?;
    computed["negative_checks"]["changed_original_artifact"] = json!(true);
    computed["negative_checks"]["changed_retained_output"] = json!(true);
    computed["negative_errors"]["changed_original_artifact"] = json!("DEMO_DIGEST");
    computed["negative_errors"]["changed_retained_output"] = json!("DEMO_REPLAY");
    Ok(())
}
fn report(mode: &str, computed: Value) -> Value {
    json!({"schema":"bean-counter-non-admitting-billing-report/1",
    "mode":mode,"classification":CLASSIFICATION,"financial_admission":false,
    "authentication":"synthetic coordinator DATA; no authenticated commercial assent",
    "pass_through":"original accepted provider unit rate; arbitrary invoices unsupported",
    "replay_verified":mode=="replay","computed":computed})
}
pub fn start(source: &Path, dir: &Path) -> Result<Value> {
    let raw = read(source, 1_048_576)?;
    let i = input::parse(&raw)?;
    let p = input::prepare(&i)?;
    let mut files = prepared_files(&raw, &i, &p)?;
    // Seal the actual source and acknowledgement BEFORE any execution. Existing
    // directories/files refuse; failed partial archives have no valid manifest.
    fs::create_dir(dir)?;
    for (name, body) in &files {
        write(dir, name, body)?;
    }
    File::open(dir)?.sync_all()?;
    let mut execution = engine::execute(&i, &p, None)?;
    let rawentry = Entry {
        path: "original-input.raw.json".into(),
        bytes: raw.len() as u64,
        sha256: hash(&raw),
    };
    archive_negatives(&mut execution.computed, &rawentry, &raw)?;
    files.insert("base.evaluation.json".into(), execution.base_bytes);
    files.insert("computed.json".into(), input::bytes(&execution.computed)?);
    for name in ["base.evaluation.json", "computed.json"] {
        write(dir, name, &files[name])?;
    }
    let entries = FILES
        .iter()
        .map(|name| {
            let raw = &files[*name];
            Entry {
                path: (*name).into(),
                bytes: raw.len() as u64,
                sha256: hash(raw),
            }
        })
        .collect::<Vec<_>>();
    require(
        entries.iter().map(|e| e.bytes).sum::<u64>() <= ARCHIVE_LIMIT,
        "aggregate artifact limit",
    )?;
    let manifest = Manifest {
        schema: "bean-counter-non-admitting-billing-archive/1".into(),
        classification: CLASSIFICATION.into(),
        financial_admission: false,
        policy_terms_ref: p.terms_ref,
        acknowledgement_ref: p.acknowledgement_ref,
        files: entries,
    };
    write(dir, "manifest.json", &input::bytes(&manifest)?)?;
    File::open(dir)?.sync_all()?;
    Ok(report("start", execution.computed))
}
pub fn replay(dir: &Path) -> Result<Value> {
    let manifest_raw = read(&dir.join("manifest.json"), 65_536)?;
    let manifest: Manifest =
        serde_json::from_value(canonical::parse_bounded(&manifest_raw, 65_536)?)?;
    require(
        input::bytes(&manifest)? == manifest_raw,
        "manifest must be canonical",
    )?;
    require(
        manifest.schema == "bean-counter-non-admitting-billing-archive/1"
            && manifest.classification == CLASSIFICATION
            && !manifest.financial_admission
            && manifest.files.len() == FILES.len(),
        "closed non-admitting archive",
    )?;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for (entry, name) in manifest.files.iter().zip(FILES) {
        require(
            entry.path == name && entry.bytes <= FILE_LIMIT,
            "closed artifact names/order/length",
        )?;
        total = total
            .checked_add(entry.bytes)
            .ok_or("artifact size overflow")?;
        require(total <= ARCHIVE_LIMIT, "aggregate archive limit")?;
        let body = read(&dir.join(name), entry.bytes)?;
        verify_entry(entry, &body)?;
        files.insert(name.to_owned(), body);
    }
    let i = input::parse(&files["original-input.raw.json"])?;
    let p = input::prepare(&i)?;
    require(
        p.terms_ref == manifest.policy_terms_ref
            && p.acknowledgement_ref == manifest.acknowledgement_ref,
        "acknowledgement/original terms mismatch",
    )?;
    for (name, raw) in prepared_files(&files["original-input.raw.json"], &i, &p)? {
        exact(&raw, &files[&name])?;
    }
    // Saved computed.json is never an operand to pricing or target construction.
    let mut execution = engine::execute(&i, &p, Some(&files["base.evaluation.json"]))?;
    let rawentry = &manifest.files[1];
    archive_negatives(
        &mut execution.computed,
        rawentry,
        &files["original-input.raw.json"],
    )?;
    exact(&execution.base_bytes, &files["base.evaluation.json"])?;
    exact(&input::bytes(&execution.computed)?, &files["computed.json"])?;
    Ok(report("replay", execution.computed))
}
