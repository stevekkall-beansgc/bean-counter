//! Authenticated nonduplicating canonical-byte read and compiled source prepare.
//! Wire responses use their separate eight-MiB ceiling, not the record encoder.
use super::*;
use crate::{
    service::workflow::local::{LocalWorkflowLedger, deadline},
    store::ports::{AcceptanceStore, AcceptanceTx},
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    schema: String,
    kind: String,
    payload: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Canonical {
    r#ref: RecordRef,
    representation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    operation_kind: String,
    candidate: Value,
}
fn wire(value: Value) -> Result<Value> {
    if serde_json::to_vec(&value)
        .map_err(|_| ServiceError::IntegrityFailure)?
        .len()
        > 8388608
    {
        Err(reject("RESPONSE_BUDGET_EXCEEDED"))
    } else {
        Ok(value)
    }
}
pub(in super::super::super) async fn read(
    ledger: &LocalWorkflowLedger,
    raw: &[u8],
) -> Result<Value> {
    let value = canonical::parse_bounded(raw, 262144).map_err(|_| reject("INVALID_SHAPE"))?;
    let read: Read = serde_json::from_value(value).map_err(|_| reject("INVALID_SHAPE"))?;
    require(read.schema == "ledger-read/1", "INVALID_SHAPE")?;
    let mut tx = ledger
        .store
        .begin(deadline())
        .await
        .map_err(crate::service::store_error)?;
    let result=async {
        let meta=tx.billing_meta().await.map_err(crate::service::store_error)?;
        let setup=crate::service::billing::permissions::effective(&meta)?;
        let key=OperationKey{logical_store_id:setup.store_id,scope:[setup.scope.tenant().into(),setup.scope.environment().into()],source:setup.source,operation_id:"source-transport-read".into()};
        ledger.admin.read(&ledger.store,&mut tx,&key).await?;
        match read.kind.as_str() {
            "policy.document"=>{
                let payload:Canonical=serde_json::from_value(read.payload).map_err(|_|reject("INVALID_SHAPE"))?;
                require(payload.representation=="canonical_bytes","INVALID_SHAPE")?;
                let r=payload.r#ref;
                require(source_kind(&r.kind)||matches!(r.kind.as_str(),"workflow-definition-fragment"|"workflow-compiled-plan"|"workflow-publication"|"workflow-authority"|"workflow-build-manifest"),"INVALID_REFERENCE")?;
                require(records::bytes(&r)?.len()<=1024,"INVALID_REFERENCE")?;
                core::AgreementRef::parse(&records::bytes(&r)?).map_err(|_|reject("INVALID_REFERENCE"))?;
                let d=loaded(&mut tx,&key,&r).await?;
                Box::pin(verify::document_origin(&mut tx,&key,&r)).await?;
                require(d.bytes.len()<=4194304,"RESPONSE_BUDGET_EXCEEDED")?;
                wire(json!({"schema":"ledger-workflow-document-read/2","ref":r,"canonical_bytes_base64":super::super::super::publication::base64(&d.bytes),"canonical_bytes_sha256":records::raw_hash(&d.bytes),"body_bytes":d.bytes.len().to_string()}))
            },
            "operation.descriptors"=>{
                let payload:Prepare=serde_json::from_value(read.payload).map_err(|_|reject("INVALID_SHAPE"))?;
                require(payload.operation_kind=="evidence.retain" && selected(&payload.candidate),"UNSUPPORTED_SCHEMA_PROFILE")?;
                let resolution=resolve::resolve(&mut tx,&key,&payload.candidate,true).await?;
                wire(evidence::descriptors(&resolution)?.0)
            },
            _=>Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
        }
    }.await;
    tx.rollback().await.map_err(crate::service::store_error)?;
    result
}
