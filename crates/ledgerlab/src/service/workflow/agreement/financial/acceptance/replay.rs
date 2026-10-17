//! Actual saved AppAccept input/authority/result material under an already
//! folded earlier prefix. Equality remains DATA until exact complete journal
//! and introduction/ProfileB obligations finish in the chronological driver.
use super::*;
pub(in super::super) struct ReplayedAcceptance {
    original: ReconstructedAcceptance,
    incoming: ingress::IncomingRequest,
    local_root: wire::SourceRecord,
    authority: wire::SourceRecord,
    private_documents: private::InlineDocuments,
    sources: Vec<current::source::ObservedSource>,
    consents: consent::Coverage,
    head_records: emit::AcceptanceHeadRecords,
    normalized: request::NormalizedAcceptance,
    _sources: ExternalBufferLease,
}
impl std::ops::Deref for ReplayedAcceptance {
    type Target = ReconstructedAcceptance;
    fn deref(&self) -> &Self::Target {
        &self.original
    }
}
pub(in super::super) async fn reconstruct(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    publication: &prefix::RecompiledPublication,
    prefix: &mut prefix::FoldedInventory,
    setup: &crate::service::billing::Setup,
    s: &mut OriginalAuditSession,
) -> Result<ReplayedAcceptance> {
    let result = async {
        let original = super::reconstruct(tx, origin, publication, s).await?;
        let incoming =
            ingress::IncomingRequest::parse(origin.wrappers().original_request().raw(), s)?;
        let input = &original.input;
        let operation = origin.wrappers().operation();
        let asserted = request_wrapper::field(operation, "authority_snapshot", s)?;
        let actor = request_wrapper::text(asserted, "actor", s)?;
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
        let root = current::source::retained_record(
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
            root.body(),
            origin.key(),
            actor,
            &original.at,
            s,
        )?;
        prefix::source::custody(root.body(), setup, s)?;
        let raw = s
            .copy_representation(root.canonical_body(), Kind::Authority)
            .map_err(core_error)?;
        let local_root = wire::source_record(
            "workflow-authority",
            "ledger-workflow-local-authority-snapshot/1",
            raw,
            s,
        )?;
        let private_documents = private::normalize_inline(&incoming, input, s)?;
        let private_documents = private::resolve_graph(
            private_documents,
            tx,
            origin.key(),
            input,
            origin.wrappers().sequence(),
            s,
        )
        .await?;
        private_documents.match_retained(prefix, s)?;
        private::associate_graph(&private_documents, input, s)?;
        let binding = s
            .parse_representation(original.binding.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        private::validate_slots(
            &private_documents,
            input,
            publication,
            binding.value(),
            &origin.key().scope,
            s,
        )?;
        let refs = input
            .authority_refs()
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        size(refs.len(), 1, 32, s)?;
        s.semantic_work()
            .arithmetic(
                Op::Multiply,
                &[
                    &refs.len().into(),
                    &mem::size_of::<current::source::ObservedSource>().into(),
                ],
            )
            .map_err(core_error)?;
        let bytes = refs
            .len()
            .checked_mul(mem::size_of::<current::source::ObservedSource>())
            .ok_or(ServiceError::Unavailable)?;
        let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        for r in refs {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let source = prefix
                .source_at(tx, origin.key(), r, origin.wrappers().sequence(), s)
                .await?;
            s.semantic_work()
                .write_bytes(mem::size_of::<current::source::ObservedSource>())
                .map_err(core_error)?;
            sources.push(source);
        }
        nominations::validate(
            &sources,
            &private_documents,
            input,
            publication,
            binding.value(),
            original.at.as_str(),
            s,
        )?;
        let index = admission::select(&sources, actor, origin.key(), s)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&index.into(), &sources.len().into()])
            .map_err(core_error)?;
        let source = sources.get(index).ok_or(ServiceError::IntegrityFailure)?;
        let consents = consent::reconstruct_all(
            tx,
            &incoming,
            input,
            publication,
            &private_documents,
            source,
            &local_root,
            actor,
            &original.at,
            origin.wrappers().sequence(),
            prefix,
            s,
        )
        .await?;
        consents.match_prefix(prefix, s)?;
        let authority = admission::authority(
            input,
            source,
            &consents,
            &local_root,
            origin.key(),
            actor,
            &original.at,
            s,
        )?;
        let asserted = encode(asserted, s)?;
        same_bytes(authority.bytes(), asserted.bytes(), s)?;
        let prior = prefix.acceptance_head(
            request_wrapper::field(binding.value(), "customer", s)?,
            request_wrapper::field(binding.value(), "agreement_id", s)?,
            s,
        )?;
        let head_records = emit::build(
            input,
            &original.binding,
            &authority,
            &incoming,
            actor,
            &original.at,
            origin.wrappers().sequence(),
            prior.as_ref(),
            s,
        )?;
        let normalized = request::build(
            &incoming,
            input,
            binding.value(),
            &private_documents,
            &consents,
            s,
        )?;
        let saved = request_wrapper::field(
            request_wrapper::field(operation, "request_evidence", s)?,
            "normalized_request",
            s,
        )?;
        let saved = encode(request_wrapper::field(saved, "payload", s)?, s)?;
        same_bytes(normalized.payload.bytes(), saved.bytes(), s)?;
        // Results are recomputed from literal original input and actual as-of
        // operands. Saved result bodies are comparison-only, never inputs.
        for record in [
            &head_records.accepted,
            &head_records.head,
            &normalized.intent,
        ] {
            let reference = wire::reference(record.reference(), s)?;
            let reference = s
                .parse_representation(reference.bytes(), Kind::Record)
                .map_err(core_error)?;
            origin.require_member(reference.value(), s)?;
            let schema = s
                .parse_representation(record.bytes(), Kind::Record)
                .map_err(core_error)?;
            let schema = request_wrapper::text(schema.value(), "schema", s)?;
            let stored = tx
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
            same_bytes(stored.schema.bytes(), schema.as_bytes(), s)?;
            same_bytes(stored.body.bytes(), record.bytes(), s)?;
        }
        let results = wire::RefSet::new(
            [
                head_records.accepted.reference(),
                head_records.head.reference(),
                normalized.intent.reference(),
            ]
            .into_iter(),
            3,
            true,
            s,
        )?
        .encode(s)?;
        let asserted_results = encode(request_wrapper::field(operation, "result_refs", s)?, s)?;
        same_bytes(results.bytes(), asserted_results.bytes(), s)?;
        Ok(ReplayedAcceptance {
            original,
            incoming,
            local_root,
            authority,
            private_documents,
            sources,
            consents,
            head_records,
            normalized,
            _sources: storage,
        })
    }
    .await;
    terminal(result, tx, s)
}
