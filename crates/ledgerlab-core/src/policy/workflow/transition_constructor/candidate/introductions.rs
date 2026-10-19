//! Mechanical permanent-claim/link effects derived from the SAME interpreter's
//! actual immutable introductions. No JSON supplied by the host selects keys.
use super::super::{data::*, record_body::RecordBody, shape, CoreMeter};
use super::{heads::introduced, ClaimEffect, LinkEffect, PermanentClaimKey};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Result,
};
use std::mem;
pub(in crate::policy::workflow::transition_constructor) struct OwnedIntroductions {
    pub claims: Vec<ClaimEffect>,
    pub links: Vec<LinkEffect>,
    _ownership: ExternalBufferLease,
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn field(m: &mut CoreMeter, v: &serde_json::Value, k: &str) -> Result<String> {
    let s = shape::text(m, v, k)?;
    copy(m, s)
}
fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn body(m: &mut CoreMeter, t: &Construction, reference: &ExactRef) -> Result<RecordBody> {
    Ok(introduced(m, t, reference)?.1)
}
fn multiply(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("candidate introduction metadata multiplication"))
}
fn sized_text(m: &mut CoreMeter, n: &mut usize, v: &serde_json::Value, k: &str) -> Result<()> {
    let s = shape::text(m, v, k)?;
    *n = w::usize_add(m, *n, s.len())?;
    Ok(())
}
fn sized_ref(m: &mut CoreMeter, n: &mut usize, v: &serde_json::Value) -> Result<()> {
    for k in ["kind", "id", "digest"] {
        sized_text(m, n, v, k)?;
    }
    Ok(())
}
fn sized_scope(m: &mut CoreMeter, n: &mut usize, v: &serde_json::Value) -> Result<()> {
    for value in shape::array(m, v, "scope")? {
        m.visit("candidate.scope_payload_size")?;
        let s = value.as_str().ok_or_else(|| shape::fail("scope string"))?;
        *n = w::usize_add(m, *n, s.len())?;
    }
    Ok(())
}
fn scope(m: &mut CoreMeter, c: &DraftContextData, v: &serde_json::Value) -> Result<[String; 2]> {
    let values = shape::array(m, v, "scope")?;
    if !w::compare(m, values.len(), 2usize)?.is_eq() {
        return Err(shape::fail("candidate introduction scope length"));
    }
    for (actual, want) in values.iter().zip(&c.key.scope) {
        m.visit("candidate.introduction_scope")?;
        let actual = actual
            .as_str()
            .ok_or_else(|| shape::fail("candidate scope string"))?;
        if !shape::same_text(m, actual, want)? {
            return Err(shape::fail("candidate introduction scope association"));
        }
    }
    Ok([copy(m, &c.key.scope[0])?, copy(m, &c.key.scope[1])?])
}
fn unique(m: &mut CoreMeter, refs: &[ExactRef]) -> Result<()> {
    for (i, r) in refs.iter().enumerate() {
        m.visit("candidate.unique_introduction")?;
        for prior in &refs[..i] {
            m.visit("candidate.introduction_identity")?;
            if shape::same_ref(m, prior, r)? {
                return Err(shape::fail("duplicate candidate introduction effect"));
            }
        }
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn derive(
    m: &mut CoreMeter,
    c: &DraftContextData,
    t: &Construction,
) -> Result<OwnedIntroductions> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning candidate introductions"))?;
    unique(m, &t.claim_introductions)?;
    unique(m, &t.link_introductions)?;
    let a = multiply(
        m,
        t.claim_introductions.len(),
        mem::size_of::<ClaimEffect>(),
    )?;
    let b = multiply(m, t.link_introductions.len(), mem::size_of::<LinkEffect>())?;
    let mut bytes = w::usize_add(m, a, b)?;
    bytes = w::usize_add(m, bytes, mem::size_of::<OwnedIntroductions>())?;
    for reference in t.claim_introductions.iter().chain(&t.link_introductions) {
        m.visit("candidate.introduction_payload_size")?;
        let value = body(m, t, reference)?;
        // Size precisely the selected typed fields BEFORE any String/Ref copy;
        // the immutable original payload is borrowed, never cloned here.
        sized_scope(m, &mut bytes, &value)?;
        if shape::same_text(m, &reference.kind, "workflow-work-claim")? {
            for k in [
                "logical_store_id",
                "source_namespace",
                "business_operation_id",
                "completion",
            ] {
                sized_text(m, &mut bytes, &value, k)?;
            }
        } else if shape::same_text(m, &reference.kind, "workflow-outcome-claim")? {
            for k in [
                "agreement_id",
                "family_slot",
                "source_namespace",
                "business_outcome_token",
            ] {
                sized_text(m, &mut bytes, &value, k)?;
            }
            sized_text(m, &mut bytes, &value["target_ref"], "id")?;
        } else if shape::same_text(m, &reference.kind, "workflow-link")? {
            sized_text(m, &mut bytes, &value, "slot")?;
            for k in ["child", "predecessor", "product_ref", "binding_ref"] {
                sized_ref(m, &mut bytes, &value[k])?;
            }
        } else {
            return Err(shape::fail("unknown candidate introduction dimension"));
        }
        for s in [&reference.kind, &reference.id, &reference.digest] {
            bytes = w::usize_add(m, bytes, s.len())?;
        }
    }
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let mut claims = Vec::new();
    claims
        .try_reserve_exact(t.claim_introductions.len())
        .map_err(|_| shape::fail("candidate claim effect vector"))?;
    let mut links = Vec::new();
    links
        .try_reserve_exact(t.link_introductions.len())
        .map_err(|_| shape::fail("candidate link effect vector"))?;
    for reference in &t.claim_introductions {
        m.visit("candidate.claim_effect")?;
        let value = body(m, t, reference)?;
        let key = if shape::same_text(m, &reference.kind, "workflow-work-claim")? {
            let s = shape::text(m, &value, "schema")?;
            if !shape::same_text(m, s, "ledger-workflow-work-claim/1")? {
                return Err(shape::fail("work claim effect schema"));
            }
            let store = shape::text(m, &value, "logical_store_id")?;
            if !shape::same_text(m, store, &c.key.logical_store_id)? {
                return Err(shape::fail("work claim effect store"));
            }
            PermanentClaimKey::Work {
                logical_store_id: copy(m, store)?,
                scope: scope(m, c, &value)?,
                source_namespace: field(m, &value, "source_namespace")?,
                business_operation_id: field(m, &value, "business_operation_id")?,
                completion: field(m, &value, "completion")?,
            }
        } else if shape::same_text(m, &reference.kind, "workflow-outcome-claim")? {
            let s = shape::text(m, &value, "schema")?;
            if !shape::same_text(m, s, "ledger-workflow-outcome-claim/1")? {
                return Err(shape::fail("outcome claim effect schema"));
            }
            let target = shape::field(m, &value, "target_ref")?;
            let target = shape::text(m, target, "id")?;
            PermanentClaimKey::Outcome {
                scope: scope(m, c, &value)?,
                agreement_id: field(m, &value, "agreement_id")?,
                family_slot: field(m, &value, "family_slot")?,
                target_id: copy(m, target)?,
                source_namespace: field(m, &value, "source_namespace")?,
                business_outcome_token: field(m, &value, "business_outcome_token")?,
            }
        } else {
            return Err(shape::fail("unknown ordinary permanent claim introduction"));
        };
        claims.push(ClaimEffect::derived(key, copy_ref(m, reference)?));
    }
    for reference in &t.link_introductions {
        m.visit("candidate.link_effect")?;
        let value = body(m, t, reference)?;
        let s = shape::text(m, &value, "schema")?;
        if !shape::same_text(m, &reference.kind, "workflow-link")?
            || !shape::same_text(m, s, "ledger-workflow-link/1")?
        {
            return Err(shape::fail("link effect schema/kind"));
        }
        links.push(LinkEffect {
            record_ref: copy_ref(m, reference)?,
            scope: scope(m, c, &value)?,
            child_ref: shape::reference(m, &value["child"])?,
            slot: field(m, &value, "slot")?,
            predecessor_ref: shape::reference(m, &value["predecessor"])?,
            product_ref: shape::reference(m, &value["product_ref"])?,
            binding_ref: shape::reference(m, &value["binding_ref"])?,
        });
    }
    Ok(OwnedIntroductions {
        claims,
        links,
        _ownership: ownership,
    })
}
