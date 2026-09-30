//! Read-only finance projection and exclusive atomic file publication.
use super::BillingLedger;
use crate::{
    local,
    service::billing::{add_integer_atoms, convert_atom_scale},
    store::ports::{AcceptanceStore, AcceptanceTx},
    ServiceError,
};
use ledgerlab_core::canonical::{self, Domain};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

const HEADER_V4: [&str; 27] = [
    "row_type",
    "export_id",
    "statement_hash",
    "snapshot_boundary_id",
    "m3_high_water",
    "m5_high_water",
    "posting_count",
    "control_net_atoms",
    "customer_text",
    "term_version",
    "period_index",
    "start_utc",
    "end_utc",
    "line_id",
    "basis",
    "agreement_id_text",
    "agreement_version",
    "payer_text",
    "recipient_text",
    "payer_account_text",
    "recipient_account_text",
    "currency",
    "scale",
    "direction",
    "amount_atoms",
    "source_refs_text",
    "calculation_text",
];

fn lower_hash(value: &Value) -> local::Result<&str> {
    let value = text(value)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(integrity());
    }
    Ok(value)
}

fn decimal(value: &Value, allow_zero: bool) -> local::Result<&str> {
    let value = text(value)?;
    let valid = !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
        && (allow_zero || value != "0");
    if !valid {
        return Err(integrity());
    }
    Ok(value)
}

fn signed_decimal(value: &Value) -> local::Result<&str> {
    let value = text(value)?;
    if value.is_empty()
        || value == "-0"
        || value.starts_with('+')
        || value.strip_prefix('-').unwrap_or(value).starts_with('0') && value != "0"
        || !value
            .strip_prefix('-')
            .unwrap_or(value)
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(integrity());
    }
    Ok(value)
}

fn signed_atoms(value: &Value) -> local::Result<(&str, i128)> {
    let value = signed_decimal(value)?;
    let parsed = value.parse::<i128>().map_err(|_| integrity())?;
    Ok((value, parsed))
}

fn raw_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn statement_v4_hash(statement: &Value) -> local::Result<String> {
    let mut unsigned = statement.clone();
    unsigned
        .as_object_mut()
        .ok_or_else(integrity)?
        .remove("statement_hash");
    let bytes = canonical::outcome::bytes(&unsigned).map_err(|_| integrity())?;
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/m5/statement/4\0");
    hash.update(bytes);
    Ok(raw_hex(&hash.finalize()))
}

fn line_v4_hash(statement: &Value, value: &Value) -> local::Result<String> {
    let mut unsigned = value.clone();
    unsigned
        .as_object_mut()
        .ok_or_else(integrity)?
        .remove("line_id");
    let identity = json!({
        "view_kind":"standard",
        "view_identity":{
            "customer":statement["customer"],
            "period_id":statement["period_id"]
        },
        "line":unsigned
    });
    let bytes = canonical::outcome::bytes(&identity).map_err(|_| integrity())?;
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/m5/statement-line/1\0");
    hash.update(bytes);
    Ok(raw_hex(&hash.finalize()))
}

fn parse_mapping(raw: &[u8]) -> local::Result<Mapping> {
    if raw.len() > local::CONFIG_LIMIT as usize {
        return Err(reject("BILLING_EXPORT_MAPPING"));
    }
    let input = canonical::parse(raw).map_err(|_| reject("BILLING_EXPORT_MAPPING"))?;
    let mapping: Mapping =
        serde_json::from_value(input).map_err(|_| reject("BILLING_EXPORT_MAPPING"))?;
    if mapping.schema != "ledger-finance-mapping/1"
        || mapping.accounts.iter().any(|(party, account)| {
            party.is_empty()
                || party.len() > 128
                || party.chars().any(char::is_control)
                || account.is_empty()
                || account.len() > 128
                || account.chars().any(char::is_control)
        })
    {
        return Err(reject("BILLING_EXPORT_MAPPING"));
    }
    Ok(mapping)
}

