//! Render stored bodies; never rerun economics for booked history.
use crate::{
    local::ExplainTarget,
    service::{accept::retained_record, store_error},
    store::{records::Scope, sqlite::SqliteStore},
    ServiceError,
};
use ledgerlab_core::canonical::{self, CanonicalBytes, Domain};
use serde_json::{json, Value};

pub(crate) async fn read(
    store: &SqliteStore,
    scope: &Scope,
    source: &str,
    target: &ExplainTarget,
) -> Result<Value, ServiceError> {
    let stored = store
        .inspect(scope, source, target)
        .await
        .map_err(store_error)?;
    let mut events = Vec::new();
    for event in stored {
        let mut out = json!({"event_id":event.id,"postings":[],"intentions":[],"explanations":[],"documents":[]});
        let mut retained_members = Vec::new();
        for (kind, id, record) in event.records {
            let body = canonical::parse(&record.canonical_bytes)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let expected = match kind.as_str() {
                "event" => canonical::digest(Domain::EventContent, &body),
                "decision-manifest" => canonical::digest(Domain::DecisionContent, &body),
                "document" => {
                    let schema = body["schema"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?;
                    let kind = schema
                        .strip_prefix("ledger-")
                        .and_then(|v| v.strip_suffix("/1"))
                        .ok_or(ServiceError::IntegrityFailure)?;
                    if ![
                        "policy",
                        "roles",
                        "assent",
                        "source-grant",
                        "context",
                        "binding",
                        "snapshot",
                    ]
                    .contains(&kind)
                    {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    let tuple = json!([kind, 1, body]);
                    if canonical::identity(Domain::Document, &tuple)
                        .map_err(|_| ServiceError::IntegrityFailure)?
                        != id
                    {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    canonical::digest(Domain::Document, &tuple)
                }
                _ => {
                    retained_record(&record, &kind)?;
                    Ok(record.content_hash.clone())
                }
            }
            .map_err(|_| ServiceError::IntegrityFailure)?;
            if expected != record.content_hash
                || CanonicalBytes::from_value(&body)
                    .map_err(|_| ServiceError::IntegrityFailure)?
                    .as_slice()
                    != record.canonical_bytes
            {
                return Err(ServiceError::IntegrityFailure);
            }
            if kind != "event" && kind != "document" && body["id"] != id {
                return Err(ServiceError::IntegrityFailure);
            }
            if kind != "receipt" && kind != "decision-manifest" {
                retained_members
                    .push(json!({"kind":kind,"id":id,"content_hash":record.content_hash}));
            }
            match kind.as_str() {
                "event" => {
                    out["links"] = body["links"].clone();
                    out["event"] = body;
                }
                "decision-manifest" => out["decision"] = body,
                "receipt" => out["receipt"] = body,
                _ => {
                    let key = match kind.as_str() {
                        "action" => "postings",
                        "intention" => "intentions",
                        "explanation" => "explanations",
                        "document" => "documents",
                        _ => unreachable!(),
                    };
                    out[key]
                        .as_array_mut()
                        .unwrap()
                        .push(if kind == "document" {
                            json!({"id":id,"body":body})
                        } else {
                            body
                        });
                }
            }
        }
        if out["receipt"]["event_id"] != event.id
            || out["decision"]["event_id"] != event.id
            || out["event"].is_null()
        {
            return Err(ServiceError::IntegrityFailure);
        }
        let expected_event = canonical::identity(
            Domain::Event,
            &json!([
                scope.tenant,
                scope.environment,
                out["event"]["source"],
                out["event"]["id"]
            ]),
        )
        .map_err(|_| ServiceError::IntegrityFailure)?;
        if expected_event != event.id
            || out["event"]["source"] != source
            || out["receipt"]["decision_id"] != out["decision"]["id"]
            || out["receipt"]["chain_id"] != out["event"]["chain"]
            || out["receipt"]["revision"] != out["decision"]["revision"]
            || out["receipt"]["decision_hash"]
                != canonical::digest(Domain::DecisionContent, &out["decision"])
                    .map_err(|_| ServiceError::IntegrityFailure)?
            || out["receipt"]["content_hash"]
                != canonical::digest(Domain::EventContent, &out["event"])
                    .map_err(|_| ServiceError::IntegrityFailure)?
        {
            return Err(ServiceError::IntegrityFailure);
        }
        let members = out["decision"]["members"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        for member in &retained_members {
            if !members.contains(member) {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        for member in members {
            if ["event", "action", "intention", "explanation", "document"]
                .contains(&member["kind"].as_str().unwrap_or(""))
                && !retained_members.contains(member)
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        out["explanations"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|v| v["ordinal"].as_u64().unwrap_or(0));
        // Detect missing displayed records instead of presenting an incomplete receipt.
        for (list, receipt_list) in [("postings", "action_ids"), ("intentions", "intention_ids")] {
            let ids: Vec<_> = out[list]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["id"].clone())
                .collect();
            if json!(ids) != out["receipt"][receipt_list] {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        events.push(out);
    }
    Ok(json!({"events":events}))
}
