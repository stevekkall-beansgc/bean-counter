//! Exact saved local capture DATA. Current caller capture remains separate.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    policy::workflow::transition_constructor::{
        AuthorityContextData, DraftContextData, ExactRef, OperationKey as CoreKey,
    },
    validation_work::OriginalAuditSession,
};
use std::mem;
fn text(v: &Value, key: &str, s: &mut OriginalAuditSession) -> Result<String> {
    let value = request_wrapper::text(v, key, s)?;
    s.semantic_work()
        .write_bytes(value.len())
        .map_err(core_error)?;
    Ok(value.to_owned())
}
fn reference(v: &Value, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    request_wrapper::closed(v, &["kind", "id", "digest"], s)?;
    Ok(ExactRef {
        kind: text(v, "kind", s)?,
        id: text(v, "id", s)?,
        digest: text(v, "digest", s)?,
    })
}
pub(super) fn capture(
    op: &Value,
    activation: &ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<(DraftContextData, ExternalBufferLease)> {
    capture_selected(op, activation, false, s)
}
pub(super) fn capture_selected(
    op: &Value,
    activation: &ExactRef,
    stage: bool,
    s: &mut OriginalAuditSession,
) -> Result<(DraftContextData, ExternalBufferLease)> {
    let key = request_wrapper::field(op, "key", s)?;
    let scope = request_wrapper::field(key, "scope", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require(scope.len() == 2, "INTEGRITY_FAILURE")?;
    let snapshot = request_wrapper::field(op, "authority_snapshot", s)?;
    let schema = request_wrapper::text(snapshot, "schema", s)?;
    require(
        schema == "ledger-workflow-economic-authority/8"
            || (stage && schema == "ledger-workflow-stage-authority/1"),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let originals = request_wrapper::field(snapshot, "original_grant_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut bytes = mem::size_of::<DraftContextData>()
        + mem::size_of::<CoreKey>()
        + mem::size_of::<AuthorityContextData>()
        + (6 + originals.len()) * mem::size_of::<ExactRef>();
    for (v, field) in [
        (key, "logical_store_id"),
        (key, "source"),
        (key, "operation_id"),
        (snapshot, "actor"),
        (snapshot, "canonical_economic_namespace"),
        (snapshot, "installation_mode"),
        (op, "received_at"),
        (op, "accepted_at"),
        (snapshot, "source_slot"),
    ] {
        bytes = bytes
            .checked_add(request_wrapper::text(v, field, s)?.len())
            .ok_or(ServiceError::Unavailable)?;
    }
    for value in scope {
        bytes = bytes
            .checked_add(value.as_str().ok_or(ServiceError::IntegrityFailure)?.len())
            .ok_or(ServiceError::Unavailable)?;
    }
    for field in [
        "enrollment_ref",
        "current_state_ref",
        "current_grant_ref",
        "original_binding_ref",
        "original_accepted_agreement_ref",
    ] {
        let r = request_wrapper::field(snapshot, field, s)?;
        for k in ["kind", "id", "digest"] {
            bytes = bytes
                .checked_add(request_wrapper::text(r, k, s)?.len())
                .ok_or(ServiceError::Unavailable)?;
        }
    }
    for r in originals {
        for k in ["kind", "id", "digest"] {
            bytes = bytes
                .checked_add(request_wrapper::text(r, k, s)?.len())
                .ok_or(ServiceError::Unavailable)?;
        }
    }
    for v in [&activation.kind, &activation.id, &activation.digest] {
        bytes = bytes
            .checked_add(v.len())
            .ok_or(ServiceError::Unavailable)?;
    }
    let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().scan_bytes(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut original_grant_refs = Vec::new();
    original_grant_refs
        .try_reserve_exact(originals.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for r in originals {
        original_grant_refs.push(reference(r, s)?);
    }
    let frame = AuthorityContextData {
        source_slot: text(snapshot, "source_slot", s)?,
        enrollment_ref: reference(request_wrapper::field(snapshot, "enrollment_ref", s)?, s)?,
        current_state_ref: reference(request_wrapper::field(snapshot, "current_state_ref", s)?, s)?,
        current_grant_ref: reference(request_wrapper::field(snapshot, "current_grant_ref", s)?, s)?,
        original_grant_refs,
        binding_ref: reference(
            request_wrapper::field(snapshot, "original_binding_ref", s)?,
            s,
        )?,
        accepted_agreement_ref: reference(
            request_wrapper::field(snapshot, "original_accepted_agreement_ref", s)?,
            s,
        )?,
        activation_ref: activation.clone(),
        authority_observation_ref: None,
    };
    let key = CoreKey {
        logical_store_id: text(key, "logical_store_id", s)?,
        source: text(key, "source", s)?,
        operation_id: text(key, "operation_id", s)?,
        scope: [
            scope[0]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .to_owned(),
            scope[1]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .to_owned(),
        ],
    };
    Ok((
        DraftContextData {
            key,
            actor: text(snapshot, "actor", s)?,
            canonical_economic_namespace: text(snapshot, "canonical_economic_namespace", s)?,
            installation_mode: text(snapshot, "installation_mode", s)?,
            received_at: text(op, "received_at", s)?,
            accepted_at: text(op, "accepted_at", s)?,
            authorities: vec![frame],
            source_observation_inputs: Vec::new(),
        },
        owner,
    ))
}
