//! Original Work reconstruction for a new transport alias. I is selected by
//! actual committed introductions; N remains comparison-only throughout.
use super::*;
use ledgerlab_core::{
    policy::workflow::transition_constructor::{
        self as core, EconomicCandidate, EconomicRecordView,
    },
    validation_work::{OriginalAuditSession, RepresentationKind},
};
use std::mem;
fn copy(value: &str, s: &mut OriginalAuditSession) -> Result<String> {
    s.semantic_work()
        .scan_bytes(value.len())
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(value.len())
        .map_err(core_error)?;
    Ok(value.to_owned())
}
fn reference(value: &core::ExactRef, s: &mut OriginalAuditSession) -> Result<core::ExactRef> {
    Ok(core::ExactRef {
        kind: copy(&value.kind, s)?,
        id: copy(&value.id, s)?,
        digest: copy(&value.digest, s)?,
    })
}
fn string(
    value: &str,
    s: &mut OriginalAuditSession,
) -> Result<ledgerlab_core::canonical::audit_arena::AuditedValue> {
    let b = wire::string(value, s)?;
    s.parse_representation(b.bytes(), RepresentationKind::Record)
        .map_err(core_error)
}
pub(super) async fn reconstruct(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    incoming: &ingress::IncomingRequest,
    s: &mut OriginalAuditSession,
) -> Result<EconomicCandidate> {
    require(
        local_financial_original::verify_work_alias(tx, original, incoming, s).await?,
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let w = original.wrappers();
    let key = original.key();
    let sequence = w.sequence();
    let evidence = request_wrapper::field(w.operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    let normalized = request_wrapper::field(evidence, "normalized_request", s)?;
    let payload = request_wrapper::field(normalized, "payload", s)?;
    request_wrapper::same_text(
        request_wrapper::text(payload, "schema", s)?,
        "ledger-workflow-work/1",
        s,
    )?;
    let mut pool = local_inputs::LocalInputs::new(s)?;
    pool.select_saved_registry(request_wrapper::text(resolution, "registry_digest", s)?, s)?;
    for r in request_wrapper::field(w.manifest(), "members", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if request_wrapper::text(r, "kind", s)? != "workflow-operation" {
            pool.add_value(r, s)?;
        }
    }
    pool.complete(
        tx,
        key,
        sequence
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?,
        s,
    )
    .await?;
    let all = pool.views();
    let _views = s
        .reserve_external_payload(3 * all.len() * mem::size_of::<EconomicRecordView<'_>>())
        .map_err(core_error)?;
    let mut inputs = Vec::new();
    let mut introduced = Vec::new();
    inputs
        .try_reserve_exact(all.len())
        .map_err(|_| ServiceError::Unavailable)?;
    introduced
        .try_reserve_exact(all.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for row in &all {
        let first = tx
            .workflow_selected_association_sequence(
                row.reference,
                &key.logical_store_id,
                &key.scope,
                sequence
                    .checked_add(1)
                    .ok_or(ServiceError::IntegrityFailure)?,
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        require(first <= sequence, "INTEGRITY_FAILURE")?;
        if first == sequence {
            introduced.push(*row);
        } else {
            inputs.push(*row);
        }
    }
    let (binding_ref, _binding_owner) =
        current::source::from_value_local(request_wrapper::field(payload, "binding_ref", s)?, s)?;
    let binding = s
        .parse_representation(pool.body(&binding_ref)?, RepresentationKind::Record)
        .map_err(core_error)?;
    let mut owner = prefix::acceptance_for(
        ledger,
        tx,
        key,
        request_wrapper::field(payload, "binding_ref", s)?,
        sequence,
        s,
    )
    .await?;
    require(
        owner.publication.program().uses_shared_namespace_recipe(),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let at = Timestamp::parse(request_wrapper::text(w.operation(), "accepted_at", s)?)
        .map_err(core_error)?;
    let activation = owner
        .original_initial_activation(tx, key, binding.value(), &at, sequence, s)
        .await?;
    let (context, _context_owner) = super::local_original_context::capture(
        w.operation(),
        activation.activation.reference(),
        s,
    )?;
    let (birth, _birth_owner) = prefix::source::initial_for_enrollment(
        tx,
        key,
        &context.authorities[0].enrollment_ref,
        sequence,
        s,
    )
    .await?;
    let source_correspondences = [birth.correspondence()];
    let mut heads =
        super::local_original_heads::OriginalHeads::before(tx, key, sequence, s).await?;
    // All copies below are bounded by actual retained input bytes plus explicit
    // typed carrier controls; original I bodies remain separately owned.
    let mut bytes = 0usize;
    for row in &inputs {
        bytes = bytes
            .checked_add(row.canonical_body.len())
            .ok_or(ServiceError::Unavailable)?;
    }
    bytes = bytes
        .checked_add(
            inputs
                .len()
                .checked_mul(
                    mem::size_of::<core::PartyAuthorizationHead>()
                        + mem::size_of::<core::InvocationHead>()
                        + mem::size_of::<core::StageHead>()
                        + 2 * mem::size_of::<core::ExactRef>(),
                )
                .ok_or(ServiceError::Unavailable)?,
        )
        .ok_or(ServiceError::Unavailable)?;
    let _state_owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    let frame = &context.authorities[0];
    let captured = [core::SourceAuthorityCapture {
        source_slot: &frame.source_slot,
        source: &context.key.source,
        principal: &context.actor,
        canonical_economic_namespace: &context.canonical_economic_namespace,
        enrollment_ref: &frame.enrollment_ref,
        current_state_ref: &frame.current_state_ref,
        current_grant_ref: &frame.current_grant_ref,
        original_grant_refs: &frame.original_grant_refs,
        binding_ref: &frame.binding_ref,
        accepted_agreement_ref: &frame.accepted_agreement_ref,
        activation_ref: &frame.activation_ref,
    }];
    let captured = core::EconomicCapture {
        key: &context.key,
        actor: &context.actor,
        canonical_economic_namespace: &context.canonical_economic_namespace,
        installation_mode: &context.installation_mode,
        received_at: &context.received_at,
        accepted_at: &context.accepted_at,
        sources: &captured,
    };
    let normalized_payload = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let keys = core::select_work_head_locators(
        owner.publication.program(),
        &captured,
        normalized_payload.bytes(),
        &inputs,
        s,
    )
    .map_err(core_error)?;
    let claim = string(keys.work_claim_id(), s)?;
    // This is the original economic birth, not another alias of an older
    // claim. Prove actual pre-birth absence from the chronological projection.
    require(
        heads
            .input("local-original-work-claim", &[claim.value()], &inputs, s)?
            .is_none(),
        "INTEGRITY_FAILURE",
    )?;
    let mut financial = core::DraftStateData::default();
    let mut parties = Vec::new();
    let mut invocations = Vec::new();
    parties
        .try_reserve_exact(inputs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    invocations
        .try_reserve_exact(inputs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    if let Some(row) = heads.input(
        "local-original-chain",
        &[
            request_wrapper::field(payload, "customer", s)?,
            request_wrapper::field(payload, "chain", s)?,
        ],
        &inputs,
        s,
    )? {
        financial.chain_head = Some(reference(row.reference, s)?);
    }
    for row in &inputs {
        let value = s
            .parse_representation(row.canonical_body, RepresentationKind::Record)
            .map_err(core_error)?;
        let v = value.value();
        match row.reference.kind.as_str() {
            "workflow-authority"
                if v.get("schema").and_then(Value::as_str)
                    == Some("ledger-workflow-party-authorization/1") =>
            {
                let spec = request_wrapper::field(v, "authorization", s)?;
                let id = request_wrapper::field(spec, "authorization_id", s)?;
                if let Some(selected) = heads.input("local-original-party", &[id], &inputs, s)? {
                    if selected.reference != row.reference {
                        continue;
                    }
                    let id = id.as_str().ok_or(ServiceError::IntegrityFailure)?;
                    parties.push(core::PartyAuthorizationHead {
                        authorization_id: copy(id, s)?,
                        revision: copy(request_wrapper::text(v, "revision", s)?, s)?,
                        reference: reference(row.reference, s)?,
                    });
                }
            }
            "workflow-evidence-verification" => financial
                .evidence_verifications
                .push(reference(row.reference, s)?),
            "workflow-link" => {
                let child = request_wrapper::field(v, "child", s)?;
                let (child, _lease) = current::source::from_value_local(child, s)?;
                let body = s
                    .parse_representation(pool.body(&child)?, RepresentationKind::Record)
                    .map_err(core_error)?;
                let subject = request_wrapper::field(body.value(), "payload", s)?;
                if request_wrapper::field(subject, "customer", s)?
                    == request_wrapper::field(payload, "customer", s)?
                    && request_wrapper::field(subject, "chain", s)?
                        == request_wrapper::field(payload, "chain", s)?
                    && request_wrapper::field(subject, "binding_ref", s)?
                        == request_wrapper::field(payload, "binding_ref", s)?
                {
                    financial.links.push(reference(row.reference, s)?);
                }
            }
            _ => {}
        }
    }
    if let Some(stage_ref) = payload.get("stage_instance_ref") {
        let (r, _lease) = current::source::from_value_local(stage_ref, s)?;
        let instance = s
            .parse_representation(pool.body(&r)?, RepresentationKind::Record)
            .map_err(core_error)?;
        if let Some(row) = heads.input("local-original-stage", &[stage_ref], &inputs, s)? {
            let state = s
                .parse_representation(row.canonical_body, RepresentationKind::Record)
                .map_err(core_error)?;
            financial
                .stage_heads
                .try_reserve_exact(1)
                .map_err(|_| ServiceError::Unavailable)?;
            financial.stage_heads.push(core::StageHead {
                agreement: copy(
                    request_wrapper::text(instance.value(), "agreement_id", s)?,
                    s,
                )?,
                chain: copy(request_wrapper::text(instance.value(), "chain", s)?, s)?,
                stage: copy(request_wrapper::text(instance.value(), "stage_name", s)?, s)?,
                revision: copy(request_wrapper::text(state.value(), "revision", s)?, s)?,
                reference: reference(row.reference, s)?,
            });
        }
    }
    for r in request_wrapper::field(payload, "invocation_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let id = string(request_wrapper::text(r, "id", s)?, s)?;
        if let Some(row) = heads.input("local-original-invocation", &[id.value()], &inputs, s)? {
            let state = s
                .parse_representation(row.canonical_body, RepresentationKind::Record)
                .map_err(core_error)?;
            invocations.push(core::InvocationHead {
                invocation_id: copy(
                    id.value().as_str().ok_or(ServiceError::IntegrityFailure)?,
                    s,
                )?,
                revision: copy(request_wrapper::text(state.value(), "revision", s)?, s)?,
                reference: reference(row.reference, s)?,
            });
        }
    }
    let app = core::AppStateData {
        cost_heads: &[],
        observation_refs: &[],
        cost_claim_refs: &[],
        evidence_consumption_refs: &[],
    };
    let supplier = core::SupplierStateData {
        invocation_heads: &invocations,
        cost_heads: &[],
        cost_consumption_heads: &[],
        cost_owner_heads: &[],
    };
    let state = core::EconomicStateData {
        retained_assertion: None,
        financial: &financial,
        capacity_heads: &[],
        supplier: Some(&supplier),
        app: Some(&app),
        current_party_authorizations: &parties,
        observation: None,
        audit: None,
        initial_source_correspondences: &source_correspondences,
    };
    let (id, digest) = w.operation_identity();
    let id = std::str::from_utf8(id).map_err(|_| ServiceError::IntegrityFailure)?;
    let digest = std::str::from_utf8(digest).map_err(|_| ServiceError::IntegrityFailure)?;
    let encoded = wire::object(
        &[
            ("kind", b"\"workflow-operation\""),
            ("id", wire::string(id, s)?.bytes()),
            ("digest", wire::string(digest, s)?.bytes()),
        ],
        262144,
        s,
    )?;
    let parsed = s
        .parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let (operation_ref, _operation_owner) = current::source::from_value_local(parsed.value(), s)?;
    let operation = s
        .canonical_representation(w.operation(), RepresentationKind::OperationFrame)
        .map_err(core_error)?;
    core::reconstruct_original_candidate(
        owner.publication.program(),
        &core::SavedFinancialInput {
            operation: EconomicRecordView {
                reference: &operation_ref,
                canonical_body: operation.bytes(),
            },
            original_request_bytes: w.original_request().raw(),
            original_context: &context,
            activation_ref: activation.activation.reference(),
            original_state: &state,
            original_inputs: &inputs,
            introduced_results: &introduced,
        },
        s,
    )
    .map_err(core_error)
}
