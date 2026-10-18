//! Retained raw→normalized linkage and complete typed dependency verification.
use super::super::request::OperationKey;
use super::*;

#[cfg(test)]
tokio::task_local! {
    static ORIGIN_COUNTS: std::cell::RefCell<std::collections::BTreeMap<Vec<u8>,usize>>;
}
#[cfg(test)]
pub(super) async fn audit_origins<T>(
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<(T, std::collections::BTreeMap<Vec<u8>, usize>)> {
    ORIGIN_COUNTS
        .scope(
            std::cell::RefCell::new(std::collections::BTreeMap::new()),
            async {
                let result = future.await?;
                Ok((result, ORIGIN_COUNTS.with(|counts| counts.borrow().clone())))
            },
        )
        .await
}
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(super) struct WorkCounts {
    started: Option<std::time::Instant>,
    pub(super) publications: std::collections::BTreeMap<Vec<u8>, usize>,
    pub(super) document_origins: std::collections::BTreeMap<String, usize>,
    pub(super) helper_document_loads: std::collections::BTreeMap<String, usize>,
}
#[cfg(test)]
tokio::task_local! { static WORK_COUNTS: std::cell::RefCell<WorkCounts>; }
#[cfg(test)]
pub(super) async fn audit_work<T>(
    future: impl std::future::Future<Output = Result<T>>,
) -> (
    Result<T>,
    std::collections::BTreeMap<Vec<u8>, usize>,
    WorkCounts,
) {
    WORK_COUNTS
        .scope(
            std::cell::RefCell::new(WorkCounts {
                started: Some(std::time::Instant::now()),
                ..WorkCounts::default()
            }),
            async {
                ORIGIN_COUNTS
                    .scope(
                        std::cell::RefCell::new(std::collections::BTreeMap::new()),
                        async {
                            let value = future.await;
                            (
                                value,
                                ORIGIN_COUNTS.with(|counts| counts.borrow().clone()),
                                WORK_COUNTS.with(|counts| counts.borrow().clone()),
                            )
                        },
                    )
                    .await
            },
        )
        .await
}
#[cfg(test)]
pub(super) fn diagnostic_checkpoint(label: &str) {
    let _ = WORK_COUNTS.try_with(|counts| {
        let counts = counts.borrow();
        eprintln!(
            "ACCEPT-DIAG {label} since_call_ms={} publications={} document_origins={} selected_loaded_helper_total={}",
            counts.started.map(|start| start.elapsed().as_millis()).unwrap_or(0),
            counts.publications.values().sum::<usize>(),
            counts.document_origins.values().sum::<usize>(),
            counts.helper_document_loads.values().sum::<usize>()
        );
    });
}
#[cfg(test)]
pub(super) fn count_document_load(reference: &RecordRef) {
    let _ = WORK_COUNTS.try_with(|counts| {
        *counts
            .borrow_mut()
            .helper_document_loads
            .entry(reference.id.clone())
            .or_default() += 1;
    });
}
#[cfg(test)]
fn count_origin(identity: &[u8]) {
    let _ = ORIGIN_COUNTS.try_with(|counts| {
        *counts.borrow_mut().entry(identity.to_vec()).or_default() += 1;
    });
}

pub(super) fn root(body: &Value, key: &OperationKey, expected_source: Option<&str>) -> Result<()> {
    exact_keys(
        body,
        &[
            "schema",
            "basis",
            "basis_digest",
            "verification_profile",
            "verified_at",
        ],
    )?;
    let b = &body["basis"];
    exact_keys(
        b,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "rights",
            "source_grant_revision",
        ],
    )?;
    if body["schema"] != "ledger-workflow-local-authority-snapshot/1"
        || body["verification_profile"] != "local-private-directory-owner/1"
        || b["schema"] != "ledger-workflow-local-authority-basis/1"
        || b["logical_store_id"] != key.logical_store_id
        || b["scope"] != json!(key.scope)
        || b["rights"] != json!(["admin", "read"])
        || b["canonical_economic_namespace"] != b["source"]
        || records::hash("local-authority-basis", 1, b)? != body["basis_digest"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let revision = text(b, "source_grant_revision")?;
    if revision
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .as_deref()
        != Some(revision)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Timestamp::parse(text(body, "verified_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    for field in ["principal_id", "source"] {
        if text(b, field)?.is_empty() {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    if expected_source.is_some_and(|s| b["source"] != s) {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(())
}
fn parse_ref(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
fn dependencies(body: &Value, kind: &str, key: &OperationKey) -> Result<Vec<RecordRef>> {
    match kind {
        "workflow-authority" => {
            root(body, key, None)?;
            Ok(vec![])
        }
        "workflow-evidence" => {
            let input = evidence_input(body)?;
            Timestamp::parse(text(body, "retained_at")?)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            if !matches!(
                body["installation_mode"].as_str(),
                Some("real_local" | "isolated_synthetic")
            ) || text(body, "retained_by")?.is_empty()
            {
                return Err(ServiceError::IntegrityFailure);
            }
            let mut refs = vec![parse_ref(&body["authority_ref"])?];
            if input.purpose() == "operator_attestation" {
                refs.push(parse_ref(&input.metadata()["assent_evidence_ref"])?);
            } else if input.purpose() == "admission_stop" {
                return Err(ServiceError::IntegrityFailure);
            }
            Ok(refs)
        }
        "workflow-customer" => {
            exact_keys(
                body,
                &[
                    "schema",
                    "scope",
                    "customer",
                    "acceptor",
                    "evidence_ref",
                    "registered_by",
                    "registered_at",
                    "authority_ref",
                    "installation_mode",
                ],
            )?;
            if body["schema"] != "ledger-workflow-customer-registration/1"
                || body["scope"] != json!(key.scope)
                || !matches!(
                    body["installation_mode"].as_str(),
                    Some("real_local" | "isolated_synthetic")
                )
            {
                return Err(ServiceError::IntegrityFailure);
            }
            Timestamp::parse(text(body, "registered_at")?)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            for field in ["customer", "acceptor", "registered_by"] {
                if text(body, field)?.is_empty() {
                    return Err(ServiceError::IntegrityFailure);
                }
            }
            Ok(vec![
                parse_ref(&body["evidence_ref"])?,
                parse_ref(&body["authority_ref"])?,
            ])
        }
        "workflow-source-enrollment" => {
            let e =
                ledgerlab_core::policy::workflow::SourceEnrollment::parse(&records::bytes(body)?)
                    .map_err(|_| ServiceError::IntegrityFailure)?;
            if e.scope() != &key.scope {
                return Err(ServiceError::IntegrityFailure);
            }
            let mut refs: Vec<RecordRef> = serde_json::from_value(body["customer_refs"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            refs.push(parse_ref(&body["evidence_ref"])?);
            refs.push(parse_ref(&body["authority_ref"])?);
            Ok(refs)
        }
        "workflow-source-grant" => {
            ledgerlab_core::policy::workflow::SourceGrant::parse(&records::bytes(body)?)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            Ok(vec![parse_ref(&body["original_ceiling_ref"])?])
        }
        "workflow-source-state" => {
            let mut refs = vec![parse_ref(&body["enrollment_ref"])?];
            for field in ["current_grant_ref", "prior_state_ref"] {
                if let Some(v) = body.get(field) {
                    refs.push(parse_ref(v)?);
                }
            }
            Ok(refs)
        }
        _ => Err(ServiceError::IntegrityFailure),
    }
}
pub(super) async fn closure(
    tx: &mut SqliteTx,
    key: &OperationKey,
    seeds: &[RecordRef],
) -> Result<Vec<Document>> {
    for seed in seeds.iter().filter(|r| r.kind == "workflow-evidence") {
        let evidence = super::loaded(tx, key, seed).await?;
        let body = canonical_body(&evidence)?;
        let input = evidence_input(&body)?;
        if input.purpose() == "admission_stop" {
            let mut docs = activation_verify::stop_evidence_dependencies(tx, key, &input).await?;
            let root = parse_ref(&body["authority_ref"])?;
            let root_doc = super::loaded(tx, key, &root).await?;
            let root_body = canonical_body(&root_doc)?;
            require(
                root_body["basis"]["principal_id"] == body["retained_by"],
                "INTEGRITY_FAILURE",
            )?;
            docs.push(evidence);
            docs.push(root_doc);
            for seed in seeds.iter().filter(|r| r.kind == "workflow-authority") {
                docs.push(super::loaded(tx, key, seed).await?);
            }
            let mut unique = std::collections::BTreeMap::new();
            for d in docs {
                if let Some(old) = unique.insert(d.reference.id.clone(), d) {
                    let d = unique
                        .get(&old.reference.id)
                        .ok_or(ServiceError::IntegrityFailure)?;
                    require(
                        d.reference == old.reference && d.bytes == old.bytes,
                        "INTEGRITY_FAILURE",
                    )?;
                }
            }
            let docs: Vec<Document> = unique.into_values().collect();
            require(
                docs.len() <= 4095 && docs.iter().map(|d| d.bytes.len()).sum::<usize>() <= 8388608,
                "PROGRAM_BUDGET_EXCEEDED",
            )?;
            return Ok(docs);
        }
    }
    let mut todo = seeds.to_vec();
    let mut loaded = std::collections::BTreeMap::new();
    let mut total = 0usize;
    while let Some(r) = todo.pop() {
        if let Some(d) = loaded.get(&r.id) {
            let d: &Document = d;
            if d.reference != r {
                return Err(ServiceError::IntegrityFailure);
            }
            continue;
        }
        require(loaded.len() < 4095, "PROGRAM_BUDGET_EXCEEDED")?;
        let d = super::loaded(tx, key, &r).await.map_err(|e| match e {
            ServiceError::Rejection(code) if code == "AUTHORITY_REQUIRED" => {
                ServiceError::IntegrityFailure
            }
            e => e,
        })?;
        let body = canonical_body(&d)?;
        total = total
            .checked_add(d.bytes.len())
            .ok_or(ServiceError::IntegrityFailure)?;
        require(total <= 8 * 1024 * 1024, "PROGRAM_BUDGET_EXCEEDED")?;
        todo.extend(dependencies(&body, &r.kind, key)?);
        loaded.insert(r.id.clone(), d);
    }
    for d in loaded.values() {
        let b = canonical_body(d)?;
        if d.reference.kind == "workflow-evidence" || d.reference.kind == "workflow-customer" {
            let authority = parse_ref(&b["authority_ref"])?;
            let root_doc = loaded
                .get(&authority.id)
                .ok_or(ServiceError::IntegrityFailure)?;
            if root_doc.reference != authority || authority.kind != "workflow-authority" {
                return Err(ServiceError::IntegrityFailure);
            }
            let root = canonical_body(root_doc)?;
            let actor = if d.reference.kind == "workflow-evidence" {
                &b["retained_by"]
            } else {
                &b["registered_by"]
            };
            if root["basis"]["principal_id"] != *actor {
                return Err(ServiceError::IntegrityFailure);
            }
            if d.reference.kind == "workflow-customer" {
                let e = parse_ref(&b["evidence_ref"])?;
                let proof = loaded.get(&e.id).ok_or(ServiceError::IntegrityFailure)?;
                if proof.reference != e || e.kind != "workflow-evidence" {
                    return Err(ServiceError::IntegrityFailure);
                }
                let p = canonical_body(proof)?;
                let input = evidence_input(&p)?;
                if input.purpose() != "customer_registration"
                    || input.customer() != text(&b, "customer")?
                    || input.metadata()["acceptor"] != b["acceptor"]
                    || p["installation_mode"] != b["installation_mode"]
                {
                    return Err(ServiceError::IntegrityFailure);
                }
            } else {
                let input = evidence_input(&b)?;
                if input.purpose() == "operator_attestation" {
                    let e = parse_ref(&input.metadata()["assent_evidence_ref"])?;
                    let proof = loaded.get(&e.id).ok_or(ServiceError::IntegrityFailure)?;
                    if proof.reference != e || e.kind != "workflow-evidence" {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    let p = canonical_body(proof)?;
                    let assent = evidence_input(&p)?;
                    if assent.purpose() != "assent"
                        || assent.customer() != input.customer()
                        || assent.agreement_id() != input.agreement_id()
                        || assent.metadata()["terms_digest"] != input.metadata()["terms_digest"]
                        || assent.metadata()["acceptor"] != input.metadata()["acceptor"]
                        || p["retained_by"] != b["retained_by"]
                        || p["installation_mode"] != b["installation_mode"]
                    {
                        return Err(ServiceError::IntegrityFailure);
                    }
                }
            }
        }
    }
    source::verify_closure(key, &loaded)?;
    Ok(loaded.into_values().collect())
}
pub(super) async fn locator_dependencies(
    tx: &mut SqliteTx,
    key: &OperationKey,
    req: &request::Request,
    payload: &Value,
) -> Result<Vec<Document>> {
    if req.kind() == "evidence.retain" && payload["purpose"] == "admission_stop" {
        let input =
            ledgerlab_core::policy::workflow::EvidenceInput::parse(&records::bytes(payload)?)
                .map_err(typed_error)?;
        return activation_verify::stop_evidence_dependencies(tx, key, &input).await;
    }
    let mut refs = vec![];
    for l in journal::locators(req, payload)? {
        refs.push(parse_ref(&l["ref"])?);
    }
    authenticated_closure(tx, key, &refs).await
}
pub(super) fn checked(raw: &[u8], limit: usize) -> Result<Value> {
    let v = canonical::parse_bounded(raw, limit).map_err(|_| ServiceError::IntegrityFailure)?;
    if records::bytes(&v)? != raw {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(v)
}
pub(super) struct LocalCertificate {
    pub(super) value: Value,
    pub(super) documents: Vec<Document>,
    pub(super) original: Option<Vec<u8>>,
}

// This checks one complete certificate without recursively trusting origins.
// The outer verifier resolves all origin obligations with a finite worklist.
async fn retained_local(
    tx: &mut SqliteTx,
    key: &OperationKey,
    o: &JournalOperation,
) -> Result<LocalCertificate> {
    let op = checked(&o.operation_bytes, 8 * 1024 * 1024)?;
    let manifest = checked(&o.manifest_bytes, 4 * 1024 * 1024)?;
    let receipt = checked(&o.receipt_bytes, 1024 * 1024)?;
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
    if op["schema"] != "ledger-workflow-operation-record/1"
        || op["key"] != json!(key)
        || op["kind"] != o.kind
        || op["contract"] != "workflow/1"
        || o.identity != key.bytes()?
        || op["commit_sequence"] != sequence
        || Timestamp::parse(text(&op, "accepted_at")?)
            .map_err(|_| ServiceError::IntegrityFailure)?
            .micros()
            != o.accepted_at_us
        || manifest["schema"] != "ledger-workflow-agreement-manifest/1"
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
    let raw = ledgerlab_core::adjudication::proofs::decode_base64(
        text(evidence, "original_request_bytes_base64")?,
        262144,
    )
    .map_err(|_| ServiceError::IntegrityFailure)?;
    let req = request::Request::parse(&raw).map_err(|_| ServiceError::IntegrityFailure)?;
    if req.key() != key || req.kind() != o.kind {
        return Err(ServiceError::IntegrityFailure);
    }
    let payload = service::normalize_payload(&req).map_err(|_| ServiceError::IntegrityFailure)?;
    let reproduced =
        journal::evidence(&req, &payload).map_err(|_| ServiceError::IntegrityFailure)?;
    if reproduced != *evidence
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
        || receipt["commit_sequence"] != sequence
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
    let results: Vec<RecordRef> = serde_json::from_value(op["result_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    source::verify_result_kinds(&payload, o.kind.as_str(), &results)?;
    let mut seeds = results.clone();
    seeds.extend(roots.clone());
    for l in journal::locators(&req, &payload)? {
        seeds.push(parse_ref(&l["ref"])?);
    }
    let docs = closure(tx, key, &seeds).await?;
    let root_doc = docs
        .iter()
        .find(|d| d.reference == roots[0])
        .ok_or(ServiceError::IntegrityFailure)?;
    let root_body = canonical_body(root_doc)?;
    root(&root_body, key, Some(&key.source))?;
    if root_body["basis"]["principal_id"] != authority["principal_id"] {
        return Err(ServiceError::IntegrityFailure);
    }
    let result_doc = docs
        .iter()
        .find(|d| d.reference == results[0])
        .ok_or(ServiceError::IntegrityFailure)?;
    let body = canonical_body(result_doc)?;
    let semantic = if o.kind == "evidence.retain" {
        let input = evidence_input(&body)?;
        if input
            .canonical_value()
            .map_err(|_| ServiceError::IntegrityFailure)?
            != payload
        {
            return Err(ServiceError::IntegrityFailure);
        }
        evidence_identity(key, &input)?
    } else if payload["action"] == "customer.register" {
        if body["customer"] != payload["customer"]
            || body["acceptor"] != payload["acceptor"]
            || body["evidence_ref"] != payload["registration_evidence_ref"]
        {
            return Err(ServiceError::IntegrityFailure);
        }
        records::bytes(&json!(["customer.register", body["customer"]]))?
    } else {
        source::verify_intent(key, &payload, &results, &docs)?
    };
    let original = tx
        .workflow_agreement_semantic(&key.logical_store_id, &key.scope, &o.kind, &semantic)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let is_alias = original != o.identity;
    if receipt["result_kind"]
        != if is_alias {
            "semantic_alias"
        } else {
            "administrative"
        }
    {
        return Err(ServiceError::IntegrityFailure);
    }
    if !is_alias {
        if payload["action"]
            .as_str()
            .is_some_and(|a| a.starts_with("source."))
        {
            source::verify_original(key, &payload, &results, &docs, &op, authority, &roots[0])?;
        } else {
            let result_time = if o.kind == "evidence.retain" {
                &body["retained_at"]
            } else {
                &body["registered_at"]
            };
            let result_actor = if o.kind == "evidence.retain" {
                &body["retained_by"]
            } else {
                &body["registered_by"]
            };
            if *result_time != op["accepted_at"]
                || *result_actor != authority["principal_id"]
                || body["authority_ref"] != json!(roots[0])
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
    } else {
        let old = tx
            .workflow_agreement_lookup(&original)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let old_op = checked(&old.operation_bytes, 8 * 1024 * 1024)?;
        if old_op["result_refs"] != op["result_refs"]
            || old.kind != o.kind
            || old.sequence >= o.sequence
            || old_op["request_evidence"]["normalized_request"]["payload"] != payload
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    let mut expected: Vec<_> = docs.iter().map(|d| d.reference.clone()).collect();
    expected.push(o.operation_ref.clone());
    records::sort_refs(&mut expected)?;
    let mut indexed = o.members.clone();
    records::sort_refs(&mut indexed)?;
    if indexed != expected || manifest["members"] != json!(expected) {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(LocalCertificate {
        value: json!({"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":evidence}),
        documents: docs,
        original: is_alias.then_some(original),
    })
}

fn origin_key(identity: &[u8], expected: &OperationKey) -> Result<OperationKey> {
    let key = OperationKey::parse(identity).map_err(|_| ServiceError::IntegrityFailure)?;
    if key.logical_store_id != expected.logical_store_id || key.scope != expected.scope {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(key)
}

pub(super) fn retained<'a>(
    tx: &'a mut SqliteTx,
    key: &'a OperationKey,
    o: &'a JournalOperation,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
    Box::pin(async move {
        if proof_session::local_phase(tx, key)? {
            let _view = proof_session::completed(
                tx,
                key,
                &o.identity,
                &proof_session::journal_binding(o)?,
                false,
            )?;
            return proof_session::journal_response(o);
        }
        if !proof_session::present(tx, key)? {
            return proof_session::with_proofs(tx, key, |tx| {
                Box::pin(retained_in_scope(tx, key, o))
            })
            .await;
        }
        retained_in_scope(tx, key, o).await
    })
}

async fn retained_in_scope(
    tx: &mut SqliteTx,
    key: &OperationKey,
    o: &JournalOperation,
) -> Result<Value> {
    use origin_driver::{LocalContinuation, LocalStep, OriginDemand, Stage};
    // Preparation is untrusted. Only this one driver schedules origins; local
    // semantic continuations can consume exact completed proofs, never reenter.
    enum Frame {
        Enter {
            identity: Vec<u8>,
            original: bool,
            demand: Option<OriginDemand>,
        },
        Resume {
            continuation: LocalContinuation,
            guard: proof_session::Guard,
            original: bool,
        },
        Exit {
            guard: proof_session::Guard,
            original: bool,
        },
    }
    let mut frames = vec![Frame::Enter {
        identity: o.identity.clone(),
        original: false,
        demand: None,
    }];
    let mut result = None;
    while let Some(frame) = frames.pop() {
        let (operation, saved_key, guard, must_be_original, continuation) = match frame {
            Frame::Exit { guard, original } => {
                guard.finish(original)?;
                continue;
            }
            Frame::Enter {
                identity,
                original,
                demand,
            } => {
                let operation = if identity == o.identity {
                    o.clone()
                } else {
                    tx.workflow_agreement_lookup(&identity)
                        .await
                        .map_err(crate::service::store_error)?
                        .ok_or(ServiceError::IntegrityFailure)?
                };
                let saved_key = origin_key(&operation.identity, key)?;
                if let Some(demand) = demand {
                    if demand.identity != operation.identity
                        || demand.kind != operation.kind
                        || demand.sequence != operation.sequence
                        || operation.sequence >= demand.parent_sequence
                    {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    for reference in demand.references {
                        let actual = tx
                            .workflow_agreement_document_origin(
                                &key.logical_store_id,
                                &key.scope,
                                &reference,
                            )
                            .await
                            .map_err(crate::service::store_error)?
                            .ok_or(ServiceError::IntegrityFailure)?;
                        if actual
                            != (
                                operation.identity.clone(),
                                operation.kind.clone(),
                                operation.sequence,
                            )
                            || !operation.members.contains(&reference)
                        {
                            return Err(ServiceError::IntegrityFailure);
                        }
                    }
                }
                let binding = proof_session::journal_binding(&operation)?;
                let guard = match proof_session::enter(
                    tx,
                    &saved_key,
                    &identity,
                    binding.clone(),
                    original,
                )? {
                    proof_session::Entry::Completed => {
                        if identity == o.identity {
                            result = Some(proof_session::journal_response(&operation)?);
                        }
                        continue;
                    }
                    proof_session::Entry::Started(guard) => guard,
                };
                #[cfg(test)]
                count_origin(&operation.identity);
                let continuation = LocalContinuation {
                    identity,
                    binding,
                    stage: Stage::Members,
                };
                (operation, saved_key, guard, original, continuation)
            }
            Frame::Resume {
                continuation,
                guard,
                original,
            } => {
                let operation = if continuation.identity == o.identity {
                    o.clone()
                } else {
                    tx.workflow_agreement_lookup(&continuation.identity)
                        .await
                        .map_err(crate::service::store_error)?
                        .ok_or(ServiceError::IntegrityFailure)?
                };
                let saved_key = origin_key(&operation.identity, key)?;
                (operation, saved_key, guard, original, continuation)
            }
        };
        let certificate =
            match origin_driver::prepare(tx, &saved_key, &operation, continuation).await? {
                LocalStep::NeedOrigins { demands, resume } => {
                    frames.push(Frame::Resume {
                        continuation: resume,
                        guard,
                        original: must_be_original,
                    });
                    for demand in demands.into_iter().rev() {
                        if demand.kind == "policy.publish" {
                            let origin = tx
                                .workflow_lookup(&demand.identity)
                                .await
                                .map_err(crate::service::store_error)?
                                .ok_or(ServiceError::IntegrityFailure)?;
                            if origin.sequence != demand.sequence
                                || origin.sequence >= demand.parent_sequence
                            {
                                return Err(ServiceError::IntegrityFailure);
                            }
                            for reference in &demand.references {
                                let actual = tx
                                    .workflow_agreement_document_origin(
                                        &key.logical_store_id,
                                        &key.scope,
                                        reference,
                                    )
                                    .await
                                    .map_err(crate::service::store_error)?
                                    .ok_or(ServiceError::IntegrityFailure)?;
                                if actual
                                    != (
                                        demand.identity.clone(),
                                        demand.kind.clone(),
                                        demand.sequence,
                                    )
                                    || !origin.members.contains(reference)
                                {
                                    return Err(ServiceError::IntegrityFailure);
                                }
                            }
                            verify_publication_origin(tx, key, &demand.identity, demand.sequence)
                                .await?;
                        } else {
                            frames.push(Frame::Enter {
                                identity: demand.identity.clone(),
                                original: demand.original,
                                demand: Some(demand),
                            });
                        }
                    }
                    continue;
                }
                LocalStep::Complete(certificate) => certificate,
            };
        let is_original = certificate.original.is_none();
        if must_be_original && !is_original {
            return Err(ServiceError::IntegrityFailure);
        }
        if operation.identity == o.identity {
            result = Some(certificate.value);
        }
        let mut children = std::collections::BTreeMap::<Vec<u8>, bool>::new();
        if let Some(identity) = certificate.original {
            let original = tx
                .workflow_agreement_lookup(&identity)
                .await
                .map_err(crate::service::store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            if original.sequence >= operation.sequence {
                return Err(ServiceError::IntegrityFailure);
            }
            children.insert(original.identity, true);
        }
        for document in certificate.documents {
            let (identity, kind, sequence) = tx
                .workflow_agreement_document_origin(
                    &key.logical_store_id,
                    &key.scope,
                    &document.reference,
                )
                .await
                .map_err(crate::service::store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            if identity == operation.identity {
                // Only this exact, already locally checked certificate's own
                // members may terminate here. Cross-origin cycles do not.
                if sequence != operation.sequence {
                    return Err(ServiceError::IntegrityFailure);
                }
                continue;
            }
            if sequence >= operation.sequence {
                return Err(ServiceError::IntegrityFailure);
            }
            if kind == "policy.publish" {
                verify_publication_origin(tx, key, &identity, sequence).await?;
            } else {
                children.entry(identity).or_insert(false);
            }
        }
        // Documents/status graphs are dropped now; frames retain compact
        // bindings and identity obligations, not transitive record BLOBs.
        frames.push(Frame::Exit {
            guard,
            original: is_original,
        });
        for (identity, original) in children.into_iter().rev() {
            frames.push(Frame::Enter {
                identity,
                original,
                demand: None,
            });
        }
    }
    result.ok_or(ServiceError::IntegrityFailure)
}

/// One semantic step. The private completed-only phase established by the
/// driver forbids every reachable checker from polling a new origin driver.
pub(super) async fn validate_local(
    tx: &mut SqliteTx,
    saved_key: &OperationKey,
    operation: &JournalOperation,
) -> Result<LocalCertificate> {
    if !proof_session::local_phase(tx, saved_key)? {
        return Err(ServiceError::IntegrityFailure);
    }
    let parsed = checked(&operation.operation_bytes, 8 * 1024 * 1024)?;
    let certificate = if parsed["schema"] == "ledger-workflow-agreement-alias-operation/1" {
        Box::pin(alias04::retained_local(tx, saved_key, operation)).await?
    } else if transport::selected(&parsed["request_evidence"]["normalized_request"]["payload"]) {
        transport::certificate::retained_local(tx, saved_key, operation).await?
    } else if matches!(
        operation.kind.as_str(),
        "agreement.activate" | "agreement.revoke"
    ) {
        Box::pin(activation_verify::retained_local(tx, saved_key, operation)).await?
    } else if operation.kind == "agreement.accept" {
        Box::pin(accepted_verify::retained_local(tx, saved_key, operation)).await?
    } else {
        retained_local(tx, saved_key, operation).await?
    };
    Ok(certificate)
}

async fn verify_publication_origin(
    tx: &mut SqliteTx,
    key: &OperationKey,
    identity: &[u8],
    sequence: i64,
) -> Result<()> {
    let saved_key = origin_key(identity, key)?;
    let origin = tx
        .workflow_lookup(identity)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if origin.sequence != sequence {
        return Err(ServiceError::IntegrityFailure);
    }
    let binding = proof_session::publication_binding(&origin)?;
    if proof_session::local_phase(tx, key)? {
        let _view = proof_session::completed(tx, &saved_key, identity, &binding, false)?;
        return Ok(());
    }
    let guard = match proof_session::enter(tx, &saved_key, identity, binding, false)? {
        proof_session::Entry::Completed => return Ok(()),
        proof_session::Entry::Started(guard) => guard,
    };
    #[cfg(test)]
    {
        let _ = WORK_COUNTS.try_with(|counts| {
            *counts
                .borrow_mut()
                .publications
                .entry(identity.to_vec())
                .or_default() += 1;
        });
    }
    if financial::local_publication_origin(tx,&saved_key,sequence).await? {
        return guard.finish(true);
    }
    let proof = records::verify_retained(tx, &saved_key, &origin).await?;
    guard.finish(proof["receipt"]["result_kind"] != "semantic_alias")
}

/// Authenticate an immutable dependency against its complete originating
/// certificate. The scoped member index is used only to locate that origin.
pub(super) fn document_origin<'a>(
    tx: &'a mut SqliteTx,
    key: &'a OperationKey,
    reference: &'a RecordRef,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if !proof_session::present(tx, key)? {
            return proof_session::with_proofs(tx, key, |tx| {
                Box::pin(document_origin_in_scope(tx, key, reference))
            })
            .await;
        }
        document_origin_in_scope(tx, key, reference).await
    })
}

async fn document_origin_in_scope(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &RecordRef,
) -> Result<()> {
    #[cfg(test)]
    {
        let _ = WORK_COUNTS.try_with(|counts| {
            *counts
                .borrow_mut()
                .document_origins
                .entry(reference.id.clone())
                .or_default() += 1;
        });
    }
    let (identity, kind, sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, reference)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if financial::local_selected_origin(tx,key,reference,sequence).await? {
        return Ok(());
    }
    if kind == "policy.publish" {
        let origin = tx
            .workflow_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        if origin.sequence != sequence || !origin.members.contains(reference) {
            return Err(ServiceError::IntegrityFailure);
        }
        verify_publication_origin(tx, key, &identity, sequence).await
    } else {
        let saved_key = origin_key(&identity, key)?;
        let origin = tx
            .workflow_agreement_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        if origin.sequence != sequence || !origin.members.contains(reference) {
            return Err(ServiceError::IntegrityFailure);
        }
        retained(tx, &saved_key, &origin).await?;
        Ok(())
    }
}

pub(super) async fn authenticated_closure(
    tx: &mut SqliteTx,
    key: &OperationKey,
    seeds: &[RecordRef],
) -> Result<Vec<Document>> {
    if seeds.len() == 1 && seeds[0].kind == "workflow-head-transition" {
        document_origin(tx, key, &seeds[0]).await?;
        return Box::pin(replay::predecessor_closure(tx, key, &seeds[0])).await;
    }
    let documents = closure(tx, key, seeds).await?;
    for document in &documents {
        document_origin(tx, key, &document.reference).await?;
    }
    Ok(documents)
}
