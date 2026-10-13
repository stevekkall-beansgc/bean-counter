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
    verify::document_origin(tx, key, &state_document.reference).await?;
    let state = SourceState::parse(&state_document.bytes, &enrollment)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if state.logical_store_id() != key.logical_store_id
        || state.revision()
            != u64::try_from(head.revision).map_err(|_| ServiceError::IntegrityFailure)?
        || state.status() != head.status
        || json!(state.reference()) != json!(state_document.reference)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let grant = match (
        head.status.as_str(),
        head.grant.as_deref(),
        state.current_grant_ref(),
    ) {
        ("revoked", None, None) => None,
        ("active", Some(id), Some(reference)) => {
            let d = by_id(tx, key, id).await?;
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
