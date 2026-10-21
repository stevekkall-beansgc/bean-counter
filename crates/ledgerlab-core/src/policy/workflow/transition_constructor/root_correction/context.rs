//! Owning selection of an existing source frame. This copies DATA under the
//! SAME arena before producers run; it neither changes the operation key/actor
//! nor certifies the selected source's original/current authority.
use super::super::{data::*, emitted_record::ConstructedEnvelope, records, shape, CoreMeter};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Result,
};
use serde_json::Value;
use std::{mem, ops::Deref};
pub(super) struct RootContext {
    value: DraftContextData,
    _ownership: Option<ExternalBufferLease>,
}
impl Deref for RootContext {
    type Target = DraftContextData;
    fn deref(&self) -> &DraftContextData {
        &self.value
    }
}
impl RootContext {
    pub(super) fn legacy(value: DraftContextData) -> Self {
        Self {
            value,
            _ownership: None,
        }
    }
}
fn add(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    w::usize_add(m, a, b)
}
fn mul(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("root context allocation product"))
}
fn size_text(m: &mut CoreMeter, n: &mut usize, s: &str) -> Result<()> {
    m.visit("root.context_text_size")?;
    *n = add(m, *n, s.len())?;
    Ok(())
}
fn size_ref(m: &mut CoreMeter, n: &mut usize, r: &ExactRef) -> Result<()> {
    for s in [&r.kind, &r.id, &r.digest] {
        size_text(m, n, s)?;
    }
    Ok(())
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.visit("root.context_text_copy")?;
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
pub(super) fn selected(
    m: &mut CoreMeter,
    c: &DraftContextData,
    frame: &AuthorityContextData,
    verifier: &Value,
    introduced: &[ConstructedEnvelope],
) -> Result<RootContext> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning root context"))?;
    shape::closed(m, verifier, &["kind", "id", "digest"], &[])?;
    let kind = shape::text(m, verifier, "kind")?;
    let id = shape::text(m, verifier, "id")?;
    let digest = shape::text(m, verifier, "digest")?;
    records::reference_parts(m, kind, id, digest)?;
    let mut pending: [Option<&RecordInput>; 2] = [None, None];
    let mut count = 0usize;
    for input in &c.source_observation_inputs {
        m.visit("root.pending_source_operand")?;
        let observed = shape::same_ref(m, &input.reference, frame.required_observation_ref()?)?;
        let verified = shape::same_text(m, &input.reference.kind, kind)?
            && shape::same_text(m, &input.reference.id, id)?
            && shape::same_text(m, &input.reference.digest, digest)?;
        if !observed && !verified {
            continue;
        }
        let mut known = false;
        for record in introduced {
            m.visit("root.pending_already_introduced")?;
            if shape::same_ref(m, &record.reference, &input.reference)? {
                known = true;
                break;
            }
        }
        if known {
            continue;
        }
        for previous in pending.iter().flatten() {
            if shape::same_ref(m, &previous.reference, &input.reference)? {
                return Err(shape::fail("duplicate pending root source operand"));
            }
        }
        if !w::compare(m, count, pending.len())?.is_lt() {
            return Err(shape::fail("more than observation/verifier root operands"));
        }
        pending[count] = Some(input);
        count = add(m, count, 1)?;
    }
    let mut bytes = add(
        m,
        mem::size_of::<RootContext>(),
        mem::size_of::<AuthorityContextData>(),
    )?;
    let refs = mul(
        m,
        frame.original_grant_refs.len(),
        mem::size_of::<ExactRef>(),
    )?;
    bytes = add(m, bytes, refs)?;
    let rows = mul(m, count, mem::size_of::<RecordInput>())?;
    bytes = add(m, bytes, rows)?;
    for s in [
        &c.key.logical_store_id,
        &c.key.scope[0],
        &c.key.scope[1],
        &c.key.source,
        &c.key.operation_id,
        &c.actor,
        &c.canonical_economic_namespace,
        &c.installation_mode,
        &c.received_at,
        &c.accepted_at,
        &frame.source_slot,
    ] {
        size_text(m, &mut bytes, s)?;
    }
    for r in [
        &frame.enrollment_ref,
        &frame.current_state_ref,
        &frame.current_grant_ref,
        &frame.binding_ref,
        &frame.accepted_agreement_ref,
        &frame.activation_ref,
        frame.required_observation_ref()?,
    ] {
        size_ref(m, &mut bytes, r)?;
    }
    for r in &frame.original_grant_refs {
        size_ref(m, &mut bytes, r)?;
    }
    for input in pending.iter().flatten() {
        size_ref(m, &mut bytes, &input.reference)?;
        bytes = add(m, bytes, input.canonical_body.len())?;
    }
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let mut originals = Vec::new();
    originals
        .try_reserve_exact(frame.original_grant_refs.len())
        .map_err(|_| shape::fail("root context original grant vector"))?;
    for r in &frame.original_grant_refs {
        originals.push(reference(m, r)?);
    }
    let selected = AuthorityContextData {
        source_slot: copy(m, &frame.source_slot)?,
        enrollment_ref: reference(m, &frame.enrollment_ref)?,
        current_state_ref: reference(m, &frame.current_state_ref)?,
        current_grant_ref: reference(m, &frame.current_grant_ref)?,
        original_grant_refs: originals,
        binding_ref: reference(m, &frame.binding_ref)?,
        accepted_agreement_ref: reference(m, &frame.accepted_agreement_ref)?,
        activation_ref: reference(m, &frame.activation_ref)?,
        authority_observation_ref: Some(reference(m, frame.required_observation_ref()?)?),
    };
    let mut authorities = Vec::new();
    authorities
        .try_reserve_exact(1)
        .map_err(|_| shape::fail("root context authority vector"))?;
    authorities.push(selected);
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("root context pending vector"))?;
    for input in pending.into_iter().flatten() {
        let r = reference(m, &input.reference)?;
        m.scan_bytes(input.canonical_body.len())?;
        m.write_bytes(input.canonical_body.len())?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(input.canonical_body.len())
            .map_err(|_| shape::fail("root pending body allocation"))?;
        raw.extend_from_slice(&input.canonical_body);
        inputs.push(RecordInput {
            reference: r,
            canonical_body: raw,
        });
    }
    let key = OperationKey {
        logical_store_id: copy(m, &c.key.logical_store_id)?,
        scope: [copy(m, &c.key.scope[0])?, copy(m, &c.key.scope[1])?],
        source: copy(m, &c.key.source)?,
        operation_id: copy(m, &c.key.operation_id)?,
    };
    let value = DraftContextData {
        key,
        actor: copy(m, &c.actor)?,
        canonical_economic_namespace: copy(m, &c.canonical_economic_namespace)?,
        installation_mode: copy(m, &c.installation_mode)?,
        received_at: copy(m, &c.received_at)?,
        accepted_at: copy(m, &c.accepted_at)?,
        authorities,
        source_observation_inputs: inputs,
    };
    Ok(RootContext {
        value,
        _ownership: Some(ownership),
    })
}

