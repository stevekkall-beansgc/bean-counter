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
    _sources: Vec<current::source::ObservedSource>,
    _parties: acceptance::CurrentPlatformConsents,
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
        prefix.fold_initial_activation(
            origin,
            binding,
            &self.root,
            &self.authority,
            &self.records,
            s,
        )
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
    let result = async {
        let state_storage = s
            .reserve_external_payload(std::mem::size_of::<ReplayedInitialActivation>())
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(origin.wrappers().operation(), "kind", s)?,
            "agreement.activate",
            s,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
            "administrative",
            s,
        )?;
        let incoming =
            ingress::IncomingRequest::parse(origin.wrappers().original_request().raw(), s)?;
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
        // agreement.activate also names replacement payloads. Only the exact
        // initial recipe can complete here; revision zero and an absent head
        // alone do not establish a valid replacement target/history.
        let schema = request_wrapper::text(parsed.value(), "schema", s)?;
        if !acceptance::same_recipe_text(schema, "ledger-workflow-activation/1", s)? {
            return Err(ServiceError::Unavailable);
        }
        let shape = activation_verify::shape::select(incoming.kind(), schema, |a, b| {
            acceptance::same_recipe_text(a, b, s)
        })?;
        request_wrapper::same_text(shape.action, "activate", s)?;
        // This bounded bridge implements the first schedule entry. It never
        // fabricates an empty history for an existing head or target.
        let expected = request_wrapper::text(parsed.value(), "expected_revision", s)?;
        if !acceptance::same_recipe_text(expected, "0", s)? {
            return Err(ServiceError::Unavailable);
        }
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(
            prefix.activation_head(binding.value(), s)?.is_none(),
            "INTEGRITY_FAILURE",
        )?;
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
        binding_interval(parsed.value(), binding.value(), &at, s)?;
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
        let selected = original.current_activation_source(
            &sources,
            publication,
            binding.value(),
            origin.key(),
            (actor, &at),
            s,
        )?;
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        let source = sources
            .get(selected)
            .ok_or(ServiceError::IntegrityFailure)?;
        let parties = original
            .reconstruct_activation_platforms(
                tx,
                origin.key(),
                binding.value(),
                &at,
                origin.wrappers().sequence(),
                prefix,
                s,
            )
            .await?;
        let authority =
            original.activation_authority(source, &root, origin.key(), actor, &at, s)?;
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
            None,
            s,
        )?;
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
        let members = prefix.resolve_members(
            &[
                &root,
                &authority,
                &records.intent,
                &records.head,
                &records.activation,
            ],
            std::iter::once(original.accepted_reference()).chain(publication.required_refs()),
            s,
        )?;
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
