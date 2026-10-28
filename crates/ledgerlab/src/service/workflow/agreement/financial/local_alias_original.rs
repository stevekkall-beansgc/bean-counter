//! Saved semantic-alias originals rederive the same sealed original meaning
//! and the alias's historical delivery authority. No current admission occurs.
use super::*;
use ledgerlab_core::{
    policy::workflow::transition_constructor::{self as core, EconomicRecordView},
    validation_work::{OriginalAuditSession, RepresentationKind},
};
pub(super) async fn verify(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    incoming: Option<&ingress::IncomingRequest>,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    Ok(reconstruct(ledger, tx, original, incoming, s)
        .await?
        .is_some())
}
pub(super) async fn reconstruct(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    incoming: Option<&ingress::IncomingRequest>,
    s: &mut OriginalAuditSession,
) -> Result<Option<core::EconomicCandidate>> {
    let w = original.wrappers();
    if w.class() != wrappers::WireClass::Financial
        || request_wrapper::text(w.receipt(), "result_kind", s)? != "semantic_alias"
        || request_wrapper::text(w.operation(), "kind", s)? != "work.accept"
    {
        return Ok(None);
    }
    let ev = request_wrapper::field(w.operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(ev, "descriptor_resolution", s)?;
    if !wrappers::is_local_registry(request_wrapper::text(resolution, "registry_digest", s)?, s)? {
        return Ok(None);
    }
    require(
        local_financial_original::verify_alias_transport(tx, original, incoming, s).await?,
        "INTEGRITY_FAILURE",
    )?;
    let saved = ingress::IncomingRequest::parse(w.original_request().raw(), s)?;
    let key = original.key();
    let before = w
        .sequence()
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut pool = local_inputs::LocalInputs::new(s)?;
    pool.select_saved_registry(request_wrapper::text(resolution, "registry_digest", s)?, s)?;
    for r in request_wrapper::field(w.manifest(), "members", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let kind = request_wrapper::text(r, "kind", s)?;
        let own = kind == "workflow-operation"
            && request_wrapper::text(r, "id", s)?.as_bytes() == w.operation_identity().0;
        if !own {
            pool.add_value(r, s)?;
        }
    }
    pool.complete(tx, key, before, s).await?;
    let mut work = None;
    for r in request_wrapper::field(w.receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if request_wrapper::text(r, "kind", s)? == "workflow-work" {
            require(work.replace(r).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    let (work, _work_owner) =
        current::source::from_value_local(work.ok_or(ServiceError::IntegrityFailure)?, s)?;
    let sequence = tx
        .workflow_selected_association_sequence(
            &work,
            &key.logical_store_id,
            &key.scope,
            w.sequence(),
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let prior = sequence
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let economic = prefix::next(tx, key, prior, sequence, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    let reconstructed = local_original_work::reconstruct(ledger, tx, &economic, &saved, s).await?;
    let mut activation = None;
    for row in reconstructed.introduced() {
        if row.reference().kind == "workflow-chain-state" {
            let body = s
                .parse_representation(row.canonical_body(), RepresentationKind::Record)
                .map_err(core_error)?;
            let (r, owner) = current::source::from_value_local(
                request_wrapper::field(body.value(), "activation_ref", s)?,
                s,
            )?;
            require(
                activation.replace((r, owner)).is_none(),
                "INTEGRITY_FAILURE",
            )?;
        }
    }
    let (activation, _activation_owner) = activation.ok_or(ServiceError::IntegrityFailure)?;
    let (context, _context_owner) = local_original_context::capture(w.operation(), &activation, s)?;
    let (birth, _birth_owner) = prefix::source::initial_for_enrollment(
        tx,
        key,
        &context.authorities[0].enrollment_ref,
        w.sequence(),
        s,
    )
    .await?;
    let correspondences = [birth.correspondence()];
    let accepted_ref = request_wrapper::field(
        request_wrapper::field(w.operation(), "authority_snapshot", s)?,
        "original_accepted_agreement_ref",
        s,
    )?;
    let (accepted_ref, _accepted_owner) = current::source::from_value_local(accepted_ref, s)?;
    let accepted = s
        .parse_representation(pool.body(&accepted_ref)?, RepresentationKind::Record)
        .map_err(core_error)?;
    let publication = prefix::publication_for(
        tx,
        key,
        request_wrapper::field(accepted.value(), "publication_ref", s)?,
        w.sequence(),
        s,
    )
    .await?;
    require(
        publication.program().uses_shared_namespace_recipe(),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let pointer = economic.wrappers().origin_pointer(s)?;
    let pointer = s
        .parse_representation(pointer.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let (op, _op_owner) = current::source::from_value_local(
        request_wrapper::field(pointer.value(), "operation_ref", s)?,
        s,
    )?;
    let (receipt, _receipt_owner) = current::source::from_value_local(
        request_wrapper::field(pointer.value(), "receipt_ref", s)?,
        s,
    )?;
    let operation_body = s
        .canonical_representation(
            economic.wrappers().operation(),
            RepresentationKind::OperationFrame,
        )
        .map_err(core_error)?;
    let receipt_body = s
        .canonical_representation(economic.wrappers().receipt(), RepresentationKind::Record)
        .map_err(core_error)?;
    let source = &context.authorities[0];
    let frames = [core::SourceAuthorityCapture {
        source_slot: &source.source_slot,
        source: &context.key.source,
        principal: &context.actor,
        canonical_economic_namespace: &context.canonical_economic_namespace,
        enrollment_ref: &source.enrollment_ref,
        current_state_ref: &source.current_state_ref,
        current_grant_ref: &source.current_grant_ref,
        original_grant_refs: &source.original_grant_refs,
        binding_ref: &source.binding_ref,
        accepted_agreement_ref: &source.accepted_agreement_ref,
        activation_ref: &source.activation_ref,
    }];
    let capture = core::EconomicCapture {
        key: &context.key,
        actor: &context.actor,
        canonical_economic_namespace: &context.canonical_economic_namespace,
        installation_mode: &context.installation_mode,
        received_at: &context.received_at,
        accepted_at: &context.accepted_at,
        sources: &frames,
    };
    let saved_payload = saved.payload(s)?;
    let payload = s
        .canonical_representation(saved_payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let (inputs, _views_owner) = pool.owned_views(0, s)?;
    let candidate = core::construct_local_work_alias_candidate(
        publication.program(),
        &capture,
        payload.bytes(),
        reconstructed,
        (
            EconomicRecordView {
                reference: &work,
                canonical_body: pool.body(&work)?,
            },
            EconomicRecordView {
                reference: &op,
                canonical_body: operation_body.bytes(),
            },
            EconomicRecordView {
                reference: &receipt,
                canonical_body: receipt_body.bytes(),
            },
        ),
        (&inputs, &correspondences),
        s,
    )
    .map_err(core_error)?;
    let financial = core::DraftStateData::default();
    let state = core::EconomicStateData {
        retained_assertion: None,
        financial: &financial,
        capacity_heads: &[],
        supplier: None,
        app: None,
        current_party_authorizations: &[],
        observation: None,
        audit: None,
        initial_source_correspondences: &correspondences,
    };
    let evidence = core::produce_local_financial_request_evidence(
        publication.program(),
        saved.raw(),
        &candidate,
        &inputs,
        &state,
        s,
    )
    .map_err(core_error)?;
    let received =
        Timestamp::parse_with_work(&context.received_at, s.semantic_work()).map_err(core_error)?;
    let accepted_at =
        Timestamp::parse_with_work(&context.accepted_at, s.semantic_work()).map_err(core_error)?;
    let journal = wire::frame_candidate(
        &candidate,
        key,
        "work.accept",
        evidence.value(),
        &received,
        &accepted_at,
        w.sequence(),
        s,
    )?;
    journal.compare_original(w, s)?;
    Ok(Some(candidate))
}
