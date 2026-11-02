//! Complete Binding2 dependencies for the unchanged R1 stop prerequisite.
//! Legacy acquisition remains with R1; this adapter owns its selected reads.
use super::*;
use ledgerlab_core::validation_work::{ArithmeticOperation, RepresentationKind};

pub(in super::super) async fn dependencies(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: &ledgerlab_core::policy::workflow::EvidenceInput,
    target: &Document,
    acceptance_origin: (&RecordRef, i64),
) -> Result<Vec<Document>> {
    let mut session = take_read_session(tx)?;
    let bytes = {
        let future = dependencies_in(ledger, tx, key, input, target, acceptance_origin, &mut session);
        std::mem::size_of_val(&future)
    };
    let storage = session.reserve_external_payload(bytes).map_err(core_error)?;
    session.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let future: std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Document>>> + Send + '_>> =
        Box::pin(dependencies_in(ledger, tx, key, input, target, acceptance_origin, &mut session));
    let result = future.await;
    drop(storage);
    retain_read_session(tx, session);
    result
}

async fn dependencies_in(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: &ledgerlab_core::policy::workflow::EvidenceInput,
    target: &Document,
    acceptance_origin: (&RecordRef, i64),
    session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
) -> Result<Vec<Document>> {
    require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
    let target_body = session.parse_representation(&target.bytes, RepresentationKind::Record).map_err(core_error)?;
    let sequence = acceptance::counter(request_wrapper::text(target_body.value(), "commit_sequence", session)?, session)?;
    session.semantic_work().arithmetic(ArithmeticOperation::Compare, &[&sequence.into(), &i64::MAX.into()]).map_err(core_error)?;
    require(sequence < i64::MAX as u64, "INTEGRITY_FAILURE")?;
    session.semantic_work().arithmetic(ArithmeticOperation::Add, &[&sequence.into(), &1u64.into()]).map_err(core_error)?;
    // The target's immutable cutoff is sufficient for retention. Today's head,
    // source grant, binding validity interval and CAS belong to fresh revoke.
    let before = (sequence as i64).checked_add(1).ok_or(ServiceError::IntegrityFailure)?;
    let binding = request_wrapper::field(input.metadata(), "binding_ref", session)?;
    let target_ref = request_wrapper::field(input.metadata(), "target_transition_ref", session)?;
    let mut accepted = prefix::acceptance_for_original(ledger, tx, key, binding, before, session).await?;
    accepted.require_original_sequence(acceptance_origin.1, session)?;
    let actual = accepted.acceptance.accepted_reference();
    for (saved, reconstructed) in [
        (&acceptance_origin.0.kind, &actual.kind),
        (&acceptance_origin.0.id, &actual.id),
        (&acceptance_origin.0.digest, &actual.digest),
    ] {
        session.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::Element).map_err(core_error)?;
        request_wrapper::same_text(saved, reconstructed, session)?;
    }
    require(accepted.publication.program().uses_local_execution_recipe(), "UNSUPPORTED_SCHEMA_PROFILE")?;
    let setup = ledger.admin.read(&ledger.store, tx, key).await?;
    accepted.current_sources(tx, key, (ledger, &setup), &[], before, session).await?;
    let body = session.parse_representation(accepted.acceptance.binding.bytes(), RepresentationKind::Record).map_err(core_error)?;
    request_wrapper::same_text(request_wrapper::text(body.value(), "schema", session)?, "ledger-workflow-binding/2", session)?;
    require(acceptance::same_recipe_text(input.customer(), request_wrapper::text(body.value(), "customer", session)?, session)?, "INVALID_REFERENCE")?;
    require(acceptance::same_recipe_text(input.agreement_id().ok_or_else(|| reject("INVALID_SHAPE"))?, request_wrapper::text(body.value(), "agreement_id", session)?, session)?, "INVALID_REFERENCE")?;
    let schedule = accepted.completed_schedule_at_cutoff(key, body.value(), before, session)?;
    schedule.require_target_ref(target_ref, accepted.acceptance.binding.reference(), session)?;
    let documents = accepted.r1_stop_documents(tx, key, &schedule, target_ref, session).await?;
    session.check_cancelled().map_err(core_error)?;
    Ok(documents)
}
