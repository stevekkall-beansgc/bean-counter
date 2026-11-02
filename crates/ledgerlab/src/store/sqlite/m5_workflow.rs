//! Mechanical statement/5 custody and once-only projection. Economic selection
//! is supplied by the service's completed held-prefix owner, never by this SQL.
use super::{m5, SqliteTx};
use crate::store::errors::StoreError;
use ledgerlab_core::canonical::{self, CanonicalBytes};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};
type PresentationRow = (i64, Vec<u8>, Vec<u8>);
pub(crate) const CLOSE_MANUAL: &str = "ledger-billing-period-close/3";
pub(crate) const RECORD_MANUAL: &str = "ledger-billing-period-close-record/3";
pub(crate) const AD_HOC_MANUAL: &str = "ledger-billing-ad-hoc-statement/3";
pub(crate) const AD_HOC_RECORD_MANUAL: &str = "ledger-billing-ad-hoc-statement-record/3";
pub(crate) const CLOSE: &str = "ledger-billing-period-close/2";
pub(crate) const RECORD: &str = "ledger-billing-period-close-record/2";
pub(crate) const AD_HOC: &str = "ledger-billing-ad-hoc-statement/2";
pub(crate) const AD_HOC_RECORD: &str = "ledger-billing-ad-hoc-statement-record/2";
const LEGACY_CLAIM: &str = "ledger-billing-presentation-claim/1";
pub(crate) const CLAIM: &str = "ledger-billing-workflow-presentation-claim/1";
fn bad() -> StoreError {
    StoreError::Integrity("workflow statement custody")
}
fn bytes(v: &Value) -> Result<Vec<u8>, StoreError> {
    CanonicalBytes::from_value(v)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| bad())
}
fn parse(b: &[u8]) -> Result<Value, StoreError> {
    let v = canonical::parse_bounded(b, 262144).map_err(|_| bad())?;
    if bytes(&v)? != b {
        return Err(bad());
    }
    Ok(v)
}
fn txt(v: &Value) -> Result<&str, StoreError> {
    v.as_str().ok_or_else(bad)
}
fn number(v: &Value) -> Result<i64, StoreError> {
    let raw = txt(v)?;
    let n: i64 = raw.parse().map_err(|_| bad())?;
    if n < 0 || n.to_string() != raw {
        return Err(bad());
    }
    Ok(n)
}
pub(crate) fn statement_hash(v: &Value) -> Result<String, StoreError> {
    let mut v = v.clone();
    v.as_object_mut().ok_or_else(bad)?.remove("statement_hash");
    let mut h = Sha256::new();
    h.update(match (standard(&v)?, manual_profile(&v)) {
        (true, true) => b"bean-counter/m5/statement/6\0".as_slice(),
        (false, true) => b"bean-counter/m5/ad-hoc-statement/3\0".as_slice(),
        (true, false) => b"bean-counter/m5/statement/5\0".as_slice(),
        (false, false) => b"bean-counter/m5/ad-hoc-statement/2\0".as_slice(),
    });
    h.update(bytes(&v)?);
    Ok(m5::hex(&h.finalize()))
}
fn standard(v: &Value) -> Result<bool, StoreError> {
    match txt(&v["schema"])? {
        "ledger-billing-statement/5" | "ledger-billing-statement/6" => Ok(true),
        "ledger-billing-ad-hoc-statement-result/2" | "ledger-billing-ad-hoc-statement-result/3" => {
            Ok(false)
        }
        _ => Err(bad()),
    }
}
fn manual_profile(v: &Value) -> bool {
    matches!(
        v["schema"].as_str(),
        Some("ledger-billing-statement/6" | "ledger-billing-ad-hoc-statement-result/3")
    )
}
fn manual_line(v: &Value) -> bool {
    v.get("manual_credit_ref").is_some()
}
fn command_family(v: &Value) -> Result<&'static str, StoreError> {
    Ok(match (standard(v)?, manual_profile(v)) {
        (true, true) => CLOSE_MANUAL,
        (false, true) => AD_HOC_MANUAL,
        (true, false) => CLOSE,
        (false, false) => AD_HOC,
    })
}
fn record_family(v: &Value) -> Result<&'static str, StoreError> {
    Ok(match (standard(v)?, manual_profile(v)) {
        (true, true) => RECORD_MANUAL,
        (false, true) => AD_HOC_RECORD_MANUAL,
        (true, false) => RECORD,
        (false, false) => AD_HOC_RECORD,
    })
}
fn presentation(v: &Value) -> Result<(&str, &str), StoreError> {
    if standard(v)? {
        Ok(("standard-period", txt(&v["statement_hash"])?))
    } else {
        Ok(("ad-hoc", txt(&v["statement_id"])?))
    }
}
fn workflow_line(v: &Value) -> bool {
    v.get("source_record_ref").is_some()
}
async fn exact_document(conn: &mut SqliteConnection, r: &Value) -> Result<Value, StoreError> {
    let b: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT canonical_bytes FROM workflow_documents WHERE kind=? AND id=? AND digest=?",
    )
    .bind(txt(&r["kind"])?)
    .bind(txt(&r["id"])?)
    .bind(txt(&r["digest"])?)
    .fetch_optional(&mut *conn)
    .await?;
    canonical::parse_bounded(&b.ok_or_else(bad)?, 4194304).map_err(|_| bad())
}
/// Custody comparison only. This is not a substitute for original kernel proof.
async fn check_line(
    conn: &mut SqliteConnection,
    line: &Value,
    statement: &Value,
) -> Result<(), StoreError> {
    let a = exact_document(conn, &line["source_record_ref"]).await?;
    let d = exact_document(conn, &line["decision_ref"]).await?;
    let e = exact_document(conn, &line["evaluation_ref"]).await?;
    let b = exact_document(conn, &line["binding_ref"]).await?;
    let scope = &statement["workflow_boundary"];
    let row:Option<(i64,Vec<u8>,Vec<u8>)>=sqlx::query_as("SELECT commit_sequence,operation_bytes,receipt_bytes FROM workflow_operations WHERE logical_store_id=? AND tenant=? AND environment=? AND receipt_id=? AND receipt_digest=?")
 .bind(txt(&scope["store"])?).bind(txt(&scope["scope"][0])?).bind(txt(&scope["scope"][1])?)
 .bind(txt(&line["producer_receipt_ref"]["id"])?).bind(txt(&line["producer_receipt_ref"]["digest"])?).fetch_optional(&mut *conn).await?;
    let (seq, op, receipt) = row.ok_or_else(bad)?;
    let op = canonical::parse_bounded(&op, 4194304).map_err(|_| bad())?;
    let receipt = canonical::parse_bounded(&receipt, 4194304).map_err(|_| bad())?;
    check_line_loaded(line, statement, [&a, &d, &e, &b], (seq, &op, &receipt))
}

