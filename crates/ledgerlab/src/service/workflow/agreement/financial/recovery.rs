//! Incoming retry equality after complete ORIGINAL reconstruction. This cannot
//! certify an origin, select today's policy, price again or return a receipt.
use super::*;
use ledgerlab_core::{
    policy::workflow::{
        selected_program::SelectedProgram,
        transition_constructor::{EconomicCandidate, normalize_saved_intent},
    },
    validation_work::{
        ArithmeticOperation, BigInt, OriginalAuditSession, RepresentationKind, ValidationVisit,
    },
};

impl OriginalQuery<'_> {
    /// Only after the original's actual party fold has completed. The incoming
    /// payload is normalized by the SAME party producer normalizer; it never
    /// replaces literal saved evidence or checks today's mapping/clock.
    pub(super) fn compare_party_intent(
        &self,
        original: &prefix::LocatedWrappers,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let result = (|| {
            let operation = original.wrappers().operation();
            let evidence = request_wrapper::field(operation, "request_evidence", session)?;
            let normalized = request_wrapper::field(evidence, "normalized_request", session)?;
            let saved = request_wrapper::field(normalized, "payload", session)?;
            let schema = request_wrapper::text(saved, "schema", session)?;
            let mut party = false;
            for allowed in [
                "ledger-workflow-party-evidence-retain/1",
                "ledger-workflow-party-evidence-retain/2",
                "ledger-workflow-party-authorize/1",
                "ledger-workflow-party-revoke/1",
            ] {
                session
                    .semantic_work()
                    .visit(ValidationVisit::MemberCompare)
                    .map_err(core_error)?;
                session
                    .semantic_work()
                    .scan_bytes(schema.len())
                    .map_err(core_error)?;
                session
                    .semantic_work()
                    .scan_bytes(allowed.len())
                    .map_err(core_error)?;
                if schema == allowed {
                    party = true;
                    break;
                }
            }
            require(party, "INTEGRITY_FAILURE")?;
            let Self::Retry(incoming) = self else {
                return Ok(());
            };
            let payload = incoming.payload(session)?;
            let incoming_schema = request_wrapper::text(payload, "schema", session)
                .map_err(party_intent_error)?;
            session.semantic_work().visit(ValidationVisit::MemberCompare).map_err(core_error)?;
            session.semantic_work().scan_bytes(schema.len()).map_err(core_error)?;
            session.semantic_work().scan_bytes(incoming_schema.len()).map_err(core_error)?;
            require(schema == incoming_schema, "IDENTITY_CONFLICT")?;
            request_wrapper::same_text(
                request_wrapper::text(normalized, "kind", session)?,
                incoming.kind(),
                session,
            )?;
            request_wrapper::key(
                request_wrapper::field(normalized, "key", session)?,
                incoming.key(),
                session,
            )?;
            let actual = current::party::normalize::payload(
                payload,
                incoming.key(),
                session,
            ).map_err(party_intent_error)?;
            let expected = session
                .canonical_representation(saved, RepresentationKind::OriginalRequest)
                .map_err(core_error)?;
            let actual = session
                .canonical_representation(actual.value(), RepresentationKind::OriginalRequest)
                .map_err(core_error)?;
            session
                .semantic_work()
                .visit(ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            require(expected.bytes() == actual.bytes(), "IDENTITY_CONFLICT")
        })();
        if result.is_err() {
            session.invalidate();
        }
        result
    }
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
            session
                .semantic_work()
                .arithmetic(
                    ArithmeticOperation::Compare,
                    &[&BigInt::from(scope.len()), &BigInt::from(2u8)],
                )
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

/// Owning response bytes for a completed management original. The enclosing
/// private commit/recovery coordinator still owns eligibility and COMMIT; this
/// DATA body alone cannot return a receipt to a public caller.
pub(super) fn party_reply(
    original: &prefix::LocatedWrappers,
    session: &mut OriginalAuditSession,
) -> Result<ledgerlab_core::canonical::audit_arena::AuditedValue> {
    let result = (|| {
        let controls = 3 * std::mem::size_of::<ledgerlab_core::canonical::audit_arena::AuditedBytes>()
            + 4 * std::mem::size_of::<(&str, &[u8])>();
        let _control = session.reserve_external_payload(controls).map_err(core_error)?;
        session.semantic_work().write_bytes(controls).map_err(core_error)?;
        let receipt = session
            .canonical_representation(
                original.wrappers().receipt(),
                RepresentationKind::ReceiptFrame,
            )
            .map_err(core_error)?;
        let evidence =
            request_wrapper::field(original.wrappers().operation(), "request_evidence", session)?;
        let evidence = session
            .canonical_representation(evidence, RepresentationKind::OperationFrame)
            .map_err(core_error)?;
        let body = wire::object(
            &[
                ("receipt", receipt.bytes()),
                ("request_evidence", evidence.bytes()),
                ("schema", b"\"ledger-operation-status/1\""),
                ("status", b"\"committed\""),
            ],
            4194304,
            session,
        )?;
        session
            .parse_representation(body.bytes(), RepresentationKind::Record)
            .map_err(core_error)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn party_intent_error(error: ServiceError) -> ServiceError {
    match error {
        ServiceError::IntegrityFailure => reject("IDENTITY_CONFLICT"),
        ServiceError::Rejection(ref code) if matches!(code.as_str(),
            "INVALID_SCALAR" | "INVALID_SHAPE" | "INVALID_REFERENCE" | "AUTHORITY_REQUIRED"
        ) => reject("IDENTITY_CONFLICT"),
        _ => error,
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
