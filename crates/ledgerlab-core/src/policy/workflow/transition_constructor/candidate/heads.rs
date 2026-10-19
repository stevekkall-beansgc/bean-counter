//! Mechanical head associations of the SAME interpreter output. This is one
//! candidate assembly step, not an economic/resource/origin certificate.
use super::super::{
    arithmetic, data::*, meter, record_body::RecordBody, shape, working_value::owning_reference,
    CoreMeter,
};
use super::{FinancialHeadKey, HeadEffect};
use crate::{
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Result,
};
use std::mem;

pub(in crate::policy::workflow::transition_constructor) struct OwnedHeads {
    pub effects: Vec<HeadEffect>,
    _ownership: ExternalBufferLease,
}
fn add(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    w::usize_add(m, a, b)
}
fn text_size(m: &mut CoreMeter, n: &mut usize, s: &str) -> Result<()> {
    *n = add(m, *n, s.len())?;
    Ok(())
}
fn ref_size(m: &mut CoreMeter, n: &mut usize, r: &ExactRef) -> Result<()> {
    for s in [&r.kind, &r.id, &r.digest] {
        text_size(m, n, s)?;
    }
    Ok(())
}
fn text_is(m: &mut CoreMeter, v: &serde_json::Value, k: &str, wanted: &str) -> Result<bool> {
    let actual = shape::text(m, v, k)?;
    shape::same_text(m, actual, wanted)
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn expected_decision<'a>(
    m: &mut CoreMeter,
    s: &'a DraftStateData,
    id: &str,
) -> Result<Option<&'a DecisionHead>> {
    let mut found = None;
    for h in &s.decision_heads {
        m.visit("candidate.expected_decision")?;
        if shape::same_text(m, &h.decision_id, id)? {
            if found.is_some() {
                return Err(shape::fail("ambiguous expected decision head"));
            }
            found = Some(h);
        }
    }
    Ok(found)
}
fn expected_family<'a>(
    m: &mut CoreMeter,
    s: &'a DraftStateData,
    target: &ExactRef,
    slot: &str,
) -> Result<Option<&'a FamilyHead>> {
    let mut found = None;
    for h in &s.family_heads {
        m.visit("candidate.expected_family")?;
        if shape::same_ref(m, &h.target_ref, target)? && shape::same_text(m, &h.family_slot, slot)?
        {
            if found.is_some() {
                return Err(shape::fail("ambiguous expected family head"));
            }
            found = Some(h);
        }
    }
    Ok(found)
}
pub(super) fn introduced(
    m: &mut CoreMeter,
    t: &Construction,
    r: &ExactRef,
) -> Result<(usize, RecordBody)> {
    let mut found = None;
    for (i, e) in t.new_envelopes.iter().enumerate() {
        m.visit("candidate.head_introduction")?;
        if shape::same_ref(m, &e.reference, r)? {
            if found.is_some() {
                return Err(shape::fail("head result introduced twice"));
            }
            found = Some((i, e));
        }
    }
    let (i, e) = found.ok_or_else(|| shape::fail("proposed head is not newly introduced"))?;
    Ok((i, e.decoded(m, 4194304)?))
}
fn lineage(
    m: &mut CoreMeter,
    arena: &AuditArena,
    value: &serde_json::Value,
    revision: &str,
    old: Option<&ExactRef>,
    old_revision: Option<&str>,
) -> Result<()> {
    let raw = shape::text(m, value, "revision")?;
    if !shape::same_text(m, raw, revision)? {
        return Err(shape::fail("proposed head revision/body"));
    }
    let n = arithmetic::integer(m, raw, false)?;
    if let Some(old) = old {
        let prior = owning_reference(m, arena, old)?;
        shape::equal(m, &value["prior_ref"], prior.value())?;
        if let Some(previous) = old_revision {
            let p = arithmetic::integer(m, previous, false)?;
            let next = meter::add(m, &p, &1.into())?;
            if !meter::cmp(m, &next, &n)?.is_eq() {
                return Err(shape::fail(
                    "proposed head does not increment current revision",
                ));
            }
        }
    } else if value.get("prior_ref").is_some() || !meter::cmp(m, &n, &1.into())?.is_eq() {
        return Err(shape::fail(
            "new head must begin at revision1 without predecessor",
        ));
    }
    Ok(())
}
/// Every output position is derived after interpreter output order is fixed.
/// App claims and generic decisions are independent projections and both are
/// retained; neither a missing head nor this association proves absence.
pub(in crate::policy::workflow::transition_constructor) fn derive(
    m: &mut CoreMeter,
    c: &DraftContextData,
    state: &EconomicStateData,
    t: &Construction,
) -> Result<OwnedHeads> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning candidate head effects"))?;
    let mut count = add(
        m,
        t.proposed_heads.decision.len(),
        t.proposed_heads.family.len(),
    )?;
    if t.proposed_heads.chain.is_some() {
        count = add(m, count, 1)?;
    }
    if let Some(app) = &t.app_effects {
        count = add(m, count, app.heads.len())?;
    }
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Multiply,
        &[&count.into(), &mem::size_of::<HeadEffect>().into()],
    )?;
    let mut bytes = count
        .checked_mul(mem::size_of::<HeadEffect>())
        .ok_or_else(|| shape::fail("head effect payload multiplication"))?;
    bytes = add(m, bytes, mem::size_of::<OwnedHeads>())?;
    for (id, revision, new) in &t.proposed_heads.decision {
        m.visit("candidate.decision_head_size")?;
        text_size(m, &mut bytes, id)?;
        text_size(m, &mut bytes, revision)?;
        ref_size(m, &mut bytes, new)?;
        if let Some(h) = expected_decision(m, state.financial, id)? {
            ref_size(m, &mut bytes, &h.reference)?;
        }
    }
    for (target, slot, revision, new) in &t.proposed_heads.family {
        m.visit("candidate.family_head_size")?;
        ref_size(m, &mut bytes, target)?;
        text_size(m, &mut bytes, slot)?;
        text_size(m, &mut bytes, revision)?;
        ref_size(m, &mut bytes, new)?;
        if let Some(h) = expected_family(m, state.financial, target, slot)? {
            ref_size(m, &mut bytes, &h.reference)?;
        }
    }
    if let Some(new) = &t.proposed_heads.chain {
        m.visit("candidate.chain_head_size")?;
        ref_size(m, &mut bytes, new)?;
        if let Some(old) = &state.financial.chain_head {
            ref_size(m, &mut bytes, old)?;
        }
        let (_, value) = introduced(m, t, new)?;
        for s in [&c.key.logical_store_id, &c.key.scope[0], &c.key.scope[1]] {
            text_size(m, &mut bytes, s)?;
        }
        for k in ["customer", "chain", "revision"] {
            let v = shape::text(m, &value, k)?;
            text_size(m, &mut bytes, v)?;
        }
    }
    if let Some(app) = &t.app_effects {
        for h in &app.heads {
            m.visit("candidate.app_head_size")?;
            ref_size(m, &mut bytes, &h.claim_ref)?;
            ref_size(m, &mut bytes, &h.proposed)?;
            text_size(m, &mut bytes, &h.revision)?;
            if let Some(old) = &h.expected {
                ref_size(m, &mut bytes, old)?;
            }
        }
    }
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let mut effects = Vec::new();
    effects
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("head effect vector allocation"))?;
    for (id, revision, new) in &t.proposed_heads.decision {
        m.visit("candidate.decision_head_effect")?;
        let old = expected_decision(m, state.financial, id)?;
        let (position, value) = introduced(m, t, new)?;
        if !shape::same_text(m, &new.kind, "workflow-decision")? {
            return Err(shape::fail("decision head kind"));
        }
        if !text_is(m, &value, "decision_id", id)? {
            return Err(shape::fail("decision head stable identity"));
        }
        lineage(
            m,
            &arena,
            &value,
            revision,
            old.map(|h| &h.reference),
            old.map(|h| h.revision.as_str()),
        )?;
        effects.push(HeadEffect {
            key: FinancialHeadKey::Decision {
                decision_id: copy(m, id)?,
            },
            expected: old.map(|h| copy_ref(m, &h.reference)).transpose()?,
            proposed: copy_ref(m, new)?,
            revision: copy(m, revision)?,
            introduced_position: position,
        });
    }
    for (target, slot, revision, new) in &t.proposed_heads.family {
        m.visit("candidate.family_head_effect")?;
        let old = expected_family(m, state.financial, target, slot)?;
        let (position, value) = introduced(m, t, new)?;
        let target_value = owning_reference(m, &arena, target)?;
        shape::equal(m, &value["target_ref"], target_value.value())?;
        if !shape::same_text(m, &new.kind, "workflow-family-state")?
            || !text_is(m, &value, "family_slot", slot)?
        {
            return Err(shape::fail("family head identity"));
        }
        lineage(
            m,
            &arena,
            &value,
            revision,
            old.map(|h| &h.reference),
            old.map(|h| h.revision.as_str()),
        )?;
        effects.push(HeadEffect {
            key: FinancialHeadKey::Family {
                target_ref: copy_ref(m, target)?,
                family_slot: copy(m, slot)?,
            },
            expected: old.map(|h| copy_ref(m, &h.reference)).transpose()?,
            proposed: copy_ref(m, new)?,
            revision: copy(m, revision)?,
            introduced_position: position,
        });
    }
    if let Some(new) = &t.proposed_heads.chain {
        m.visit("candidate.chain_head_effect")?;
        let (position, value) = introduced(m, t, new)?;
        let revision = shape::text(m, &value, "revision")?;
        if !shape::same_text(m, &new.kind, "workflow-chain-state")?
            || !text_is(m, &value, "logical_store_id", &c.key.logical_store_id)?
        {
            return Err(shape::fail("chain head store identity"));
        }
        for (i, want) in c.key.scope.iter().enumerate() {
            m.visit("candidate.chain_scope")?;
            let actual = value["scope"][i]
                .as_str()
                .ok_or_else(|| shape::fail("chain head scope"))?;
            if !shape::same_text(m, actual, want)? {
                return Err(shape::fail("chain head scope association"));
            }
        }
        lineage(
            m,
            &arena,
            &value,
            revision,
            state.financial.chain_head.as_ref(),
            None,
        )?;
        let customer = shape::text(m, &value, "customer")?;
        let chain = shape::text(m, &value, "chain")?;
        effects.push(HeadEffect {
            key: FinancialHeadKey::Chain {
                logical_store_id: copy(m, &c.key.logical_store_id)?,
                scope: [copy(m, &c.key.scope[0])?, copy(m, &c.key.scope[1])?],
                customer: copy(m, customer)?,
                chain: copy(m, chain)?,
            },
            expected: state
                .financial
                .chain_head
                .as_ref()
                .map(|r| copy_ref(m, r))
                .transpose()?,
            proposed: copy_ref(m, new)?,
            revision: copy(m, revision)?,
            introduced_position: position,
        });
    }
    if let Some(app) = &t.app_effects {
        for h in &app.heads {
            m.visit("candidate.app_head_effect")?;
            let mut old = None;
            for current in state
                .app
                .ok_or_else(|| shape::fail("app cost head state"))?
                .cost_heads
            {
                m.visit("candidate.expected_app_head")?;
                if shape::same_ref(m, &current.cost_claim_ref, &h.claim_ref)? {
                    if old.is_some() {
                        return Err(shape::fail("ambiguous current app head"));
                    }
                    old = Some(current);
                }
            }
            match (old, &h.expected) {
                (Some(a), Some(b)) => {
                    if !shape::same_ref(m, &a.reference, b)? {
                        return Err(shape::fail("app head expected current Ref"));
                    }
                }
                (None, None) => {}
                _ => return Err(shape::fail("app head expected presence")),
            }
            let (position, value) = introduced(m, t, &h.proposed)?;
            let claim = owning_reference(m, &arena, &h.claim_ref)?;
            shape::equal(m, &value["cost_claim_ref"], claim.value())?;
            if !text_is(m, &value, "schema", "ledger-workflow-app-cost-revision/1")? {
                return Err(shape::fail("app head schema"));
            }
            lineage(
                m,
                &arena,
                &value,
                &h.revision,
                h.expected.as_ref(),
                old.map(|h| h.revision.as_str()),
            )?;
            effects.push(HeadEffect {
                key: FinancialHeadKey::AppCost {
                    cost_claim_ref: copy_ref(m, &h.claim_ref)?,
                },
                expected: h.expected.as_ref().map(|r| copy_ref(m, r)).transpose()?,
                proposed: copy_ref(m, &h.proposed)?,
                revision: copy(m, &h.revision)?,
                introduced_position: position,
            });
        }
    }
    Ok(OwnedHeads {
        effects,
        _ownership: ownership,
    })
}
