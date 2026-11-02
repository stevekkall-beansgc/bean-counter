//! Private initial local activation, retaining actual current source/party
//! checks and the held absent-head prestate through the existing atomic writer.
use super::*;
use crate::store::sqlite::workflow::financial::{AppActivationAppend, FinancialDocument};
use ledgerlab_core::validation_work::{OriginalAuditSession, RepresentationKind};
pub(super) async fn prepare_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    management: &ManagementObservation,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply> {
    let payload = req.payload(s)?;
    request_wrapper::same_text(
        request_wrapper::text(payload, "schema", s)?,
        "ledger-workflow-activation/1",
        s,
    )?;
    let observed = current::app::observe_activation(ledger, tx, req, payload, s).await?;
    require(
        observed
            .publication()
            .program()
            .uses_local_execution_recipe(),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let admission = observed
        .admission_records
        .as_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    let parsed = s
        .parse_representation(
            observed.normalized_payload.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    request_wrapper::same_text(
        request_wrapper::text(parsed.value(), "expected_revision", s)?,
        "0",
        s,
    )?;
    let generated = [
        &admission.root,
        &admission.authority,
        &observed.records.intent,
        &observed.records.head,
        &observed.records.activation,
    ];
    let original = &observed.binding.original;
    original
        .publication
        .program()
        .require_local_admission_with_session(s)
        .map_err(core_error)?;
    let binding = s
        .parse_representation(
            original.acceptance.binding.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    original.acceptance.require_local_admission_sources(
        &observed.binding.sources,
        &original.publication,
        binding.value(),
        &observed.binding.at,
        s,
    )?;
    let selected = original
        .acceptance
        .work_locator_records()
        .map(|r| r.reference)
        .chain(original.publication.required_refs())
        .chain(
            observed
                .binding
                .sources
                .iter()
                .flat_map(current::source::ObservedSource::record_refs),
        )
        .chain(
            observed
                .binding
                .parties
                .iter()
                .flat_map(|p| p.selected_refs()),
        );
    let mut members = prefix::ResolvedMembers::local_selected(
        tx,
        req.key(),
        observed.before(),
        generated.into_iter(),
        selected,
        s,
    )
    .await?;
    let authority = s
        .parse_representation(admission.authority.bytes(), RepresentationKind::Authority)
        .map_err(core_error)?;
    let fields: &[&str] = if original.publication.program().uses_stage_lifecycle_recipe() {
        &[
            "app_consent_verification_refs",
            "supplier_consent_verification_refs",
        ]
    } else {
        &["app_consent_verification_refs"]
    };
    for field in fields {
        for r in request_wrapper::field(authority.value(), field, s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            let (r, _owner) = current::source::from_value(r, "workflow-verification", s)?;
            prefix::verify_selected_association(tx, req.key(), &r, observed.before(), s).await?;
            members.add_record_ref(&r, s)?;
        }
    }
    let evidence = evidence::activation_material(
        req,
        observed.normalized_payload.bytes(),
        observed.publication(),
        s,
    )?;
    let journal = wire::frame_activation(
        req,
        observed.normalized_payload.bytes(),
        observed.authority()?,
        observed.at(),
        observed.before(),
        observed.result_refs(),
        &evidence,
        members,
        s,
    )?;
    let reply = WorkflowReply::agreement(&journal, &evidence, s)?;
    let _storage = s
        .reserve_external_payload(
            5 * std::mem::size_of::<FinancialDocument>()
                + journal.members().len()
                    * std::mem::size_of::<
                        &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
                    >(),
        )
        .map_err(core_error)?;
    let documents: Vec<_> = generated
        .iter()
        .map(|r| FinancialDocument {
            reference: r.reference(),
            schema: r.schema(),
            canonical_bytes: r.bytes(),
        })
        .collect();
    let mut refs = Vec::with_capacity(journal.members().len());
    for i in 0..journal.members().len() {
        let r = journal.members().at(i, s)?;
        if r != journal.operation_reference() {
            refs.push(r);
        }
    }
    let journal_view = journal.storage_view(
        req,
        &evidence,
        &refs,
        observed.before(),
        observed.at().micros(),
        s,
    )?;

    let activation = s
        .parse_representation(
            observed.records.activation.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    let effective = Timestamp::parse_with_work(
        request_wrapper::text(activation.value(), "effective_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let append = AppActivationAppend {
        logical_store_id: &req.key().logical_store_id,
        scope: &req.key().scope,
        source: &req.key().source,
        operation_id: &req.key().operation_id,
        documents: &documents,
        journal: journal_view,
        customer: request_wrapper::text(binding.value(), "customer", s)?,
        agreement: request_wrapper::text(binding.value(), "agreement_id", s)?,
        expected_revision: 0,
        revision: 1,
        head: observed.records.head.reference(),
        activation: observed.records.activation.reference(),
        binding: original.acceptance.binding.reference(),
        intent: observed.records.intent.reference(),
        effective_at_us: effective.micros(),
        prior_head: None,
        target: None,
        action: "activate",
    };
    management.recheck(ledger, tx, req.key(), s).await?;
    tx.workflow_app_activation_append(&append, s)
        .await
        .map_err(prefix::read_error)?;
    Ok(reply)
}

pub(super) async fn prepare_later_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    management: &ManagementObservation,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply> {
    let payload = req.payload(s)?;
    let observed = current::app::observe_activation(ledger, tx, req, payload, s).await?;
    require(
        observed
            .publication()
            .program()
            .uses_local_execution_recipe(),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let parsed = s
        .parse_representation(
            observed.normalized_payload.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    let shape = activation_verify::shape::select(
        req.kind(),
        request_wrapper::text(parsed.value(), "schema", s)?,
        |a, b| acceptance::same_recipe_text(a, b, s),
    )?;
    let expected = acceptance::counter(
        request_wrapper::text(parsed.value(), "expected_revision", s)?,
        s,
    )?;
    for bound in [0u64, i64::MAX as u64] {
        s.semantic_work()
            .arithmetic(
                ledgerlab_core::validation_work::ArithmeticOperation::Compare,
                &[&expected.into(), &bound.into()],
            )
            .map_err(core_error)?;
    }
    require(expected > 0 && expected < i64::MAX as u64, "INVALID_SCALAR")?;
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Add,
            &[&expected.into(), &1u64.into()],
        )
        .map_err(core_error)?;
    let revision = expected
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let admission = if shape.action == "revoke" {
        observed.stop_records.as_ref()
    } else {
        observed.admission_records.as_ref()
    }
    .ok_or(ServiceError::IntegrityFailure)?;
    let generated = [
        &admission.root,
        &admission.authority,
        &observed.records.intent,
        &observed.records.head,
        &observed.records.activation,
    ];
    let original = &observed.binding.original;
    if shape.action != "revoke" {
        original
            .publication
            .program()
            .require_local_admission_with_session(s)
            .map_err(core_error)?;
    }
    let binding = s
        .parse_representation(
            original.acceptance.binding.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    if shape.action != "revoke" {
        original.acceptance.require_local_admission_sources(
            &observed.binding.sources,
            &original.publication,
            binding.value(),
            &observed.binding.at,
            s,
        )?;
    }
    // Full chronological acceptance owner, extended to the held writer cutoff.
    let mut completed = prefix::acceptance_for_original(
        ledger,
        tx,
        req.key(),
        request_wrapper::field(parsed.value(), "accepted_binding_ref", s)?,
        observed.before(),
        s,
    )
    .await?;
    let setup = ledger.admin.read(&ledger.store, tx, req.key()).await?;
    completed
        .current_sources(
            tx,
            req.key(),
            (ledger, &setup),
            &observed.binding.sources,
            observed.before(),
            s,
        )
        .await?;
    let schedule =
        completed.completed_schedule(req.key(), binding.value(), observed.before(), expected, s)?;
    if shape.action == "revoke" {
        completed
            .completed_stop_evidence(
                tx,
                req.key(),
                parsed.value(),
                binding.value(),
                &setup.operator,
                observed.before(),
                s,
            )
            .await?;
        schedule.require_stop_target(parsed.value(), original.acceptance.binding.reference(), s)?;
    }
    let (head_doc, _h) = prefix::schedule_document(
        observed.records.head.reference(),
        observed.records.head.schema(),
        observed.records.head.bytes(),
        s,
    )?;
    let (activation_doc, _a) = prefix::schedule_document(
        observed.records.activation.reference(),
        observed.records.activation.schema(),
        observed.records.activation.bytes(),
        s,
    )?;
    let (binding_doc, _b) = prefix::schedule_document(
        original.acceptance.binding.reference(),
        original.acceptance.binding.schema(),
        original.acceptance.binding.bytes(),
        s,
    )?;
    let (next, _next_refs) = prefix::paid_transition(
        req.key(),
        request_wrapper::text(binding.value(), "customer", s)?,
        request_wrapper::text(binding.value(), "agreement_id", s)?,
        head_doc,
        activation_doc,
        binding_doc,
        s,
    )?;
    activation_verify::schedule::check_next_with_session(&schedule.transitions, &next, s)?;
    let prior = schedule
        .transitions
        .last()
        .ok_or(ServiceError::IntegrityFailure)?;
    let kind = wire::string(&prior.head.reference.kind, s)?;
    let id = wire::string(&prior.head.reference.id, s)?;
    let digest = wire::string(&prior.head.reference.digest, s)?;
    let encoded = wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        262144,
        s,
    )?;
    let prior_json = s
        .parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let (prior_ref, _prior_lease) =
        current::source::from_value(prior_json.value(), "workflow-head-transition", s)?;
    let target_name = match shape.action {
        "replace" => Some("replaces_transition_ref"),
        "revoke" => Some("transition_ref"),
        _ => None,
    };
    let target = target_name
        .map(|name| {
            current::source::from_value(
                request_wrapper::field(parsed.value(), name, s)?,
                "workflow-activation",
                s,
            )
        })
        .transpose()?;

    let selected = original
        .acceptance
        .work_locator_records()
        .map(|r| r.reference)
        .chain(original.publication.required_refs())
        .chain(
            observed
                .binding
                .sources
                .iter()
                .flat_map(current::source::ObservedSource::record_refs),
        )
        .chain(
            observed
                .binding
                .parties
                .iter()
                .flat_map(|p| p.selected_refs()),
        );
    let mut members = prefix::ResolvedMembers::local_selected(
        tx,
        req.key(),
        observed.before(),
        generated.into_iter(),
        selected,
        s,
    )
    .await?;
    prefix::verify_selected_association(tx, req.key(), &prior_ref, observed.before(), s).await?;
    members.add_record_ref(&prior_ref, s)?;
    if let Some((reference, _)) = &target {
        prefix::verify_selected_association(tx, req.key(), reference, observed.before(), s).await?;
        members.add_record_ref(reference, s)?;
    }
    if shape.action == "revoke" {
        for v in request_wrapper::field(parsed.value(), "evidence_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            let (reference, _lease) = current::source::from_value(v, "workflow-evidence", s)?;
            prefix::verify_selected_association(tx, req.key(), &reference, observed.before(), s)
                .await?;
            members.add_record_ref(&reference, s)?;
        }
    }
    let authority = s
        .parse_representation(admission.authority.bytes(), RepresentationKind::Authority)
        .map_err(core_error)?;
    let fields: &[&str] = if shape.action == "revoke" {
        &[]
    } else if original.publication.program().uses_stage_lifecycle_recipe() {
        &[
            "app_consent_verification_refs",
            "supplier_consent_verification_refs",
        ]
    } else {
        &["app_consent_verification_refs"]
    };
    for field in fields {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        for r in request_wrapper::field(authority.value(), field, s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            let (r, _owner) = current::source::from_value(r, "workflow-verification", s)?;
            prefix::verify_selected_association(tx, req.key(), &r, observed.before(), s).await?;
            members.add_record_ref(&r, s)?;
        }
    }
    let evidence = evidence::activation_material(
        req,
        observed.normalized_payload.bytes(),
        observed.publication(),
        s,
    )?;
    let journal = wire::frame_activation(
        req,
        observed.normalized_payload.bytes(),
        &admission.authority,
        observed.at(),
        observed.before(),
        observed.result_refs(),
        &evidence,
        members,
        s,
    )?;
    let reply = WorkflowReply::agreement(&journal, &evidence, s)?;
    let count = journal.members().len();
    let stride =
        std::mem::size_of::<&ledgerlab_core::policy::workflow::transition_constructor::ExactRef>();
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Multiply,
            &[&count.into(), &stride.into()],
        )
        .map_err(core_error)?;
    let refs_bytes = count.checked_mul(stride).ok_or(ServiceError::Unavailable)?;
    let document_bytes = 5 * std::mem::size_of::<FinancialDocument>();
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Add,
            &[&refs_bytes.into(), &document_bytes.into()],
        )
        .map_err(core_error)?;
    let bytes = refs_bytes
        .checked_add(document_bytes)
        .ok_or(ServiceError::Unavailable)?;
    let _storage = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut documents = Vec::new();
    documents
        .try_reserve_exact(5)
        .map_err(|_| ServiceError::Unavailable)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    for (actual, paid) in [(documents.capacity(), 5), (refs.capacity(), count)] {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(
                ledgerlab_core::validation_work::ArithmeticOperation::Compare,
                &[&actual.into(), &paid.into()],
            )
            .map_err(core_error)?;
        require(actual <= paid, "PROGRAM_BUDGET_EXCEEDED")?;
    }
    for r in generated {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        documents.push(FinancialDocument {
            reference: r.reference(),
            schema: r.schema(),
            canonical_bytes: r.bytes(),
        });
    }
    for i in 0..count {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let r = journal.members().at(i, s)?;
        if !local_original_work::same_ref(r, journal.operation_reference(), s)? {
            refs.push(r);
        }
    }
    let journal_view = journal.storage_view(
        req,
        &evidence,
        &refs,
        observed.before(),
        observed.at().micros(),
        s,
    )?;

    let activation = s
        .parse_representation(
            observed.records.activation.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    let effective = Timestamp::parse_with_work(
        request_wrapper::text(activation.value(), "effective_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let append = AppActivationAppend {
        logical_store_id: &req.key().logical_store_id,
        scope: &req.key().scope,
        source: &req.key().source,
        operation_id: &req.key().operation_id,
        documents: &documents,
        journal: journal_view,
        customer: request_wrapper::text(binding.value(), "customer", s)?,
        agreement: request_wrapper::text(binding.value(), "agreement_id", s)?,
        expected_revision: expected as i64,
        revision: revision as i64,
        head: observed.records.head.reference(),
        activation: observed.records.activation.reference(),
        binding: original.acceptance.binding.reference(),
        intent: observed.records.intent.reference(),
        effective_at_us: effective.micros(),
        prior_head: Some(&prior_ref),
        target: target.as_ref().map(|(r, _)| r),
        action: shape.action,
    };
    management.recheck(ledger, tx, req.key(), s).await?;
    tx.workflow_app_activation_append(&append, s)
        .await
        .map_err(prefix::read_error)?;
    Ok(reply)
}

/// Existing R1 management authority snapshot; this is framed DATA under the
/// held real-local management owner, not a new stop authorization.
pub(super) fn stop_retention_authority(
    setup: &crate::service::billing::Setup,
    root: &wire::SourceRecord,
    at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
    let actor = wire::string(&setup.operator, s)?;
    let source = wire::string(&setup.source, s)?;
    let refs = wire::RefSet::new(std::iter::once(root.reference()), 1, true, s)?.encode(s)?;
    let at = wire::string(at.as_str(), s)?;
    wire::object(
        &[
            ("authority_refs", refs.bytes()),
            ("principal_id", actor.bytes()),
            ("rights", br#"["admin"]"#),
            ("schema", br#""ledger-workflow-publication-authority/1""#),
            ("source", source.bytes()),
            ("validated_at", at.bytes()),
        ],
        262144,
        s,
    )
}

/// Exact local initial-activation originals reuse the completed acceptance/PUB
/// owner and its chronological activation replay, including original journal
/// equality. Selected activation rows alone are never a certificate.
pub(super) async fn verify_original(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    incoming: Option<&ingress::IncomingRequest>,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    let original = origin.wrappers();
    if original.class() != wrappers::WireClass::Agreement
        || !matches!(
            request_wrapper::text(original.operation(), "kind", s)?,
            "agreement.activate" | "agreement.revoke"
        )
    {
        return Ok(false);
    }
    let evidence = request_wrapper::field(original.operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    if !wrappers::is_local_registry(request_wrapper::text(resolution, "registry_digest", s)?, s)? {
        return Ok(false);
    }
    let saved = ingress::IncomingRequest::parse(original.original_request().raw(), s)?;
    let payload = saved.payload(s)?;
    if !matches!(
        request_wrapper::text(payload, "schema", s)?,
        "ledger-workflow-activation/1"
            | "ledger-workflow-activation-replace/1"
            | "ledger-workflow-agreement-revoke/1"
    ) {
        return Ok(false);
    }
    let mut completed = prefix::acceptance_for_original(
        ledger,
        tx,
        origin.key(),
        request_wrapper::field(payload, "accepted_binding_ref", s)?,
        original.sequence(),
        s,
    )
    .await?;
    let setup = ledger.admin.read(&ledger.store, tx, origin.key()).await?;
    s.semantic_work()
        .arithmetic(
            ledgerlab_core::validation_work::ArithmeticOperation::Add,
            &[&original.sequence().into(), &1i64.into()],
        )
        .map_err(core_error)?;
    let before = original
        .sequence()
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    completed
        .current_sources(tx, origin.key(), (ledger, &setup), &[], before, s)
        .await?;
    completed.require_original_sequence(original.sequence(), s)?;
    if let Some(incoming) = incoming {
        request_wrapper::key(
            request_wrapper::field(incoming.value(), "key", s)?,
            origin.key(),
            s,
        )?;
        require(incoming.kind() == saved.kind(), "IDENTITY_CONFLICT")?;
        require(
            super::acceptance::same_recipe_text(
                request_wrapper::text(incoming.payload(s)?, "schema", s)?,
                request_wrapper::text(payload, "schema", s)?,
                s,
            )?,
            "IDENTITY_CONFLICT",
        )?;
        let binding = s
            .parse_representation(
                completed.acceptance.binding.bytes(),
                RepresentationKind::Record,
            )
            .map_err(core_error)?;
        let actual = super::activation::normalize_original_retry(
            incoming.kind(),
            incoming.payload(s)?,
            completed.acceptance.binding.reference(),
            binding.value(),
            s,
        )?;
        let normalized = request_wrapper::field(evidence, "normalized_request", s)?;
        let expected_payload = request_wrapper::field(normalized, "payload", s)?;
        let expected = s
            .canonical_representation(expected_payload, RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(expected.bytes().len())
            .map_err(core_error)?;
        require(actual.bytes() == expected.bytes(), "IDENTITY_CONFLICT")?;
    }
    s.check_cancelled().map_err(core_error)?;
    Ok(true)
}

/// Routing DATA only. The selected writer independently reloads and validates
/// the exact original agreement before preparation; no decision comes from this read.
pub(super) async fn selected(
    ledger: &LocalWorkflowLedger,
    raw: &[u8],
    snapshot: &crate::service::workflow::resources::Snapshot,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    use crate::store::ports::AcceptanceTx;
    let req = super::super::request::Request::parse_agreement(raw)?;
    if !matches!(req.kind(), "agreement.activate" | "agreement.revoke") {
        return Ok(false);
    }
    let Some(r) = req.payload().get("accepted_binding_ref") else {
        return Ok(false);
    };
    let mut tx = snapshot.begin(&ledger.store, s).await?;
    let result = async {
        ledger.admin.read(&ledger.store, &mut tx, req.key()).await?;
        let before = tx
            .workflow_sequence()
            .await
            .map_err(crate::service::store_error)?;
        let identity = req.key().bytes()?;
        if tx
            .workflow_original_identity_exists(&identity, s)
            .await
            .map_err(prefix::read_error)?
        {
            // This is routing DATA, not an original certificate. The exact
            // wrapper read still needs the same held transaction's proof scope;
            // owning dispatch independently completes the saved original below.
            return proof_session::with_proofs(&mut tx, req.key(), |tx| {
                Box::pin(async {
                    let owned = s
                        .copy_representation(&identity, RepresentationKind::Record)
                        .map_err(core_error)?;
                    let old = prefix::target(tx, req.key(), owned, s)
                        .await?
                        .checked_wrappers(tx, s)
                        .await?;
                    let e =
                        request_wrapper::field(old.wrappers().operation(), "request_evidence", s)?;
                    let d = request_wrapper::field(e, "descriptor_resolution", s)?;
                    wrappers::is_local_registry(request_wrapper::text(d, "registry_digest", s)?, s)
                })
            })
            .await;
        }
        let (reference, _lease) = current::source::from_value(r, "workflow-binding", s)?;
        let stored = tx
            .workflow_financial_document(
                &reference,
                &req.key().logical_store_id,
                &req.key().scope,
                before,
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        if stored.schema.bytes() != b"ledger-workflow-binding/2" {
            return Ok(false);
        }
        let binding = current::source::retained_record(
            &mut tx,
            req.key(),
            r,
            "workflow-binding",
            "ledger-workflow-binding/2",
            before,
            s,
        )
        .await?;
        let policy_ref = request_wrapper::field(binding.body(), "policy_ref", s)?;
        let policy = current::source::retained_record(
            &mut tx,
            req.key(),
            policy_ref,
            "workflow-policy",
            "ledger-workflow-policy/4",
            before,
            s,
        )
        .await?;
        let actual = request_wrapper::field(policy.body(), "compiler_ref", s)?;
        match publication_prepare::local_support(actual, s) {
            Ok(_support) => Ok(true),
            Err(ServiceError::Rejection(code)) if code == "UNSUPPORTED_SCHEMA_PROFILE" => Ok(false),
            Err(error) => Err(error),
        }
    }
    .await;
    tx.rollback().await.map_err(crate::service::store_error)?;
    result
}
