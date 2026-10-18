//! Owning root-work capture for the existing complete-constructor DATA ABI.
//! Actual origin, pending/reused partition, all source-frame selection and
//! eligibility remain enclosing obligations; no observation Ref is invented.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    AuthorityContextData, DraftContextData, OperationKey as CoreKey, RecordInput,
};
use std::mem;
pub(super) struct OwnedWorkContext {
    pub(super) context: DraftContextData,
    _storage: ExternalBufferLease,
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
fn copied(v: &str, s: &mut OriginalAuditSession) -> Result<String> {
    s.semantic_work().scan_bytes(v.len()).map_err(core_error)?;
    s.semantic_work().write_bytes(v.len()).map_err(core_error)?;
    let mut result = String::new();
    result
        .try_reserve_exact(v.len())
        .map_err(|_| ServiceError::Unavailable)?;
    result.push_str(v);
    Ok(result)
}
fn ref_payload(mut n: usize, r: &ExactRef, s: &mut OriginalAuditSession) -> Result<usize> {
    for v in [&r.kind, &r.id, &r.digest] {
        s.semantic_work()
            .visit(ValidationVisit::Field)
            .map_err(core_error)?;
        n = add(n, v.len(), s)?;
    }
    Ok(n)
}
fn copied_ref(r: &ExactRef, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copied(&r.kind, s)?,
        id: copied(&r.id, s)?,
        digest: copied(&r.digest, s)?,
    })
}
fn copied_value_ref(value: &Value, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    check::closed(value, &["kind", "id", "digest"], s)?;
    let kind = copied(check::text(value, "kind", s)?, s)?;
    let id = copied(check::text(value, "id", s)?, s)?;
    let digest = copied(check::text(value, "digest", s)?, s)?;
    Ok(ExactRef { kind, id, digest })
}
fn record(row: &wire::SourceRecord, s: &mut OriginalAuditSession) -> Result<RecordInput> {
    let reference = copied_ref(row.reference(), s)?;
    s.semantic_work()
        .scan_bytes(row.bytes().len())
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(row.bytes().len())
        .map_err(core_error)?;
    let mut canonical_body = Vec::new();
    canonical_body
        .try_reserve_exact(row.bytes().len())
        .map_err(|_| ServiceError::Unavailable)?;
    canonical_body.extend_from_slice(row.bytes());
    Ok(RecordInput {
        reference,
        canonical_body,
    })
}
impl ObservedAppBinding {
    pub(super) fn root_work_context(
        &self,
        key: &OperationKey,
        observation: &wire::SourceRecord,
        new_verifier: Option<&wire::SourceRecord>,
        s: &mut OriginalAuditSession,
    ) -> Result<OwnedWorkContext> {
        let result = (|| {
            s.semantic_work()
                .visit(ValidationVisit::Constraint)
                .map_err(core_error)?;
            let received = self
                .work_received_at
                .as_ref()
                .ok_or(ServiceError::Unavailable)?;
            let activation = self.activation.as_ref().ok_or(ServiceError::Unavailable)?;
            s.semantic_work()
                .visit(ValidationVisit::RefLookup)
                .map_err(core_error)?;
            let source = self
                .sources
                .get(self.work_source_index.ok_or(ServiceError::Unavailable)?)
                .ok_or(ServiceError::IntegrityFailure)?;
            let grant = source
                .current_grant_body()
                .ok_or(ServiceError::Unavailable)?;
            let namespace = check::text(grant, "canonical_economic_namespace", s)?;
            let current_grant = source
                .current_grant_ref()
                .ok_or(ServiceError::Unavailable)?;
            let slot = self
                .work_source_slot
                .as_ref()
                .ok_or(ServiceError::Unavailable)?;
            let slot = s
                .parse_representation(slot.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .visit(ValidationVisit::Constraint)
                .map_err(core_error)?;
            let slot = slot
                .value()
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?;
            let originals = work_seeds::original_refs(self, source, s)?;
            let originals = s
                .parse_representation(originals.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .visit(ValidationVisit::Constraint)
                .map_err(core_error)?;
            let originals = originals
                .value()
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            check::same_text(
                &observation.reference().kind,
                "workflow-authority-prefix-observation",
                s,
            )?;
            check::same_text(
                observation.schema(),
                "ledger-workflow-authority-prefix-observation/1",
                s,
            )?;
            if let Some(verifier) = new_verifier {
                check::same_text(
                    &verifier.reference().kind,
                    "workflow-authority-audit-verifier",
                    s,
                )?;
                check::same_text(verifier.schema(), "ledger-authority-audit-verifier/2", s)?;
            }
            let mut bytes = mem::size_of::<OwnedWorkContext>()
                + 2 * mem::size_of::<AuthorityContextData>()
                + 3 * mem::size_of::<RecordInput>()
                + 2 * mem::size_of::<CoreKey>()
                + 2 * mem::size_of::<ExactRef>();
            s.semantic_work()
                .arithmetic(
                    Op::Multiply,
                    &[&originals.len().into(), &mem::size_of::<ExactRef>().into()],
                )
                .map_err(core_error)?;
            bytes = add(
                bytes,
                originals
                    .len()
                    .checked_mul(mem::size_of::<ExactRef>())
                    .ok_or(ServiceError::Unavailable)?,
                s,
            )?;
            for value in [
                &key.logical_store_id,
                &key.scope[0],
                &key.scope[1],
                &key.source,
                &key.operation_id,
                &self.setup.operator,
                namespace,
                "real_local",
                received.as_str(),
                self.at.as_str(),
                slot,
            ] {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                bytes = add(bytes, value.len(), s)?;
            }
            for reference in [
                source.enrollment_ref(),
                source.state_ref(),
                current_grant,
                self.original.acceptance.binding.reference(),
                self.original.acceptance.accepted_reference(),
                activation.activation.reference(),
                observation.reference(),
            ] {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                bytes = ref_payload(bytes, reference, s)?;
            }
            for reference in originals {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                for field in ["kind", "id", "digest"] {
                    bytes = add(bytes, check::text(reference, field, s)?.len(), s)?;
                }
            }
            let mut pending_count = 0usize;
            for row in std::iter::once(observation).chain(new_verifier) {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                pending_count = add(pending_count, 1, s)?;
                bytes = ref_payload(bytes, row.reference(), s)?;
                bytes = add(bytes, row.bytes().len(), s)?;
            }
            let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
            // Every reserved control/payload write is debited before any
            // allocation so a later malformed-input refusal retains its cost.
            s.semantic_work().write_bytes(bytes).map_err(core_error)?;
            let key = CoreKey {
                logical_store_id: copied(&key.logical_store_id, s)?,
                scope: [copied(&key.scope[0], s)?, copied(&key.scope[1], s)?],
                source: copied(&key.source, s)?,
                operation_id: copied(&key.operation_id, s)?,
            };
            let mut original_grant_refs = Vec::new();
            original_grant_refs
                .try_reserve_exact(originals.len())
                .map_err(|_| ServiceError::Unavailable)?;
            for value in originals {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                original_grant_refs.push(copied_value_ref(value, s)?);
            }
            let authority = AuthorityContextData {
                source_slot: copied(slot, s)?,
                enrollment_ref: copied_ref(source.enrollment_ref(), s)?,
                current_state_ref: copied_ref(source.state_ref(), s)?,
                current_grant_ref: copied_ref(current_grant, s)?,
                original_grant_refs,
                binding_ref: copied_ref(self.original.acceptance.binding.reference(), s)?,
                accepted_agreement_ref: copied_ref(
                    self.original.acceptance.accepted_reference(),
                    s,
                )?,
                activation_ref: copied_ref(activation.activation.reference(), s)?,
                authority_observation_ref: copied_ref(observation.reference(), s)?,
            };
            let mut authorities = Vec::new();
            authorities
                .try_reserve_exact(1)
                .map_err(|_| ServiceError::Unavailable)?;
            authorities.push(authority);
            let mut source_observation_inputs = Vec::new();
            source_observation_inputs
                .try_reserve_exact(pending_count)
                .map_err(|_| ServiceError::Unavailable)?;
            for row in std::iter::once(observation).chain(new_verifier) {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                source_observation_inputs.push(record(row, s)?);
            }
            let context = DraftContextData {
                key,
                actor: copied(&self.setup.operator, s)?,
                canonical_economic_namespace: copied(namespace, s)?,
                installation_mode: copied("real_local", s)?,
                received_at: copied(received.as_str(), s)?,
                accepted_at: copied(self.at.as_str(), s)?,
                authorities,
                source_observation_inputs,
            };
            Ok(OwnedWorkContext {
                context,
                _storage: storage,
            })
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}
