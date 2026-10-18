//! Mechanical source/F seed DATA from the held work capture. This does not
//! certify complete I, metadata support, custody, or permit header emission.
use super::*;
use ledgerlab_core::canonical::audit_arena::AuditedBytes;
use std::mem;
pub(super) struct WorkObservationSeeds {
    pub(super) source: AuditedValue,
    pub(super) financial: AuditedValue,
    _storage: ExternalBufferLease,
}
fn same_ref(a: &ExactRef, b: &ExactRef, s: &mut OriginalAuditSession) -> Result<bool> {
    for (a, b) in [(&a.kind, &b.kind), (&a.id, &b.id), (&a.digest, &b.digest)] {
        s.semantic_work()
            .visit(ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
        s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}
fn original_refs(
    observed: &ObservedAppBinding,
    source: &source::ObservedSource,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let count = observed.sources.len();
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[&count.into(), &mem::size_of::<&ExactRef>().into()],
        )
        .map_err(core_error)?;
    let bytes = count
        .checked_mul(mem::size_of::<&ExactRef>())
        .ok_or(ServiceError::Unavailable)?;
    s.semantic_work()
        .arithmetic(
            Op::Add,
            &[&bytes.into(), &mem::size_of::<Vec<&ExactRef>>().into()],
        )
        .map_err(core_error)?;
    let bytes = bytes
        .checked_add(mem::size_of::<Vec<&ExactRef>>())
        .ok_or(ServiceError::Unavailable)?;
    let _storage = s.reserve_external_payload(bytes).map_err(core_error)?;
    let mut references = Vec::new();
    references
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    for other in &observed.sources {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        if same_ref(other.enrollment_ref(), source.enrollment_ref(), s)? {
            s.semantic_work()
                .write_bytes(mem::size_of::<&ExactRef>())
                .map_err(core_error)?;
            references.push(other.original_grant_ref());
        }
    }
    let selected = references.len();
    wire::RefSet::new(references.into_iter(), selected, false, s)?.encode(s)
}
impl ObservedAppBinding {
    /// Real code-owned verifier outputs are required by the enclosing producer.
    /// These borrowed references are DATA, never a caller support capability.
    pub(super) fn work_observation_seeds(
        &self,
        key: &OperationKey,
        key_body: &Value,
        source_verifier: &ledgerlab_core::policy::workflow::DefinitionRef,
        financial_verifier: &ledgerlab_core::policy::workflow::DefinitionRef,
        s: &mut OriginalAuditSession,
    ) -> Result<WorkObservationSeeds> {
        let result = (|| {
            let storage = s
                .reserve_external_payload(mem::size_of::<WorkObservationSeeds>())
                .map_err(core_error)?;
            let _parts = s
                .reserve_external_payload(
                    2 * mem::size_of::<[(&str, &[u8]); 16]>() + 16 * mem::size_of::<AuditedBytes>(),
                )
                .map_err(core_error)?;
            check::key(key_body, key, s)?;
            s.semantic_work()
                .visit(ValidationVisit::RefLookup)
                .map_err(core_error)?;
            let index = self.work_source_index.ok_or(ServiceError::Unavailable)?;
            let source = self
                .sources
                .get(index)
                .ok_or(ServiceError::IntegrityFailure)?;
            let root = self.work_root.as_ref().ok_or(ServiceError::Unavailable)?;
            let inputs = self
                .work_explicit_inputs
                .as_ref()
                .ok_or(ServiceError::Unavailable)?;
            let heads = self.work_heads.as_ref().ok_or(ServiceError::Unavailable)?;
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&self.before.into(), &1i64.into()])
                .map_err(core_error)?;
            let through = self
                .before
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&through.into(), &0i64.into()])
                .map_err(core_error)?;
            require(through >= 0, "INTEGRITY_FAILURE")?;
            let through = wire::counter_string(through as u64, s)?;
            let store = wire::string(&key.logical_store_id, s)?;
            let scope_value = check::field(key_body, "scope", s)?;
            let scope = s
                .canonical_representation(scope_value, RepresentationKind::Record)
                .map_err(core_error)?;
            let root_ref = wire::reference(root.reference(), s)?;
            let enrollment = wire::reference(source.enrollment_ref(), s)?;
            let state = wire::reference(source.state_ref(), s)?;
            let grant = wire::reference(
                source
                    .current_grant_ref()
                    .ok_or(ServiceError::Unavailable)?,
                s,
            )?;
            let originals = original_refs(self, source, s)?;
            check::same_text(
                source_verifier.kind(),
                "workflow-authority-audit-verifier",
                s,
            )?;
            check::same_text(
                financial_verifier.kind(),
                "workflow-authority-audit-verifier",
                s,
            )?;
            let source_verifier = wire::definition_reference(source_verifier, s)?;
            let financial_verifier = wire::definition_reference(financial_verifier, s)?;
            let observed_at = wire::string(self.at.as_str(), s)?;
            let revision_value = check::field(source.state_body(), "revision", s)?;
            let revision = s
                .canonical_representation(revision_value, RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<[(&str, &[u8]); 12]>())
                .map_err(core_error)?;
            let body = wire::object(
                &[
                    ("current_grant_ref", grant.bytes()),
                    ("current_state_ref", state.bytes()),
                    ("enrollment_ref", enrollment.bytes()),
                    ("head_revision", revision.bytes()),
                    ("installation_mode", b"\"real_local\""),
                    ("logical_store_id", store.bytes()),
                    ("observed_at", observed_at.bytes()),
                    ("original_grant_refs", originals.bytes()),
                    ("root_ref", root_ref.bytes()),
                    ("scope", scope.bytes()),
                    ("verified_through_sequence", through.bytes()),
                    ("verifier_ref", source_verifier.bytes()),
                ],
                4194304,
                s,
            )?;
            let source = s
                .parse_representation(body.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            drop(body);
            let preexisting = inputs.members.encode(s)?;
            let key = s
                .canonical_representation(key_body, RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<[(&str, &[u8]); 11]>())
                .map_err(core_error)?;
            let body = wire::object(
                &[
                    ("head_pins", heads.pins.bytes()),
                    ("history_edges", inputs.history_rows.bytes()),
                    ("installation_mode", b"\"real_local\""),
                    ("logical_store_id", store.bytes()),
                    ("observed_at", observed_at.bytes()),
                    ("pending_operation_key", key.bytes()),
                    ("preexisting_input_refs", preexisting.bytes()),
                    ("root_ref", root_ref.bytes()),
                    ("scope", scope.bytes()),
                    ("snapshot_through_sequence", through.bytes()),
                    ("verifier_ref", financial_verifier.bytes()),
                ],
                4194304,
                s,
            )?;
            let financial = s
                .parse_representation(body.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<WorkObservationSeeds>())
                .map_err(core_error)?;
            Ok(WorkObservationSeeds {
                source,
                financial,
                _storage: storage,
            })
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}
