//! Dedicated reviewed alias authority/operation/manifest recipe. Reuses an
//! original non-alias result; it never performs fresh admission or head CAS.
use super::super::{local::LocalWorkflowLedger, request::OperationKey};
use super::*;
use crate::service::billing::Setup;
use crate::store::ports::AcceptanceTx;
mod certificate;
pub(super) use certificate::retained_local;
fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
fn copy(d: &Document) -> Document {
    Document {
        reference: d.reference.clone(),
        schema: d.schema.clone(),
        bytes: d.bytes.clone(),
    }
}
fn add(docs: &mut std::collections::BTreeMap<String, Document>, d: Document) -> Result<()> {
    if let Some(prior) = docs.get(&d.reference.id) {
        if prior.reference != d.reference || prior.bytes != d.bytes || prior.schema != d.schema {
            return Err(ServiceError::IntegrityFailure);
        }
    } else {
        docs.insert(d.reference.id.clone(), d);
    }
    Ok(())
}
pub(super) fn evidence(
    req: &request::Request,
    payload: &Value,
    descriptors: &[RecordRef],
) -> Result<Value> {
    // Alias04 freezes its own payload tuple; accept3 has a distinct original
    // recipe and cannot be silently relabelled as the older alias recipe.
    require(
        req.kind() != "agreement.accept"
            || payload["schema"] == "ledger-workflow-agreement-accept/2",
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let mut v = descriptor::evidence(req, payload, descriptors)?;
    let registry=canonical::parse_bounded(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/agreement-alias-04-registry.json"),65536).map_err(|_|ServiceError::IntegrityFailure)?;
    let resolution = &mut v["descriptor_resolution"];
    resolution["registry_digest"] = json!(format!(
        "sha256:{}",
        records::raw_hash(&records::bytes(&registry)?)
    ));
    resolution
        .as_object_mut()
        .ok_or(ServiceError::IntegrityFailure)?
        .remove("closure_digest");
    resolution["closure_digest"] = json!(records::hash("descriptor-closure", 1, resolution)?);
    v["request_hash"] = json!(records::hash(
        "operation-request",
        2,
        &json!([
            v["descriptor_resolution"]["closure_digest"],
            v["normalized_request"]
        ])
    )?);
    Ok(v)
}
fn semantic(kind: &str, payload: &Value, customer: &str, agreement: &str) -> Result<Vec<u8>> {
    if kind == "agreement.accept" {
        records::bytes(&json!([
            customer,
            agreement,
            payload["binding_inputs"]["version"]
        ]))
    } else {
        let n = text(payload, "expected_revision")?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        records::bytes(&json!([customer, agreement, "activation", n.to_string()]))
    }
}
fn comparison(kind: &str, payload: &Value) -> Result<Value> {
    let mut p = payload.clone();
    if kind == "agreement.accept" {
        p.as_object_mut()
            .ok_or(ServiceError::IntegrityFailure)?
            .remove("documents");
    }
    Ok(p)
}
struct Original {
    accepted: context::ObservedAcceptedBinding,
    payload: Value,
    results: Vec<RecordRef>,
    intent: RecordRef,
    documents: Vec<Document>,
}
async fn original(tx: &mut SqliteTx, key: &OperationKey, o: &JournalOperation) -> Result<Original> {
    let k = OperationKey::parse(&o.identity).map_err(|_| ServiceError::IntegrityFailure)?;
    if k.logical_store_id != key.logical_store_id || k.scope != key.scope || k.source != key.source
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let value = Box::pin(verify::retained(tx, &k, o)).await?;
    let body = verify::checked(&o.operation_bytes, 8388608)?;
    if body["schema"] != "ledger-workflow-operation-record/1"
        || value["receipt"]["result_kind"] != "administrative"
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let payload = value["request_evidence"]["normalized_request"]["payload"].clone();
    let results: Vec<RecordRef> = serde_json::from_value(value["receipt"]["result_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let intents: Vec<_> = results
        .iter()
        .filter(|r| r.kind == "workflow-agreement-intent")
        .collect();
    if intents.len() != 1 {
        return Err(ServiceError::IntegrityFailure);
    }
    let intent = intents[0].clone();
    let binding_ref = if o.kind == "agreement.accept" {
        let accepted: Vec<_> = results
            .iter()
            .filter(|r| r.kind == "workflow-accepted-agreement")
            .collect();
        if accepted.len() != 1 {
            return Err(ServiceError::IntegrityFailure);
        }
        let d = loaded(tx, key, accepted[0]).await?;
        reference(&verify::checked(&d.bytes, 262144)?["binding_ref"])?
    } else {
        reference(&payload["accepted_binding_ref"])?
    };
    let accepted = context::observe_accepted_binding(
        tx,
        key,
        &binding_ref,
        o.sequence
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?,
    )
    .await?;
    let mut documents = vec![];
    for r in &o.members {
        if *r != o.operation_ref {
            if r.kind == "workflow-operation" {
                return Err(ServiceError::IntegrityFailure);
            }
            documents.push(loaded(tx, key, r).await?);
        }
    }
    Ok(Original {
        accepted,
        payload,
        results,
        intent,
        documents,
    })
}
async fn attempted(
    tx: &mut SqliteTx,
    key: &OperationKey,
    req: &request::Request,
    original: &Original,
) -> Result<Value> {
    if req.kind() == "agreement.accept" {
        let data = replay::acceptance_data(tx, key, req).await?;
        require(
            data.publication.publication().reference
                == original
                    .accepted
                    .acceptance
                    .publication
                    .publication()
                    .reference,
            "SEMANTIC_CONFLICT",
        )?;
        data.input.value()
    } else {
        activation_verify::normalize(req, &original.accepted.binding)
    }
}
// Exact reviewed authority3 dictionary, derived only from verified observations.
#[allow(clippy::too_many_arguments)]
fn authority(
    req: &request::Request,
    setup: &Setup,
    at: &Timestamp,
    root: &RecordRef,
    o: &JournalOperation,
    original: &Original,
    source: &context::ObservedSource,
) -> Result<Document> {
    let b = canonical_body(&original.accepted.binding)?;
    records::source_document(
        "workflow-authority",
        json!({"schema":"ledger-workflow-agreement-authority/3","purpose":"semantic_alias","operation":req.kind(),
        "actor":setup.operator,"source":req.key().source,"scope":req.key().scope,"customer":b["customer"],"agreement_id":b["agreement_id"],"local_admin_ref":root,
        "original_operation_ref":o.operation_ref,"original_intent_ref":original.intent,"accepted_agreement_ref":original.accepted.accepted.reference,"binding_ref":original.accepted.binding.reference,
        "binding_source_namespace":b["source_namespace"],"source_observation":activation_verify::source_observation(source,at)?,"observed_at":at}),
    )
}
/// Exact wrapper encoding from previously certified result/authority data.
/// This data helper cannot confer admin authority or construct a public append.
fn wrappers(
    req: &request::Request,
    evidence: &Value,
    snapshot: &Value,
    at: &Timestamp,
    sequence: i64,
    results: &[RecordRef],
    mut members: Vec<RecordRef>,
) -> Result<JournalOperation> {
    let key = req.key();
    let op = json!({"schema":"ledger-workflow-agreement-alias-operation/1","key":key,"kind":req.kind(),"contract":"workflow/1","request_evidence":evidence,"authority_snapshot":snapshot,"accepted_at":at,"commit_sequence":sequence.to_string(),"result_refs":results});
    let operation_ref = records::record_reference("workflow-operation", &key.scope, &op)?;
    members.push(operation_ref.clone());
    records::sort_refs(&mut members)?;
    require(members.len() <= 4096, "PROGRAM_BUDGET_EXCEEDED")?;
    let manifest =
        json!({"schema":"ledger-workflow-agreement-alias-manifest/1","key":key,"members":members});
    let manifest_ref = records::record_reference("workflow-manifest", &key.scope, &manifest)?;
    let mut receipt = json!({"schema":"ledger-operation-receipt/2","key":key,"request_hash":evidence["request_hash"],"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":evidence["schema_bundle_digest"],"descriptor_closure_digest":evidence["descriptor_resolution"]["closure_digest"],"original_request_bytes_sha256":evidence["original_request_bytes_sha256"],"result_kind":"semantic_alias","result_refs":results,"accepted_at":at,"commit_sequence":sequence.to_string(),"manifest_hash":manifest_ref.digest});
    receipt["receipt_hash"] = json!(records::hash("operation-receipt", 2, &receipt)?);
    let receipt_ref = records::record_reference("workflow-receipt", &key.scope, &receipt)?;
    Ok(JournalOperation {
        identity: key.bytes()?,
        kind: req.kind().into(),
        operation_ref,
        operation_bytes: records::bytes(&op)?,
        request_hash: text(evidence, "request_hash")?.into(),
        manifest_ref,
        manifest_bytes: records::bytes(&manifest)?,
        receipt_ref,
        receipt_bytes: records::bytes(&receipt)?,
        sequence,
        accepted_at_us: at.micros(),
        members,
    })
}
// One reviewed journal dictionary: metadata stays explicit at this private boundary.
#[allow(clippy::too_many_arguments)]
fn assemble(
    req: &request::Request,
    evidence: Value,
    setup: &Setup,
    at: &Timestamp,
    sequence: i64,
    root: Document,
    authority: Document,
    original: &JournalOperation,
    data: &Original,
    source: &context::ObservedSource,
) -> Result<JournalAppend> {
    let mut documents = std::collections::BTreeMap::new();
    for d in data
        .documents
        .iter()
        .chain(source.documents())
        .map(copy)
        .chain([root, authority])
    {
        add(&mut documents, d)?;
    }
    let snapshot = canonical_body(
        documents
            .values()
            .find(|d| d.schema == "ledger-workflow-agreement-authority/3")
            .ok_or(ServiceError::IntegrityFailure)?,
    )?;
    let key = req.key();
    let mut members: Vec<_> = documents.values().map(|d| d.reference.clone()).collect();
    members.push(original.operation_ref.clone());
    let operation = wrappers(
        req,
        &evidence,
        &snapshot,
        at,
        sequence,
        &data.results,
        members,
    )?;
    let bytes = documents
        .values()
        .try_fold(
            operation
                .operation_bytes
                .len()
                .checked_add(original.operation_bytes.len())
                .ok_or(ServiceError::IntegrityFailure)?,
            |n, d| n.checked_add(d.bytes.len()),
        )
        .ok_or(ServiceError::IntegrityFailure)?;
    require(bytes <= 8388608, "PROGRAM_BUDGET_EXCEEDED")?;
    let b = canonical_body(&data.accepted.binding)?;
    let semantic_key = semantic(
        req.kind(),
        &evidence["normalized_request"]["payload"],
        text(&b, "customer")?,
        text(&b, "agreement_id")?,
    )?;
    let _ = setup; // Setup was actual/authenticated before constructing authority.
    Ok(JournalAppend {
        store: key.logical_store_id.clone(),
        tenant: key.scope[0].clone(),
        environment: key.scope[1].clone(),
        source: key.source.clone(),
        operation_id: key.operation_id.clone(),
        documents: documents.into_values().collect(),
        operation,
        semantic_key,
        alias_of: Some(original.identity.clone()),
        projection: Projection::None,
    })
}
pub(super) async fn prepare(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &request::Request,
    o: &JournalOperation,
    normalized: &Value,
) -> Result<JournalAppend> {
    let key = req.key();
    let data = original(tx, key, o).await?;
    require(o.kind == req.kind(), "SEMANTIC_CONFLICT")?;
    let p = attempted(tx, key, req, &data).await?;
    require(
        p == *normalized
            && comparison(req.kind(), &p)? == comparison(o.kind.as_str(), &data.payload)?,
        "SEMANTIC_CONFLICT",
    )?;
    let b = canonical_body(&data.accepted.binding)?;
    let semantic_key = semantic(
        req.kind(),
        &p,
        text(&b, "customer")?,
        text(&b, "agreement_id")?,
    )?;
    require(
        tx.workflow_agreement_semantic(
            &key.logical_store_id,
            &key.scope,
            req.kind(),
            &semantic_key,
        )
        .await
        .map_err(crate::service::store_error)?
        .as_deref()
            == Some(o.identity.as_slice()),
        "SEMANTIC_CONFLICT",
    )?;
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let installation = tx
        .load_installation()
        .await
        .map_err(crate::service::store_error)?;
    require(installation.mode == "real", "INSTALLATION_MODE_MISMATCH")?;
    let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let root = match tx
        .workflow_agreement_root_candidate(&key.logical_store_id, &key.scope)
        .await
        .map_err(crate::service::store_error)?
    {
        Some(d) => {
            verify::document_origin(tx, key, &d.reference).await?;
            let body = canonical_body(&d)?;
            verify::root(&body, key, None)?;
            if body["basis"] == journal::basis(&setup) {
                d
            } else {
                journal::fresh_root(&setup, &at)?
            }
        }
        None => journal::fresh_root(&setup, &at)?,
    };
    let grant_ref = reference(&b["authority_refs"][0])?;
    let grant = data
        .documents
        .iter()
        .find(|d| d.reference == grant_ref)
        .ok_or(ServiceError::IntegrityFailure)?;
    let grant =
        ledgerlab_core::policy::workflow::SourceGrant::parse(&grant.bytes).map_err(typed_error)?;
    let source =
        context::observe_source(tx, key, &reference(&json!(grant.original_ceiling_ref()))?).await?;
    let sequence = tx
        .workflow_sequence()
        .await
        .map_err(crate::service::store_error)?;
    let authority = authority(req, &setup, &at, &root.reference, o, &data, &source)?;
    assemble(
        req,
        evidence(req, &p, &data.accepted.acceptance.descriptors)?,
        &setup,
        &at,
        sequence,
        root,
        authority,
        o,
        &data,
        &source,
    )
}

#[cfg(test)]
mod tests;
