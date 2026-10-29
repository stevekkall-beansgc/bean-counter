//! Owning local original reconstruction from the completed chronological prefix.
//! I is selected by actual earlier introductions; N stays comparison-only.
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
pub(super) fn same_ref(
    a: &core::ExactRef,
    b: &core::ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    for (a, b) in [(&a.kind, &b.kind), (&a.id, &b.id), (&a.digest, &b.digest)] {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(a.len() + b.len())
            .map_err(core_error)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
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
    let bytes = {
        let acquire = prefix::before_inputs(
            ledger,
            tx,
            original.key(),
            None,
            original.wrappers().sequence(),
            None,
            s,
        );
        mem::size_of_val(&acquire)
    };
    let _acquisition = s.reserve_external_payload(bytes).map_err(core_error)?;
    let (mut prefix, _) = Box::pin(prefix::before_inputs(
        ledger,
        tx,
        original.key(),
        None,
        original.wrappers().sequence(),
        None,
        s,
    ))
    .await?;
    reconstruct_selected(
        ledger,
        tx,
        original,
        prefix.as_mut().ok_or(ServiceError::IntegrityFailure)?,
        s,
    )
    .await
}
/// SAME saved constructor for local operations. The enclosing chronological
/// owner supplies authenticated I and prestate; saved N is comparison-only.
pub(super) async fn reconstruct_selected(
    _ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    prefix: &mut prefix::FoldedInventory,
    s: &mut OriginalAuditSession,
) -> Result<EconomicCandidate> {
    reconstruct_selected_capture(_ledger, tx, original, prefix, s, false).await.map(|(candidate, _)| candidate)
}
pub(super) async fn reconstruct_selected_capture(
    _ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    prefix: &mut prefix::FoldedInventory,
    s: &mut OriginalAuditSession,
    capture_path: bool,
) -> Result<(EconomicCandidate, Option<core::SelectedReportPath>)> {
    require(
        local_financial_original::verify(tx, original, None, s).await?,
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;

    let w = original.wrappers();
    let key = original.key();
    let sequence = w.sequence();
    let evidence = request_wrapper::field(w.operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    let normalized = request_wrapper::field(evidence, "normalized_request", s)?;
    let payload = request_wrapper::field(normalized, "payload", s)?;
    let work = request_wrapper::text(payload, "schema", s)? == "ledger-workflow-work/1";
    let mut pool = local_inputs::LocalInputs::new(s)?;
    pool.select_saved_registry(request_wrapper::text(resolution, "registry_digest", s)?, s)?;
    for r in request_wrapper::field(w.manifest(), "members", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if !w.is_own_operation(r, s)? {
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
    // Original operation authority supplies only the exact accepted/Binding
    // locator; its body remains independently checked against completed I.
    let snapshot = request_wrapper::field(w.operation(), "authority_snapshot", s)?;
    let (binding_ref, _binding_owner) = current::source::from_value_local(
        request_wrapper::field(snapshot, "original_binding_ref", s)?,
        s,
    )?;
    let binding = s
        .parse_representation(pool.body(&binding_ref)?, RepresentationKind::Record)
        .map_err(core_error)?;
    for row in &inputs {
        prefix
            ._inventory
            .compare_record(row.reference, row.canonical_body, s)?;
    }

    let accepted_ref = request_wrapper::field(snapshot, "original_accepted_agreement_ref", s)?;
    let (accepted_ref, _accepted_owner) = current::source::from_value_local(accepted_ref, s)?;
    let accepted = s
        .parse_representation(pool.body(&accepted_ref)?, RepresentationKind::Record)
        .map_err(core_error)?;
    let owner = prefix::publication_for(
        tx,
        key,
        request_wrapper::field(accepted.value(), "publication_ref", s)?,
        sequence,
        s,
    )
    .await?;

    owner.compare_completed_inputs(&mut prefix._inventory, s)?;

    require(
        owner.program().uses_local_execution_recipe(),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let schema = request_wrapper::text(payload, "schema", s)?;
    require(
        work || (owner.program().uses_stage_lifecycle_recipe()
            && matches!(
                schema,
                "ledger-workflow-fact-attest/1"
                    | "ledger-workflow-stage-authorize/1"
                    | "ledger-workflow-stage-close/1"
                    | "ledger-workflow-invocation-authorize/1"
                    | "ledger-workflow-invocation-close/1"
                    | "ledger-workflow-work-reverse/2"
                    | "ledger-workflow-quantity-correction/2"
            ))
            || (owner.program().uses_component_lifecycle_recipe()
                && matches!(
                    schema,
                    "ledger-workflow-cost-observation-attest/1"
                        | "ledger-workflow-link-assert/1"
                        | "ledger-workflow-fact-attest/1"
                        | "ledger-workflow-family-close-attest/1"
                        | "ledger-workflow-outcome/1"
                        | "ledger-workflow-outcome/2"
                        | "ledger-workflow-outcome-correction/1"
                        | "ledger-workflow-outcome-correction/2"
                        | "ledger-workflow-outcome-close/2"
                        | "ledger-workflow-work-reverse/2"
                        | "ledger-workflow-quantity-correction/2"
                )),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;

    let at = Timestamp::parse(request_wrapper::text(w.operation(), "accepted_at", s)?)
        .map_err(core_error)?;
    let activation = prefix.completed_initial_activation(&binding_ref, binding.value(), &at, s)?;

    let (mut context, _context_owner) = super::local_original_context::capture_selected(
        w.operation(),
        activation.activation.reference(),
        owner.program(),
        s,
    )?;

    let normalized_payload = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let frame = &context.authorities[0];
    let primary = [core::SourceAuthorityCapture {
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
    let capture = core::EconomicCapture {
        key: &context.key,
        actor: &context.actor,
        canonical_economic_namespace: &context.canonical_economic_namespace,
        installation_mode: &context.installation_mode,
        received_at: &context.received_at,
        accepted_at: &context.accepted_at,
        sources: &primary,
    };
    let mut locators = if work {
        None
    } else {
        Some(
            core::select_local_operation_locators(
                owner.program(),
                &capture,
                normalized_payload.bytes(),
                &inputs,
                s,
            )
            .map_err(core_error)?,
        )
    };

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
    if work {
        let frames: Vec<_> = context
            .authorities
            .iter()
            .map(|frame| core::SourceAuthorityCapture {
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
            })
            .collect();
        let capture = core::EconomicCapture {
            key: &context.key,
            actor: &context.actor,
            canonical_economic_namespace: &context.canonical_economic_namespace,
            installation_mode: &context.installation_mode,
            received_at: &context.received_at,
            accepted_at: &context.accepted_at,
            sources: &frames,
        };
        let keys = core::select_work_head_locators(
            owner.program(),
            &capture,
            normalized_payload.bytes(),
            &inputs,
            s,
        )
        .map_err(core_error)?;
        let claim = string(keys.work_claim_id(), s)?;
        require(
            prefix
                .original_input("local-original-work-claim", &[claim.value()], &inputs, s)?
                .is_none(),
            "INTEGRITY_FAILURE",
        )?;
        let decision = string(keys.root_decision_id(), s)?;
        require(
            prefix
                .original_input("local-original-decision", &[decision.value()], &inputs, s)?
                .is_none(),
            "INTEGRITY_FAILURE",
        )?;
        require(
            prefix
                .original_input("local-original-capacity", &[decision.value()], &inputs, s)?
                .is_none(),
            "INTEGRITY_FAILURE",
        )?;
    }
    let original_outcome_claim = if owner.program().uses_component_lifecycle_recipe()
        && schema == "ledger-workflow-outcome-correction/2"
    {
        let (claim, _lease) = current::source::from_value_local(
            request_wrapper::field(payload, "original_claim_ref", s)?, s,
        )?;
        Some(s.parse_representation(pool.body(&claim)?, RepresentationKind::Record).map_err(core_error)?)
    } else {
        None
    };
    let selector_start = if owner.program().uses_component_lifecycle_recipe() {
        match schema {
            "ledger-workflow-outcome/2" => payload.get("selector_start_work_ref"),
            "ledger-workflow-outcome-correction/2" => original_outcome_claim.as_ref()
                .and_then(|claim| claim.value().get("original_payload"))
                .and_then(|original| original.get("selector_start_work_ref")),
            _ => None,
        }
    } else {
        None
    };
    let chain_owner = if let Some(r) = payload
        .get("stage_instance_ref")
        .or_else(|| payload.get("work_ref"))
        .or(selector_start)
        .or_else(|| {
            if owner.program().uses_component_lifecycle_recipe()
                && schema == "ledger-workflow-link-assert/1"
            {
                payload.get("child")
            } else {
                None
            }
        }) {
        let (r, _lease) = current::source::from_value_local(r, s)?;
        Some(
            s.parse_representation(pool.body(&r)?, RepresentationKind::Record)
                .map_err(core_error)?,
        )
    } else {
        None
    };
    let chain_subject = if let Some(body) = &chain_owner {
        match request_wrapper::text(body.value(), "schema", s)? {
            "ledger-workflow-stage-instance/1" => body.value(),
            "ledger-workflow-work-record/1" => request_wrapper::field(body.value(), "payload", s)?,
            _ => return Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
        }
    } else {
        payload.get("stage_instance").unwrap_or(payload)
    };
    if selector_start.is_some() {
        let (selected_binding, _lease) = current::source::from_value_local(
            request_wrapper::field(chain_subject, "binding_ref", s)?, s,
        )?;
        require(same_ref(&selected_binding, &binding_ref, s)?, "INTEGRITY_FAILURE")?;
    }
    let declared_proofs = if owner.program().uses_stage_lifecycle_recipe()
        && request_wrapper::text(payload, "schema", s)? == "ledger-workflow-stage-close/1"
    {
        Some(
            request_wrapper::field(payload, "fact_proofs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .as_slice(),
        )
    } else if owner.program().uses_component_lifecycle_recipe() {
        let subject = current::app::outcome_proof_payload(payload, schema, s)?;
        Some(if let Some(proofs) = subject.get("fact_proofs") {
            proofs
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .as_slice()
        } else {
            &[]
        })
    } else {
        None
    };
    let mut financial = core::DraftStateData::default();
    let mut parties = Vec::new();
    let mut invocations = Vec::new();
    parties
        .try_reserve_exact(inputs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    invocations
        .try_reserve_exact(inputs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    if let (Some(customer), Some(chain)) =
        (chain_subject.get("customer"), chain_subject.get("chain"))
    {
        if let Some(row) =
            prefix.original_input("local-original-chain", &[customer, chain], &inputs, s)?
        {
            financial.chain_head = Some(reference(row.reference, s)?);
        }
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
                if let Some(selected) = prefix.original_input("party-head", &[id], &inputs, s)? {
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
            "workflow-evidence-verification" if declared_proofs.is_none() => financial
                .evidence_verifications
                .push(reference(row.reference, s)?),
            "workflow-link" if chain_subject.get("chain").is_some() => {
                let child = request_wrapper::field(v, "child", s)?;
                let (child, _lease) = current::source::from_value_local(child, s)?;
                let body = s
                    .parse_representation(pool.body(&child)?, RepresentationKind::Record)
                    .map_err(core_error)?;
                let outcome_predecessor = if owner.program().uses_component_lifecycle_recipe()
                    && request_wrapper::text(body.value(), "schema", s)? == "ledger-workflow-outcome-claim/2"
                {
                    let predecessor = request_wrapper::field(v, "predecessor", s)?;
                    let original = request_wrapper::field(body.value(), "original_payload", s)?;
                    let start = request_wrapper::field(original, "selector_start_work_ref", s)?;
                    let (predecessor_ref, _lease) = current::source::from_value_local(predecessor, s)?;
                    let (start_ref, _lease) = current::source::from_value_local(start, s)?;
                    require(same_ref(&predecessor_ref, &start_ref, s)?, "INTEGRITY_FAILURE")?;
                    Some(s.parse_representation(pool.body(&predecessor_ref)?, RepresentationKind::Record).map_err(core_error)?)
                } else {
                    None
                };
                let subject_body = outcome_predecessor.as_ref().unwrap_or(&body);
                let subject = match request_wrapper::text(subject_body.value(), "schema", s)? {
                    "ledger-workflow-work-record/1" => {
                        request_wrapper::field(subject_body.value(), "payload", s)?
                    }
                    "ledger-workflow-stage-instance/1" => subject_body.value(),
                    _ => return Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
                };
                let mut matches = true;
                for field in ["customer", "chain", "binding_ref"] {
                    let a_value = request_wrapper::field(subject, field, s)?;
                    let a = s
                        .canonical_representation(a_value, RepresentationKind::Record)
                        .map_err(core_error)?;
                    let b_value = request_wrapper::field(chain_subject, field, s)?;
                    let b = s
                        .canonical_representation(b_value, RepresentationKind::Record)
                        .map_err(core_error)?;
                    s.semantic_work()
                        .scan_bytes(a.bytes().len() + b.bytes().len())
                        .map_err(core_error)?;
                    if a.bytes() != b.bytes() {
                        matches = false;
                    }
                }
                if matches {
                    financial.links.push(reference(row.reference, s)?);
                }
            }
            _ => {}
        }
    }
    if let Some(proofs) = declared_proofs {
        // Repeat the forward declaration selector at the original cutoff. All
        // transitive Work proofs remain I, but are not this closure's proofs.
        financial
            .evidence_verifications
            .try_reserve_exact(inputs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        for proof in proofs {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            let (attestation, _attestation_owner) = current::source::from_value_local(
                request_wrapper::field(proof, "attestation_ref", s)?,
                s,
            )?;
            let (selected, _selected_owner) =
                current::app::retained_verification(tx, key, sequence, &attestation, s).await?;
            let mut matched = None;
            for input in &inputs {
                if same_ref(input.reference, &selected, s)? {
                    require(matched.replace(input).is_none(), "INTEGRITY_FAILURE")?;
                }
            }
            let matched = matched.ok_or(ServiceError::IntegrityFailure)?;
            let mut duplicate = false;
            for old in &financial.evidence_verifications {
                if same_ref(old, matched.reference, s)? {
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                financial
                    .evidence_verifications
                    .push(reference(matched.reference, s)?);
            }
        }
    }
    if let Some(stage_ref) = payload.get("stage_instance_ref") {
        let (r, _lease) = current::source::from_value_local(stage_ref, s)?;
        let instance = s
            .parse_representation(pool.body(&r)?, RepresentationKind::Record)
            .map_err(core_error)?;
        if let Some(row) =
            prefix.original_input("local-original-stage", &[stage_ref], &inputs, s)?
        {
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
    for r in payload
        .get("invocation_refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = string(request_wrapper::text(r, "id", s)?, s)?;
        if let Some(row) =
            prefix.original_input("local-original-invocation", &[id.value()], &inputs, s)?
        {
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
    // Required immutable identities and every prestate hit come from the
    // completed chronological projection. A missing I body is an error; an
    // absent projection is therefore not confused with an omitted operand.
    let mut capacities = Vec::new();
    let _head_controls = s
        .reserve_external_payload(
            inputs.len()
                * (mem::size_of::<core::CapacityHead>()
                    + mem::size_of::<core::DecisionHead>()
                    + mem::size_of::<core::FamilyHead>()),
        )
        .map_err(core_error)?;
    for row in &inputs {
        let body = s
            .parse_representation(row.canonical_body, RepresentationKind::Record)
            .map_err(core_error)?;
        let v = body.value();
        match row.reference.kind.as_str() {
            "workflow-decision" => {
                let unpriced_reverse = schema == "ledger-workflow-work-reverse/2"
                    && locators.as_ref().and_then(|l| l.work_heads()).is_some_and(|heads|
                        heads.family_keys().is_empty() && heads.app_decision_ids().is_empty())
                    && payload.get("family_closures").and_then(Value::as_array).is_some_and(Vec::is_empty)
                    && payload.get("dependent_reversals").and_then(Value::as_array).is_some_and(Vec::is_empty);
                if work && owner.program().uses_component_lifecycle_recipe()
                    && request_wrapper::text(v, "kind", s)? == "outcome"
                {
                    continue;
                }
                let id = request_wrapper::field(v, "decision_id", s)?;
                if matches!(schema, "ledger-workflow-fact-attest/1" | "ledger-workflow-outcome-correction/2" | "ledger-workflow-work-reverse/2" | "ledger-workflow-quantity-correction/2") && owner.program().uses_component_lifecycle_recipe() {
                    let selected = locators.as_ref().ok_or(ServiceError::IntegrityFailure)?;
                    let heads = selected.work_heads().ok_or(ServiceError::IntegrityFailure)?;
                    let decision_id = id.as_str().ok_or(ServiceError::IntegrityFailure)?;
                    let mut matches = false;
                    for wanted in std::iter::once(heads.root_decision_id())
                        .chain(heads.family_keys().iter().map(|h| h.decision_id()))
                        .chain(heads.app_decision_ids().iter().map(String::as_str))
                        .chain(selected.selector_work_heads().iter().map(|h| h.root_decision_id()))
                    {
                        s.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare).map_err(core_error)?;
                        s.semantic_work().scan_bytes(decision_id.len() + wanted.len()).map_err(core_error)?;
                        matches |= decision_id == wanted;
                    }
                    if !matches { continue; }
                }
                if unpriced_reverse && owner.program().uses_component_lifecycle_recipe() {
                    let root = locators.as_ref().and_then(|l| l.work_heads()).ok_or(ServiceError::IntegrityFailure)?.root_decision_id();
                    s.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare).map_err(core_error)?;
                    s.semantic_work().scan_bytes(id.as_str().ok_or(ServiceError::IntegrityFailure)?.len() + root.len()).map_err(core_error)?;
                    if id.as_str() != Some(root) { continue; }
                }
                if prefix
                    .original_input("local-original-decision", &[id], &inputs, s)?
                    .is_some_and(|r| r.reference == row.reference)
                {
                    financial.decision_heads.push(core::DecisionHead {
                        decision_id: copy(id.as_str().ok_or(ServiceError::IntegrityFailure)?, s)?,
                        revision: copy(request_wrapper::text(v, "revision", s)?, s)?,
                        reference: reference(row.reference, s)?,
                    });
                }
            }
            "workflow-correction-capacity" => {
                let root = if request_wrapper::text(v, "schema", s)?
                    == "ledger-correction-capacity/2"
                {
                    let (reference, _lease) = current::source::from_value_local(
                        request_wrapper::field(v, "root_ref", s)?,
                        s,
                    )?;
                    request_wrapper::same_text(&reference.kind, "workflow-decision", s)?;
                    Some(
                        s.parse_representation(pool.body(&reference)?, RepresentationKind::Record)
                            .map_err(core_error)?,
                    )
                } else {
                    None
                };
                let id = if let Some(root) = &root {
                    request_wrapper::field(root.value(), "decision_id", s)?
                } else {
                    request_wrapper::field(v, "root_decision_id", s)?
                };
                if prefix
                    .original_input("local-original-capacity", &[id], &inputs, s)?
                    .is_some_and(|r| r.reference == row.reference)
                {
                    capacities.push(core::CapacityHead {
                        root_decision_id: copy(
                            id.as_str().ok_or(ServiceError::IntegrityFailure)?,
                            s,
                        )?,
                        revision: copy(request_wrapper::text(v, "revision", s)?, s)?,
                        reference: reference(row.reference, s)?,
                    });
                }
            }
            "workflow-family-state" => {
                let target = request_wrapper::field(v, "target_ref", s)?;
                let slot = request_wrapper::field(v, "family_slot", s)?;
                if prefix
                    .original_input("local-original-family", &[target, slot], &inputs, s)?
                    .is_some_and(|r| r.reference == row.reference)
                {
                    let (target, _owner) = current::source::from_value_local(target, s)?;
                    financial.family_heads.push(core::FamilyHead {
                        target_ref: target,
                        family_slot: copy(slot.as_str().ok_or(ServiceError::IntegrityFailure)?, s)?,
                        revision: copy(request_wrapper::text(v, "revision", s)?, s)?,
                        reference: reference(row.reference, s)?,
                    });
                }
            }
            _ => {}
        }
    }
    let _selected_owner;
    if owner.program().uses_component_lifecycle_recipe()
        && schema == "ledger-workflow-outcome-correction/2"
    {
        let root = locators.as_ref().and_then(|l| l.work_heads())
            .ok_or(ServiceError::IntegrityFailure)?.root_decision_id();
        let bytes = financial.decision_heads.len().checked_mul(mem::size_of::<core::DecisionHead>()).ok_or(ServiceError::Unavailable)?;
        _selected_owner = Some(s.reserve_external_payload(bytes).map_err(core_error)?);
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        let mut selected = Vec::new();
        selected.try_reserve_exact(financial.decision_heads.len()).map_err(|_| ServiceError::Unavailable)?;
        for head in financial.decision_heads.drain(..) {
            s.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare).map_err(core_error)?;
            s.semantic_work().scan_bytes(head.decision_id.len() + root.len()).map_err(core_error)?;
            let mut keep = head.decision_id == root;
            for family in &financial.family_heads {
                let state = s.parse_representation(pool.body(&family.reference)?, RepresentationKind::Record).map_err(core_error)?;
                if let Some(value) = state.value().get("decision_ref") {
                    let (decision, _lease) = current::source::from_value_local(value, s)?;
                    keep |= same_ref(&head.reference, &decision, s)?;
                }
            }
            if keep { selected.push(head); }
        }
        financial.decision_heads = selected;
    } else {
        _selected_owner = None;
    }
    let (ck, _ck_owner) = local_inputs::core_key_with_owner(key, s)?;
    let stage_locator = core::select_stage_operation_locators(
        owner.program(),
        &ck,
        normalized_payload.bytes(),
        &inputs,
        s,
    )
    .map_err(core_error)?;
    let invocation_locator = core::select_invocation_operation_locators(
        owner.program(),
        &ck,
        normalized_payload.bytes(),
        &inputs,
        s,
    )
    .map_err(core_error)?;
    for embedded in payload
        .get("invocation_inputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let raw = s
            .canonical_representation(embedded, RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        let locator = core::select_invocation_operation_locators(
            owner.program(),
            &ck,
            raw.bytes(),
            &inputs,
            s,
        )
        .map_err(core_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
        let id = string(locator.invocation_id(), s)?;
        if let Some(row) =
            prefix.original_input("local-original-invocation", &[id.value()], &inputs, s)?
        {
            let body = s
                .parse_representation(row.canonical_body, RepresentationKind::Record)
                .map_err(core_error)?;
            invocations.push(core::InvocationHead {
                invocation_id: copy(locator.invocation_id(), s)?,
                revision: copy(request_wrapper::text(body.value(), "revision", s)?, s)?,
                reference: reference(row.reference, s)?,
            });
        }
    }
    if let Some(locator) = &invocation_locator {
        let id = string(locator.invocation_id(), s)?;
        if let Some(row) =
            prefix.original_input("local-original-invocation", &[id.value()], &inputs, s)?
        {
            if !invocations
                .iter()
                .any(|head| head.invocation_id == locator.invocation_id())
            {
                let body = s
                    .parse_representation(row.canonical_body, RepresentationKind::Record)
                    .map_err(core_error)?;
                invocations.push(core::InvocationHead {
                    invocation_id: copy(locator.invocation_id(), s)?,
                    revision: copy(request_wrapper::text(body.value(), "revision", s)?, s)?,
                    reference: reference(row.reference, s)?,
                });
            }
        }
        if let Some(id) = locator.work_claim_id() {
            let id = string(id, s)?;
            if let Some(row) =
                prefix.original_input("local-original-work-claim", &[id.value()], &inputs, s)?
            {
                financial.work_claims.push(reference(row.reference, s)?);
            }
        }
    }
    if let Some(locator) = &stage_locator {
        let a = string(locator.agreement(), s)?;
        let c = string(locator.chain(), s)?;
        let t = string(locator.stage(), s)?;
        if let Some(row) = prefix.original_input(
            "local-original-stage-name",
            &[a.value(), c.value(), t.value()],
            &inputs,
            s,
        )? {
            if financial.stage_heads.is_empty() {
                let body = s
                    .parse_representation(row.canonical_body, RepresentationKind::Record)
                    .map_err(core_error)?;
                financial.stage_heads.push(core::StageHead {
                    agreement: copy(locator.agreement(), s)?,
                    chain: copy(locator.chain(), s)?,
                    stage: copy(locator.stage(), s)?,
                    revision: copy(request_wrapper::text(body.value(), "revision", s)?, s)?,
                    reference: reference(row.reference, s)?,
                });
            }
        }
        for expected in locator.expected_work_heads() {
            let id = string(expected.work_claim_id(), s)?;
            if let Some(row) =
                prefix.original_input("local-original-work-claim", &[id.value()], &inputs, s)?
            {
                financial.work_claims.push(reference(row.reference, s)?);
            }
            if schema == "ledger-workflow-stage-close/1" {
                let claim_id = string(expected.work_claim_id(), s)?;
                if let Some(claim) = prefix.original_input(
                    "local-original-work-claim",
                    &[claim_id.value()],
                    &inputs,
                    s,
                )? {
                    let named =
                        pool.stage_member_invocations(owner.program(), &ck, claim.reference, s)?;
                    for invocation in named.heads() {
                        let id = string(invocation.invocation_id(), s)?;
                        if let Some(row) = prefix.original_input(
                            "local-original-invocation",
                            &[id.value()],
                            &inputs,
                            s,
                        )? {
                            let body = s
                                .parse_representation(
                                    row.canonical_body,
                                    RepresentationKind::Record,
                                )
                                .map_err(core_error)?;
                            let mut duplicate = false;
                            for old in &invocations {
                                s.semantic_work().visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare).map_err(core_error)?;
                                s.semantic_work()
                                    .scan_bytes(
                                        old.invocation_id.len() + invocation.invocation_id().len(),
                                    )
                                    .map_err(core_error)?;
                                if old.invocation_id == invocation.invocation_id() {
                                    require(
                                        same_ref(&old.reference, row.reference, s)?,
                                        "INTEGRITY_FAILURE",
                                    )?;
                                    request_wrapper::same_text(
                                        &old.revision,
                                        request_wrapper::text(body.value(), "revision", s)?,
                                        s,
                                    )?;
                                    duplicate = true;
                                    break;
                                }
                            }
                            if duplicate {
                                continue;
                            }
                            invocations.push(core::InvocationHead {
                                invocation_id: copy(invocation.invocation_id(), s)?,
                                revision: copy(
                                    request_wrapper::text(body.value(), "revision", s)?,
                                    s,
                                )?,
                                reference: reference(row.reference, s)?,
                            });
                        }
                    }
                }
            }
            let id = string(expected.decision_id(), s)?;
            prefix.original_input("local-original-decision", &[id.value()], &inputs, s)?;
            prefix.original_input("local-original-capacity", &[id.value()], &inputs, s)?;
        }
        if let Some(id) = locator.closure_decision_id() {
            let id = string(id, s)?;
            prefix.original_input("local-original-decision", &[id.value()], &inputs, s)?;
        }
    }
    if let Some(locator) = locators.as_ref().and_then(|l| l.work_heads()) {
        let id = string(locator.work_claim_id(), s)?;
        if let Some(row) =
            prefix.original_input("local-original-work-claim", &[id.value()], &inputs, s)?
        {
            if !(owner.program().uses_component_lifecycle_recipe()
                && matches!(schema, "ledger-workflow-outcome/1" | "ledger-workflow-outcome/2" | "ledger-workflow-outcome-correction/1" | "ledger-workflow-outcome-correction/2"))
            {
                financial.work_claims.push(reference(row.reference, s)?);
            }
        }
        let id = string(locator.root_decision_id(), s)?;
        prefix.original_input("local-original-decision", &[id.value()], &inputs, s)?;
        if !(owner.program().uses_component_lifecycle_recipe()
            && matches!(schema, "ledger-workflow-link-assert/1" | "ledger-workflow-fact-attest/1"))
        {
            prefix.original_input("local-original-capacity", &[id.value()], &inputs, s)?;
        }
    }
    if owner.program().uses_component_lifecycle_recipe() {
        if let Some(locators) = &locators {
            for report in locators.selector_work_heads() {
                let claim = string(report.work_claim_id(), s)?;
                require(
                    prefix
                        .original_input("local-original-work-claim", &[claim.value()], &inputs, s)?
                        .is_some(),
                    "INTEGRITY_FAILURE",
                )?;
                let id = string(report.root_decision_id(), s)?;
                require(
                    prefix
                        .original_input("local-original-decision", &[id.value()], &inputs, s)?
                        .is_some(),
                    "INTEGRITY_FAILURE",
                )?;
            }
        }
    }
    // Match the forward acquisition owner: Stage operations capture their
    // Chain, while Invocation close captures only its Invocation and Work claim.
    let chain = stage_locator.as_ref().map(|l| l.chain());
    if let Some(chain) = chain {
        let chain = string(chain, s)?;
        if let Some(row) = prefix.original_input(
            "local-original-chain",
            &[
                request_wrapper::field(binding.value(), "customer", s)?,
                chain.value(),
            ],
            &inputs,
            s,
        )? {
            financial.chain_head = Some(reference(row.reference, s)?);
        }
    }
    let mut retained_assertion = None;
    if let Some(locators) = &locators {
        for claim in locators.claim_keys() {
            let key = prefix::local_claim_key(claim, s)?;
            if let Some(body) = prefix._inventory.head_body(&key, s)? {
                // The constructor must receive that exact earlier claim, not N.
                let matched = inputs
                    .iter()
                    .find(|row| row.canonical_body == body.bytes())
                    .ok_or(ServiceError::IntegrityFailure)?;
                match claim {
                    core::PermanentClaimKey::Assertion { .. } => {
                        retained_assertion = Some(reference(matched.reference, s)?)
                    }
                    core::PermanentClaimKey::Work { .. } => {
                        financial.work_claims.push(reference(matched.reference, s)?)
                    }
                    core::PermanentClaimKey::Outcome { .. } => financial
                        .outcome_claims
                        .push(reference(matched.reference, s)?),
                    _ => return Err(reject("UNSUPPORTED_COMPOSITION")),
                }
            }
        }
    }
    if owner.program().uses_component_lifecycle_recipe()
        && matches!(
            schema,
            "ledger-workflow-work-reverse/2" | "ledger-workflow-quantity-correction/2"
        )
    {
        let selected = locators.as_mut().ok_or(ServiceError::IntegrityFailure)?;
        selected
            .retain_current_root_predecessor(
                owner.program(),
                &ck,
                (
                    &financial.decision_heads,
                    &capacities,
                    &financial.stage_heads,
                ),
                &inputs,
                s,
            )
            .map_err(core_error)?;
        if let Some(prior) = selected.current_root_predecessor_ref() {
            let mut found = false;
            for input in &inputs {
                found |= same_ref(input.reference, prior, s)?;
            }
            require(found, "INTEGRITY_FAILURE")?;
        }
        selected
            .retain_current_family_outcome_links(
                owner.program(),
                &ck,
                &financial.family_heads,
                &inputs,
                s,
            )
            .map_err(core_error)?;
    }
    // As in fresh nonWork capture, actual held original heads refine the
    // required family roles before any secondary source group is selected.
    // Match fresh with_capture: only actual local locator roles enable
    // secondary source groups. Plain Work captures its initiating source.
    let secondary_roles = owner.program().uses_work_lifecycle_recipe()
        && locators
            .as_ref()
            .is_some_and(|l| !l.required_authority_slots().is_empty());
    let source_value = request_wrapper::field(binding.value(), "source_slots", s)?;
    let source_metadata = s
        .canonical_representation(source_value, RepresentationKind::Record)
        .map_err(core_error)?;
    let originals_value = request_wrapper::field(binding.value(), "authority_refs", s)?;
    let original_metadata = s
        .canonical_representation(originals_value, RepresentationKind::Record)
        .map_err(core_error)?;
    let primary_metadata = s
        .canonical_representation(snapshot, RepresentationKind::Authority)
        .map_err(core_error)?;
    let source_bytes = source_metadata
        .bytes()
        .len()
        .checked_add(original_metadata.bytes().len())
        .and_then(|n| n.checked_add(primary_metadata.bytes().len()))
        .ok_or(ServiceError::Unavailable)?;
    let _frame_storage = s
        .reserve_external_payload(
            source_bytes
                .checked_mul(32)
                .ok_or(ServiceError::Unavailable)?
                + 32 * (mem::size_of::<core::AuthorityContextData>()
                    + mem::size_of::<current::source::ObservedSource>()
                    + 32 * mem::size_of::<core::ExactRef>()
                    + mem::size_of::<Vec<core::ExactRef>>()
                    + mem::size_of::<prefix::source::InitialSource>()
                    + mem::size_of::<&core::InitialSourceCorrespondence>()
                    + mem::size_of::<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>(
                    )),
        )
        .map_err(core_error)?;
    context
        .authorities
        .try_reserve_exact(31)
        .map_err(|_| ServiceError::Unavailable)?;
    let mut observations = Vec::new();
    observations
        .try_reserve_exact(32)
        .map_err(|_| ServiceError::Unavailable)?;
    let slots = request_wrapper::field(binding.value(), "source_slots", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    for slot in slots {
        let name = request_wrapper::text(slot, "name", s)?;
        let primary_slot = name == context.authorities[0].source_slot;
        let required = primary_slot
            || locators
                .as_ref()
                .is_some_and(|l| l.required_authority_slots().iter().any(|v| v == name));
        if !required {
            continue;
        }
        let mut groups: Vec<(current::source::ObservedSource, Vec<core::ExactRef>)> = Vec::new();
        groups
            .try_reserve_exact(32)
            .map_err(|_| ServiceError::Unavailable)?;
        for nominated in request_wrapper::field(binding.value(), "authority_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            if request_wrapper::text(nominated, "kind", s)? != "workflow-source-grant" {
                continue;
            }
            let (r, _lease) = current::source::from_value_local(nominated, s)?;
            let body = s
                .parse_representation(pool.body(&r)?, RepresentationKind::Record)
                .map_err(core_error)?;
            let source = request_wrapper::text(body.value(), "source", s)?;
            let eligible = request_wrapper::field(slot, "sources", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .iter()
                .any(|v| v.as_str() == Some(source));
            if !eligible || (!secondary_roles && source != key.source) {
                continue;
            }
            let observed = prefix.source_at(tx, key, nominated, sequence, s).await?;
            if observed.current_grant_ref().is_none() {
                continue;
            }
            let mut same = None;
            for (position, (old, _)) in groups.iter().enumerate() {
                if same_ref(old.enrollment_ref(), observed.enrollment_ref(), s)? {
                    require(
                        same_ref(old.state_ref(), observed.state_ref(), s)?
                            && same_ref(
                                old.current_grant_ref()
                                    .ok_or(ServiceError::IntegrityFailure)?,
                                observed
                                    .current_grant_ref()
                                    .ok_or(ServiceError::IntegrityFailure)?,
                                s,
                            )?,
                        "STALE_REVISION",
                    )?;
                    same = Some(position);
                }
            }
            if let Some(position) = same {
                groups[position]
                    .1
                    .push(reference(observed.original_grant_ref(), s)?);
            } else {
                require(groups.len() < 32, "UNSUPPORTED_COMPOSITION")?;
                let mut originals = Vec::new();
                originals
                    .try_reserve_exact(32)
                    .map_err(|_| ServiceError::Unavailable)?;
                originals.push(reference(observed.original_grant_ref(), s)?);
                groups.push((observed, originals));
            }
        }
        for (observed, originals) in groups {
            let source = request_wrapper::text(observed.original_grant_body(), "source", s)?;
            if primary_slot && source == key.source {
                let frame = &context.authorities[0];
                for (a, b) in [
                    (&frame.enrollment_ref, observed.enrollment_ref()),
                    (&frame.current_state_ref, observed.state_ref()),
                    (
                        &frame.current_grant_ref,
                        observed
                            .current_grant_ref()
                            .ok_or(ServiceError::IntegrityFailure)?,
                    ),
                ] {
                    require(same_ref(a, b, s)?, "INTEGRITY_FAILURE")?;
                }
                require(
                    frame.original_grant_refs.len() == originals.len(),
                    "INTEGRITY_FAILURE",
                )?;
                for (a, b) in frame.original_grant_refs.iter().zip(&originals) {
                    require(same_ref(a, b, s)?, "INTEGRITY_FAILURE")?;
                }
            } else {
                require(context.authorities.len() < 32, "UNSUPPORTED_COMPOSITION")?;
                context.authorities.push(core::AuthorityContextData {
                    source_slot: copy(name, s)?,
                    enrollment_ref: reference(observed.enrollment_ref(), s)?,
                    current_state_ref: reference(observed.state_ref(), s)?,
                    current_grant_ref: reference(
                        observed
                            .current_grant_ref()
                            .ok_or(ServiceError::IntegrityFailure)?,
                        s,
                    )?,
                    original_grant_refs: originals,
                    binding_ref: reference(&binding_ref, s)?,
                    accepted_agreement_ref: reference(&accepted_ref, s)?,
                    activation_ref: reference(activation.activation.reference(), s)?,
                    authority_observation_ref: None,
                });
            }
            observations.push(observed);
        }
    }
    let mut primary_present = false;
    for observed in &observations {
        if same_ref(
            observed.enrollment_ref(),
            &context.authorities[0].enrollment_ref,
            s,
        )? {
            primary_present = true;
        }
    }
    require(primary_present, "AUTHORITY_REQUIRED")?;

    let mut births = Vec::new();
    births
        .try_reserve_exact(context.authorities.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for (i, frame) in context.authorities.iter().enumerate() {
        let mut duplicate = false;
        for prior in &context.authorities[..i] {
            duplicate |= same_ref(&prior.enrollment_ref, &frame.enrollment_ref, s)?;
        }
        if !duplicate {
            births.push(
                prefix::source::initial_for_enrollment(tx, key, &frame.enrollment_ref, sequence, s)
                    .await?,
            );
        }
    }
    let source_correspondences: Vec<_> = births
        .iter()
        .map(|(birth, _)| birth.correspondence())
        .collect();

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
        retained_assertion: retained_assertion.as_ref(),
        financial: &financial,
        capacity_heads: &capacities,
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
            ("digest", wire::string(digest, s)?.bytes()),
            ("id", wire::string(id, s)?.bytes()),
            ("kind", b"\"workflow-operation\""),
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

    // A Component report reversal can regenerate its unchanged original evaluation.
    // It remains authenticated input, and is also an actual producer result.
    if owner.program().uses_component_lifecycle_recipe()
        && schema == "ledger-workflow-work-reverse/2"
        && locators.as_ref().and_then(|l| l.work_heads()).is_some_and(|heads|
            heads.family_keys().is_empty() && heads.app_decision_ids().is_empty())
    {
        let root = locators.as_ref().and_then(|l| l.work_heads()).ok_or(ServiceError::IntegrityFailure)?.root_decision_id();
        let head = financial.decision_heads.iter().find(|h| h.decision_id == root).ok_or(ServiceError::IntegrityFailure)?;
        let decision = s.parse_representation(pool.body(&head.reference)?, RepresentationKind::Record).map_err(core_error)?;
        let (evaluation, _lease) = current::source::from_value_local(request_wrapper::field(decision.value(), "evaluation_ref", s)?, s)?;
        for row in &inputs {
            if same_ref(row.reference, &evaluation, s)? {
                let receipt_results = request_wrapper::field(w.receipt(), "result_refs", s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
                let mut present = false;
                for value in receipt_results {
                    let (result, _lease) = current::source::from_value_local(value, s)?;
                    present |= same_ref(&result, &evaluation, s)?;
                }
                if present { introduced.push(*row); }
            }
        }
    }
    let saved = core::SavedFinancialInput {
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
        };
    let (candidate, path) = if capture_path && request_wrapper::text(w.operation(), "kind", s)? == "outcome.submit" {
        core::reconstruct_original_candidate_with_report_path(owner.program(), &saved, s).map_err(core_error)?
    } else {
        (core::reconstruct_original_candidate(owner.program(), &saved, s).map_err(core_error)?, None)
    };

    let evidence = core::produce_local_financial_request_evidence(
        owner.program(),
        w.original_request().raw(),
        &candidate,
        &inputs,
        &state,
        s,
    )
    .map_err(core_error)?;

    let received = Timestamp::parse(&context.received_at).map_err(core_error)?;
    let accepted = Timestamp::parse(&context.accepted_at).map_err(core_error)?;
    let journal = wire::frame_candidate(
        &candidate,
        key,
        request_wrapper::text(w.operation(), "kind", s)?,
        evidence.value(),
        &received,
        &accepted,
        sequence,
        s,
    )?;

    journal.compare_original(w, s)?;

    Ok((candidate, path))
}
