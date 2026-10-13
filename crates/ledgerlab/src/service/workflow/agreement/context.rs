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
    let enrollment = source::enrollment(tx, key, reference).await?;
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
    let observed = observe_source_at(tx, key, reference, &state_document.reference).await?;
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
    let enrollment = source::enrollment(tx, key, enrollment_ref).await?;
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
    let documents = verify::authenticated_closure(tx, key, &[state_document.reference]).await?;
    Ok(ObservedSource {
        enrollment,
        state,
        grant,
        documents,
    })
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
