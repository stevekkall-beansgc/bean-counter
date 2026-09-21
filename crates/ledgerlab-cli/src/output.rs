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
        LocalError::Io(e) => super::error("IO", &format!("local file operation failed: {e}"), 2),
    }
}
fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("?")
}
fn posting(out: &mut String, v: &Value) {
    out.push_str(&format!(
        "  {}: {} {} atoms (scale {}), {}\n",
        string(&v["component"]),
        string(&v["amount"]["atoms"]),
        string(&v["amount"]["currency"]),
        v["amount"]["scale"],
        string(&v["book"])
    ));
}
fn intention(out: &mut String, v: &Value) {
    out.push_str(&format!(
        "  Export intention {}: {} {} atoms to {} (no delivery performed)\n",
        string(&v["id"]),
        string(&v["amount"]["atoms"]),
        string(&v["amount"]["currency"]),
        string(&v["destination_id"])
    ));
}
pub fn text(v: &Value) -> String {
    let mut out = String::new();
    match string(&v["status"]) {
        "initialized" => out.push_str(&format!("Initialized synthetic SQLite demo: {}\nNext: preview examples/generated.json, accept it, then explain --chain demo-slice.\nDispatch is held; no payment is executed.\n",string(&v["config"]))),
        "accepted"|"duplicate" => {
            out.push_str(if v["status"]=="accepted" {"Accepted.\n"} else {"Already recorded; original receipt returned.\n"});
            out.push_str(&format!("Event: {}\nReceipt: {}\nChain: {} revision {}\nUse ledger explain {} for postings and export intentions.\n",string(&v["receipt"]["event_id"]),string(&v["receipt"]["id"]),string(&v["receipt"]["chain_id"]),string(&v["receipt"]["revision"]),string(&v["receipt"]["event_id"])));
        },
        "preview" => {
            out.push_str(&format!("Preview: {} (can accept: {}). Nothing committed.\n{}\n",string(&v["outcome"]),v["can_accept"],string(&v["warning"])));
            if let Some(records)=v["records"].as_array() { for r in records { match string(&r["kind"]) { "action"=>posting(&mut out,&r["body"]),"intention"=>intention(&mut out,&r["body"]),_=>() } } }
            for field in ["code","kind","missing","event_id"] { if let Some(value)=v.get(field) { out.push_str(&format!("{field}: {value}\n")); } }
        },
        "explained" => {
            for e in v["events"].as_array().unwrap() {
                out.push_str(&format!("Event {} ({})\nSource event: {}\nDecision: {} (revision {})\nLinks: {}\n",string(&e["event"]["id"]),string(&e["event_id"]),e["event"],string(&e["decision"]["id"]),string(&e["decision"]["revision"]),e["links"]));
                for p in e["postings"].as_array().unwrap() { posting(&mut out,p); }
                for x in e["explanations"].as_array().unwrap() { out.push_str(&format!("  {}\n",string(&x["code"]))); }
                for i in e["intentions"].as_array().unwrap() { intention(&mut out,i); }
                out.push_str(&format!("Receipt: {}\n",string(&e["receipt"]["id"])));
            }
        },
        _ => out.push_str(&format!("{}\n",serde_json::to_string_pretty(v).unwrap())),
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
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
