//! Initial activation replay against the SAME already completed acceptance/PUB
//! owner. This is one chronological recipe, not a current-time replay or an
//! all-history accepted-program cache. Later schedule transitions remain gated.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease, validation_work::ArithmeticOperation as Op,
};

pub(in super::super) struct ReplayedInitialActivation {
    root: wire::SourceRecord,
    authority: wire::SourceRecord,
    records: ActivationRecords,
    later: bool,
    _sources: Vec<current::source::ObservedSource>,
    _parties: Option<acceptance::CurrentPlatformConsents>,
    _source_storage: ExternalBufferLease,
    _time_storage: ExternalBufferLease,
    _state_storage: ExternalBufferLease,
}
impl ReplayedInitialActivation {
    pub(in super::super) fn fold(
        &self,
        origin: &prefix::LocatedWrappers,
        prefix: &mut prefix::FoldedInventory,
        binding: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        if self.later {
            prefix.fold_later_activation(origin, binding, &self.root, &self.authority, &self.records, s)
        } else {
            prefix.fold_initial_activation(origin, binding, &self.root, &self.authority, &self.records, s)
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(in super::super) async fn reconstruct_initial(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    original: &acceptance::ReplayedAcceptance,
    publication: &prefix::RecompiledPublication,
    prefix: &mut prefix::FoldedInventory,
    setup: &crate::service::billing::Setup,
    s: &mut OriginalAuditSession,
) -> Result<ReplayedInitialActivation> {
    reconstruct_selected(tx, origin, original, publication, prefix, setup, false, s).await
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) async fn reconstruct_later(
    tx: &mut SqliteTx, origin: &prefix::LocatedWrappers, original: &acceptance::ReplayedAcceptance,
    publication: &prefix::RecompiledPublication, prefix: &mut prefix::FoldedInventory,
    setup: &crate::service::billing::Setup, s: &mut OriginalAuditSession,
) -> Result<ReplayedInitialActivation> {
    reconstruct_selected(tx, origin, original, publication, prefix, setup, true, s).await
}

#[allow(clippy::too_many_arguments)]
async fn reconstruct_selected(
    tx: &mut SqliteTx, origin: &prefix::LocatedWrappers, original: &acceptance::ReplayedAcceptance,
    publication: &prefix::RecompiledPublication, prefix: &mut prefix::FoldedInventory,
    setup: &crate::service::billing::Setup, later: bool, s: &mut OriginalAuditSession,
) -> Result<ReplayedInitialActivation> {
    let result = async {
        let state_storage = s
            .reserve_external_payload(std::mem::size_of::<ReplayedInitialActivation>())
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
            "administrative",
            s,
        )?;
        let incoming =
            ingress::IncomingRequest::parse(origin.wrappers().original_request().raw(), s)?;
        request_wrapper::same_text(
            request_wrapper::text(origin.wrappers().operation(), "kind", s)?,
            incoming.kind(),
            s,
        )?;
        request_wrapper::key(
            request_wrapper::field(incoming.value(), "key", s)?,
            origin.key(),
            s,
        )?;
        let binding = s
            .parse_representation(
                original.binding.bytes(),
                RepresentationKind::SourceDefinition,
            )
            .map_err(core_error)?;
        let literal = incoming.payload(s)?;
        let supplied = request_wrapper::field(literal, "accepted_binding_ref", s)?;
        acceptance::validate_reference(supplied, "workflow-binding", s)?;
        for (field, expected) in [
            ("kind", original.binding.reference().kind.as_str()),
            ("id", original.binding.reference().id.as_str()),
            ("digest", original.binding.reference().digest.as_str()),
        ] {
            if !acceptance::same_recipe_text(
                request_wrapper::text(supplied, field, s)?,
                expected,
                s,
            )? {
                // This caller retains precisely its selected acceptance owner.
                // An older/different binding requires its own completed owner.
                return Err(ServiceError::Unavailable);
            }
        }
        let payload = normalize(
            incoming.kind(),
            incoming.payload(s)?,
            original.binding.reference(),
            binding.value(),
            s,
        )?;
        let parsed = s
            .parse_representation(payload.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let schema = request_wrapper::text(parsed.value(), "schema", s)?;
        let shape = activation_verify::shape::select(incoming.kind(), schema, |a, b| {
            acceptance::same_recipe_text(a, b, s)
        })?;
        let expected_text = request_wrapper::text(parsed.value(), "expected_revision", s)?;
        let expected = super::acceptance::counter(expected_text, s)?;
        let (schedule, prior) = if later {
            require(expected > 0, "INTEGRITY_FAILURE")?;
            require(matches!(shape.action, "activate" | "replace" | "revoke"), "UNSUPPORTED_SCHEMA_PROFILE")?;
            // The count is taken from the completed owning head, not trusted
            // from the request as an allocation or history-completeness claim.
            let head = prefix.activation_head(binding.value(), s)?.ok_or(ServiceError::IntegrityFailure)?;
            let head = s.parse_representation(head.bytes(), RepresentationKind::Record).map_err(core_error)?;
            let actual_revision = super::acceptance::counter(
                request_wrapper::text(head.value(), "new_revision", s)?, s,
            )?;
            s.semantic_work().arithmetic(Op::Compare, &[&expected.into(), &actual_revision.into()]).map_err(core_error)?;
            require(expected == actual_revision, "INTEGRITY_FAILURE")?;
            let schedule = prefix.activation_schedule(origin.key(), binding.value(), actual_revision, s)?;
            let last = schedule.transitions.last().ok_or(ServiceError::IntegrityFailure)?;
            let kind = wire::string(&last.head.reference.kind, s)?;
            let id = wire::string(&last.head.reference.id, s)?;
            let digest = wire::string(&last.head.reference.digest, s)?;
            let encoded = wire::object(&[("digest",digest.bytes()),("id",id.bytes()),("kind",kind.bytes())], 262144, s)?;
            let prior = s.parse_representation(encoded.bytes(), RepresentationKind::Record).map_err(core_error)?;
            (Some(schedule), Some(prior))
        } else {
            if !acceptance::same_recipe_text(schema, "ledger-workflow-activation/1", s)?
                || !acceptance::same_recipe_text(expected_text, "0", s)? { return Err(ServiceError::Unavailable); }
            request_wrapper::same_text(shape.action, "activate", s)?;
            s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
            require(prefix.activation_head(binding.value(), s)?.is_none(), "INTEGRITY_FAILURE")?;
            (None, None)
        };
        let asserted =
            request_wrapper::field(origin.wrappers().operation(), "authority_snapshot", s)?;
        let actor = request_wrapper::text(asserted, "actor", s)?;
        let time_storage = s
            .reserve_external_payload(4 * 64 + 27)
            .map_err(core_error)?;
        let at = Timestamp::parse_with_work(
            request_wrapper::text(origin.wrappers().operation(), "accepted_at", s)?,
            s.semantic_work(),
        )
        .map_err(core_error)?;
        if shape.action != "revoke" { binding_interval(parsed.value(), binding.value(), &at, s)?; }
        let root_ref = request_wrapper::field(asserted, "local_admin_ref", s)?;
        origin.require_member(root_ref, s)?;
        s.semantic_work()
            .arithmetic(
                Op::Add,
                &[&origin.wrappers().sequence().into(), &1i64.into()],
            )
            .map_err(core_error)?;
        let own_before = origin
            .wrappers()
            .sequence()
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let retained = current::source::retained_record(
            tx,
            origin.key(),
            root_ref,
            "workflow-authority",
            "ledger-workflow-local-authority-snapshot/1",
            own_before,
            s,
        )
        .await?;
        current::party::produce::validate_original_root(
            retained.body(),
            origin.key(),
            actor,
            &at,
            s,
        )?;
        prefix::source::custody(retained.body(), setup, s)?;
        let raw = s
            .copy_representation(retained.canonical_body(), RepresentationKind::Authority)
            .map_err(core_error)?;
        let root = wire::source_record(
            "workflow-authority",
            "ledger-workflow-local-authority-snapshot/1",
            raw,
            s,
        )?;
        let root_actual = wire::reference(root.reference(), s)?;
        let root_wanted = encode(root_ref, s)?;
        same(root_actual.bytes(), root_wanted.bytes(), s)?;
        let refs = request_wrapper::field(binding.value(), "authority_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&refs.len().into(), &1usize.into()])
            .map_err(core_error)?;
        require(!refs.is_empty(), "INTEGRITY_FAILURE")?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&refs.len().into(), &32usize.into()])
            .map_err(core_error)?;
        require(refs.len() <= 32, "INTEGRITY_FAILURE")?;
        s.semantic_work()
            .arithmetic(
                Op::Multiply,
                &[
                    &refs.len().into(),
                    &std::mem::size_of::<current::source::ObservedSource>().into(),
                ],
            )
            .map_err(core_error)?;
        let bytes = refs
            .len()
            .checked_mul(std::mem::size_of::<current::source::ObservedSource>())
            .ok_or(ServiceError::Unavailable)?;
        let source_storage = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        for reference in refs {
            if request_wrapper::text(reference, "kind", s)? == "workflow-authority" {
                // Same original party-authorization input as the producer and
                // acceptance replay; this is not a source grant. Historical
                // platform mapping/consent checks still follow below.
                let mapping = current::source::retained_record(
                    tx,
                    origin.key(),
                    reference,
                    "workflow-authority",
                    "ledger-workflow-party-authorization/1",
                    origin.wrappers().sequence(),
                    s,
                )
                .await?;
                prefix::verify_selected_association(
                    tx,
                    origin.key(),
                    mapping.reference(),
                    origin.wrappers().sequence(),
                    s,
                )
                .await?;
                continue;
            }
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let observed = prefix
                .source_at(tx, origin.key(), reference, origin.wrappers().sequence(), s)
                .await?;
            s.semantic_work()
                .write_bytes(std::mem::size_of::<current::source::ObservedSource>())
                .map_err(core_error)?;
            sources.push(observed);
        }
        let selected = if shape.action == "revoke" {
            acceptance::select_stop(&sources, actor, origin.key(), s)?
        } else {
            original.current_activation_source(&sources, publication, binding.value(), origin.key(), (actor, &at), s)?
        };
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        let source = sources
            .get(selected)
            .ok_or(ServiceError::IntegrityFailure)?;
        let parties = if shape.action == "revoke" { None } else {
            Some(original.reconstruct_activation_platforms(tx, origin.key(), binding.value(), &at, origin.wrappers().sequence(), prefix, s).await?)
        };
        let authority = if shape.action == "revoke" {
            prefix.verify_stop_evidence(tx, origin.key(), parsed.value(), binding.value(), actor, origin.wrappers().sequence(), s).await?;
            schedule.as_ref().ok_or(ServiceError::IntegrityFailure)?.require_stop_target(parsed.value(), original.binding.reference(), s)?;
            stop_authority(source, &root, original.accepted_reference(), original.binding.reference(), binding.value(), parsed.value(), origin.key(), actor, &at, s)?
        } else { original.activation_authority(source, &root, origin.key(), actor, &at, s)? };
        let asserted = encode(asserted, s)?;
        same(authority.bytes(), asserted.bytes(), s)?;
        let records = build_records(
            incoming.kind(),
            parsed.value(),
            request_wrapper::field(incoming.value(), "key", s)?,
            original.binding.reference(),
            binding.value(),
            actor,
            &at,
            origin.wrappers().sequence(),
            prior.as_ref().map(|v| v.value()),
            s,
        )?;
        if let Some(schedule) = &schedule {
            let customer = request_wrapper::text(binding.value(), "customer", s)?;
            let agreement = request_wrapper::text(binding.value(), "agreement_id", s)?;
            let (head, _head_lease) = prefix::schedule_document(records.head.reference(), records.head.schema(), records.head.bytes(), s)?;
            let (activation, _activation_lease) = prefix::schedule_document(records.activation.reference(), records.activation.schema(), records.activation.bytes(), s)?;
            let (original_binding, _binding_lease) = prefix::schedule_document(original.binding.reference(), original.binding.schema(), original.binding.bytes(), s)?;
            let (transition, _refs) = prefix::paid_transition(origin.key(), customer, agreement, head, activation, original_binding, s)?;
            activation_verify::schedule::check_next_with_session(&schedule.transitions, &transition, s)?;
        }
        // N is comparison-only: each actual retained introduced body must
        // equal the same producer's row before the scratch fold can stage it.
        for record in [
            &authority,
            &records.intent,
            &records.head,
            &records.activation,
        ] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let reference = wire::reference(record.reference(), s)?;
            let reference = s
                .parse_representation(reference.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            origin.require_member(reference.value(), s)?;
            let retained = tx
                .workflow_financial_document(
                    record.reference(),
                    &origin.key().logical_store_id,
                    &origin.key().scope,
                    own_before,
                    s,
                )
                .await
                .map_err(prefix::read_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            same(retained.schema.bytes(), record.schema().as_bytes(), s)?;
            same(retained.body.bytes(), record.bytes(), s)?;
        }
        let evidence = evidence::activation_material(&incoming, payload.bytes(), publication, s)?;
        let _controls = s
            .reserve_external_payload(2 * std::mem::size_of::<[&wire::SourceRecord; 5]>())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<[&wire::SourceRecord; 5]>())
            .map_err(core_error)?;
        let generated = [
            &root,
            &authority,
            &records.intent,
            &records.head,
            &records.activation,
        ];
        let members = if publication.program().uses_local_execution_recipe() {
            // Historical LOCAL receipts keep their exact selected union. Fresh
            // NEW-promise admission is not reapplied during original recovery.
            let selected = original
                .work_locator_records()
                .map(|r| r.reference)
                .chain(publication.required_refs())
                .chain(
                    sources
                        .iter()
                        .flat_map(current::source::ObservedSource::record_refs),
                )
                .chain(parties.iter().flat_map(|p| p.selected_refs()));
            let mut members = prefix::ResolvedMembers::local_selected(
                tx,
                origin.key(),
                origin.wrappers().sequence(),
                generated.into_iter(),
                selected,
                s,
            )
            .await?;
            // These are actual prior producer operands, not caller assertions.
            // Their introduction is verified in the same scoped history before
            // adding them to the exact original operation member union.
            if let Some(prior) = &prior {
                let (reference, _owner) = current::source::from_value(
                    prior.value(), "workflow-head-transition", s,
                )?;
                prefix::verify_selected_association(
                    tx, origin.key(), &reference, origin.wrappers().sequence(), s,
                ).await?;
                members.add_record_ref(&reference, s)?;
            }
            if shape.action == "replace" || shape.action == "revoke" {
                let target_field = if shape.action == "revoke" { "transition_ref" } else { "replaces_transition_ref" };
                let target = request_wrapper::field(parsed.value(), target_field, s)?;
                let (reference, _owner) = current::source::from_value(
                    target, "workflow-activation", s,
                )?;
                prefix::verify_selected_association(
                    tx, origin.key(), &reference, origin.wrappers().sequence(), s,
                ).await?;
                members.add_record_ref(&reference, s)?;
            }
            if shape.action == "revoke" {
                for value in request_wrapper::field(parsed.value(), "evidence_refs", s)?.as_array().ok_or(ServiceError::IntegrityFailure)? {
                    let (reference, _owner) = current::source::from_value(value, "workflow-evidence", s)?;
                    prefix::verify_selected_association(tx, origin.key(), &reference, origin.wrappers().sequence(), s).await?;
                    members.add_record_ref(&reference, s)?;
                }
            }
            let parsed_authority = s
                .parse_representation(authority.bytes(), RepresentationKind::Authority)
                .map_err(core_error)?;
            let fields: &[&str] = if shape.action == "revoke" { &[] } else if publication.program().uses_stage_lifecycle_recipe() {
                &[
                    "app_consent_verification_refs",
                    "supplier_consent_verification_refs",
                ]
            } else {
                &["app_consent_verification_refs"]
            };
            for field in fields {
                for value in request_wrapper::field(parsed_authority.value(), field, s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    let (reference, _owner) =
                        current::source::from_value(value, "workflow-verification", s)?;
                    prefix::verify_selected_association(
                        tx,
                        origin.key(),
                        &reference,
                        origin.wrappers().sequence(),
                        s,
                    )
                    .await?;
                    members.add_record_ref(&reference, s)?;
                }
            }
            members
        } else {
            prefix.resolve_members(
                &generated,
                std::iter::once(original.accepted_reference()).chain(publication.required_refs()),
                s,
            )?
        };
        let journal = wire::frame_activation(
            &incoming,
            payload.bytes(),
            &authority,
            &at,
            origin.wrappers().sequence(),
            [
                records.intent.reference(),
                records.head.reference(),
                records.activation.reference(),
            ],
            &evidence,
            members,
            s,
        )?;
        journal.compare_original(origin, s)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<ReplayedInitialActivation>())
            .map_err(core_error)?;
        Ok(ReplayedInitialActivation {
            root,
            authority,
            records,
            later,
            _sources: sources,
            _parties: parties,
            _source_storage: source_storage,
            _time_storage: time_storage,
            _state_storage: state_storage,
        })
    }
    .await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
