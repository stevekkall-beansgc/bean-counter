//! Actual retained/current source observations, never an admission capability.
//! The host separately checks its current owner, customer, mode, grant rights
//! and trusted time. A revoked observation is legitimate stop evidence only.
use super::super::request::OperationKey;
use super::*;
use ledgerlab_core::policy::workflow::{SourceEnrollment, SourceGrant, SourceState};

pub(super) struct ObservedSource {
    enrollment: SourceEnrollment,
    state: SourceState,
    grant: Option<SourceGrant>,
    documents: Vec<Document>,
}
impl ObservedSource {
    pub(super) fn enrollment(&self) -> &SourceEnrollment {
        &self.enrollment
    }
    pub(super) fn state(&self) -> &SourceState {
        &self.state
    }
    pub(super) fn grant(&self) -> Option<&SourceGrant> {
        self.grant.as_ref()
    }
    pub(super) fn documents(&self) -> &[Document] {
        &self.documents
    }
}
async fn by_id(tx: &mut SqliteTx, key: &OperationKey, id: &str) -> Result<Document> {
    tx.workflow_document_by_id(id, &key.logical_store_id, &key.scope[0], &key.scope[1])
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)
}
pub(super) async fn observe_source(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &RecordRef,
) -> Result<ObservedSource> {
    require(
        reference.kind == "workflow-source-enrollment",
        "INVALID_REFERENCE",
    )?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before latest source enrollment proof",
        key.operation_id
    ));
    let enrollment = source::enrollment(tx, key, reference).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after latest source enrollment proof",
        key.operation_id
    ));
    let head = tx
        .workflow_agreement_source_head(
            &key.logical_store_id,
            &key.scope,
            enrollment.enrollment_id(),
        )
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let state_document = by_id(tx, key, &head.state).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before latest pinned source proof",
        key.operation_id
    ));
    let observed = observe_source_at(tx, key, reference, &state_document.reference).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after latest pinned source proof",
        key.operation_id
    ));
    if observed.state.revision()
        != u64::try_from(head.revision).map_err(|_| ServiceError::IntegrityFailure)?
        || observed.state.status() != head.status
        || observed.state.current_grant_ref().map(|r| r.id()) != head.grant.as_deref()
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(observed)
}
/// Pinned historical data for a retained observation. It cannot be presented
/// as the latest source state or reauthorize current admission.
pub(super) async fn observe_source_at(
    tx: &mut SqliteTx,
    key: &OperationKey,
    enrollment_ref: &RecordRef,
    state_ref: &RecordRef,
) -> Result<ObservedSource> {
    require(
        enrollment_ref.kind == "workflow-source-enrollment"
            && state_ref.kind == "workflow-source-state",
        "INVALID_REFERENCE",
    )?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before pinned enrollment proof",
        key.operation_id
    ));
    let enrollment = source::enrollment(tx, key, enrollment_ref).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after pinned enrollment proof",
        key.operation_id
    ));
    let state_document = loaded(tx, key, state_ref).await?;
    verify::document_origin(tx, key, &state_document.reference).await?;
    let state = SourceState::parse(&state_document.bytes, &enrollment)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if state.logical_store_id() != key.logical_store_id
        || json!(state.reference()) != json!(state_document.reference)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let grant = match (state.status(), state.current_grant_ref()) {
        ("revoked", None) => None,
        ("active", Some(reference)) => {
            let d = by_id(tx, key, reference.id()).await?;
            verify::document_origin(tx, key, &d.reference).await?;
            if json!(reference) != json!(d.reference) {
                return Err(ServiceError::IntegrityFailure);
            }
            let grant = SourceGrant::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
            if grant.grant_id() != enrollment.enrollment_id()
                || grant.revision() != state.revision()
                || grant.original_ceiling_ref() != enrollment.reference()
                || !grant.ceiling().is_subset_of(enrollment.ceiling())
                || json!(
                    grant
                        .canonical_value()
                        .map_err(|_| ServiceError::IntegrityFailure)?
                ) != canonical_body(&d)?
            {
                return Err(ServiceError::IntegrityFailure);
            }
            Some(grant)
        }
        _ => return Err(ServiceError::IntegrityFailure),
    };
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before pinned state full closure",
        key.operation_id
    ));
    let documents = verify::authenticated_closure(tx, key, &[state_document.reference]).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after pinned state full closure",
        key.operation_id
    ));
    Ok(ObservedSource {
        enrollment,
        state,
        grant,
        documents,
    })
}

