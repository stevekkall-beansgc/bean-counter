//! Actual saved AppAccept input/authority/result material under an already
//! folded earlier prefix. Equality remains DATA until exact complete journal
//! and introduction/ProfileB obligations finish in the chronological driver.
use super::*;
pub(in super::super) struct ReplayedAcceptance {
    original: ReconstructedAcceptance,
    _incoming: ingress::IncomingRequest,
    local_root: wire::SourceRecord,
    terms_record: wire::SourceRecord,
    authority: wire::SourceRecord,
    private_documents: private::InlineDocuments,
    _observed_sources: Vec<current::source::ObservedSource>,
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
            if request_wrapper::text(r, "kind", s)? == "workflow-authority" {
                let mapping = current::source::retained_record(
                    tx,
                    origin.key(),
                    r,
                    "workflow-authority",
                    "ledger-workflow-party-authorization/1",
                    origin.wrappers().sequence(),
                    s,
                )
                .await?;
                super::super::prefix::verify_selected_association(
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
            (
                source,
                &local_root,
                actor,
                &original.at,
                origin.wrappers().sequence(),
                prefix,
            ),
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
            (actor, &original.at, "agreement.accept"),
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
        let terms_record = input.terms_record(s)?;
        for record in [
            &local_root,
            &authority,
            &original.binding,
            &terms_record,
            &head_records.accepted,
            &head_records.head,
            &normalized.intent,
        ]
        .into_iter()
        .chain(private_documents.inline_records())
        .chain(consents.generated_records())
        {
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
        let evidence =
            evidence::acceptance_material(&incoming, normalized.payload.bytes(), publication, s)?;
        let asserted_evidence = request_wrapper::field(operation, "request_evidence", s)?;
        let asserted_evidence = s
            .canonical_representation(asserted_evidence, Kind::Record)
            .map_err(core_error)?;
        same_bytes(evidence.bytes(), asserted_evidence.bytes(), s)?;
        let members = resolve_records(
            prefix,
            &local_root,
            &authority,
            &original.binding,
            &terms_record,
            &head_records,
            &normalized,
            &private_documents,
            &consents,
            publication,
            s,
        )?;
        let journal = wire::frame_acceptance(
            &incoming,
            normalized.payload.bytes(),
            &authority,
            &original.at,
            origin.wrappers().sequence(),
            [
                head_records.accepted.reference(),
                head_records.head.reference(),
                normalized.intent.reference(),
            ],
            &evidence,
            members,
            s,
        )?;
        journal.compare_original(origin, s)?;
        Ok(ReplayedAcceptance {
            original,
            _incoming: incoming,
            local_root,
            terms_record,
            authority,
            private_documents,
            _observed_sources: sources,
            consents,
            head_records,
            normalized,
            _sources: storage,
        })
    }
    .await;
    terminal(result, tx, s)
}

impl ReplayedAcceptance {
    /// Compare only after the complete saved certificate has succeeded. The
    /// incoming request uses the ORIGINAL publication and verified consent
    /// material; it never reauthorizes the saved acceptance under today's grant.
    pub(in super::super) async fn compare_retry(
        &self,
        tx: &mut SqliteTx,
        origin: &prefix::LocatedWrappers,
        publication: &prefix::RecompiledPublication,
        incoming: &ingress::IncomingRequest,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(incoming.kind() == "agreement.accept", "IDENTITY_CONFLICT")?;
        let input = AppAcceptanceInput::normalize(incoming, publication, s)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(input.terms().canonical_bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(self.input.terms().canonical_bytes().len())
            .map_err(core_error)?;
        require(
            input.terms().canonical_bytes() == self.input.terms().canonical_bytes(),
            "IDENTITY_CONFLICT",
        )?;
        let documents = private::normalize_inline(incoming, &input, s)?;
        s.semantic_work()
            .arithmetic(
                Op::Add,
                &[&origin.wrappers().sequence().into(), &1i64.into()],
            )
            .map_err(core_error)?;
        let before = origin
            .wrappers()
            .sequence()
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let documents =
            private::resolve_graph(documents, tx, origin.key(), &input, before, s).await?;
        let binding = s
            .parse_representation(self.binding.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        let normalized = request::build(
            incoming,
            &input,
            binding.value(),
            &documents,
            &self.consents,
            s,
        )?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(normalized.payload.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(self.normalized.payload.bytes().len())
            .map_err(core_error)?;
        require(
            normalized.payload.bytes() == self.normalized.payload.bytes(),
            "IDENTITY_CONFLICT",
        )
    }
    /// Commit only the invocation-local chronological DATA fold after SAME
    /// original evidence, closure and journal equality above. Persistent heads
    /// and private prepared capabilities are not created by this operation.
    pub(in super::super) fn fold_original(
        &self,
        origin: &prefix::LocatedWrappers,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let binding = s
            .parse_representation(self.original.binding.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        let customer = encode(request_wrapper::field(binding.value(), "customer", s)?, s)?;
        let agreement = encode(
            request_wrapper::field(binding.value(), "agreement_id", s)?,
            s,
        )?;
        let version = encode(request_wrapper::field(binding.value(), "version", s)?, s)?;
        let semantic = prefix::inventory::StateKey::new(
            "agreement-acceptance-semantic",
            &[customer.bytes(), agreement.bytes(), version.bytes()],
            s,
        )?;
        let owner = prefix::inventory::StateKey::new("agreement-owner", &[agreement.bytes()], s)?;
        let head = prefix::inventory::StateKey::new(
            "agreement-acceptance-head",
            &[customer.bytes(), agreement.bytes()],
            s,
        )?;
        prefix._inventory.absent(&semantic, s)?;
        if let Some(old) = prefix._inventory.head_body(&owner, s)? {
            let old = s
                .parse_representation(old.bytes(), Kind::SourceDefinition)
                .map_err(core_error)?;
            let old_customer = encode(request_wrapper::field(old.value(), "customer", s)?, s)?;
            same_bytes(customer.bytes(), old_customer.bytes(), s)?;
        }
        for record in [
            &self.local_root,
            &self.authority,
            &self.original.binding,
            &self.terms_record,
            &self.head_records.accepted,
            &self.head_records.head,
            &self.normalized.intent,
        ]
        .into_iter()
        .chain(self.private_documents.inline_records())
        .chain(self.consents.generated_records())
        {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            prefix._inventory.introduce(
                record.reference(),
                record.bytes(),
                origin.wrappers().sequence(),
                s,
            )?;
        }
        // Permanent identity and logical head derive from the freshly
        // reconstructed record group, never an asserted projection or latest
        // SQL row. A failed enclosing fold discards this entire scratch owner.
        prefix
            ._inventory
            .set_head(&semantic, self.head_records.accepted.reference(), s)?;
        prefix
            ._inventory
            .set_head(&owner, self.head_records.accepted.reference(), s)?;
        prefix
            ._inventory
            .set_head(&head, self.head_records.head.reference(), s)
    }
}

impl ReplayedAcceptance {
    /// Reuse the actual completed acceptance's original normalized assents;
    /// never substitute current terms or the admin as resource payer/acceptor.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn current_platforms(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<consent::CurrentPlatformConsents> {
        self.consents
            .observe_current_platforms(
                &self.original.input,
                binding,
                tx,
                key,
                at,
                before,
                prefix,
                s,
            )
            .await
    }
}

impl ReplayedAcceptance {
    /// Activation's current coverage uses the SAME originally accepted graph
    /// and nominated-source validator. The selected index is DATA only; stop
    /// deliberately follows its separate evidence-based Authority2 recipe.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn current_activation_source(
        &self,
        sources: &[current::source::ObservedSource],
        publication: &prefix::RecompiledPublication,
        binding: &Value,
        key: &OperationKey,
        context: (&str, &Timestamp),
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
        let (actor, at) = context;
        nominations::validate(
            sources,
            &self.private_documents,
            &self.original.input,
            publication,
            binding,
            at.as_str(),
            s,
        )?;
        admission::select(sources, actor, key, s)
    }
}

impl ReplayedAcceptance {
    /// Activation retains the exact original fullterms verification Refs.
    /// Today's mapping/source checks are separate held observations; they do
    /// not rewrite the original assent or verification's accepted timestamps.
    pub(in super::super) fn activation_authority(
        &self,
        source: &current::source::ObservedSource,
        root: &wire::SourceRecord,
        key: &OperationKey,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<wire::SourceRecord> {
        admission::authority(
            &self.original.input,
            source,
            &self.consents,
            root,
            key,
            (actor, at, "agreement.activate"),
            s,
        )
    }
}

impl ReplayedAcceptance {
    pub(in super::super) fn accepted_reference(
        &self,
    ) -> &ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
        self.head_records.accepted.reference()
    }
}

impl ReplayedAcceptance {
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn reconstruct_activation_platforms(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<consent::CurrentPlatformConsents> {
        self.consents
            .reconstruct_activation_platforms(
                &self.original.input,
                binding,
                tx,
                key,
                at,
                before,
                prefix,
                s,
            )
            .await
    }
}

impl ReplayedAcceptance {
    /// Borrow exact original selected bodies as a locator pool. Membership
    /// alone is not consumption; the core selector independently records use.
    pub(in super::super) fn work_locator_records(
        &self,
    ) -> impl Iterator<
        Item = ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView<'_>,
    > {
        [&self.original.binding, &self.head_records.accepted]
            .into_iter()
            .map(|record| {
                ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView {
                    reference: record.reference(),
                    canonical_body: record.bytes(),
                }
            })
            .chain(self.private_documents.record_views())
    }
}
