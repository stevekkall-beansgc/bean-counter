//! Complete original acceptance certificate. Mutable projections locate
//! immutable records; current admission, compiler and build are never replayed.
use super::super::request::OperationKey;
use super::acceptance::{authority, construct};
use super::*;

fn checked(raw: &[u8], limit: usize) -> Result<Value> {
    let body = canonical::parse_bounded(raw, limit).map_err(|_| ServiceError::IntegrityFailure)?;
    if records::bytes(&body)? != raw {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(body)
}
fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
fn one(v: &Value, family: &str) -> Result<RecordRef> {
    let values = v.as_array().ok_or(ServiceError::IntegrityFailure)?;
    if values.len() != 1 {
        return Err(ServiceError::IntegrityFailure);
    }
    let r = reference(&values[0])?;
    if r.kind != family {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(r)
}
async fn prior_origin(
    tx: &mut SqliteTx,
    key: &OperationKey,
    r: &RecordRef,
    sequence: i64,
) -> Result<()> {
    let (_, kind, origin_sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, r)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let expected = match r.kind.as_str() {
        "workflow-source-enrollment"
        | "workflow-source-grant"
        | "workflow-source-state"
        | "workflow-customer" => "authority.change",
        "workflow-evidence" => "evidence.retain",
        _ => return Err(ServiceError::IntegrityFailure),
    };
    if kind != expected || origin_sequence >= sequence {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(())
}
pub(super) async fn retained_local(
    tx: &mut SqliteTx,
    key: &OperationKey,
    o: &JournalOperation,
) -> Result<verify::LocalCertificate> {
    let op = checked(&o.operation_bytes, 8388608)?;
    let manifest = checked(&o.manifest_bytes, 4194304)?;
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
    let sequence = o.sequence.to_string();
    let at =
        Timestamp::parse(text(&op, "accepted_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    if o.sequence <= 0
        || op["schema"] != "ledger-workflow-operation-record/1"
        || op["kind"] != "agreement.accept"
        || o.kind != "agreement.accept"
        || op["contract"] != "workflow/1"
        || op["key"] != json!(key)
        || o.identity != key.bytes()?
        || op["commit_sequence"] != sequence.as_str()
        || at.micros() != o.accepted_at_us
        || manifest["schema"] != "ledger-workflow-agreement-manifest/1"
        || manifest["key"] != json!(key)
        || receipt["schema"] != "ledger-operation-receipt/2"
        || receipt["key"] != json!(key)
        || records::record_reference("workflow-operation", &key.scope, &op)? != o.operation_ref
        || records::record_reference("workflow-manifest", &key.scope, &manifest)? != o.manifest_ref
        || records::record_reference("workflow-receipt", &key.scope, &receipt)? != o.receipt_ref
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
    let publication_ref = reference(&req.payload()["publication_ref"])?;
    let (_, family, published_sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, &publication_ref)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if family != "policy.publish" || published_sequence >= o.sequence {
        return Err(ServiceError::IntegrityFailure);
    }
    let data = replay::acceptance_data(tx, key, &req)
        .await
        .map_err(|e| match e {
            ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
            e => e,
        })?;
    let payload = data.input.value()?;
    let revision = data
        .input
        .expected_revision()
        .parse::<u64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if revision.checked_add(1).map(|r| r.to_string()).as_deref()
        != payload["binding_inputs"]["version"].as_str()
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let evidence = descriptor::evidence(&req, &payload, &data.descriptors)?;
    if evidence != op["request_evidence"] || evidence["request_hash"] != o.request_hash {
        return Err(ServiceError::IntegrityFailure);
    }
    let a = &op["authority_snapshot"];
    exact_keys(
        a,
        &[
            "schema",
            "purpose",
            "actor",
            "source",
            "canonical_economic_namespace",
            "operation",
            "scope",
            "customer",
            "agreement_id",
            "local_admin_ref",
            "source_grant_refs",
            "assent_refs",
            "observed_at",
            "source_revision",
            "source_state_refs",
        ],
    )?;
    if a["schema"] != "ledger-workflow-agreement-authority/2"
        || a["purpose"] != "admit"
        || a["operation"] != "agreement.accept"
        || a["source"] != key.source
        || a["canonical_economic_namespace"] != payload["binding_inputs"]["source_namespace"]
        || a["scope"] != json!(key.scope)
        || a["customer"] != data.input.customer()
        || a["agreement_id"] != data.input.agreement_id()
        || a["observed_at"] != op["accepted_at"]
        || a["source_grant_refs"] != payload["authority_refs"]
        || a["assent_refs"] != payload["assent_refs"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let root_ref = reference(&a["local_admin_ref"])?;
    if root_ref.kind != "workflow-authority" {
        return Err(ServiceError::IntegrityFailure);
    }
    let root = loaded(tx, key, &root_ref).await?;
    let root_body = canonical_body(&root)?;
    verify::root(&root_body, key, Some(&key.source))?;
    if Timestamp::parse(text(&root_body, "verified_at")?)
        .map_err(|_| ServiceError::IntegrityFailure)?
        .micros()
        > at.micros()
    {
        return Err(ServiceError::IntegrityFailure);
    }
    if root_body["basis"]["principal_id"] != a["actor"] {
        return Err(ServiceError::IntegrityFailure);
    }
    let grant_ref = one(&a["source_grant_refs"], "workflow-source-grant")?;
    let grant = loaded(tx, key, &grant_ref).await?;
    prior_origin(tx, key, &grant_ref, o.sequence).await?;
    let typed = ledgerlab_core::policy::workflow::SourceGrant::parse(&grant.bytes)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let enrollment_ref = reference(&json!(typed.original_ceiling_ref()))?;
    let state_ref = one(&a["source_state_refs"], "workflow-source-state")?;
    prior_origin(tx, key, &enrollment_ref, o.sequence).await?;
    prior_origin(tx, key, &state_ref, o.sequence).await?;
    if context::source_as_of(tx, key, &enrollment_ref, o.sequence).await? != state_ref {
        return Err(ServiceError::IntegrityFailure);
    }
    // Guard every immutable proof origin before the capture helper verifies
    // certificates, so coherent index corruption cannot recurse to this row.
    let customer_id = tx
        .workflow_agreement_customer(&key.logical_store_id, &key.scope, data.input.customer())
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let customer = tx
        .workflow_document_by_id(
            &customer_id,
            &key.logical_store_id,
            &key.scope[0],
            &key.scope[1],
        )
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    prior_origin(tx, key, &customer.reference, o.sequence).await?;
    for d in &data.private_documents {
        let b = checked(&d.bytes, 262144)?;
        let mut refs = vec![];
        if d.reference.kind == "workflow-assent" {
            refs.push(reference(&b["evidence_ref"])?);
            refs.push(reference(&b["operator_attestation_ref"])?);
        }
        if d.reference.kind == "workflow-slot-terms" {
            refs.extend(
                serde_json::from_value::<Vec<RecordRef>>(b["source_grant_refs"].clone())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            );
        }
        for r in refs {
            prior_origin(tx, key, &r, o.sequence).await?;
        }
    }
    for leg in payload["binding_inputs"]["funding_legs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        for r in leg["evidence_refs"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            prior_origin(tx, key, &reference(r)?, o.sequence).await?;
        }
    }
    let source = context::observe_source_at(tx, key, &enrollment_ref, &state_ref).await?;
    let source_revision = source.state().revision().to_string();
    if a["source_revision"].as_str() != Some(source_revision.as_str()) {
        return Err(ServiceError::IntegrityFailure);
    }
    let proofs = authority::verify_capture(
        tx,
        key,
        &data.input,
        &data.definitions,
        &data.private_documents,
        authority::AdmissionContext {
            actor: text(a, "actor")?,
            at: &at,
            source: &source,
        },
    )
    .await
    .map_err(|e| match e {
        ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
        e => e,
    })?;
    let authority = records::source_document("workflow-authority", a.clone())?;
    let results: Vec<RecordRef> = serde_json::from_value(op["result_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let head_ref = results
        .iter()
        .find(|r| r.kind == "workflow-head-transition")
        .ok_or(ServiceError::IntegrityFailure)?;
    let head = loaded(tx, key, head_ref).await?;
    let head_body = checked(&head.bytes, 262144)?;
    let prior = head_body.get("prior_head_ref").map(reference).transpose()?;
    let prepared = construct::build(
        &data.input,
        &data.definitions,
        data.publication.publication(),
        data.publication.plan(),
        &authority,
        construct::CommitMetadata {
            key,
            actor: text(a, "actor")?,
            at: &at,
            sequence: o.sequence,
            prior: prior.clone(),
        },
    )?;
    if prepared.results != results {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut documents = std::collections::BTreeMap::<String, Document>::new();
    for d in data
        .publication
        .documents()
        .iter()
        .map(|d| Document {
            reference: d.reference.clone(),
            schema: d.schema.clone(),
            bytes: d.bytes.clone(),
        })
        .chain(data.private_documents)
        .chain(proofs)
        .chain(prepared.documents)
        .chain([root, authority])
    {
        if let Some(old) = documents.get(&d.reference.id) {
            if old.reference != d.reference || old.bytes != d.bytes || old.schema != d.schema {
                return Err(ServiceError::IntegrityFailure);
            }
        } else {
            documents.insert(d.reference.id.clone(), d);
        }
    }
    if let Some(r) = prior {
        // The predecessor is immutable, not today's mutable head. Its complete
        // originating certificate is verified by the outer origin worklist.
        let d = loaded(tx, key, &r).await?;
        let body = checked(&d.bytes, 262144)?;
        let saved_key = OperationKey::parse(&records::bytes(&body["operation_key"])?)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let old = text(&body, "new_revision")?
            .parse::<u64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        if body["head_kind"] != "agreement_acceptance"
            || body["customer"] != data.input.customer()
            || body["agreement_id"] != data.input.agreement_id()
            || body["new_revision"] != data.input.expected_revision()
            || saved_key.scope != key.scope
            || saved_key.logical_store_id != key.logical_store_id
            || record::semantic_document(
                "workflow-head-transition",
                &saved_key,
                data.input.customer(),
                data.input.agreement_id(),
                old,
                body,
            )?
            .reference
                != r
        {
            return Err(ServiceError::IntegrityFailure);
        }
        let (_, _, prior_sequence) = tx
            .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, &r)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        if prior_sequence >= o.sequence {
            return Err(ServiceError::IntegrityFailure);
        }
        for d in replay::predecessor_closure(tx, key, &r).await? {
            if let Some(old) = documents.get(&d.reference.id) {
                if old.reference != d.reference || old.schema != d.schema || old.bytes != d.bytes {
                    return Err(ServiceError::IntegrityFailure);
                }
            } else {
                documents.insert(d.reference.id.clone(), d);
            }
        }
    }
    let semantic = records::bytes(&json!([
        data.input.customer(),
        data.input.agreement_id(),
        payload["binding_inputs"]["version"]
    ]))?;
    if tx
        .workflow_agreement_semantic(
            &key.logical_store_id,
            &key.scope,
            "agreement.accept",
            &semantic,
        )
        .await
        .map_err(crate::service::store_error)?
        .as_deref()
        != Some(o.identity.as_slice())
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut members = documents
        .values()
        .map(|d| d.reference.clone())
        .collect::<Vec<_>>();
    let bytes = documents
        .values()
        .try_fold(o.operation_bytes.len(), |n, d| n.checked_add(d.bytes.len()))
        .ok_or(ServiceError::IntegrityFailure)?;
    if bytes > 8388608 || members.len() >= 4096 {
        return Err(ServiceError::IntegrityFailure);
    }
    members.push(o.operation_ref.clone());
    records::sort_refs(&mut members)?;
    let mut indexed = o.members.clone();
    records::sort_refs(&mut indexed)?;
    if manifest["members"] != json!(members) || members != indexed {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut expected = json!({"schema":"ledger-operation-receipt/2","key":key,"request_hash":evidence["request_hash"],"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":evidence["schema_bundle_digest"],"descriptor_closure_digest":evidence["descriptor_resolution"]["closure_digest"],"original_request_bytes_sha256":evidence["original_request_bytes_sha256"],"result_kind":"administrative","result_refs":results,"accepted_at":at,"commit_sequence":sequence,"manifest_hash":o.manifest_ref.digest});
    expected["receipt_hash"] = json!(records::hash("operation-receipt", 2, &expected)?);
    if receipt != expected {
        return Err(ServiceError::IntegrityFailure);
    }
    for d in documents.values() {
        if loaded(tx, key, &d.reference).await?.bytes != d.bytes {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(verify::LocalCertificate {
        value: json!({"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":evidence}),
        documents: documents.into_values().collect(),
        original: None,
    })
}