/// Latest authenticated immutable source transition BEFORE one original
/// operation, not today's mutable head or eligibility. Two independent birth
/// locators prevent a missing auxiliary projection from resurrecting revision1.
pub(super) async fn source_as_of(
    tx: &mut SqliteTx,
    key: &OperationKey,
    enrollment_ref: &RecordRef,
    before_sequence: i64,
) -> Result<RecordRef> {
    // Data only until the complete original revision1 birth certificate below
    // authenticates this exact enrollment. Avoid reproving that same origin.
    let document=loaded(tx,key,enrollment_ref).await?;
    let enrollment=SourceEnrollment::parse(&document.bytes).map_err(|_|ServiceError::IntegrityFailure)?;
    if enrollment.scope()!=&key.scope || json!(enrollment.reference())!=json!(enrollment_ref)
        || enrollment.canonical_value().map_err(|_|ServiceError::IntegrityFailure)?!=canonical_body(&document)? {
        return Err(ServiceError::IntegrityFailure);
    }
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before source birth prefix lookup frontier={}",
        key.operation_id, before_sequence
    ));
    let candidates = tx
        .workflow_agreement_source_prefix(
            &key.logical_store_id,
            &key.scope,
            enrollment_ref,
            enrollment.enrollment_id(),
        )
        .await
        .map_err(crate::service::store_error)?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} source birth prefix candidates={} frontier={}",
        key.operation_id,
        candidates.len(),
        before_sequence
    ));
    let mut states = std::collections::BTreeMap::<u64, (i64, RecordRef, Option<RecordRef>)>::new();
    for identity in candidates {
        let operation = tx
            .workflow_agreement_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let saved_key =
            OperationKey::parse(&identity).map_err(|_| ServiceError::IntegrityFailure)?;
        if saved_key.logical_store_id != key.logical_store_id
            || saved_key.scope != key.scope
            || operation.kind != "authority.change"
        {
            return Err(ServiceError::IntegrityFailure);
        }
        // Validate the immutable birth sequence copy before choosing the past
        // prefix: moving an index row forward cannot hide an earlier mutation.
        let body = canonical::parse_bounded(&operation.operation_bytes, 8388608)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let sequence = operation.sequence.to_string();
        if records::bytes(&body)? != operation.operation_bytes
            || body["key"] != json!(saved_key)
            || body["commit_sequence"] != sequence.as_str()
            || body["kind"] != operation.kind
            || records::record_reference("workflow-operation", &key.scope, &body)?
                != operation.operation_ref
        {
            return Err(ServiceError::IntegrityFailure);
        }
        if operation.sequence >= before_sequence {
            continue;
        }
        #[cfg(test)]
        verify::diagnostic_checkpoint(&format!(
            "{} before source birth certificate={} sequence={}",
            key.operation_id, saved_key.operation_id, operation.sequence
        ));
        let certificate = Box::pin(verify::retained(tx, &saved_key, &operation)).await?;
        #[cfg(test)]
        verify::diagnostic_checkpoint(&format!(
            "{} after source birth certificate={} sequence={}",
            key.operation_id, saved_key.operation_id, operation.sequence
        ));
        if certificate["receipt"]["result_kind"] == "semantic_alias" {
            continue;
        }
        let refs: Vec<RecordRef> =
            serde_json::from_value(certificate["receipt"]["result_refs"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
        for r in refs
            .into_iter()
            .filter(|r| r.kind == "workflow-source-state")
        {
            let d = loaded(tx, key, &r).await?;
            let state = SourceState::parse(&d.bytes, &enrollment)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            if json!(state.enrollment_ref()) != json!(enrollment_ref) {
                return Err(ServiceError::IntegrityFailure);
            }
            let body = canonical::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
            if body["operation_key"] != json!(saved_key) || json!(state.reference()) != json!(r) {
                return Err(ServiceError::IntegrityFailure);
            }
            let prior = state.prior_state_ref().map(|r| RecordRef {
                kind: r.kind().into(),
                id: r.id().into(),
                digest: r.digest().into(),
            });
            if states
                .insert(state.revision(), (operation.sequence, r, prior))
                .is_some()
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
    }
    let mut previous = None;
    let mut previous_sequence = 0;
    for (index, (revision, (sequence, r, prior))) in states.iter().enumerate() {
        if *revision != u64::try_from(index + 1).map_err(|_| ServiceError::IntegrityFailure)?
            || *sequence <= previous_sequence
            || *prior != previous
        {
            return Err(ServiceError::IntegrityFailure);
        }
        previous = Some(r.clone());
        previous_sequence = *sequence;
    }
    previous.ok_or(ServiceError::IntegrityFailure)
}

/// Data from one fully verified publication boundary, without its historical
/// operation/manifest/receipt wrappers. No current compiler or build is loaded.
pub(super) struct ObservedPublication {
    body: Value,
    documents: Vec<Document>,
    publication_index: usize,
    plan_index: usize,
    source_indices: Vec<usize>,
}
impl ObservedPublication {
    pub(super) fn body(&self) -> &Value {
        &self.body
    }
    pub(super) fn documents(&self) -> &[Document] {
        &self.documents
    }
    pub(super) fn publication(&self) -> &Document {
        &self.documents[self.publication_index]
    }
    pub(super) fn plan(&self) -> &Document {
        &self.documents[self.plan_index]
    }
    pub(super) fn sources(&self) -> impl Iterator<Item = &Document> {
        self.source_indices.iter().map(|i| &self.documents[*i])
    }
}
pub(super) async fn observe_publication(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &RecordRef,
) -> Result<ObservedPublication> {
    require(
        reference.kind == "workflow-publication",
        "INVALID_REFERENCE",
    )?;
    let (identity, kind, sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, reference)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if kind != "policy.publish" {
        return Err(ServiceError::IntegrityFailure);
    }
    // A corrupt membership index must not route a publication back through
    // its dependent agreement certificate before the origin family is checked.
    verify::document_origin(tx, key, reference).await?;
    let origin = tx
        .workflow_lookup(&identity)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if origin.sequence != sequence || origin.publication_id != reference.id {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut documents = vec![];
    let mut total = 0usize;
    for member in origin.members {
        if member.kind == "workflow-operation" {
            continue;
        }
        if matches!(
            member.kind.as_str(),
            "workflow-manifest" | "workflow-receipt"
        ) {
            return Err(ServiceError::IntegrityFailure);
        }
        let document = loaded(tx, key, &member).await.map_err(|e| match e {
            ServiceError::Rejection(_) => ServiceError::IntegrityFailure,
            e => e,
        })?;
        total = total
            .checked_add(document.bytes.len())
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            total <= 8388608 && documents.len() < 4096,
            "PROGRAM_BUDGET_EXCEEDED",
        )?;
        documents.push(document);
    }
    let publication_index = documents
        .iter()
        .position(|d| d.reference == *reference)
        .ok_or(ServiceError::IntegrityFailure)?;
    let publication = &documents[publication_index];
    let body = canonical::parse_bounded(&publication.bytes, 1048576)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if records::bytes(&body)? != publication.bytes
        || records::publication_ref(&key.logical_store_id, &key.scope, &body)? != *reference
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let plan: RecordRef = serde_json::from_value(body["compiled_plan_ref"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let plan_index = documents
        .iter()
        .position(|d| d.reference == plan)
        .ok_or(ServiceError::IntegrityFailure)?;
    let sources: Vec<RecordRef> = serde_json::from_value(body["source_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let source_indices = sources
        .iter()
        .map(|r| {
            documents
                .iter()
                .position(|d| d.reference == *r)
                .ok_or(ServiceError::IntegrityFailure)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ObservedPublication {
        body,
        documents,
        publication_index,
        plan_index,
        source_indices,
    })
}

/// One independently accepted binding and its original immutable inputs. This
/// is origin-authenticated data, never a current source or owner capability.
pub(super) struct ObservedAcceptedBinding {
    pub(super) accepted: Document,
    pub(super) binding: Document,
    pub(super) original: JournalOperation,
    pub(super) original_key: OperationKey,
    pub(super) acceptance: replay::AcceptanceData,
    pub(super) documents: Vec<Document>,
}
pub(super) async fn observe_accepted_binding(
    tx: &mut SqliteTx,
    key: &OperationKey,
    binding_ref: &RecordRef,
    before_sequence: i64,
) -> Result<ObservedAcceptedBinding> {
    require(binding_ref.kind == "workflow-binding", "INVALID_REFERENCE")?;
    let accepted_ref = tx
        .workflow_agreement_accepted_binding(&key.logical_store_id, &key.scope, binding_ref)
        .await
        .map_err(crate::service::store_error)?
        .ok_or_else(|| reject("DESCRIPTOR_DEPENDENCY_MISSING"))?;
    let (identity, kind, sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, &accepted_ref)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if kind != "agreement.accept" || sequence >= before_sequence {
        return Err(ServiceError::IntegrityFailure);
    }
    let original = tx
        .workflow_agreement_lookup(&identity)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let original_key =
        OperationKey::parse(&identity).map_err(|_| ServiceError::IntegrityFailure)?;
    if original_key.logical_store_id != key.logical_store_id
        || original_key.scope != key.scope
        || original.sequence != sequence
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let certificate = Box::pin(verify::retained(tx, &original_key, &original)).await?;
    if certificate["receipt"]["result_kind"] != "administrative" {
        return Err(ServiceError::IntegrityFailure);
    }
    let results: Vec<RecordRef> =
        serde_json::from_value(certificate["receipt"]["result_refs"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
    if !results.contains(&accepted_ref) {
        return Err(ServiceError::IntegrityFailure);
    }
    let accepted = loaded(tx, key, &accepted_ref).await?;
    let body = canonical::parse_bounded(&accepted.bytes, 262144)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let binding = loaded(tx, key, binding_ref).await?;
    let binding_body = canonical_body(&binding)?;
    if body["operation_key"] != json!(original_key)
        || body["binding_ref"] != json!(binding_ref)
        || body["scope"] != json!(key.scope)
        || body["customer"] != binding_body["customer"]
        || body["agreement_id"] != binding_body["agreement_id"]
        || body["version"] != binding_body["version"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let raw = ledgerlab_core::adjudication::proofs::decode_base64(
        text(
            &certificate["request_evidence"],
            "original_request_bytes_base64",
        )?,
        262144,
    )
    .map_err(|_| ServiceError::IntegrityFailure)?;
    let req =
        request::Request::parse_agreement(&raw).map_err(|_| ServiceError::IntegrityFailure)?;
    let acceptance = replay::acceptance_data(tx, &original_key, &req).await?;
    let mut documents = vec![];
    let mut bytes = 0usize;
    for r in &original.members {
        if *r == original.operation_ref {
            continue;
        }
        let d = loaded(tx, key, r).await?;
        bytes = bytes
            .checked_add(d.bytes.len())
            .ok_or(ServiceError::IntegrityFailure)?;
        if bytes > 8388608 || documents.len() >= 4096 {
            return Err(ServiceError::IntegrityFailure);
        }
        documents.push(d);
    }
    Ok(ObservedAcceptedBinding {
        accepted,
        binding,
        original,
        original_key,
        acceptance,
        documents,
    })
}
