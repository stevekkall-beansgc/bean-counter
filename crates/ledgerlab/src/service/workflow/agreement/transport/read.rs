//! Authenticated nonduplicating canonical-byte read and compiled source prepare.
//! Wire responses use their separate eight-MiB ceiling, not the record encoder.
use super::*;
use crate::{
    service::workflow::local::{deadline, LocalWorkflowLedger},
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
        proof_session::with_proofs(&mut tx, &key, |tx| Box::pin(async {
        ledger.admin.read(&ledger.store,tx,&key).await?;
        match read.kind.as_str() {
            "policy.document"=>{
                let payload:Canonical=serde_json::from_value(read.payload).map_err(|_|reject("INVALID_SHAPE"))?;
                require(payload.representation=="canonical_bytes","INVALID_SHAPE")?;
                let r=payload.r#ref;
                require(source_kind(&r.kind)||matches!(r.kind.as_str(),"workflow-definition-fragment"|"workflow-compiled-plan"|"workflow-publication"|"workflow-authority"|"workflow-build-manifest"),"INVALID_REFERENCE")?;
                require(records::bytes(&r)?.len()<=1024,"INVALID_REFERENCE")?;
                core::AgreementRef::parse(&records::bytes(&r)?).map_err(|_|reject("INVALID_REFERENCE"))?;
                let d=loaded(tx,&key,&r).await?;
                Box::pin(verify::document_origin(tx,&key,&r)).await?;
                require(d.bytes.len()<=4194304,"RESPONSE_BUDGET_EXCEEDED")?;
                wire(json!({"schema":"ledger-workflow-document-read/2","ref":r,"canonical_bytes_base64":super::super::super::publication::base64(&d.bytes),"canonical_bytes_sha256":records::raw_hash(&d.bytes),"body_bytes":d.bytes.len().to_string()}))
            },
            "operation.descriptors"=>{
                let payload:Prepare=serde_json::from_value(read.payload).map_err(|_|reject("INVALID_SHAPE"))?;
                if payload.operation_kind=="evidence.retain" && selected(&payload.candidate) {
                    let resolution=resolve::resolve(tx,&key,&payload.candidate,true).await?;
                    wire(evidence::descriptors(&resolution)?.0)
                }else if payload.operation_kind=="policy.publish" {publication_prepare(tx,&key,&payload.candidate).await}
                else {Err(reject("UNSUPPORTED_SCHEMA_PROFILE"))}
            },
            _=>Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
        }
        })).await
    }.await;
    tx.rollback().await.map_err(crate::service::store_error)?;
    result
}

async fn publication_prepare(
    tx: &mut SqliteTx,
    key: &OperationKey,
    candidate: &Value,
) -> Result<Value> {
    // This read-only projection has no original write bytes and cannot be
    // retained as request evidence. The actual read ingress already has its
    // own byte ceiling; an invented write wrapper must not consume it again.
    let request =
        super::super::super::request::PublicationRequest::from_read_candidate(key, candidate)?;
    let raw_documents = if let Some(documents) = request.inline_documents() {
        documents
            .iter()
            .map(records::bytes)
            .collect::<Result<Vec<_>>>()?
    } else {
        let mut documents = vec![];
        for r in request
            .retained_refs()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            require(source_kind(&r.kind), "INVALID_REFERENCE")?;
            let doc = loaded(tx, key, r).await?;
            verify_source_origin(tx, key, r).await?;
            documents.push(doc.bytes);
        }
        documents
    };
    let root = core::DefinitionRef::new(
        &request.root().kind,
        &request.root().id,
        &request.root().digest,
    )
    .map_err(transport_error)?;
    let support = core::VerifiedCompilerSupport::builtin().map_err(transport_error)?;
    // Read-only compilation validates the declared expected digest without
    // creating publication, build provenance, claims or cached prices.
    core::compile_publication(&raw_documents, &root, request.expected_digest(), &support)
        .map_err(transport_error)?;
    let sources =
        core::normalize_publication_sources(&raw_documents, &root).map_err(transport_error)?;
    wire(
        super::super::super::publication::evidence(&request, &sources)?["descriptor_resolution"]
            .clone(),
    )
}
