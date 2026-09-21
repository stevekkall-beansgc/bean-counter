use ledgerlab::{
    local::LocalError, AcceptResult, ConflictKind, DuplicateKind, PreviewResult, ServiceError,
};
use serde_json::{json, Value};
fn duplicate(k: DuplicateKind) -> &'static str {
    match k {
        DuplicateKind::Identity => "identity",
        DuplicateKind::Semantic => "semantic",
    }
}
fn conflict(k: ConflictKind) -> &'static str {
    match k {
        ConflictKind::Identity => "identity",
        ConflictKind::Semantic => "semantic",
    }
}
fn rejection(code: &str) -> u8 {
    if code == "SOURCE_UNAUTHORIZED" {
        6
    } else {
        3
    }
}
fn explain_boundary(v: &mut Value) {
    if v["code"] == "UNSUPPORTED_SLICE" {
        v["message"] = json!("Linked events, outcomes and reversals cannot yet be saved or previewed. Local commands support the generation demo.");
    }
}
pub fn accept(result: AcceptResult) -> (Value, u8) {
    let (mut v, c) = match result {
        AcceptResult::Accepted { receipt } => (
            json!({"status":"accepted","receipt":serde_json::from_slice::<Value>(&receipt).expect("facade receipt")}),
            0,
        ),
        AcceptResult::Duplicate { kind, receipt } => (
            json!({"status":"duplicate","kind":duplicate(kind),"receipt":serde_json::from_slice::<Value>(&receipt).expect("facade receipt")}),
            0,
        ),
        AcceptResult::Conflict(kind) => (json!({"status":"conflict","kind":conflict(kind)}), 4),
        AcceptResult::Rejected { code } => {
            let exit = rejection(&code);
            (json!({"status":"rejected","code":code}), exit)
        }
        AcceptResult::Waiting { missing } => (json!({"status":"waiting","missing":missing}), 5),
    };
    explain_boundary(&mut v);
    v["schema"] = json!("ledger-cli/1");
    (v, c)
}
pub fn preview(result: PreviewResult) -> (Value, u8) {
    let (mut v, c) = match result {
        PreviewResult::WouldAccept { records } => (
            json!({"outcome":"would_accept","can_accept":true,"records":records}),
            0,
        ),
        PreviewResult::Duplicate { kind, event_id } => (
            json!({"outcome":"duplicate","can_accept":true,"kind":duplicate(kind),"event_id":event_id}),
            0,
        ),
        PreviewResult::Conflict(kind) => (
            json!({"outcome":"conflict","can_accept":false,"kind":conflict(kind)}),
            4,
        ),
        PreviewResult::Rejected { code } => {
            let exit = rejection(&code);
            (
                json!({"outcome":"rejected","can_accept":false,"code":code}),
                exit,
            )
        }
        PreviewResult::Waiting { missing } => (
            json!({"outcome":"waiting","can_accept":false,"missing":missing}),
            5,
        ),
    };
    explain_boundary(&mut v);
    v["schema"] = json!("ledger-cli/1");
    v["status"] = json!("preview");
    v["committed"] = json!(false);
    v["warning"]=json!("Estimate from current locked context; no receipt, journal writes, or commit. Acceptance rechecks state. Opening/locking may touch SQLite sidecars.");
    (v, c)
}
pub fn local_error(e: LocalError) -> (Value, u8) {
    match e {
        LocalError::Service(ServiceError::OutcomeUnknown {
            scope,
            source,
            external_id,
        }) => (
            json!({"schema":"ledger-cli/1","status":"outcome_unknown","code":"OUTCOME_UNKNOWN","scope":scope,"source":source,"external_id":external_id,"message":"Retry the identical event through acceptance; never replace its ID."}),
            8,
        ),
        LocalError::Service(ServiceError::IntegrityFailure) => {
            super::error("INTEGRITY_FAILURE", "retained data failed verification", 9)
        }
        LocalError::Service(ServiceError::Retryable | ServiceError::Unavailable) => super::error(
            "UNAVAILABLE",
            "local store is busy or unavailable; retry the same input",
            7,
        ),
        LocalError::Service(ServiceError::Rejection(code)) => {
            let exit = rejection(&code);
            super::error(&code, "request rejected", exit)
        }
        LocalError::Config(message) => super::error("CONFIG", message, 2),
        LocalError::Diagnostic(message) => super::error("CONFIG", &message, 2),
        LocalError::Io(e) => super::error("IO", &format!("local file operation failed: {e}"), 2),
    }
}
fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("?")
}
// Decimal placement only: no summing, repricing, rounding, or floating point.
fn money(v: &Value) -> String {
    let atoms = string(&v["atoms"]);
    let sign = if atoms.starts_with('-') { "-" } else { "" };
    let digits = atoms.trim_start_matches('-');
    let scale = v["scale"].as_u64().unwrap_or(0) as usize;
    let padded = format!("{:0>width$}", digits, width = scale + 1);
    let number = if scale == 0 {
        padded
    } else {
        let at = padded.len() - scale;
        format!("{}.{}", &padded[..at], &padded[at..])
    };
    format!("{} {sign}{number}", string(&v["currency"]))
}
fn reason(v: &Value) -> &str {
    match string(&v["component"]) {
        "generation.base" => "generation charge",
        "generation.discount" => "customer tier discount",
        other => other,
    }
}
fn economics(out: &mut String, postings: &[Value], intentions: &[Value]) {
    for (book, label) in [
        ("retail", "Customer charges"),
        ("supplier", "Supplier obligations"),
        ("cost_observation", "Observed provider costs"),
        ("allocation", "Allocations"),
    ] {
        let entries: Vec<_> = postings.iter().filter(|p| p["book"] == book).collect();
        out.push_str(&format!("{label}:\n"));
        if entries.is_empty() {
            out.push_str("  None recorded.\n");
            continue;
        }
        for i in intentions.iter().filter(|i| i["payload"]["book"] == book) {
            let roles = &i["payload"]["roles"];
            out.push_str(&format!(
                "  {} -> {}: {} net obligation for {}.\n",
                string(&roles["payer"]),
                string(&roles["recipient"]),
                money(&i["amount"]),
                entries
                    .iter()
                    .map(|p| reason(p))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ));
        }
        for p in entries {
            if book == "cost_observation" {
                out.push_str(&format!(
                    "  {}: {} observed; bearer {} (observation alone creates no payable).\n",
                    string(&p["roles"]["provider"]),
                    money(&p["amount"]),
                    string(&p["roles"]["bearer"])
                ));
            } else {
                out.push_str(&format!(
                    "  {} -> {}: {} — {}.\n",
                    string(&p["roles"]["payer"]),
                    string(&p["roles"]["recipient"]),
                    money(&p["amount"]),
                    reason(p)
                ));
            }
        }
    }
}
fn history(out: &mut String, v: &Value) {
    if let Some(events) = v["events"].as_array() {
        for e in events {
            economics(
                out,
                e["postings"].as_array().unwrap(),
                e["intentions"].as_array().unwrap(),
            );
            if e["postings"].as_array().unwrap().is_empty() {
                out.push_str(
                    "No charge booked for this event; use --json for the retained explanation.\n",
                );
            }
            out.push_str(&format!(
                "Event: {} | Chain: {} | Revision: {}\n",
                string(&e["event"]["id"]),
                string(&e["event"]["chain"]),
                string(&e["receipt"]["revision"])
            ));
        }
    }
}
pub fn file_error(e: LocalError, path: &std::path::Path, next: &str) -> (Value, u8) {
    let (mut value, code) = local_error(e);
    if code == 2 {
        value["message"] = json!(format!(
            "{}: {}. Next: {next}.",
            path.display(),
            string(&value["message"])
        ));
    }
    (value, code)
}
pub fn event_guidance(v: &mut Value, path: &str) {
    let next = match string(&v["code"]) {
        "UNSUPPORTED_SLICE" => "event.type/links: local commands support only the generation demo. Linked events, outcomes and reversals cannot yet be saved or previewed; use examples/generated.json. See docs/upcoming-story.md for the documentation-only story",
        "SOURCE_UNAUTHORIZED" => "event.source or auth context: use the source provisioned in your sandbox and check config auth.source; changing a source does not grant authority",
        _ => "event input: compare this file with examples/generated.json and contracts/schemas/v1/event.schema.json; use strict JSON, string quantities, valid fields and supported generation facts",
    };
    v["message"] = json!(format!("{path}: {next}."));
}
pub fn text(v: &Value) -> String {
    let mut out = String::new();
    match string(&v["status"]) {
        "initialized" => out.push_str(&format!("Initialized synthetic SQLite demo: {}\nNext: ledger preview examples/generated.json, ledger accept examples/generated.json, then ledger explain --chain demo-slice.\nDispatch is held; no payment is executed.\n",string(&v["config"]))),
        "accepted" | "duplicate" => {
            history(&mut out, &v["human_history"]);
            out.push_str(if v["status"] == "accepted" { "Accepted; receipt saved.\n" } else { "Already recorded; original receipt returned.\n" });
            if let Some(warning) = v["human_warning"].as_str() {
                out.push_str(&format!("{warning}\nEvent: {}\n", string(&v["receipt"]["event_id"])));
            }
            out.push_str("Dispatch is held; no payment is executed. Use --json for the exact receipt.\n");
        },
        "preview" => {
            out.push_str("Estimate only — nothing committed.\n");
            if let Some(records) = v["records"].as_array() {
                let bodies = |kind| records.iter().filter(|r| r["kind"] == kind).map(|r| r["body"].clone()).collect::<Vec<_>>();
                economics(&mut out, &bodies("action"), &bodies("intention"));
            }
            out.push_str(&format!("Preview: {} (can accept: {}).\n{}\n", string(&v["outcome"]), v["can_accept"], string(&v["warning"])));
            if v["outcome"] == "duplicate" { out.push_str("Already recorded; no new charge. Use ledger explain EVENT_ID for the saved breakdown.\n"); }
            for field in ["code", "message", "kind", "missing"] { if let Some(value) = v.get(field) { out.push_str(&format!("{field}: {value}\n")); } }
        },
        "explained" => {
            history(&mut out, v);
            out.push_str("Read from saved decisions; no repricing. No payment is executed by this command.\nUse --json for source events, exact atoms, IDs and retained explanations.\n");
        },
        _ => out.push_str(&format!("{}\n", serde_json::to_string_pretty(v).unwrap())),
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn money_preserves_large_signed_values_and_scale_without_floats() {
        for (atoms, scale, expected) in [
            ("-1", 2, "USD -0.01"),
            ("0", 2, "USD 0.00"),
            (
                "999999999999999999999999999999",
                18,
                "USD 999999999999.999999999999999999",
            ),
            ("123", 0, "USD 123"),
        ] {
            assert_eq!(
                money(&json!({"atoms": atoms, "scale": scale, "currency": "USD"})),
                expected
            );
        }
    }
    #[test]
    fn unknown_result_preserves_retry_identity() {
        let (v, c) = local_error(LocalError::Service(ServiceError::OutcomeUnknown {
            scope: ["demo".into(), "sandbox".into()],
            source: "urn:demo:app".into(),
            external_id: "g1".into(),
        }));
        assert_eq!(c, 8);
        assert_eq!(v["external_id"], "g1");
        assert_eq!(v["status"], "outcome_unknown");
    }
}
