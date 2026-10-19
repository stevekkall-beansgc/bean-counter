//! Mechanical projection of actual constructed dependency/capacity records.
//! This does not establish the prospective envelope or authenticate a head.
use super::super::{
    arithmetic, body, data::*, meter, record_body::RecordBody, shape,
    working_value::owning_reference, CoreMeter,
};
use super::{DependencyEffect, DependencyKind, FinancialHeadKey, HeadEffect};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Result,
};
use std::mem;
pub(super) struct OwnedReservations {
    pub heads: Vec<HeadEffect>,
    pub dependencies: Vec<DependencyEffect>,
    _ownership: ExternalBufferLease,
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
fn id<'a>(m: &mut CoreMeter, v: &'a serde_json::Value, k: &str) -> Result<&'a str> {
    let s = shape::text(m, v, k)?;
    if w::compare(m, s.len(), 128usize)?.is_gt() {
        return Err(shape::fail("capacity projection identity length"));
    }
    Ok(s)
}
fn original_root(
    m: &mut CoreMeter,
    c: &DraftContextData,
    t: &mut Construction,
    reference: &ExactRef,
) -> Result<RecordBody> {
    let mut found = None;
    for row in &t.new_envelopes {
        m.visit("candidate.capacity_original_root_output")?;
        if shape::same_ref(m, &row.reference, reference)? {
            if found.is_some() {
                return Err(shape::fail("duplicate original root output"));
            }
            found = Some(row.decoded(m, 4194304)?);
        }
    }
    let value = match found {
        Some(v) => v,
        None => t.records.get(
            m,
            &c.key,
            reference,
            "workflow-decision",
            "ledger-workflow-decision-revision/1",
        )?,
    };
    let schema = shape::text(m, &value, "schema")?;
    if !shape::same_text(m, &reference.kind, "workflow-decision")?
        || !shape::same_text(m, schema, "ledger-workflow-decision-revision/1")?
    {
        return Err(shape::fail("capacity original root schema"));
    }
    let revision = shape::text(m, &value, "revision")?;
    if !shape::same_text(m, revision, "1")? {
        return Err(shape::fail("capacity original root is not revision1"));
    }
    Ok(value)
}
pub(super) fn derive(
    m: &mut CoreMeter,
    c: &DraftContextData,
    state: &EconomicStateData,
    t: &mut Construction,
    local: bool,
) -> Result<OwnedReservations> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning capacity projection"))?;
    let mut capacities = 0usize;
    let mut dependencies = 0usize;
    for row in &t.new_envelopes {
        m.visit("candidate.reservation_dimensions")?;
        if shape::same_text(m, &row.reference.kind, "workflow-correction-capacity")? {
            capacities = w::usize_add(m, capacities, 1)?;
        } else if shape::same_text(m, &row.reference.kind, "workflow-dependency")? {
            dependencies = w::usize_add(m, dependencies, 1)?;
        }
    }
    // Ref and ID grammar bounds are checked before copying. Include temporary
    // parsed Refs and tuple/hex String controls overlapping retained effects.
    let n = w::usize_add(m, capacities, dependencies)?;
    let per = mem::size_of::<HeadEffect>()
        + mem::size_of::<DependencyEffect>()
        + 6 * (mem::size_of::<ExactRef>() + 64 + 128 + 71)
        + 5 * 128
        + 8 * mem::size_of::<String>();
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&n.into(), &per.into()])?;
    let bytes = n
        .checked_mul(per)
        .ok_or_else(|| shape::fail("capacity projection payload product"))?;
    let bytes = w::usize_add(m, bytes, mem::size_of::<OwnedReservations>())?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let mut heads = Vec::new();
    heads
        .try_reserve_exact(capacities)
        .map_err(|_| shape::fail("capacity head effects"))?;
    let mut effects = Vec::new();
    effects
        .try_reserve_exact(dependencies)
        .map_err(|_| shape::fail("dependency effects"))?;
    for position in 0..t.new_envelopes.len() {
        m.visit("candidate.capacity_effect")?;
        if !shape::same_text(
            m,
            &t.new_envelopes[position].reference.kind,
            "workflow-correction-capacity",
        )? {
            continue;
        }
        let value = t.new_envelopes[position].decoded(m, 4194304)?;
        let schema = shape::text(m, &value, "schema")?;
        if !shape::same_text(
            m,
            schema,
            if local {
                "ledger-correction-capacity/2"
            } else {
                "ledger-correction-capacity/1"
            },
        )? {
            return Err(shape::fail("capacity output schema"));
        }
        let root_ref = shape::reference(m, &value["root_ref"])?;
        super::super::records::reference(m, &root_ref)?;
        let root = original_root(m, c, t, &root_ref)?;
        let root_id = id(m, &root, "decision_id")?;
        for prior in &heads {
            let prior: &HeadEffect = prior;
            if let FinancialHeadKey::Capacity { root_decision_id } = prior.key() {
                if shape::same_text(m, root_decision_id, root_id)? {
                    return Err(shape::fail("multiple capacity revisions for one root"));
                }
            }
        }
        let revision = id(m, &value, "revision")?;
        let number = arithmetic::integer(m, revision, false)?;
        let mut expected = None;
        for h in state.capacity_heads {
            m.visit("candidate.expected_capacity_head")?;
            if shape::same_text(m, &h.root_decision_id, root_id)? {
                if expected.is_some() {
                    return Err(shape::fail("ambiguous held capacity head"));
                }
                expected = Some(h);
            }
        }
        if let Some(old) = expected {
            let before = arithmetic::integer(m, &old.revision, false)?;
            let next = meter::add(m, &before, &1.into())?;
            if !meter::cmp(m, &next, &number)?.is_eq() {
                return Err(shape::fail("capacity counter must advance current head"));
            }
            let old_value = t.records.get(
                m,
                &c.key,
                &old.reference,
                "workflow-correction-capacity",
                if local {
                    "ledger-correction-capacity/2"
                } else {
                    "ledger-correction-capacity/1"
                },
            )?;
            let old_revision = shape::text(m, &old_value, "revision")?;
            if !shape::same_text(m, old_revision, &old.revision)? {
                return Err(shape::fail("held capacity revision/body"));
            }
            shape::equal(m, &old_value["root_ref"], &value["root_ref"])?;
        } else if !meter::cmp(m, &number, &1.into())?.is_eq() {
            return Err(shape::fail("new capacity counter must begin at1"));
        }
        let a = arena.string_value(root_id, m)?;
        let b = arena.string_value(revision, m)?;
        let seed = arena.array_from_values(&[a.value(), b.value()], m)?;
        let wanted = body::semantic_id(
            m,
            "wcapacity_",
            "workflow-correction-capacity",
            seed.value(),
        )?;
        if !shape::same_text(m, &wanted, &t.new_envelopes[position].reference.id)? {
            return Err(shape::fail("capacity effect stable identity"));
        }
        heads.push(HeadEffect {
            key: FinancialHeadKey::Capacity {
                root_decision_id: copy(m, root_id)?,
            },
            expected: expected.map(|h| copy_ref(m, &h.reference)).transpose()?,
            proposed: copy_ref(m, &t.new_envelopes[position].reference)?,
            revision: copy(m, revision)?,
            introduced_position: position,
        });
    }
    for row in &t.new_envelopes {
        m.visit("candidate.dependency_effect")?;
        if !shape::same_text(m, &row.reference.kind, "workflow-dependency")? {
            continue;
        }
        let value = row.decoded(m, 4194304)?;
        let schema = shape::text(m, &value, "schema")?;
        if !shape::same_text(
            m,
            schema,
            if local {
                "ledger-workflow-dependency/2"
            } else {
                "ledger-workflow-dependency/1"
            },
        )? {
            return Err(shape::fail("dependency output schema"));
        }
        let ancestor = id(m, &value, "ancestor_decision_id")?;
        let dependent = id(m, &value, "dependent_decision_id")?;
        let target = shape::reference(m, &value["target_ref"])?;
        let terms = shape::reference(m, &value["original_terms_ref"])?;
        super::super::records::reference(m, &target)?;
        super::super::records::reference(m, &terms)?;
        let kind = shape::text(m, &value, "kind")?;
        let (kind, family) = if shape::same_text(m, kind, "outcome_family")? {
            (
                DependencyKind::OutcomeFamily,
                Some(id(m, &value, "family_slot")?),
            )
        } else if shape::same_text(m, kind, "delayed_cost")? {
            (DependencyKind::DelayedCost, None)
        } else if shape::same_text(m, kind, "work_basis")? {
            (DependencyKind::WorkBasis, None)
        } else {
            return Err(shape::fail("dependency effect kind"));
        };
        let a = arena.string_value(ancestor, m)?;
        let b = arena.string_value(dependent, m)?;
        let terms_value = owning_reference(m, &arena, &terms)?;
        let tuple = arena.array_from_values(&[a.value(), b.value(), terms_value.value()], m)?;
        let wanted = body::semantic_id(m, "wdep_", "workflow-dependency", tuple.value())?;
        if !shape::same_text(m, &wanted, &row.reference.id)? {
            return Err(shape::fail("dependency effect identity"));
        }
        let mut capacity = None;
        for head in &heads {
            m.visit("candidate.dependency_ancestor_capacity")?;
            if let FinancialHeadKey::Capacity { root_decision_id } = head.key() {
                if !shape::same_text(m, root_decision_id, ancestor)? {
                    continue;
                }
                let cap = t.new_envelopes[head.introduced_position()].decoded(m, 4194304)?;
                let reference = owning_reference(m, &arena, &row.reference)?;
                let mut member = false;
                for reserved in shape::array(m, &cap, "reserved_dependency_refs")? {
                    m.visit("candidate.dependency_capacity_membership")?;
                    if shape::same(m, reserved, reference.value())? {
                        if member {
                            return Err(shape::fail("duplicate capacity dependency member"));
                        }
                        member = true;
                    }
                }
                if member {
                    if capacity.is_some() {
                        return Err(shape::fail("ambiguous dependency ancestor capacity"));
                    }
                    capacity = Some(head.proposed());
                }
            }
        }
        let capacity = capacity
            .ok_or_else(|| shape::fail("new dependency lacks its exact ancestor capacity"))?;
        effects.push(DependencyEffect {
            record_ref: copy_ref(m, &row.reference)?,
            ancestor_decision_id: copy(m, ancestor)?,
            dependent_decision_id: copy(m, dependent)?,
            kind,
            target_ref: target,
            family_slot: family.map(|s| copy(m, s)).transpose()?,
            original_terms_ref: terms,
            capacity_ref: copy_ref(m, capacity)?,
        });
    }
    Ok(OwnedReservations {
        heads,
        dependencies: effects,
        _ownership: ownership,
    })
}
