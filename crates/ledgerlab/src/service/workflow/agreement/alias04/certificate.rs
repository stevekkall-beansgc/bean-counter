//! Alias04 recovery verifies the complete original non-alias certificate and
//! historical source observation, without current admission/CAS revalidation.
use super::*;
pub(in crate::service::workflow::agreement) async fn retained_local(
    tx: &mut SqliteTx,
    key: &OperationKey,
    o: &JournalOperation,
) -> Result<verify::LocalCertificate> {
    let op = verify::checked(&o.operation_bytes, 8388608)?;
    let manifest = verify::checked(&o.manifest_bytes, 4194304)?;
    let receipt = verify::checked(&o.receipt_bytes, 1048576)?;
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
    let sequence = o.sequence.to_string();
    let at =
        Timestamp::parse(text(&op, "accepted_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    if o.sequence <= 0
        || o.identity != key.bytes()?
        || op["schema"] != "ledger-workflow-agreement-alias-operation/1"
        || !matches!(
            o.kind.as_str(),
            "agreement.accept" | "agreement.activate" | "agreement.revoke"
        )
        || op["key"] != json!(key)
        || op["kind"] != o.kind
        || op["contract"] != "workflow/1"
        || op["commit_sequence"] != sequence.as_str()
        || at.micros() != o.accepted_at_us
        || manifest["schema"] != "ledger-workflow-agreement-alias-manifest/1"
        || manifest["key"] != json!(key)
        || receipt["schema"] != "ledger-operation-receipt/2"
        || receipt["key"] != json!(key)
        || records::record_reference("workflow-operation", &key.scope, &op)? != o.operation_ref
        || records::record_reference("workflow-manifest", &key.scope, &manifest)? != o.manifest_ref
        || records::record_reference("workflow-receipt", &key.scope, &receipt)? != o.receipt_ref
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let a = &op["authority_snapshot"];
    exact_keys(
        a,
        &[
            "schema",
            "purpose",
            "operation",
            "actor",
            "source",
            "scope",
            "customer",
            "agreement_id",
            "local_admin_ref",
            "original_operation_ref",
            "original_intent_ref",
            "accepted_agreement_ref",
            "binding_ref",
            "binding_source_namespace",
            "source_observation",
            "observed_at",
        ],
    )?;
    if a["schema"] != "ledger-workflow-agreement-authority/3"
        || a["purpose"] != "semantic_alias"
        || a["operation"] != o.kind
        || a["source"] != key.source
        || a["scope"] != json!(key.scope)
        || a["observed_at"] != json!(at)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let original_ref = reference(&a["original_operation_ref"])?;
    let saved = tx
        .workflow_agreement_operation_ref(&key.logical_store_id, &key.scope, &original_ref)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if saved.sequence >= o.sequence || saved.kind != o.kind {
        return Err(ServiceError::IntegrityFailure);
    }
    let data = original(tx, key, &saved).await?;
    let b = canonical_body(&data.accepted.binding)?;
    if a["original_operation_ref"] != json!(saved.operation_ref)
        || a["original_intent_ref"] != json!(data.intent)
        || a["accepted_agreement_ref"] != json!(data.accepted.accepted.reference)
        || a["binding_ref"] != json!(data.accepted.binding.reference)
        || a["binding_source_namespace"] != b["source_namespace"]
        || a["customer"] != b["customer"]
        || a["agreement_id"] != b["agreement_id"]
        || op["result_refs"] != json!(data.results)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let raw = ledgerlab_core::adjudication::proofs::decode_base64(
        text(&op["request_evidence"], "original_request_bytes_base64")?,
        262144,
    )
    .map_err(|_| ServiceError::IntegrityFailure)?;
    let req =
        request::Request::parse_agreement(&raw).map_err(|_| ServiceError::IntegrityFailure)?;
    if req.key() != key || req.kind() != o.kind {
        return Err(ServiceError::IntegrityFailure);
    }
    let payload = attempted(tx, key, &req, &data)
        .await
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if comparison(&o.kind, &payload)? != comparison(&saved.kind, &data.payload)? {
        return Err(ServiceError::IntegrityFailure);
    }
    let reproduced = evidence(&req, &payload, &data.accepted.acceptance.descriptors)?;
    if reproduced != op["request_evidence"] || reproduced["request_hash"] != o.request_hash {
        return Err(ServiceError::IntegrityFailure);
    }
    let semantic_key = semantic(
        &o.kind,
        &payload,
        text(&b, "customer")?,
        text(&b, "agreement_id")?,
    )?;
    if tx
        .workflow_agreement_semantic(&key.logical_store_id, &key.scope, &o.kind, &semantic_key)
        .await
        .map_err(crate::service::store_error)?
        .as_deref()
        != Some(saved.identity.as_slice())
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let root = loaded(tx, key, &reference(&a["local_admin_ref"])?).await?;
    let root_body = canonical_body(&root)?;
    verify::root(&root_body, key, Some(&key.source))?;
    if root_body["basis"]["principal_id"] != a["actor"]
        || Timestamp::parse(text(&root_body, "verified_at")?)
            .map_err(|_| ServiceError::IntegrityFailure)?
            .micros()
            > at.micros()
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let grant_ref = reference(&b["authority_refs"][0])?;
    let grant = data
        .documents
        .iter()
        .find(|d| d.reference == grant_ref)
        .ok_or(ServiceError::IntegrityFailure)?;
    let grant = ledgerlab_core::policy::workflow::SourceGrant::parse(&grant.bytes)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let enrollment = reference(&json!(grant.original_ceiling_ref()))?;
    let state = reference(&a["source_observation"]["state_ref"])?;
    if context::source_as_of(tx, key, &enrollment, o.sequence).await? != state {
        return Err(ServiceError::IntegrityFailure);
    }
    let source = context::observe_source_at(tx, key, &enrollment, &state).await?;
    if a["source_observation"] != activation_verify::source_observation(&source, &at)? {
        return Err(ServiceError::IntegrityFailure);
    }
    let authority = records::source_document("workflow-authority", a.clone())?;
    let mut docs = std::collections::BTreeMap::new();
    for d in data
        .documents
        .into_iter()
        .chain(source.documents().iter().map(copy))
        .chain([root, authority])
    {
        add(&mut docs, d)?;
    }
    let mut members: Vec<_> = docs.values().map(|d| d.reference.clone()).collect();
    members.extend([o.operation_ref.clone(), saved.operation_ref.clone()]);
    records::sort_refs(&mut members)?;
    let mut indexed = o.members.clone();
    records::sort_refs(&mut indexed)?;
    let bytes = docs
        .values()
        .try_fold(
            o.operation_bytes
                .len()
                .checked_add(saved.operation_bytes.len())
                .ok_or(ServiceError::IntegrityFailure)?,
            |n, d| n.checked_add(d.bytes.len()),
        )
        .ok_or(ServiceError::IntegrityFailure)?;
    if bytes > 8388608
        || members.len() > 4096
        || members != indexed
        || manifest["members"] != json!(members)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut expected = json!({"schema":"ledger-operation-receipt/2","key":key,"request_hash":reproduced["request_hash"],"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":reproduced["schema_bundle_digest"],"descriptor_closure_digest":reproduced["descriptor_resolution"]["closure_digest"],"original_request_bytes_sha256":reproduced["original_request_bytes_sha256"],"result_kind":"semantic_alias","result_refs":data.results,"accepted_at":at,"commit_sequence":sequence,"manifest_hash":o.manifest_ref.digest});
    expected["receipt_hash"] = json!(records::hash("operation-receipt", 2, &expected)?);
    if receipt != expected {
        return Err(ServiceError::IntegrityFailure);
    }
    for d in docs.values() {
        let actual = loaded(tx, key, &d.reference).await?;
        if actual.bytes != d.bytes || actual.schema != d.schema {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(verify::LocalCertificate {
        value: json!({"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":reproduced}),
        documents: docs.into_values().collect(),
        original: Some(saved.identity),
    })
}