fn project_v4(statement: &Value, raw: &[u8], pinned: &str) -> local::Result<(Vec<u8>, Value)> {
    if statement["schema"] != "ledger-billing-statement/4"
        || statement["status"] != "closed"
        || statement["complete"] != true
        || statement["currency"] != "USD"
        || statement["scale"] != 18
    {
        return Err(integrity());
    }
    let statement_hash = lower_hash(&statement["statement_hash"])?;
    if statement_hash != pinned {
        return Err(reject("BILLING_EXPORT_SNAPSHOT"));
    }
    if statement_v4_hash(statement)? != statement_hash {
        return Err(integrity());
    }
    let customer = text(&statement["customer"])?;
    let term_version = decimal(&statement["period_id"]["term_version"], false)?;
    let period_index = decimal(&statement["period_id"]["period_index"], true)?;
    let snapshot_boundary_id = decimal(&statement["snapshot_boundary_id"], false)?;
    let m3_high_water = decimal(&statement["m3_high_water"], true)?;
    let m5_high_water = decimal(&statement["m5_high_water"], true)?;
    let start_utc = text(&statement["start_utc"])?;
    let end_utc = text(&statement["end_utc"])?;
    let start = ledgerlab_core::domain::Timestamp::parse(start_utc).map_err(|_| integrity())?;
    let end = ledgerlab_core::domain::Timestamp::parse(end_utc).map_err(|_| integrity())?;
    if start.as_str() != start_utc || end.as_str() != end_utc || start.micros() >= end.micros() {
        return Err(integrity());
    }
    let mapping = parse_mapping(raw)?;
    let lines = statement["lines"].as_array().ok_or_else(integrity)?;
    let mut parties = BTreeSet::new();
    let mut previous_line = None;
    let mut total = 0i128;
    for value in lines {
        if value.as_object().is_none_or(|object| object.len() != 11) {
            return Err(integrity());
        }
        let line_id = lower_hash(&value["line_id"])?;
        if previous_line.is_some_and(|previous| previous >= line_id) {
            return Err(integrity());
        }
        previous_line = Some(line_id);
        if line_v4_hash(statement, value)? != line_id
            || value["currency"] != "USD"
            || value["scale"] != 18
        {
            return Err(integrity());
        }
        decimal(&value["agreement_version"], false)?;
        let payer = text(&value["payer"])?;
        let recipient = text(&value["recipient"])?;
        parties.insert(payer);
        parties.insert(recipient);
        let (amount_text, amount) = signed_atoms(&value["amount_atoms"])?;
        let calculation = value["calculation"].as_object().ok_or_else(integrity)?;
        if calculation.len() != 6
            || value["calculation"]["kind"] != value["basis"]
            || signed_decimal(&value["calculation"]["exact_atoms_numerator"]).is_err()
            || decimal(&value["calculation"]["exact_atoms_denominator"], false).is_err()
            || value["calculation"]["booked_atoms"] != amount_text
            || !matches!(
                value["calculation"]["rounding"].as_str(),
                Some("none" | "nearest_ties_away")
            )
            || !value["calculation"]["operands"].is_object()
        {
            return Err(integrity());
        }
        if value["direction"].as_str().is_some_and(|_| true) {
            return Err(integrity());
        }
        total = total.checked_add(amount).ok_or_else(integrity)?;
        let references = value["source_records"].as_array().ok_or_else(integrity)?;
        if references.is_empty() {
            return Err(integrity());
        }
        let mut previous = None;
        for reference in references {
            if reference.as_object().is_none_or(|object| object.len() != 4) {
                return Err(integrity());
            }
            let tuple = (
                text(&reference["customer"])?,
                text(&reference["source"])?,
                text(&reference["kind"])?,
                text(&reference["id"])?,
            );
            if tuple.0 != customer || previous.is_some_and(|prior| prior >= tuple) {
                return Err(integrity());
            }
            previous = Some(tuple);
        }
    }
    if parties != mapping.accounts.keys().map(String::as_str).collect() {
        return Err(reject("BILLING_EXPORT_MAPPING"));
    }
    let (net_text, net) = signed_atoms(&statement["net_atoms"])?;
    let statement_direction = if net == 0 {
        "none"
    } else if net < 0 {
        "payable"
    } else {
        "receivable"
    };
    if total != net || statement["direction"] != statement_direction {
        return Err(integrity());
    }
    let export_id = digest(&json!([
        "billing-finance-csv",
        4,
        customer,
        statement_hash,
        snapshot_boundary_id,
        mapping.accounts
    ]))?;
    let count = lines.len().to_string();
    let mut csv = String::new();
    line(&mut csv, &HEADER_V4.map(String::from));
    for value in lines {
        let amount = text(&value["amount_atoms"])?;
        let direction = if amount == "0" {
            "none"
        } else if amount.starts_with('-') {
            "payable"
        } else {
            "receivable"
        };
        let payer = text(&value["payer"])?;
        let recipient = text(&value["recipient"])?;
        let source_refs =
            canonical::outcome::bytes(&value["source_records"]).map_err(|_| integrity())?;
        let calculation =
            canonical::outcome::bytes(&value["calculation"]).map_err(|_| integrity())?;
        let fields = vec![
            "posting".into(),
            export_id.clone(),
            statement_hash.into(),
            snapshot_boundary_id.into(),
            m3_high_water.into(),
            m5_high_water.into(),
            count.clone(),
            net_text.into(),
            literal(customer),
            term_version.into(),
            period_index.into(),
            start_utc.into(),
            end_utc.into(),
            text(&value["line_id"])?.into(),
            text(&value["basis"])?.into(),
            literal(text(&value["agreement_id"])?),
            text(&value["agreement_version"])?.into(),
            literal(payer),
            literal(recipient),
            literal(&mapping.accounts[payer]),
            literal(&mapping.accounts[recipient]),
            "USD".into(),
            "18".into(),
            direction.into(),
            amount.into(),
            literal(std::str::from_utf8(&source_refs).map_err(|_| integrity())?),
            literal(std::str::from_utf8(&calculation).map_err(|_| integrity())?),
        ];
        line(&mut csv, &fields);
    }
    let mut trailer = vec![String::new(); HEADER_V4.len()];
    trailer[..8].clone_from_slice(&[
        "complete".into(),
        export_id.clone(),
        statement_hash.into(),
        snapshot_boundary_id.into(),
        m3_high_water.into(),
        m5_high_water.into(),
        count.clone(),
        net_text.into(),
    ]);
    line(&mut csv, &trailer);
    let summary = json!({
        "schema":"ledger-finance-export/4","status":"exported","complete":true,
        "export_id":export_id,"statement_hash":statement_hash,
        "snapshot_boundary_id":snapshot_boundary_id,"m3_high_water":m3_high_water,
        "m5_high_water":m5_high_water,"posting_count":count,
        "control_net_atoms":net_text,"currency":"USD","scale":18,
        "account_mapping":mapping.accounts,"delivered":false,"payment_collected":false
    });
    Ok((csv.into_bytes(), summary))
}

