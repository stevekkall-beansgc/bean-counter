//! Read-only finance projection and exclusive atomic file publication.
use super::BillingLedger;
use crate::{local, ServiceError};
use ledgerlab_core::canonical::{self, Domain};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    schema: String,
    accounts: BTreeMap<String, String>,
}
fn reject(code: &str) -> local::LocalError {
    ServiceError::Rejection(code.into()).into()
}
fn integrity() -> local::LocalError {
    ServiceError::IntegrityFailure.into()
}
fn text(v: &Value) -> local::Result<&str> {
    v.as_str().ok_or_else(integrity)
}
fn digest(v: &Value) -> local::Result<String> {
    canonical::digest(Domain::Document, v).map_err(|_| integrity())
}
fn cell(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn literal(s: &str) -> String {
    format!("text:{s}")
}
fn line(out: &mut String, fields: &[String]) {
    out.push_str(&fields.iter().map(|s| cell(s)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
}
const HEADER: [&str; 30] = [
    "row_type",
    "export_id",
    "snapshot_hash",
    "cutoff",
    "posting_count",
    "control_net_atoms",
    "record_id",
    "record_hash",
    "receipt_id",
    "decision_ordinal",
    "target_id",
    "event_id",
    "obligation_id",
    "reverses_record_id",
    "claim_id",
    "revision_id",
    "tenant_text",
    "environment_text",
    "agreement_text",
    "binding_text",
    "source_text",
    "external_id_text",
    "payer_text",
    "recipient_text",
    "payer_account_text",
    "recipient_account_text",
    "currency",
    "scale",
    "direction",
    "amount_atoms",
];

fn project(statement: &Value, raw: &[u8], pinned: &str) -> local::Result<(Vec<u8>, Value)> {
    if statement["schema"] != "ledger-billing-statement/2" {
        return Err(integrity());
    }
    if text(&statement["snapshot_hash"])? != pinned {
        return Err(reject("BILLING_EXPORT_SNAPSHOT"));
    }
    if statement["complete"] != true || statement["scale"] != 2 || statement["currency"] != "USD" {
        return Err(integrity());
    }
    if raw.len() > local::CONFIG_LIMIT as usize {
        return Err(reject("BILLING_EXPORT_MAPPING"));
    }
    let input = canonical::parse(raw).map_err(|_| reject("BILLING_EXPORT_MAPPING"))?;
    let mapping: Mapping =
        serde_json::from_value(input).map_err(|_| reject("BILLING_EXPORT_MAPPING"))?;
    if mapping.schema != "ledger-finance-mapping/1"
        || mapping
            .accounts
            .values()
            .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
    {
        return Err(reject("BILLING_EXPORT_MAPPING"));
    }
    let entries = statement["entries"].as_array().ok_or_else(integrity)?;
    let mut obligations = BTreeMap::new();
    let mut parties = BTreeSet::new();
    for entry in entries {
        for record in entry["records"].as_array().ok_or_else(integrity)? {
            if record["kind"] == "obligation" {
                let key = canonical::outcome::bytes(&json!([
                    record["body"]["agreement_id"],
                    record["body"]["book"],
                    record["body"]["currency"],
                    record["body"]["scale"],
                    record["body"]["roles"]
                ]))
                .map_err(|_| integrity())?;
                obligations.insert(key, text(&record["id"])?);
            }
        }
        for posting in entry["postings"].as_array().ok_or_else(integrity)? {
            parties.insert(text(&posting["body"]["roles"]["payer"])?);
            parties.insert(text(&posting["body"]["roles"]["recipient"])?);
        }
    }
    if parties != mapping.accounts.keys().map(String::as_str).collect() {
        return Err(reject("BILLING_EXPORT_MAPPING"));
    }
    let export_id = digest(&json!([
        "billing-finance-csv",
        2,
        statement["scope"],
        statement["customer"],
        statement["agreements"],
        statement["snapshot_hash"],
        statement["cutoff"],
        mapping.accounts
    ]))?;
    let count = entries
        .iter()
        .map(|e| e["postings"].as_array().map_or(0, Vec::len))
        .sum::<usize>()
        .to_string();
    let mut csv = String::new();
    line(&mut csv, &HEADER.map(String::from));
    let mut seen = BTreeSet::new();
    let mut total = 0i128;
    for entry in entries {
        for posting in entry["postings"].as_array().ok_or_else(integrity)? {
            let body = &posting["body"];
            let amount = &body["amount"];
            let roles = &body["roles"];
            let record_id = text(&posting["id"])?;
            if !seen.insert(record_id) {
                return Err(integrity());
            }
            let atoms = text(&amount["atoms"])?;
            total = total
                .checked_add(atoms.parse::<i128>().map_err(|_| integrity())?)
                .ok_or_else(integrity)?;
            let key = canonical::outcome::bytes(&json!([
                body["agreement_id"],
                body["book"],
                amount["currency"],
                amount["scale"],
                roles
            ]))
            .map_err(|_| integrity())?;
            let obligation = obligations.get(&key).ok_or_else(integrity)?;
            if body
                .get("obligation_id")
                .is_some_and(|id| id != *obligation)
            {
                return Err(integrity());
            }
            let optional = |key| -> local::Result<String> {
                body.get(key)
                    .map(text)
                    .transpose()
                    .map(|s| s.unwrap_or("").to_owned())
            };
            let fields = vec![
                "posting".into(),
                export_id.clone(),
                pinned.into(),
                text(&statement["cutoff"])?.into(),
                count.clone(),
                text(&statement["net_atoms"])?.into(),
                record_id.into(),
                text(&posting["content_hash"])?.into(),
                text(&entry["receipt"]["id"])?.into(),
                text(&entry["ordinal"])?.into(),
                text(&entry["target"])?.into(),
                text(&body["event_id"])?.into(),
                (*obligation).into(),
                optional("reverses")?,
                optional("claim_id")?,
                optional("revision_id")?,
                literal(text(&statement["scope"][0])?),
                literal(text(&statement["scope"][1])?),
                literal(text(&body["agreement_id"])?),
                literal(text(&body["binding_id"])?),
                literal(text(&entry["source"])?),
                literal(text(&entry["external_id"])?),
                literal(text(&roles["payer"])?),
                literal(text(&roles["recipient"])?),
                literal(&mapping.accounts[text(&roles["payer"])?]),
                literal(&mapping.accounts[text(&roles["recipient"])?]),
                text(&amount["currency"])?.into(),
                "2".into(),
                if atoms.starts_with('-') {
                    "decrease"
                } else if atoms == "0" {
                    "zero"
                } else {
                    "increase"
                }
                .into(),
                atoms.into(),
            ];
            line(&mut csv, &fields);
        }
    }
    if total.to_string() != text(&statement["net_atoms"])? {
        return Err(integrity());
    }
    let mut trailer = vec![String::new(); HEADER.len()];
    trailer[..6].clone_from_slice(&[
        "complete".into(),
        export_id.clone(),
        pinned.into(),
        text(&statement["cutoff"])?.into(),
        count.clone(),
        total.to_string(),
    ]);
    trailer[26] = "USD".into();
    trailer[27] = "2".into();
    line(&mut csv, &trailer);
    let summary = json!({"schema":"ledger-finance-export/2","status":"exported","complete":true,"export_id":export_id,"snapshot_hash":pinned,"cutoff":statement["cutoff"],"posting_count":count,"net_atoms":total.to_string(),"currency":"USD","scale":2,"account_mapping":mapping.accounts,"delivered":false,"payment_collected":false});
    Ok((csv.into_bytes(), summary))
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(std::path::PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn publish(path: &Path, write: impl FnOnce(&mut File) -> std::io::Result<()>) -> local::Result<()> {
    let path = local::normalize_path(path)?;
    let parent = path.parent().ok_or(local::LocalError::Config(
        "output requires a parent directory",
    ))?;
    // Pin a real existing directory; no recursive directory creation or symlink destination.
    let parent_handle = File::open(parent)?;
    if !parent_handle.metadata()?.is_dir() {
        return Err(local::LocalError::Config(
            "output parent is not a directory",
        ));
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut staged = None;
    for _ in 0..16 {
        let temp = parent.join(format!(
            ".ledger-finance-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match options.open(&temp) {
            Ok(file) => {
                staged = Some((Temporary(temp), file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let (temp, mut file) = staged.ok_or(local::LocalError::Config(
        "export temporary names are occupied",
    ))?;
    write(&mut file)?;
    file.sync_all()?;
    // A same-directory hard link publishes a fully written inode without replacing any entry.
    fs::hard_link(&temp.0, &path)?;
    // If directory sync fails, a complete file may exist, but no success is returned.
    parent_handle.sync_all()?;
    Ok(())
}
impl BillingLedger {
    /// Export a verified current snapshot; no journal mutation or delivery acknowledgment.
    pub async fn export_csv(
        &self,
        customer: &str,
        snapshot: &str,
        mapping: &[u8],
        output: &Path,
    ) -> local::Result<Value> {
        let statement = self.statement(customer, None).await?;
        let (csv, mut summary) = project(&statement, mapping, snapshot)?;
        publish(output, |file| file.write_all(&csv))?;
        summary["output"] = json!(output);
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_staging_never_publishes_and_existing_output_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let path = dir.join("finance.csv");
        assert!(publish(&path, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("injected full disk"))
        })
        .is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        publish(&path, |file| file.write_all(b"complete")).unwrap();
        assert!(publish(&path, |file| file.write_all(b"replacement")).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"complete");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }
    #[test]
    fn csv_literal_encoding_is_reversible_and_never_formula_prefixed() {
        for s in [
            "=1+1",
            " +SUM(A1)",
            "@cmd",
            "-2",
            "text:literal",
            "a,\"b",
            "\t=1",
        ] {
            let encoded = literal(s);
            assert_eq!(encoded.strip_prefix("text:"), Some(s));
            assert!(cell(&encoded).starts_with("\"text:"));
        }
    }
}
