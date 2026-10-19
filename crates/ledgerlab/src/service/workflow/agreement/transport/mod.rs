//! Compiled source-retention recipe. Private local owner authority is checked
//! by the coordinator; parsing source definitions does not grant that authority.
pub(super) mod certificate;
mod evidence;
mod read;
mod resolve;
pub(super) mod service;
use super::super::request::OperationKey;
use super::*;
use crate::store::sqlite::workflow::{
    agreement::transport::{Part, Upload},
    RegistryEntry,
};
use ledgerlab_core::policy::workflow as core;
#[cfg(test)]
mod tests;
pub(in super::super) use read::read;
/// Scoped membership is only an origin locator. Publication's trusted resolver
/// must authenticate the complete saved source-producing closure under its lock.
pub(in crate::service::workflow) async fn verify_source_origin(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &RecordRef,
) -> Result<()> {
    require(source_kind(&reference.kind), "INVALID_REFERENCE")?;
    Box::pin(verify::document_origin(tx, key, reference)).await
}
pub(super) fn selected(payload: &Value) -> bool {
    matches!(
        payload["schema"].as_str(),
        Some(
            "ledger-workflow-definition-retain/1"
                | "ledger-workflow-definition-fragment/1"
                | "ledger-workflow-definition-finalize/1"
        )
    )
}
fn transport_error(e: ledgerlab_core::Error) -> ServiceError {
    match e.code {
        "DEFINITION_TRANSPORT" => reject("INVALID_SHAPE"),
        "DEFINITION_LIMIT" => reject("PROGRAM_BUDGET_EXCEEDED"),
        "DEFINITION_MISSING" => reject("DESCRIPTOR_DEPENDENCY_MISSING"),
        "DEFINITION_CONFLICT" => reject("SEMANTIC_CONFLICT"),
        "DEFINITION_EXTRA" => reject("INVALID_SHAPE"),
        "UNSUPPORTED_COMPILER_DESCRIPTOR" | "PROGRAM_UNSUPPORTED_PRIMITIVE" => {
            reject("UNSUPPORTED_SCHEMA_PROFILE")
        }
        _ => super::super::core_error(e),
    }
}
fn source_kind(kind: &str) -> bool {
    matches!(
        kind,
        "workflow-product"
            | "workflow-policy"
            | "workflow-fact-schema"
            | "workflow-slot-template"
            | "workflow-compiler-descriptor"
    )
}
fn registry(d: &core::DefinitionDocument) -> Result<Option<RegistryEntry>> {
    let field = match d.kind() {
        "workflow-product" => "product_id",
        "workflow-policy" => "policy_id",
        "workflow-fact-schema" => "schema_id",
        "workflow-slot-template" => "template_id",
        "workflow-compiler-descriptor" => return Ok(None),
        _ => return Err(ServiceError::IntegrityFailure),
    };
    Ok(Some(RegistryEntry {
        kind: d.kind().into(),
        semantic_id: text(d.canonical_value(), field)?.into(),
        version: text(d.canonical_value(), "version")?.into(),
        document_id: d.id().into(),
    }))
}
fn document(d: &core::DefinitionDocument) -> Result<Document> {
    Ok(Document {
        reference: serde_json::from_value(json!(d.reference()))
            .map_err(|_| ServiceError::IntegrityFailure)?,
        schema: text(d.canonical_value(), "schema")?.into(),
        bytes: d.canonical_bytes().to_vec(),
    })
}
fn record_ref(r: &impl serde::Serialize) -> Result<RecordRef> {
    serde_json::from_value(json!(r)).map_err(|_| ServiceError::IntegrityFailure)
}
fn semantic(key: &OperationKey, resolution: &resolve::Resolution) -> Result<Vec<u8>> {
    if let Some(input) = resolution.input.fragment() {
        records::bytes(&json!([
            "source.fragment",
            key.source,
            input.upload_id(),
            input.part().to_string()
        ]))
    } else {
        let d = resolution
            .definition
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?
            .document();
        if let Some(entry) = registry(d)? {
            records::bytes(&json!([
                "source.definition",
                entry.kind,
                entry.semantic_id,
                entry.version
            ]))
        } else {
            records::bytes(&json!(["source.definition", d.kind(), d.id()]))
        }
    }
}
