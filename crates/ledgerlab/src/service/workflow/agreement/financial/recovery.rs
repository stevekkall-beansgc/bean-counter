//! Incoming retry equality after complete ORIGINAL reconstruction. This cannot
//! certify an origin, select today's policy, price again or return a receipt.
use super::*;
use ledgerlab_core::{
    policy::workflow::{
        selected_program::SelectedProgram,
        transition_constructor::{EconomicCandidate, normalize_saved_intent},
    },
    validation_work::{OriginalAuditSession, RepresentationKind, ValidationVisit},
};

impl OriginalQuery<'_> {
    /// The caller must supply the candidate/program from completed ORIGINAL
    /// replay. No present incomplete prefix path calls this or returns success.
    /// `incoming` is the already owning parse of the actual submission/key,
    /// never substituted for the saved literal raw request during replay.
    pub(super) fn compare_intent(
        &self,
        incoming: &Value,
        original: &EconomicCandidate,
        program: &SelectedProgram,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let result = (|| {
            let candidate_key = original.key();
            let (key, payload) = match self {
                Self::Status(_) => (incoming, None),
                Self::Retry(request) => {
                    let key = request_wrapper::field(incoming, "key", session)?;
                    request_wrapper::same_text(
                        request_wrapper::text(incoming, "kind", session)?,
                        request.kind(),
                        session,
                    )?;
                    request_wrapper::same_text(
                        request_wrapper::text(incoming, "contract", session)?,
                        "workflow/1",
                        session,
                    )?;
                    (
                        key,
                        Some(request_wrapper::field(incoming, "payload", session)?),
                    )
                }
            };
            // Compare the identity derived by complete core reconstruction to
            // this exact requested key without a JSON/ref allocation.
            for (field, expected) in [
                ("logical_store_id", candidate_key.logical_store_id.as_str()),
                ("source", candidate_key.source.as_str()),
                ("operation_id", candidate_key.operation_id.as_str()),
            ] {
                request_wrapper::same_text(
                    request_wrapper::text(key, field, session)?,
                    expected,
                    session,
                )?;
            }
            let scope = request_wrapper::field(key, "scope", session)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            session
                .semantic_work()
                .visit(ValidationVisit::Constraint)
                .map_err(core_error)?;
            require(scope.len() == 2, "INTEGRITY_FAILURE")?;
            for (actual, expected) in scope.iter().zip(&candidate_key.scope) {
                request_wrapper::same_text(
                    actual.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    expected,
                    session,
                )?;
            }
            let Some(payload) = payload else {
                return Ok(());
            };
            let raw_payload = session
                .canonical_representation(payload, RepresentationKind::OriginalRequest)
                .map_err(core_error)?;
            // This is normalization of PAYLOAD under the original program,
            // not a second economic evaluation or a replacement of saved raw.
            let normalized = normalize_saved_intent(
                program,
                original.normalized_payload(),
                raw_payload.bytes(),
                session,
            )
            .map_err(intent_error)?;
            let work = session.semantic_work();
            work.visit(ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            work.scan_bytes(normalized.bytes().len())
                .map_err(core_error)?;
            work.scan_bytes(original.normalized_payload().len())
                .map_err(core_error)?;
            require(
                normalized.bytes() == original.normalized_payload(),
                "IDENTITY_CONFLICT",
            )
        })();
        if result.is_err() {
            session.invalidate()
        }
        result
    }
}
fn intent_error(error: ledgerlab_core::Error) -> ServiceError {
    match error.code {
        "IDENTITY_CONFLICT" | "INVALID_SCALAR" | "INVALID_SHAPE" | "INVALID_REFERENCE"
        | "UNIT_MISMATCH" | "FACT_PRECISION" | "FACT_RANGE" | "FACT_TYPE" | "JSON" => {
            reject("IDENTITY_CONFLICT")
        }
        // Missing original normalization support is unavailable, not evidence
        // of changed intent. Resource/cancellation errors likewise retain their
        // operational/refusal mapping and never become identity conflicts.
        "UNSUPPORTED_SCHEMA_PROFILE" | "ORIGINAL_VERIFIER_UNAVAILABLE" => ServiceError::Unavailable,
        _ => core_error(error),
    }
}