fn check_line_loaded(
    line: &Value,
    statement: &Value,
    documents: [&Value; 4],
    origin: (i64, &Value, &Value),
) -> Result<(), StoreError> {
    let [a, d, e, b] = documents;
    let (seq, op, receipt) = origin;
    for k in [
        "book",
        "role_ref",
        "slot_terms_ref",
        "funding_leg",
        "component_slot",
        "root_component_slot",
        "posting_role",
        "amount",
        "basis_refs",
    ] {
        if line[k] != a[k] {
            return Err(bad());
        }
    }
    if line.get("inverse_of") != a.get("inverse_of")
        || b["customer"] != statement["customer"]
        || line["agreement_id"] != b["agreement_id"]
        || line["agreement_version"] != b["version"]
        || d["original_terms_ref"] != line["binding_ref"]
        || d["evaluation_ref"] != line["evaluation_ref"]
        || e["binding_ref"] != line["binding_ref"]
        || !d["actions_appended"]
            .as_array()
            .is_some_and(|a| a.contains(&line["source_record_ref"]))
    {
        return Err(bad());
    }
    let scope = &statement["workflow_boundary"];
    if seq > number(&scope["commit_sequence"])?
        || receipt["result_kind"] == "semantic_alias"
        || op["key"] != a["operation_key"]
        || !op["result_refs"]
            .as_array()
            .is_some_and(|rs| rs.contains(&line["source_record_ref"]))
    {
        return Err(bad());
    }
    let mut unsigned = line.clone();
    unsigned.as_object_mut().ok_or_else(bad)?.remove("line_id");
    let (kind, identity) = if standard(statement)? {
        (
            "standard",
            json!({"customer":statement["customer"],"period_id":statement["period_id"]}),
        )
    } else {
        (
            "ad_hoc",
            json!({"customer":statement["customer"],"command_id":statement["statement_id"]}),
        )
    };
    let framing = json!({"view_kind":kind,"view_identity":identity,"workflow_scope":{"store":scope["store"],"scope":scope["scope"]},"line":unsigned});
    let mut h = Sha256::new();
    h.update(b"bean-counter/workflow/1/statement-line\0");
    h.update(bytes(&framing)?);
    if line["line_id"] != m5::hex(&h.finalize()) {
        return Err(bad());
    }
    Ok(())
}
fn totals(lines: &[Value]) -> Result<Value, StoreError> {
    let mut grouped = BTreeMap::<Vec<u8>, (Value, i128)>::new();
    for line in lines {
        let workflow = workflow_line(line) || manual_line(line);
        let (currency, scale, raw) = if workflow {
            (
                &line["amount"]["currency"],
                &line["amount"]["scale"],
                txt(&line["amount"]["atoms"])?,
            )
        } else {
            (
                &line["currency"],
                &line["scale"],
                txt(&line["amount_atoms"])?,
            )
        };
        if currency != "USD" || scale != 18 {
            return Err(bad());
        }
        let amount = raw.parse::<i128>().map_err(|_| bad())?;
        if (workflow && amount == 0) || amount.to_string() != raw {
            return Err(bad());
        }
        let key = if workflow {
            json!({"book":line["book"],"role_ref":line["role_ref"],"slot_terms_ref":line["slot_terms_ref"],"funding_leg":line["funding_leg"]})
        } else {
            json!({"payer":line["payer"],"recipient":line["recipient"]})
        };
        let row = grouped.entry(bytes(&key)?).or_insert((key, 0));
        row.1 = row.1.checked_add(amount).ok_or_else(bad)?;
    }
    let rows = grouped
        .into_values()
        .map(|(mut key, amount)| {
            key["amount"] = json!({"currency":"USD","scale":18,"atoms":amount.to_string()});
            key["direction"] = json!(if amount == 0 {
                "none"
            } else if amount < 0 {
                "payable"
            } else {
                "receivable"
            });
            key
        })
        .collect::<Vec<_>>();
    Ok(json!(rows))
}
fn closed(v: &Value, fields: &[&str]) -> Result<(), StoreError> {
    let o = v.as_object().ok_or_else(bad)?;
    if o.len() != fields.len() || fields.iter().any(|k| !o.contains_key(*k)) {
        return Err(bad());
    }
    Ok(())
}
fn reference(v: &Value, kind: &str) -> Result<(), StoreError> {
    closed(v, &["digest", "id", "kind"])?;
    let id = txt(&v["id"])?;
    let digest = txt(&v["digest"])?;
    if v["kind"] != kind
        || id.is_empty()
        || id.len() > 256
        || digest.len() != 71
        || !digest.starts_with("sha256:")
        || !digest.as_bytes()[7..]
            .iter()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(bad());
    }
    Ok(())
}
fn line_shape(v: &Value) -> Result<(), StoreError> {
    if manual_line(v) {
        closed(
            v,
            &ledgerlab_core::domain::workflow_statement::manual_credit::LINE_FIELDS,
        )?;
        reference(&v["manual_credit_ref"], "billing-manual-credit")?;
        reference(&v["binding_ref"], "workflow-binding")?;
        reference(&v["role_ref"], "workflow-roles")?;
        reference(&v["slot_terms_ref"], "workflow-slot-terms")?;
        reference(&v["work_ref"], "workflow-work")?;
        reference(&v["stage_instance_ref"], "workflow-stage-instance")?;
        closed(&v["amount"], &["atoms", "currency", "scale"])?;
        let raw = txt(&v["amount"]["atoms"])?;
        let n = raw.parse::<i128>().map_err(|_| bad())?;
        if v["book"] != "retail"
            || v["amount"]["currency"] != "USD"
            || v["amount"]["scale"] != 18
            || n >= 0
            || raw.len() > 31
            || n.to_string() != raw
        {
            return Err(bad());
        }
        for k in [
            "agreement_id",
            "agreement_version",
            "funding_leg",
            "reason",
            "line_id",
        ] {
            if txt(&v[k])?.is_empty() {
                return Err(bad());
            }
        }
        let refs = v["source_action_refs"].as_array().ok_or_else(bad)?;
        if refs.is_empty() {
            return Err(bad());
        }
        let mut prior = None;
        for r in refs {
            reference(r, "workflow-action")?;
            let raw = bytes(r)?;
            if prior.as_ref().is_some_and(|p| p >= &raw) {
                return Err(bad());
            }
            prior = Some(raw);
        }
        return Ok(());
    }
    if !workflow_line(v) {
        closed(
            v,
            &[
                "agreement_id",
                "agreement_version",
                "amount_atoms",
                "basis",
                "calculation",
                "currency",
                "line_id",
                "payer",
                "recipient",
                "scale",
                "source_records",
            ],
        )?;
        if v["source_records"].as_array().is_none_or(Vec::is_empty)
            || v["currency"] != "USD"
            || v["scale"] != 18
        {
            return Err(bad());
        }
        for k in [
            "agreement_id",
            "agreement_version",
            "basis",
            "payer",
            "recipient",
            "line_id",
        ] {
            if txt(&v[k])?.is_empty() {
                return Err(bad());
            }
        }
        let raw = txt(&v["amount_atoms"])?;
        let n = raw.parse::<i128>().map_err(|_| bad())?;
        if n.to_string() != raw {
            return Err(bad());
        }
        return Ok(());
    }
    let mut fields = vec![
        "agreement_id",
        "agreement_version",
        "amount",
        "basis_refs",
        "binding_ref",
        "book",
        "component_slot",
        "decision_ref",
        "evaluation_ref",
        "funding_leg",
        "line_id",
        "posting_role",
        "producer_receipt_ref",
        "role_ref",
        "root_component_slot",
        "slot_terms_ref",
        "source_record_ref",
    ];
    if v["posting_role"] == "inverse" {
        fields.push("inverse_of");
        reference(&v["inverse_of"], "workflow-action")?;
    }
    closed(v, &fields)?;
    if !matches!(txt(&v["book"])?, "retail" | "supplier")
        || !matches!(
            txt(&v["posting_role"])?,
            "original" | "inverse" | "replacement"
        )
    {
        return Err(bad());
    }
    for k in [
        "agreement_id",
        "agreement_version",
        "component_slot",
        "funding_leg",
        "root_component_slot",
    ] {
        let t = txt(&v[k])?;
        if t.is_empty() || t.len() > 256 {
            return Err(bad());
        }
    }
    for (k, kind) in [
        ("binding_ref", "workflow-binding"),
        ("decision_ref", "workflow-decision"),
        ("evaluation_ref", "workflow-evaluation"),
        ("producer_receipt_ref", "workflow-receipt"),
        ("role_ref", "workflow-roles"),
        ("slot_terms_ref", "workflow-slot-terms"),
        ("source_record_ref", "workflow-action"),
    ] {
        reference(&v[k], kind)?;
    }
    let id = txt(&v["line_id"])?;
    if id.len() != 64
        || !id
            .as_bytes()
            .iter()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(bad());
    }
    closed(&v["amount"], &["atoms", "currency", "scale"])?;
    let atoms = txt(&v["amount"]["atoms"])?;
    if atoms.strip_prefix('-').unwrap_or(atoms).len() > 30 {
        return Err(bad());
    }
    let mut prior = None;
    for r in v["basis_refs"].as_array().ok_or_else(bad)? {
        reference(r, "workflow-action")?;
        let raw = bytes(r)?;
        if prior.as_ref().is_some_and(|p| p >= &raw) {
            return Err(bad());
        }
        prior = Some(raw);
    }
    Ok(())
}
async fn check_statement_header(conn: &mut SqliteConnection, v: &Value) -> Result<(), StoreError> {
    let is_standard = standard(v)?;
    if is_standard {
        closed(
            v,
            &[
                "boundary_resolution_id",
                "boundary_rule_version",
                "close_acceptance_time",
                "complete",
                "currency",
                "customer",
                "end_utc",
                "lines",
                "m3_high_water",
                "m5_high_water",
                "period_id",
                "scale",
                "schema",
                "snapshot_boundary_id",
                "start_utc",
                "statement_hash",
                "status",
                "timezone",
                "timezone_rules_version",
                "totals",
                "workflow_boundary",
            ],
        )?;
        closed(&v["period_id"], &["period_index", "term_version"])?;
    } else {
        let mut fields = vec![
            "schema",
            "status",
            "customer",
            "statement_id",
            "adjustments",
            "workflow_actions",
            "lines",
            "currency",
            "scale",
            "totals",
            "complete",
            "workflow_boundary",
            "m3_high_water",
            "m5_high_water",
            "snapshot_boundary_id",
            "statement_hash",
        ];
        if manual_profile(v) {
            fields.push("manual_credits");
        }
        closed(v, &fields)?;
        if v["status"] != "issued" || txt(&v["statement_id"])?.is_empty() {
            return Err(bad());
        }
        let refs = v["adjustments"].as_array().ok_or_else(bad)?;
        let mut prior = None;
        for r in refs {
            closed(r, &["source", "adjustment_id"])?;
            let key = (txt(&r["source"])?, txt(&r["adjustment_id"])?);
            if key.0.is_empty() || key.1.is_empty() || prior.is_some_and(|p| p >= key) {
                return Err(bad());
            }
            prior = Some(key);
        }
        let actions = v["workflow_actions"].as_array().ok_or_else(bad)?;
        let mut prior = None;
        for r in actions {
            reference(r, "workflow-action")?;
            let raw = bytes(r)?;
            if prior.as_ref().is_some_and(|p| p >= &raw) {
                return Err(bad());
            }
            prior = Some(raw);
        }
        let manual_count = if manual_profile(v) {
            let refs = v["manual_credits"].as_array().ok_or_else(bad)?;
            let mut prior = None;
            for r in refs {
                reference(r, "billing-manual-credit")?;
                let raw = bytes(r)?;
                if prior.as_ref().is_some_and(|p| p >= &raw) {
                    return Err(bad());
                }
                prior = Some(raw);
            }
            refs.len()
        } else {
            0
        };
        if refs.is_empty() && actions.is_empty() && manual_count == 0 {
            return Err(bad());
        }
    }
    closed(
        &v["workflow_boundary"],
        &["commit_sequence", "scope", "store"],
    )?;
    if v["currency"] != "USD"
        || v["scale"] != 18
        || v["workflow_boundary"]["scope"]
            .as_array()
            .is_none_or(|v| v.len() != 2 || v.iter().any(|v| v.as_str().is_none_or(str::is_empty)))
    {
        return Err(bad());
    }
    if txt(&v["customer"])?.is_empty() {
        return Err(bad());
    }
    if is_standard {
        for k in [
            "boundary_resolution_id",
            "boundary_rule_version",
            "customer",
            "timezone",
            "timezone_rules_version",
        ] {
            if txt(&v[k])?.is_empty() {
                return Err(bad());
            }
        }
    }
    if txt(&v["workflow_boundary"]["store"])?.is_empty() {
        return Err(bad());
    }
    if v["complete"] != true
        || (is_standard && v["status"] != "closed")
        || v["statement_hash"] != statement_hash(v)?
        || v.get("net_atoms").is_some()
        || v.get("direction").is_some()
    {
        return Err(bad());
    }
    let boundary:Option<(i64,i64)>=sqlx::query_as("SELECT m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries WHERE boundary_id=?").bind(number(&v["snapshot_boundary_id"])?).fetch_optional(&mut *conn).await?;
    if boundary != Some((number(&v["m3_high_water"])?, number(&v["m5_high_water"])?)) {
        return Err(bad());
    }
    if is_standard {
        let period:Option<(i64,i64)>=sqlx::query_as("SELECT start_at_us,end_at_us FROM billing_m5_period_resolutions WHERE customer=? AND term_version=? AND period_index=? AND resolution_id=?")
 .bind(txt(&v["customer"])?).bind(number(&v["period_id"]["term_version"])?).bind(number(&v["period_id"]["period_index"])?).bind(txt(&v["boundary_resolution_id"])?).fetch_optional(&mut *conn).await?;
        // Return only scalar equality facts, not another unowned term body. Existing
        // M5 custody already validates this immutable term and its projection; compare
        // the exact retained version used for this logical period, never current terms.
        let term:Option<bool>=sqlx::query_scalar("SELECT COALESCE(json_extract(CAST(t.term_bytes AS TEXT),'$.timezone')=? AND json_extract(CAST(t.term_bytes AS TEXT),'$.timezone_rules_version')=? AND json_extract(CAST(t.term_bytes AS TEXT),'$.boundary_rule_version')=? AND json_extract(CAST(r.payload_bytes AS TEXT),'$.term')=json(CAST(t.term_bytes AS TEXT)) AND json_extract(CAST(r.payload_bytes AS TEXT),'$.customer')=t.customer AND json_extract(CAST(r.payload_bytes AS TEXT),'$.term_version')=CAST(t.term_version AS TEXT),0) FROM billing_m5_term_versions t JOIN billing_m5_records r ON r.sequence=t.record_sequence WHERE t.customer=? AND t.term_version=? AND r.family='ledger-billing-term-version/1' AND length(t.term_bytes) BETWEEN 1 AND 262144 AND length(r.payload_bytes) BETWEEN 1 AND 262144")
  .bind(txt(&v["timezone"])?).bind(txt(&v["timezone_rules_version"])?).bind(txt(&v["boundary_rule_version"])?).bind(txt(&v["customer"])?).bind(number(&v["period_id"]["term_version"])?).fetch_optional(&mut *conn).await?;
        if term != Some(true) {
            return Err(bad());
        }
        let start = ledgerlab_core::domain::Timestamp::parse(txt(&v["start_utc"])?)
            .map_err(|_| bad())?
            .micros();
        let end = ledgerlab_core::domain::Timestamp::parse(txt(&v["end_utc"])?)
            .map_err(|_| bad())?
            .micros();
        let accepted = ledgerlab_core::domain::Timestamp::parse(txt(&v["close_acceptance_time"])?)
            .map_err(|_| bad())?
            .micros();
        if period != Some((start, end))
            || start >= end
            || accepted < end
            || number(&v["workflow_boundary"]["commit_sequence"])? < 0
        {
            return Err(bad());
        }
    }
    let lines = v["lines"].as_array().ok_or_else(bad)?;
    if totals(lines)? != v["totals"] {
        return Err(bad());
    }
    let mut seen = BTreeSet::new();
    let mut previous = None;
    for line in lines {
        line_shape(line)?;
        if manual_line(line) && !manual_profile(v) {
            return Err(bad());
        }
        let id = txt(&line["line_id"])?;
        if previous.is_some_and(|p| p >= id) {
            return Err(bad());
        }
        previous = Some(id);
        if line.get("source_record_ref").is_some()
            && !seen.insert(bytes(&line["source_record_ref"])?)
        {
            return Err(bad());
        }
    }
    let mut manual_seen = BTreeSet::new();
    for line in lines {
        if manual_line(line) && !manual_seen.insert(bytes(&line["manual_credit_ref"])?) {
            return Err(bad());
        }
    }
    if manual_profile(v) && !is_standard {
        let expected = v["manual_credits"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .map(bytes)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if expected != manual_seen {
            return Err(bad());
        }
    }
    if !is_standard {
        let expected = v["workflow_actions"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .map(bytes)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if seen != expected {
            return Err(bad());
        }
    }
    Ok(())
}
async fn check_statement(conn: &mut SqliteConnection, v: &Value) -> Result<(), StoreError> {
    check_statement_header(conn, v).await?;
    let (legacy, _, _) = m5::workflow_legacy_components(conn, v).await?;
    let actual = v["lines"]
        .as_array()
        .ok_or_else(bad)?
        .iter()
        .filter(|l| l.get("source_records").is_some())
        .cloned()
        .collect::<Vec<_>>();
    if legacy != json!(actual) {
        return Err(bad());
    }
    for line in v["lines"].as_array().ok_or_else(bad)? {
        if workflow_line(line) {
            check_line(conn, line, v).await?;
        }
    }
    Ok(())
}

mod paid;
/// Reconcile every new presentation and both independent claim families. SQL
/// locators remain DATA; installation custody does not replace producer proof.
pub(crate) async fn verify(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version < 20 {
        return Ok(());
    }
    let (all,recognized):(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_m5_period_closes),(SELECT count(*) FROM billing_m5_period_closes p JOIN billing_m5_records r ON r.sequence=p.close_sequence WHERE r.family IN ('ledger-billing-period-close-record/1','ledger-billing-period-close-record/2','ledger-billing-period-close-record/3'))").fetch_one(&mut *conn).await?;
    if all != recognized {
        return Err(bad());
    }
    let rows:Vec<PresentationRow>=sqlx::query_as("SELECT r.command_sequence,r.payload_bytes,c.response_bytes FROM billing_m5_records r JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE (r.family='ledger-billing-period-close-record/2' AND c.family='ledger-billing-period-close/2') OR (r.family='ledger-billing-ad-hoc-statement-record/2' AND c.family='ledger-billing-ad-hoc-statement/2') OR (r.family='ledger-billing-period-close-record/3' AND c.family='ledger-billing-period-close/3') OR (r.family='ledger-billing-ad-hoc-statement-record/3' AND c.family='ledger-billing-ad-hoc-statement/3') ORDER BY r.sequence").fetch_all(&mut *conn).await?;
    let actual:i64=sqlx::query_scalar("SELECT count(*) FROM billing_m5_records WHERE family IN ('ledger-billing-period-close-record/2','ledger-billing-ad-hoc-statement-record/2','ledger-billing-period-close-record/3','ledger-billing-ad-hoc-statement-record/3')").fetch_one(&mut *conn).await?;
    let commands:i64=sqlx::query_scalar("SELECT count(*) FROM billing_m5_commands WHERE family IN ('ledger-billing-period-close/2','ledger-billing-ad-hoc-statement/2','ledger-billing-period-close/3','ledger-billing-ad-hoc-statement/3')").fetch_one(&mut *conn).await?;
    if actual != rows.len() as i64 || commands != actual {
        return Err(bad());
    }
    let mut claims = BTreeSet::new();
    let mut command_seen = BTreeSet::new();
    for (seq, raw, response) in rows {
        if !command_seen.insert(seq) {
            return Err(bad());
        }
        let v = parse(&response)?;
        let record = parse(&raw)?;
        let is_standard = standard(&v)?;
        check_statement(conn, &v).await?;
        let mut record_fields = if is_standard {
            vec![
                "boundary_resolution_id",
                "customer",
                "end_utc",
                "included_records",
                "included_workflow_actions",
                "lines",
                "m3_high_water",
                "m5_high_water",
                "period_id",
                "record",
                "schema",
                "snapshot_boundary_id",
                "start_utc",
                "statement_hash",
                "totals",
                "workflow_boundary",
            ]
        } else {
            vec![
                "schema",
                "record",
                "customer",
                "statement_id",
                "adjustments",
                "workflow_actions",
                "workflow_boundary",
                "lines",
                "totals",
                "statement_hash",
                "snapshot_boundary_id",
                "m3_high_water",
                "m5_high_water",
            ]
        };
        if manual_profile(&v) {
            record_fields.push("manual_credits");
        }
        closed(&record, &record_fields)?;
        record_envelope(&record["record"])?;
        let command:Option<(Vec<u8>,Vec<u8>,i64,bool)>=sqlx::query_as("SELECT request_bytes,identity_key,accepted_at_us,identity_domain='customer-admin' AND customer=? AND source IS NULL FROM billing_m5_commands WHERE command_sequence=? AND family=? AND length(request_bytes) BETWEEN 1 AND 262144 AND length(identity_key) BETWEEN 1 AND 4096")
            .bind(txt(&v["customer"])?).bind(seq).bind(command_family(&v)?).fetch_optional(&mut *conn).await?;
        let (request, identity, accepted, domain) = command.ok_or_else(bad)?;
        let request = canonical::parse_bounded(&request, 262144).map_err(|_| bad())?;
        let mut fields = if is_standard {
            vec!["schema", "customer", "period_id", "workflow_scope"]
        } else {
            vec![
                "schema",
                "customer",
                "command_id",
                "workflow_scope",
                "adjustments",
                "workflow_actions",
            ]
        };
        if manual_profile(&v) && !is_standard {
            fields.push("manual_credits");
        }
        closed(&request, &fields)?;
        let scope = json!({"store":v["workflow_boundary"]["store"],"scope":v["workflow_boundary"]["scope"]});
        let identity_expected = json!({"schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin","family":if is_standard {"ledger-billing-period-close/1"}else{"ledger-billing-ad-hoc-statement/1"},"customer":v["customer"],"key_kind":if is_standard {"logical_period_id"}else{"command_id"},"key":if is_standard {format!("{}:{}",txt(&v["period_id"]["term_version"])?,txt(&v["period_id"]["period_index"])?) }else{txt(&v["statement_id"])?.to_owned()}});
        if !domain
            || request["schema"] != command_family(&v)?
            || request["customer"] != v["customer"]
            || request["workflow_scope"] != scope
            || parse(&identity)? != identity_expected
            || record["schema"] != record_family(&v)?
            || number(&record["record"]["command_sequence"])? != seq
            || ledgerlab_core::domain::Timestamp::parse(txt(&record["record"]["accepted_at"])?)
                .map_err(|_| bad())?
                .micros()
                != accepted
        {
            return Err(bad());
        }
        if is_standard {
            if request["period_id"] != v["period_id"]
                || record["record"]["accepted_at"] != v["close_acceptance_time"]
            {
                return Err(bad());
            }
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=? AND boundary_resolution_id=? AND snapshot_boundary_id=? AND m3_high_water=? AND m5_high_water=? AND statement_hash=? AND close_sequence=? AND statement_bytes=?")
                .bind(txt(&v["customer"])?).bind(number(&v["period_id"]["term_version"])?).bind(number(&v["period_id"]["period_index"])?).bind(txt(&v["boundary_resolution_id"])?).bind(number(&v["snapshot_boundary_id"])?).bind(number(&v["m3_high_water"])?).bind(number(&v["m5_high_water"])?).bind(txt(&v["statement_hash"])?).bind(number(&record["record"]["sequence"])?).bind(&response).fetch_one(&mut *conn).await?;
            if count != 1 {
                return Err(bad());
            }
            for k in [
                "period_id",
                "boundary_resolution_id",
                "start_utc",
                "end_utc",
            ] {
                if record[k] != v[k] {
                    return Err(bad());
                }
            }
        } else if request["command_id"] != v["statement_id"]
            || record["statement_id"] != v["statement_id"]
            || request["adjustments"] != v["adjustments"]
            || request["workflow_actions"] != v["workflow_actions"]
            || record["workflow_actions"] != v["workflow_actions"]
        {
            return Err(bad());
        }
        if manual_profile(&v) {
            let from_lines = v["lines"]
                .as_array()
                .ok_or_else(bad)?
                .iter()
                .filter_map(|l| l.get("manual_credit_ref"))
                .map(bytes)
                .collect::<Result<BTreeSet<_>, _>>()?;
            let recorded = record["manual_credits"]
                .as_array()
                .ok_or_else(bad)?
                .iter()
                .map(bytes)
                .collect::<Result<BTreeSet<_>, _>>()?;
            if from_lines != recorded
                || recorded.len() != record["manual_credits"].as_array().ok_or_else(bad)?.len()
            {
                return Err(bad());
            }
            if !is_standard
                && (request["manual_credits"] != v["manual_credits"]
                    || record["manual_credits"] != v["manual_credits"])
            {
                return Err(bad());
            }
        }
        for k in [
            "customer",
            "snapshot_boundary_id",
            "m3_high_water",
            "m5_high_water",
            "workflow_boundary",
            "lines",
            "totals",
            "statement_hash",
        ] {
            if record[k] != v[k] {
                return Err(bad());
            }
        }
        let (_, included, expected_legacy_claims) =
            m5::workflow_legacy_components(conn, &v).await?;
        if record[if is_standard {
            "included_records"
        } else {
            "adjustments"
        }] != included
        {
            return Err(bad());
        }
        let expected = v["lines"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .filter(|l| workflow_line(l))
            .map(|l| bytes(&l["source_record_ref"]))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let included = record[if is_standard {
            "included_workflow_actions"
        } else {
            "workflow_actions"
        }]
        .as_array()
        .ok_or_else(bad)?;
        if included.len() != expected.len()
            || included
                .iter()
                .map(bytes)
                .collect::<Result<BTreeSet<_>, _>>()?
                != expected
        {
            return Err(bad());
        }
        let (kind, id) = presentation(&v)?;
        let rows:Vec<(i64,Vec<u8>)>=sqlx::query_as("SELECT sequence,payload_bytes FROM billing_m5_records WHERE command_sequence=? AND family='ledger-billing-workflow-presentation-claim/1'").bind(seq).fetch_all(&mut *conn).await?;
        let mut actual = BTreeSet::new();
        for (sequence, raw) in rows {
            let c = parse(&raw)?;
            check_claim(&c, &v)?;
            let key = json!({"customer":c["customer"],"kind":CLAIM,"role":"workflow-presentation-claim","key":{"workflow_scope":c["workflow_scope"],"source_record_ref":c["source_record_ref"]}});
            if c["record"]["record_id"] != m5::record_id(&identity, &bytes(&key)?) {
                return Err(bad());
            }
            if number(&c["record"]["sequence"])? != sequence
                || number(&c["record"]["command_sequence"])? != seq
                || c["record"]["accepted_at"] != record["record"]["accepted_at"]
                || !actual.insert(bytes(&c["source_record_ref"])?)
                || !claims.insert(sequence)
            {
                return Err(bad());
            }
            let r = &c["source_record_ref"];
            let scope = &c["workflow_scope"];
            let matches:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_statement_membership WHERE claim_sequence=? AND customer=? AND logical_store_id=? AND tenant=? AND environment=? AND action_kind=? AND action_id=? AND action_digest=? AND presentation_kind=? AND statement_id=? AND statement_hash=?")
                .bind(sequence).bind(txt(&c["customer"])?).bind(txt(&scope["store"])?).bind(txt(&scope["scope"][0])?).bind(txt(&scope["scope"][1])?).bind(txt(&r["kind"])?).bind(txt(&r["id"])?).bind(txt(&r["digest"])?).bind(kind).bind(id).bind(txt(&v["statement_hash"])?).fetch_one(&mut *conn).await?;
            if matches != 1 {
                return Err(bad());
            }
        }
        if actual != expected {
            return Err(bad());
        }
        let rows:Vec<Vec<u8>>=sqlx::query_scalar("SELECT payload_bytes FROM billing_m5_records WHERE command_sequence=? AND family='ledger-billing-presentation-claim/1'").bind(seq).fetch_all(&mut *conn).await?;
        let mut legacy = BTreeSet::new();
        for raw in rows {
            let c = parse(&raw)?;
            check_legacy_claim(&c, &v)?;
            if !legacy.insert(bytes(&json!({"customer":c["customer"],"source":c["source"],"adjustment_id":c["adjustment_id"]}))?){return Err(bad())}
        }
        let expected = expected_legacy_claims
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .map(bytes)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if legacy != expected {
            return Err(bad());
        }
        let children: i64 =
            sqlx::query_scalar("SELECT count(*) FROM billing_m5_records WHERE command_sequence=?")
                .bind(seq)
                .fetch_one(&mut *conn)
                .await?;
        let manual_rows:Vec<Vec<u8>>=sqlx::query_scalar("SELECT payload_bytes FROM billing_m5_records WHERE command_sequence=? AND family='ledger-billing-manual-credit-presentation-claim/1'").bind(seq).fetch_all(&mut *conn).await?;
        let mut manual_claims = BTreeSet::new();
        for raw in &manual_rows {
            let c = parse(raw)?;
            if !manual_profile(&v)
                || c["statement_hash"] != v["statement_hash"]
                || !manual_claims.insert(bytes(&c["manual_credit_ref"])?)
            {
                return Err(bad());
            }
        }
        let manual_lines = v["lines"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .filter_map(|l| l.get("manual_credit_ref"))
            .map(bytes)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if manual_claims != manual_lines {
            return Err(bad());
        }
        if children != 1 + actual.len() as i64 + legacy.len() as i64 + manual_claims.len() as i64 {
            return Err(bad());
        }
    }
    let (retained,projected):(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-workflow-presentation-claim/1'),(SELECT count(*) FROM workflow_statement_membership)").fetch_one(&mut *conn).await?;
    if retained != claims.len() as i64 || projected != retained {
        return Err(bad());
    }
    Ok(())
}
fn record_envelope(v: &Value) -> Result<(), StoreError> {
    closed(
        v,
        &[
            "accepted_at",
            "command_sequence",
            "payload_hash",
            "record_id",
            "sequence",
        ],
    )
}
fn check_claim(c: &Value, v: &Value) -> Result<(), StoreError> {
    closed(
        c,
        &[
            "customer",
            "presentation_kind",
            "record",
            "schema",
            "source_record_ref",
            "statement_hash",
            "statement_id",
            "workflow_scope",
        ],
    )?;
    record_envelope(&c["record"])?;
    closed(&c["workflow_scope"], &["scope", "store"])?;
    reference(&c["source_record_ref"], "workflow-action")?;
    let (kind, id) = presentation(v)?;
    if c["schema"] != CLAIM
        || c["customer"] != v["customer"]
        || c["presentation_kind"] != kind
        || c["statement_id"] != id
        || c["statement_hash"] != v["statement_hash"]
        || c["workflow_scope"]["store"] != v["workflow_boundary"]["store"]
        || c["workflow_scope"]["scope"] != v["workflow_boundary"]["scope"]
    {
        return Err(bad());
    }
    Ok(())
}
fn check_legacy_claim(c: &Value, v: &Value) -> Result<(), StoreError> {
    closed(
        c,
        &[
            "schema",
            "record",
            "customer",
            "source",
            "adjustment_id",
            "presentation_kind",
            "statement_id",
        ],
    )?;
    record_envelope(&c["record"])?;
    let (kind, id) = presentation(v)?;
    if c["schema"] != LEGACY_CLAIM
        || c["customer"] != v["customer"]
        || c["presentation_kind"] != kind
        || c["statement_id"] != id
        || txt(&c["source"])?.is_empty()
        || txt(&c["adjustment_id"])?.is_empty()
    {
        return Err(bad());
    }
    Ok(())
}

fn check_workflow_claim_key(key: &Value, c: &Value) -> Result<(), StoreError> {
    closed(key, &["customer", "kind", "role", "key"])?;
    closed(&key["key"], &["workflow_scope", "source_record_ref"])?;
    if key["customer"] != c["customer"]
        || key["kind"] != CLAIM
        || key["role"] != "workflow-presentation-claim"
        || key["key"]["workflow_scope"] != c["workflow_scope"]
        || key["key"]["source_record_ref"] != c["source_record_ref"]
    {
        return Err(bad());
    }
    Ok(())
}

impl SqliteTx {
    pub(crate) async fn legacy_manual_profile_check(
        &mut self,
        customer: &str,
    ) -> Result<(), StoreError> {
        m5::require_no_manual_credits(self.conn(), Some(customer), i64::MIN, i64::MAX, i64::MAX)
            .await
    }
}
