//! Full source-retention certificate verification under the saved compiled
//! registry recipe. Current compiler support/freshness is deliberately absent.
use super::*;
fn checked(raw: &[u8], limit: usize) -> Result<Value> {
    let v = canonical::parse_bounded(raw, limit).map_err(|_| ServiceError::IntegrityFailure)?;
    if records::bytes(&v)? != raw {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(v)
}
pub(in super::super) async fn retained_local(
    tx: &mut SqliteTx,
    key: &OperationKey,
    o: &JournalOperation,
) -> Result<verify::LocalCertificate> {
    let op = checked(&o.operation_bytes, 8 * 1024 * 1024)?;
    let manifest = checked(&o.manifest_bytes, 4 * 1024 * 1024)?;
    let receipt = checked(&o.receipt_bytes, 1048576)?;
    exact_keys(
        &op,
        &[
            "schema",
            "key",
            "kind",
            "contract",
            "request_evidence",
            "authority_snapshot",
            "accepted_at",
            "commit_sequence",
            "result_refs",
        ],
    )?;
    exact_keys(&manifest, &["schema", "key", "members"])?;
    exact_keys(
        &receipt,
        &[
            "schema",
            "key",
            "request_hash",
            "request_hash_profile",
            "normalization_profile",
            "schema_bundle_digest",
            "descriptor_closure_digest",
            "original_request_bytes_sha256",
            "result_kind",
            "result_refs",
            "accepted_at",
            "commit_sequence",
            "manifest_hash",
            "receipt_hash",
        ],
    )?;
    if op["schema"] != "ledger-workflow-operation-record/1"
        || op["key"] != json!(key)
        || op["kind"] != "evidence.retain"
        || o.kind != "evidence.retain"
        || op["contract"] != "workflow/1"
        || o.identity != key.bytes()?
        || op["commit_sequence"] != o.sequence.to_string()
        || Timestamp::parse(text(&op, "accepted_at")?)
            .map_err(|_| ServiceError::IntegrityFailure)?
            .micros()
            != o.accepted_at_us
        || manifest["schema"] != "ledger-workflow-definition-manifest/1"
        || manifest["key"] != json!(key)
        || receipt["schema"] != "ledger-operation-receipt/2"
        || receipt["key"] != json!(key)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    if records::record_reference("workflow-operation", &key.scope, &op)? != o.operation_ref
        || records::record_reference("workflow-manifest", &key.scope, &manifest)? != o.manifest_ref
        || records::record_reference("workflow-receipt", &key.scope, &receipt)? != o.receipt_ref
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let evidence = &op["request_evidence"];
    if evidence["descriptor_resolution"]["registry_digest"] != evidence::registry_digest()? {
        return Err(ServiceError::IntegrityFailure);
    }
    let raw = ledgerlab_core::adjudication::proofs::decode_base64(
        text(evidence, "original_request_bytes_base64")?,
        262144,
    )
    .map_err(|_| ServiceError::IntegrityFailure)?;
    let req = request::Request::parse(&raw).map_err(|_| ServiceError::IntegrityFailure)?;
    if req.key() != key || req.kind() != o.kind || !selected(req.payload()) {
        return Err(ServiceError::IntegrityFailure);
    }
    let resolution = resolve::resolve(tx, key, req.payload(), false)
        .await
        .map_err(|e| match e {
            ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
            e => e,
        })?;
    if evidence::request_evidence(&req, &resolution)? != *evidence
        || evidence["request_hash"] != o.request_hash
        || receipt["request_hash"] != o.request_hash
        || receipt["request_hash_profile"] != "bean-counter/operation-request/2"
        || receipt["normalization_profile"] != evidence["normalization_profile"]
        || receipt["schema_bundle_digest"] != evidence["schema_bundle_digest"]
        || receipt["descriptor_closure_digest"]
            != evidence["descriptor_resolution"]["closure_digest"]
        || receipt["original_request_bytes_sha256"] != evidence["original_request_bytes_sha256"]
        || receipt["manifest_hash"] != o.manifest_ref.digest
        || receipt["accepted_at"] != op["accepted_at"]
        || receipt["commit_sequence"] != op["commit_sequence"]
        || receipt["result_refs"] != op["result_refs"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let authority = &op["authority_snapshot"];
    exact_keys(
        authority,
        &[
            "schema",
            "principal_id",
            "source",
            "authority_refs",
            "rights",
            "validated_at",
        ],
    )?;
    let roots: Vec<RecordRef> = serde_json::from_value(authority["authority_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if authority["schema"] != "ledger-workflow-publication-authority/1"
        || authority["source"] != key.source
        || authority["rights"] != json!(["admin"])
        || authority["validated_at"] != op["accepted_at"]
        || roots.len() != 1
        || roots[0].kind != "workflow-authority"
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let root = loaded(tx, key, &roots[0]).await.map_err(|e| match e {
        ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
        e => e,
    })?;
    let root_body = canonical_body(&root)?;
    verify::root(&root_body, key, Some(&key.source))?;
    if root_body["basis"]["principal_id"] != authority["principal_id"] {
        return Err(ServiceError::IntegrityFailure);
    }
    let result = if let Some(input) = resolution.input.fragment() {
        records::source_document(
            "workflow-definition-fragment",
            input
                .record_value(&key.logical_store_id, &key.scope, &key.source)
                .map_err(|_| ServiceError::IntegrityFailure)?,
        )?
    } else {
        document(
            resolution
                .definition
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .document(),
        )?
    };
    let results: Vec<RecordRef> = serde_json::from_value(op["result_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if results != vec![result.reference.clone()] {
        return Err(ServiceError::IntegrityFailure);
    }
    let actual = loaded(tx, key, &result.reference)
        .await
        .map_err(|e| match e {
            ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
            e => e,
        })?;
    if result.bytes != actual.bytes || result.schema != actual.schema {
        return Err(ServiceError::IntegrityFailure);
    }
    if let Some(definition) = &resolution.definition {
        if let Some(entry) = registry(definition.document())? {
            if tx
                .workflow_transport_definition(
                    &key.logical_store_id,
                    &key.scope,
                    &entry.kind,
                    &entry.semantic_id,
                    &entry.version,
                )
                .await
                .map_err(crate::service::store_error)?
                .as_deref()
                != Some(entry.document_id.as_str())
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
    } else {
        let input = resolution
            .input
            .fragment()
            .ok_or(ServiceError::IntegrityFailure)?;
        let upload = tx
            .workflow_transport_upload(
                &key.logical_store_id,
                &key.scope,
                &key.source,
                input.upload_id(),
            )
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        if upload.document_schema != input.document_schema()
            || upload.digest != input.document_sha256()
            || upload.bytes != input.document_bytes() as i64
            || tx
                .workflow_transport_part(
                    &key.logical_store_id,
                    &key.scope,
                    &key.source,
                    input.upload_id(),
                    input.part() as i64,
                )
                .await
                .map_err(crate::service::store_error)?
                .as_deref()
                != Some(result.reference.id.as_str())
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    let (_, _, origin_sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, &result.reference)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if origin_sequence > o.sequence
        || receipt["result_kind"]
            != if origin_sequence < o.sequence {
                "semantic_alias"
            } else {
                "administrative"
            }
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let semantic = semantic(key, &resolution)?;
    let semantic_origin = tx
        .workflow_agreement_semantic(&key.logical_store_id, &key.scope, &o.kind, &semantic)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if tx
        .workflow_transport_result_origin(&key.logical_store_id, &key.scope, &result.reference)
        .await
        .map_err(crate::service::store_error)?
        .as_deref()
        != Some(semantic_origin.as_slice())
    {
        return Err(ServiceError::IntegrityFailure);
    }
    if semantic_origin != o.identity {
        let original = tx
            .workflow_agreement_lookup(&semantic_origin)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let body = checked(&original.operation_bytes, 8 * 1024 * 1024)?;
        if original.sequence >= o.sequence
            || body["result_refs"] != op["result_refs"]
            || !selected(&body["request_evidence"]["normalized_request"]["payload"])
        {
            return Err(ServiceError::IntegrityFailure);
        }
        // The shared worklist verifies this complete original certificate too;
        // it may itself legitimately alias a definition introduced by PUB.
    }
    let mut documents = vec![actual, root];
    documents.extend(resolution.dependencies);
    documents.extend(resolution.fragments);
    let mut unique = std::collections::BTreeMap::new();
    for d in documents {
        if let Some(previous) = unique.insert(d.reference.id.clone(), d) {
            let new = &unique[&previous.reference.id];
            if previous.reference != new.reference || previous.bytes != new.bytes {
                return Err(ServiceError::IntegrityFailure);
            }
        }
    }
    let documents: Vec<_> = unique.into_values().collect();
    let mut members: Vec<_> = documents.iter().map(|d| d.reference.clone()).collect();
    members.push(o.operation_ref.clone());
    records::sort_refs(&mut members)?;
    let mut indexed = o.members.clone();
    records::sort_refs(&mut indexed)?;
    if indexed != members || manifest["members"] != json!(members) {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(verify::LocalCertificate {
        value: json!({"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":evidence}),
        documents,
        original: if semantic_origin == o.identity {
            None
        } else {
            Some(semantic_origin)
        },
    })
}
