//! Reviewed administrative prerequisites. Customer assent is never inferred.
mod acceptance;
mod context;
mod journal;
mod record;
mod request;
mod service;
mod source;
pub(super) mod transport;
mod verify;
use super::{Result, ServiceError, records, reject, require};
use crate::store::sqlite::{
    SqliteTx,
    workflow::{
        Document, RecordRef,
        agreement::{JournalAppend, JournalOperation, Projection},
    },
};
use ledgerlab_core::{canonical, domain::Timestamp};
use serde_json::{Value, json};

fn typed_error(e: ledgerlab_core::Error) -> ServiceError {
    match e.code {
        "EVIDENCE_INPUT" | "SOURCE_CEILING" => reject("INVALID_SHAPE"),
        _ => super::core_error(e),
    }
}
fn exact_keys(v: &Value, keys: &[&str]) -> Result<()> {
    if v.as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
    {
        Ok(())
    } else {
        Err(ServiceError::IntegrityFailure)
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or(ServiceError::IntegrityFailure)
}
fn canonical_body(d: &Document) -> Result<Value> {
    let limit = match d.reference.kind.as_str() {
        "workflow-authority" => 65536,
        _ => 262144,
    };
    let body =
        canonical::parse_bounded(&d.bytes, limit).map_err(|_| ServiceError::IntegrityFailure)?;
    let identity_matches = if d.reference.kind == "workflow-source-state" {
        // Its semantic ID additionally depends on the resolved enrollment.
        // SourceState::parse below verifies that ID once the closure is loaded.
        d.reference.digest
            == records::hash(
                "workflow-record",
                1,
                &json!([d.reference.kind, body["operation_key"]["scope"], body]),
            )?
            && d.reference.id.strip_prefix("wsource_").is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
    } else {
        records::source_document(&d.reference.kind, body.clone())?.reference == d.reference
    };
    if records::bytes(&body)? != d.bytes || !identity_matches || body["schema"] != d.schema {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(body)
}
async fn loaded(
    tx: &mut SqliteTx,
    key: &super::request::OperationKey,
    r: &RecordRef,
) -> Result<Document> {
    tx.workflow_document(r, &key.logical_store_id, &key.scope[0], &key.scope[1], None)
        .await
        .map_err(crate::service::store_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))
}
fn evidence_input(body: &Value) -> Result<ledgerlab_core::policy::workflow::EvidenceInput> {
    exact_keys(
        body,
        &[
            "schema",
            "purpose",
            "subject",
            "media_type",
            "content_base64",
            "content_sha256",
            "metadata",
            "retained_by",
            "retained_at",
            "installation_mode",
            "authority_ref",
        ],
    )?;
    if body["schema"] != "ledger-workflow-retained-evidence/1" {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut input = body.clone();
    let o = input
        .as_object_mut()
        .ok_or(ServiceError::IntegrityFailure)?;
    for k in [
        "retained_by",
        "retained_at",
        "installation_mode",
        "authority_ref",
    ] {
        o.remove(k);
    }
    input["schema"] = json!("ledger-workflow-evidence-retain/1");
    ledgerlab_core::policy::workflow::EvidenceInput::parse(&records::bytes(&input)?)
        .map_err(|_| ServiceError::IntegrityFailure)
}
fn evidence_record(
    input: &ledgerlab_core::policy::workflow::EvidenceInput,
    actor: &str,
    at: &Timestamp,
    mode: &str,
    root: &RecordRef,
) -> Result<Document> {
    let mut body = input.canonical_value().map_err(typed_error)?;
    body["schema"] = json!("ledger-workflow-retained-evidence/1");
    body["retained_by"] = json!(actor);
    body["retained_at"] = json!(at);
    body["installation_mode"] = json!(mode);
    body["authority_ref"] = json!(root);
    records::source_document("workflow-evidence", body)
}
fn evidence_identity(
    key: &super::request::OperationKey,
    input: &ledgerlab_core::policy::workflow::EvidenceInput,
) -> Result<Vec<u8>> {
    let v = input.canonical_value().map_err(typed_error)?;
    Ok(records::hash(
        "workflow-evidence-input",
        1,
        &json!([
            key.logical_store_id,
            key.scope,
            v["purpose"],
            v["subject"],
            v["media_type"],
            v["content_sha256"],
            v["metadata"]
        ]),
    )?
    .into_bytes())
}

/// Called only after the facade has checked actual current local read access.
pub(super) async fn status(tx: &mut SqliteTx, key: &super::request::OperationKey) -> Result<Value> {
    let original = tx
        .workflow_agreement_lookup(&key.bytes()?)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    verify::retained(tx, key, &original).await
}

#[cfg(test)]
mod tests;
