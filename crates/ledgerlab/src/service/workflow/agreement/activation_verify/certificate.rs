//! Closed original transition certificate. The shared invocation ProofSession
//! authenticates every returned origin; no current admission is replayed.
use super::*;
use std::collections::BTreeMap;
fn checked(raw: &[u8], limit: usize) -> Result<Value> {
    verify::checked(raw, limit)
}
fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
fn one(v: &Value, kind: &str) -> Result<RecordRef> {
    let a = v.as_array().ok_or(ServiceError::IntegrityFailure)?;
    if a.len() != 1 {
        return Err(ServiceError::IntegrityFailure);
    }
    let r = reference(&a[0])?;
    if r.kind != kind {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(r)
}
fn add(docs: &mut BTreeMap<String, Document>, d: Document) -> Result<()> {
    if let Some(old) = docs.get(&d.reference.id) {
        if old.reference != d.reference || old.schema != d.schema || old.bytes != d.bytes {
            return Err(ServiceError::IntegrityFailure);
        }
    } else {
        docs.insert(d.reference.id.clone(), d);
    }
    Ok(())
}
pub(in crate::service::workflow::agreement) async fn retained_local(
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
        || o.identity != key.bytes()?
        || !matches!(o.kind.as_str(), "agreement.activate" | "agreement.revoke")
        || op["schema"] != "ledger-workflow-operation-record/1"
        || op["key"] != json!(key)
        || op["kind"] != o.kind
        || op["contract"] != "workflow/1"
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
    let binding_ref = reference(&req.payload()["accepted_binding_ref"])?;
    let accepted = context::observe_accepted_binding(tx, key, &binding_ref, o.sequence).await?;
    let payload = normalize(&req, &accepted.binding).map_err(|_| ServiceError::IntegrityFailure)?;
    let binding = canonical_body(&accepted.binding)?;
    let customer = text(&binding, "customer")?;
    let agreement = text(&binding, "agreement_id")?;
    let evidence = descriptor::evidence(&req, &payload, &accepted.acceptance.descriptors)?;
    if evidence != op["request_evidence"] || evidence["request_hash"] != o.request_hash {
        return Err(ServiceError::IntegrityFailure);
    }
    let a = &op["authority_snapshot"];
    let purpose = if o.kind == "agreement.revoke" {
        "stop"
    } else {
        "admit"
    };
    let keys: &[&str] = if purpose == "admit" {
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
            "canonical_economic_namespace",
            "source_grant_refs",
            "assent_refs",
            "source_revision",
            "source_state_refs",
            "observed_at",
        ]
    } else {
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
            "accepted_agreement_ref",
            "binding_ref",
            "target_transition_ref",
            "expected_activation_revision",
            "binding_source_namespace",
            "source_observation",
            "evidence_refs",
            "observed_at",
        ]
    };
    exact_keys(a, keys)?;
    if a["schema"] != "ledger-workflow-agreement-authority/2"
        || a["purpose"] != purpose
        || a["operation"] != o.kind
        || a["source"] != key.source
        || a["scope"] != json!(key.scope)
        || a["customer"] != customer
        || a["agreement_id"] != agreement
        || a["observed_at"] != json!(at)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let root_ref = reference(&a["local_admin_ref"])?;
    let root = loaded(tx, key, &root_ref).await?;
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
    let original_grant_ref = one(&binding["authority_refs"], "workflow-source-grant")?;
    let original_grant = accepted
        .documents
        .iter()
        .find(|d| d.reference == original_grant_ref)
        .ok_or(ServiceError::IntegrityFailure)?;
    let original_grant =
        ledgerlab_core::policy::workflow::SourceGrant::parse(&original_grant.bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
    let enrollment_ref = reference(&json!(original_grant.original_ceiling_ref()))?;
    let state_ref = if purpose == "admit" {
        one(&a["source_state_refs"], "workflow-source-state")?
    } else {
        reference(&a["source_observation"]["state_ref"])?
    };
    if context::source_as_of(tx, key, &enrollment_ref, o.sequence).await? != state_ref {
        return Err(ServiceError::IntegrityFailure);
    }
    let source = context::observe_source_at(tx, key, &enrollment_ref, &state_ref).await?;
    if purpose == "admit" {
        if a["canonical_economic_namespace"] != binding["source_namespace"]
            || a["assent_refs"] != binding["assent_refs"]
            || a["source_revision"] != source.state().revision().to_string().as_str()
            || one(&a["source_grant_refs"], "workflow-source-grant")?
                != reference(&json!(source
                    .state()
                    .current_grant_ref()
                    .ok_or(ServiceError::IntegrityFailure)?))?
        {
            return Err(ServiceError::IntegrityFailure);
        }
    } else if a["accepted_agreement_ref"] != json!(accepted.accepted.reference)
        || a["binding_ref"] != json!(binding_ref)
        || a["target_transition_ref"] != payload["transition_ref"]
        || a["expected_activation_revision"] != payload["expected_revision"]
        || a["binding_source_namespace"] != binding["source_namespace"]
        || a["evidence_refs"] != payload["evidence_refs"]
        || a["source_observation"] != source_observation(&source, &at)?
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let proofs = verify_capture(
        tx,
        key,
        &accepted,
        &source,
        text(a, "actor")?,
        &at,
        &payload,
        purpose,
    )
    .await
    .map_err(|e| match e {
        ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
        e => e,
    })?;
    let history = schedule::history_before(tx, key, customer, agreement, o.sequence).await?;
    let expected = text(&payload, "expected_revision")?
        .parse::<i64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if history.last().map_or(0, |t| t.revision) != expected {
        return Err(ServiceError::IntegrityFailure);
    }
    let prior = history.last().map(|t| t.head.reference.clone());
    let target = payload
        .get(if purpose == "stop" {
            "transition_ref"
        } else {
            "replaces_transition_ref"
        })
        .map(reference)
        .transpose()?;
    let prepared = build_records(
        &req,
        &payload,
        &TransitionMetadata {
            key,
            actor: text(a, "actor")?,
            at: &at,
            sequence: o.sequence,
            binding: &accepted.binding,
            prior: prior.clone(),
            target: target.clone(),
        },
    )?;
    let mut new_docs = prepared.documents.into_iter();
    let intent = new_docs.next().ok_or(ServiceError::IntegrityFailure)?;
    let head = new_docs.next().ok_or(ServiceError::IntegrityFailure)?;
    let activation = new_docs.next().ok_or(ServiceError::IntegrityFailure)?;
    let next = schedule::transition(
        key,
        customer,
        agreement,
        Document {
            reference: head.reference.clone(),
            schema: head.schema.clone(),
            bytes: head.bytes.clone(),
        },
        Document {
            reference: activation.reference.clone(),
            schema: activation.schema.clone(),
            bytes: activation.bytes.clone(),
        },
        Document {
            reference: accepted.binding.reference.clone(),
            schema: accepted.binding.schema.clone(),
            bytes: accepted.binding.bytes.clone(),
        },
    )?;
    schedule::check_next(&history, &next).map_err(|_| ServiceError::IntegrityFailure)?;
    if op["result_refs"] != json!(prepared.results) {
        return Err(ServiceError::IntegrityFailure);
    }
    let semantic = records::bytes(&json!([
        customer,
        agreement,
        "activation",
        expected
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?
            .to_string()
    ]))?;
    if tx
        .workflow_agreement_semantic(&key.logical_store_id, &key.scope, &o.kind, &semantic)
        .await
        .map_err(crate::service::store_error)?
        .as_deref()
        != Some(o.identity.as_slice())
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut documents = BTreeMap::new();
    for d in accepted
        .documents
        .into_iter()
        .filter(|d| {
            !matches!(
                d.reference.kind.as_str(),
                "workflow-agreement-intent" | "workflow-head-transition"
            )
        })
        .chain(proofs)
        .chain([
            root,
            records::source_document("workflow-authority", a.clone())?,
            intent,
            head,
            activation,
        ])
    {
        add(&mut documents, d)?;
    }
    if let Some(prior) = prior {
        for d in Box::pin(replay::predecessor_closure(tx, key, &prior)).await? {
            add(&mut documents, d)?;
        }
    }
    if let Some(target) = target {
        let t = history
            .iter()
            .find(|t| t.activation.reference == target)
            .ok_or(ServiceError::IntegrityFailure)?;
        add(
            &mut documents,
            Document {
                reference: t.activation.reference.clone(),
                schema: t.activation.schema.clone(),
                bytes: t.activation.bytes.clone(),
            },
        )?;
        for d in Box::pin(replay::predecessor_closure(tx, key, &t.head.reference)).await? {
            add(&mut documents, d)?;
        }
    }
    for r in payload["evidence_refs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let r = reference(r)?;
        verify::document_origin(tx, key, &r).await?;
        let d = loaded(tx, key, &r).await?;
        let body = canonical_body(&d)?;
        let root_ref = reference(&body["authority_ref"])?;
        add(&mut documents, loaded(tx, key, &root_ref).await?)?;
        add(&mut documents, d)?;
    }
    let mut members: Vec<_> = documents.values().map(|d| d.reference.clone()).collect();
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
    if members != indexed || manifest["members"] != json!(members) {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut expected_receipt = json!({"schema":"ledger-operation-receipt/2","key":key,"request_hash":evidence["request_hash"],"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":evidence["schema_bundle_digest"],"descriptor_closure_digest":evidence["descriptor_resolution"]["closure_digest"],"original_request_bytes_sha256":evidence["original_request_bytes_sha256"],"result_kind":"administrative","result_refs":prepared.results,"accepted_at":at,"commit_sequence":sequence,"manifest_hash":o.manifest_ref.digest});
    expected_receipt["receipt_hash"] =
        json!(records::hash("operation-receipt", 2, &expected_receipt)?);
    if receipt != expected_receipt {
        return Err(ServiceError::IntegrityFailure);
    }
    for d in documents.values() {
        let actual = loaded(tx, key, &d.reference).await?;
        if actual.schema != d.schema || actual.bytes != d.bytes {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(verify::LocalCertificate {
        value: json!({"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":evidence}),
        documents: documents.into_values().collect(),
        original: None,
    })
}