fn project(statement: &Value, raw: &[u8], pinned: &str) -> local::Result<(Vec<u8>, Value)> {
    if statement["schema"] == "ledger-billing-statement/4" {
        return project_v4(statement, raw, pinned);
    }
    let (export_version, export_scale) = match statement["schema"].as_str() {
        Some("ledger-billing-statement/2") if statement["scale"].as_u64() == Some(2) => (2u8, 2u8),
        Some("ledger-billing-statement/3") if statement["scale"].as_u64() == Some(18) => {
            (3u8, 18u8)
        }
        _ => return Err(integrity()),
    };
    if text(&statement["snapshot_hash"])? != pinned {
        return Err(reject("BILLING_EXPORT_SNAPSHOT"));
    }
    if statement["complete"] != true || statement["currency"] != "USD" {
        return Err(integrity());
    }
    let mapping = parse_mapping(raw)?;
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
        export_version,
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
    let mut total = String::from("0");
    for entry in entries {
        for posting in entry["postings"].as_array().ok_or_else(integrity)? {
            let body = &posting["body"];
            let amount = &body["amount"];
            let roles = &body["roles"];
            let record_id = text(&posting["id"])?;
            if !seen.insert(record_id) {
                return Err(integrity());
            }
            let recorded_atoms = text(&amount["atoms"])?
                .parse::<i128>()
                .map_err(|_| integrity())?;
            let recorded_scale = u8::try_from(amount["scale"].as_u64().ok_or_else(integrity)?)
                .map_err(|_| integrity())?;
            let atoms = convert_atom_scale(
                recorded_atoms,
                text(&amount["currency"])?,
                recorded_scale,
                export_scale,
            )
            .ok_or_else(integrity)?;
            total = add_integer_atoms(&total, &atoms).ok_or_else(integrity)?;
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
                export_scale.to_string(),
                if atoms.starts_with('-') {
                    "decrease"
                } else if atoms == "0" {
                    "zero"
                } else {
                    "increase"
                }
                .into(),
                atoms,
            ];
            line(&mut csv, &fields);
        }
    }
    if total.as_str() != text(&statement["net_atoms"])? {
        return Err(integrity());
    }
    let mut trailer = vec![String::new(); HEADER.len()];
    trailer[..6].clone_from_slice(&[
        "complete".into(),
        export_id.clone(),
        pinned.into(),
        text(&statement["cutoff"])?.into(),
        count.clone(),
        total.clone(),
    ]);
    trailer[26] = "USD".into();
    trailer[27] = export_scale.to_string();
    line(&mut csv, &trailer);
    let summary = json!({"schema":format!("ledger-finance-export/{export_version}"),"status":"exported","complete":true,"export_id":export_id,"snapshot_hash":pinned,"cutoff":statement["cutoff"],"posting_count":count,"net_atoms":total,"currency":"USD","scale":export_scale,"account_mapping":mapping.accounts,"delivered":false,"payment_collected":false});
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
        let mut tx = self
            .store
            .begin(tokio::time::Instant::now() + Self::REPORT_BUDGET)
            .await
            .map_err(crate::service::store_error)?;
        let retained = tx
            .m5_period_close_statement(customer, snapshot)
            .await
            .map_err(crate::service::store_error)?;
        tx.rollback().await.map_err(crate::service::store_error)?;
        let statement = match retained {
            Some(bytes) => canonical::parse(&bytes).map_err(|_| integrity())?,
            None => self.statement(customer, None).await?,
        };
        let (csv, mut summary) = project(&statement, mapping, snapshot)?;
        publish(output, |file| file.write_all(&csv))?;
        if summary["schema"] != "ledger-finance-export/4" {
            summary["output"] = json!(output);
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_export_v4_vectors_match_exact_bytes_and_identities() {
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../../../../contracts/candidates/billing-lifecycle-m5/vectors/finance-export-v4.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let mapping = serde_json::to_vec(&json!({
                "schema":"ledger-finance-mapping/1",
                "accounts":case["accounts"]
            }))
            .unwrap();
            let statement = &case["statement"];
            let pinned = case["statement_hash"]
                .as_str()
                .or_else(|| statement["statement_hash"].as_str())
                .unwrap();
            let (csv, summary) = project_v4(statement, &mapping, pinned).unwrap();
            assert_eq!(summary["schema"], "ledger-finance-export/4");
            assert_eq!(summary["export_id"], case["export_id"]);
            assert_eq!(summary["statement_hash"], pinned);
            assert_eq!(csv, case["csv_utf8"].as_str().unwrap().as_bytes());
            let csv_hash = raw_hex(&Sha256::digest(&csv));
            let expected_hash = case["csv_sha256"]
                .as_str()
                .or_else(|| case["expected_csv_sha256"].as_str())
                .unwrap();
            assert_eq!(csv_hash, expected_hash, "case {}", case["id"]);
        }
    }

    #[test]
    fn export_v4_refuses_a_changed_pin_or_inexact_mapping() {
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../../../../contracts/candidates/billing-lifecycle-m5/vectors/finance-export-v4.json"
        ))
        .unwrap();
        let statement = &fixture["cases"][1]["statement"];
        let mapping = br#"{"schema":"ledger-finance-mapping/1","accounts":{"+payer":"Assets"}}"#;
        assert!(project_v4(statement, mapping, "0").is_err());
        assert!(project_v4(
            statement,
            mapping,
            statement["statement_hash"].as_str().unwrap()
        )
        .is_err());
    }

    #[tokio::test]
    async fn export_csv_selects_an_immutable_statement_v4_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let installation = root.join("billing");
        BillingLedger::init(
            &installation,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&installation).await.unwrap();
        let term = canonical::CanonicalBytes::from_value(&json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"export-v4-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-01-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-01-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp",
                "boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        }))
        .unwrap()
        .into_vec();
        ledger.term_set(&term).await.unwrap();
        let close = canonical::CanonicalBytes::from_value(&json!({
            "schema":"ledger-billing-period-close/1","customer":"customer-1",
            "period_id":{"term_version":"1","period_index":"0"}
        }))
        .unwrap()
        .into_vec();
        let statement = ledger
            .period_close_at(
                &close,
                ledgerlab_core::domain::Timestamp::parse("2026-02-05T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let output = root.join("statement-v4.csv");
        let mapping = br#"{"schema":"ledger-finance-mapping/1","accounts":{}}"#;
        let summary = ledger
            .export_csv(
                "customer-1",
                statement["statement_hash"].as_str().unwrap(),
                mapping,
                &output,
            )
            .await
            .unwrap();
        assert_eq!(summary["schema"], "ledger-finance-export/4");
        assert_eq!(summary["statement_hash"], statement["statement_hash"]);
        assert_eq!(summary["posting_count"], "0");
        assert!(summary.get("output").is_none());
        let csv = fs::read(output).unwrap();
        assert!(csv.ends_with(b"\"\",\"\",\"\"\r\n"));
        ledger.close().await;
        let reopened = BillingLedger::open(&installation).await.unwrap();
        reopened.close().await;
    }

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
