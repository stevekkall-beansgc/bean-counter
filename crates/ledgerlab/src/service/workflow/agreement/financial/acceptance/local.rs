//! Targeted committed local agreement inputs. A projection locates its exact
//! receipt; canonical bodies, immutable fullterms and current rights stay checked.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{EconomicRecordView, ExactRef};
pub(in super::super) struct TargetedAcceptance {
    original: ReconstructedAcceptance,
    incoming: ingress::IncomingRequest,
    private_documents: private::InlineDocuments,
    accepted: current::source::SourceRecord,
    verifications: AuditedValue,
    _binding: AuditedValue,
}
impl std::ops::Deref for TargetedAcceptance {
    type Target = ReconstructedAcceptance;
    fn deref(&self) -> &Self::Target {
        &self.original
    }
}
impl TargetedAcceptance {
    pub(in super::super) async fn load(
        tx: &mut SqliteTx,
        origin: &prefix::LocatedWrappers,
        publication: &prefix::RecompiledPublication,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        require(
            publication.program().uses_local_execution_recipe(),
            "UNSUPPORTED_SCHEMA_PROFILE",
        )?;
        let original = super::reconstruct(tx, origin, publication, s).await?;
        let incoming =
            ingress::IncomingRequest::parse(origin.wrappers().original_request().raw(), s)?;
        let private_documents = private::normalize_inline(&incoming, &original.input, s)?;
        let private_documents = private::resolve_graph(
            private_documents,
            tx,
            origin.key(),
            &original.input,
            origin.wrappers().sequence() + 1,
            s,
        )
        .await?;
        private::associate_graph(&private_documents, &original.input, s)?;
        let binding = s
            .parse_representation(original.binding.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        private::validate_slots(
            &private_documents,
            &original.input,
            publication,
            binding.value(),
            &origin.key().scope,
            s,
        )?;
        for r in private_documents.record_views() {
            let v = wire::reference(r.reference, s)?;
            let v = s
                .parse_representation(v.bytes(), Kind::Record)
                .map_err(core_error)?;
            origin.require_member(v.value(), s)?;
        }
        let results = request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut selected = None;
        for r in results {
            if request_wrapper::text(r, "kind", s)? == "workflow-accepted-agreement" {
                require(selected.replace(r).is_none(), "INTEGRITY_FAILURE")?;
            }
        }
        let r = selected.ok_or(ServiceError::IntegrityFailure)?;
        origin.require_member(r, s)?;
        let accepted = current::source::retained_record(
            tx,
            origin.key(),
            r,
            "workflow-accepted-agreement",
            "ledger-workflow-accepted-agreement/1",
            origin.wrappers().sequence() + 1,
            s,
        )
        .await?;
        for (name, wanted) in [
            (
                "binding_ref",
                serde_json::to_value(original.binding.reference())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            ),
            (
                "terms_ref",
                serde_json::to_value(original.input.terms().reference())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            ),
        ] {
            let actual = encode(request_wrapper::field(accepted.body(), name, s)?, s)?;
            let wanted = encode(&wanted, s)?;
            same_bytes(actual.bytes(), wanted.bytes(), s)?;
        }
        request_wrapper::same_text(
            request_wrapper::text(accepted.body(), "terms_digest", s)?,
            original.input.terms().terms_digest(),
            s,
        )?;
        let authority =
            request_wrapper::field(origin.wrappers().operation(), "authority_snapshot", s)?;
        let refs = request_wrapper::field(authority, "app_consent_verification_refs", s)?;
        let refs = reference_set(refs, "workflow-verification", 1, 16, s)?;
        let verifications = s
            .parse_representation(refs.bytes(), Kind::Record)
            .map_err(core_error)?;
        for r in verifications
            .value()
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            origin.require_member(r, s)?;
            let v = current::source::retained_record(
                tx,
                origin.key(),
                r,
                "workflow-verification",
                "ledger-workflow-app-consent-verification/2",
                origin.wrappers().sequence() + 1,
                s,
            )
            .await?;
            // A saved verification selects only the immutable exact fullterms already
            // accepted in this same committed operation; it never supplies current maps.
            let expected = serde_json::to_value(original.input.terms().reference())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let actual = encode(
                request_wrapper::field(v.body(), "acceptance_terms_ref", s)?,
                s,
            )?;
            let expected = encode(&expected, s)?;
            same_bytes(actual.bytes(), expected.bytes(), s)?;
        }
        Ok(Self {
            original,
            incoming,
            private_documents,
            accepted,
            verifications,
            _binding: binding,
        })
    }
    pub(in super::super) fn accepted_reference(&self) -> &ExactRef {
        self.accepted.reference()
    }
    pub(in super::super) fn work_locator_records(
        &self,
    ) -> impl Iterator<Item = EconomicRecordView<'_>> {
        std::iter::once(EconomicRecordView {
            reference: self.original.binding.reference(),
            canonical_body: self.original.binding.bytes(),
        })
        .chain(std::iter::once(EconomicRecordView {
            reference: self.accepted.reference(),
            canonical_body: self.accepted.canonical_body(),
        }))
        .chain(self.private_documents.record_views())
    }
    pub(in super::super) fn require_local_admission_sources(
        &self,
        sources: &[current::source::ObservedSource],
        publication: &prefix::RecompiledPublication,
        binding: &Value,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        nominations::validate_local_admission(
            sources,
            &self.private_documents,
            &self.original.input,
            publication,
            binding,
            at.as_str(),
            s,
        )
    }
    pub(in super::super) fn current_activation_source(
        &self,
        sources: &[current::source::ObservedSource],
        publication: &prefix::RecompiledPublication,
        binding: &Value,
        key: &OperationKey,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
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
    pub(in super::super) fn activation_authority(
        &self,
        source: &current::source::ObservedSource,
        root: &wire::SourceRecord,
        key: &OperationKey,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<wire::SourceRecord> {
        let refs = encode(self.verifications.value(), s)?;
        admission::authority_with_refs(
            &self.original.input,
            source,
            refs.bytes(),
            root,
            key,
            actor,
            at,
            "agreement.activate",
            s,
        )
    }
    pub(in super::super) async fn current_platforms(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CurrentPlatformConsents> {
        consent::observe_local_platforms(
            &self.original.input,
            &self.incoming,
            binding,
            tx,
            key,
            at,
            before,
            s,
        )
        .await
    }
}