pub(super) fn selected_local(
    m: &mut CoreMeter,
    c: &DraftContextData,
    frame: &AuthorityContextData,
) -> Result<RootContext> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("local root capture owner"))?;
    if frame.authority_observation_ref.is_some() || !c.source_observation_inputs.is_empty() {
        return Err(shape::fail(
            "local root capture carries experimental metadata",
        ));
    }
    let mut bytes = mem::size_of::<RootContext>() + mem::size_of::<AuthorityContextData>();
    for s in [
        &c.key.logical_store_id,
        &c.key.scope[0],
        &c.key.scope[1],
        &c.key.source,
        &c.key.operation_id,
        &c.actor,
        &c.canonical_economic_namespace,
        &c.installation_mode,
        &c.received_at,
        &c.accepted_at,
        &frame.source_slot,
    ] {
        size_text(m, &mut bytes, s)?;
    }
    for r in [
        &frame.enrollment_ref,
        &frame.current_state_ref,
        &frame.current_grant_ref,
        &frame.binding_ref,
        &frame.accepted_agreement_ref,
        &frame.activation_ref,
    ] {
        size_ref(m, &mut bytes, r)?;
    }
    for r in &frame.original_grant_refs {
        size_ref(m, &mut bytes, r)?;
        bytes = add(m, bytes, mem::size_of::<ExactRef>())?;
    }
    let owner = a.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut value = c.clone();
    value.authorities = vec![frame.clone()];
    Ok(RootContext {
        value,
        _ownership: Some(owner),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    use serde_json::json;
    fn r(n: u8, kind: &str) -> ExactRef {
        ExactRef {
            kind: kind.into(),
            id: format!("wdoc_{:064x}", n),
            digest: format!("sha256:{:064x}", n),
        }
    }
    #[test]
    fn selected_context_keeps_root_identity_and_owns_pending_copy_until_drop() {
        let frame = AuthorityContextData {
            source_slot: "test:corrector".into(),
            enrollment_ref: r(1, "workflow-source-enrollment"),
            current_state_ref: r(2, "workflow-source-state"),
            current_grant_ref: r(3, "workflow-source-grant"),
            original_grant_refs: vec![r(4, "workflow-source-grant")],
            binding_ref: r(5, "workflow-binding"),
            accepted_agreement_ref: r(6, "workflow-accepted-agreement"),
            activation_ref: r(7, "workflow-activation"),
            authority_observation_ref: Some(r(8, "workflow-authority-prefix-observation")),
        };
        let verifier = r(9, "workflow-authority-audit-verifier");
        let value = json!(verifier);
        let c = DraftContextData {
            key: OperationKey {
                logical_store_id: "store".into(),
                scope: ["scope".into(), "workflow".into()],
                source: "actual-root-source".into(),
                operation_id: "actual-root-operation".into(),
            },
            actor: "actual-root-principal".into(),
            canonical_economic_namespace: "actual-root-namespace".into(),
            installation_mode: "real_local".into(),
            received_at: "2026-10-17T00:00:00.000000Z".into(),
            accepted_at: "2026-10-17T00:00:00.000000Z".into(),
            authorities: vec![frame],
            source_observation_inputs: vec![
                RecordInput {
                    reference: r(8, "workflow-authority-prefix-observation"),
                    canonical_body: vec![b'x'; 4096],
                },
                RecordInput {
                    reference: verifier,
                    canonical_body: vec![b'y'; 1024],
                },
            ],
        };
        // This is a memory-only copy unit; the synthetic bodies do not assert any
        // source/financial origin, authority, valid observation or accepted operation.
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let owned = selected(session.meter_mut(), &c, &c.authorities[0], &value, &[]).unwrap();
        assert_eq!(owned.key, c.key);
        assert_eq!(owned.actor, c.actor);
        assert_eq!(
            owned.canonical_economic_namespace,
            c.canonical_economic_namespace
        );
        assert_eq!(owned.authorities.len(), 1);
        assert_eq!(owned.source_observation_inputs.len(), 2);
        assert_eq!(
            owned.source_observation_inputs[0].canonical_body,
            c.source_observation_inputs[0].canonical_body
        );
        assert!(arena.usage().unwrap().live_bytes >= baseline + 4096 + 1024);
        drop(owned);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
