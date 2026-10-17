//! Distinct-decision closure over validated original monetary reservations.
//! Structural counts alone are NOT a resource proof or a capacity record.
//! Every family stays in the union regardless of unclaimed/none/zero/expired;
//! only a separately proved permanent retirement may omit an edge upstream.
use super::super::{CoreMeter, meter, shape};
use crate::{
    Error, Result,
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
};
use std::mem;

pub(in crate::policy::workflow::transition_constructor) struct Reservation<'a> {
    pub ancestor_decision_id: &'a str,
    pub dependent_decision_id: &'a str,
    /// Maximum ACTIVE actions needing inverse when an ancestor is corrected.
    /// A family's exclusive alternatives contribute MAX, not their sum.
    pub maximum_inverse_actions: usize,
}
pub(in crate::policy::workflow::transition_constructor) struct Union<'a> {
    pub decision_ids: Vec<&'a str>,
    pub maximum_appended_actions: usize,
    _workspace: ExternalBufferLease,
}
fn exceeded(s: &str) -> Error {
    Error::new("CORRECTION_CAPACITY_EXCEEDED", s)
}
fn add(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Add, &[&a.into(), &b.into()])?;
    a.checked_add(b)
        .ok_or_else(|| exceeded("capacity addition"))
}
fn mul(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| exceeded("capacity allocation product"))
}
fn contains(m: &mut CoreMeter, values: &[&str], wanted: &str) -> Result<bool> {
    for value in values {
        m.visit("capacity.decision_identity")?;
        if shape::same_text(m, value, wanted)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn visit<'a>(
    m: &mut CoreMeter,
    id: &'a str,
    reservations: &[Reservation<'a>],
    active: &mut Vec<&'a str>,
    finished: &mut Vec<&'a str>,
) -> Result<()> {
    m.visit("capacity.enter_decision")?;
    if contains(m, active, id)? {
        return Err(shape::fail("monetary dependency cycle"));
    }
    if contains(m, finished, id)? {
        return Ok(());
    }
    if !meter::cmp(m, &active.len().into(), &32usize.into())?.is_lt() {
        return Err(exceeded("dependency stack exceeds affected-decision bound"));
    }
    m.write_bytes(mem::size_of::<&str>())?;
    active.push(id);
    for edge in reservations {
        m.visit("capacity.outgoing_reservation")?;
        if shape::same_text(m, edge.ancestor_decision_id, id)? {
            visit(
                m,
                edge.dependent_decision_id,
                reservations,
                active,
                finished,
            )?;
        }
    }
    m.visit("capacity.leave_decision")?;
    active.pop();
    if !meter::cmp(m, &finished.len().into(), &32usize.into())?.is_lt() {
        return Err(exceeded("too many affected decisions including root"));
    }
    m.write_bytes(mem::size_of::<&str>())?;
    finished.push(id);
    Ok(())
}
/// The caller derives root action maxima from every permitted affected-root
/// variant of its ORIGINAL program (including reverse) and validates every
/// reservation against its original target/terms. This function never accepts
/// serialized caller graph/capacity assertions and grants no admission result.
pub(in crate::policy::workflow::transition_constructor) fn union<'a>(
    arena: &AuditArena,
    m: &mut CoreMeter,
    root_id: &'a str,
    root_inverse_and_replacement_actions: usize,
    reservations: &[Reservation<'a>],
) -> Result<Union<'a>> {
    if meter::cmp(m, &reservations.len().into(), &128usize.into())?.is_gt() {
        return Err(exceeded("reserved dependency count"));
    }
    // Three bounded borrowed-ID vectors: active recursion, completed union,
    // and maximum inverse metadata. No copied String or unbounded stack.
    let pointers = mul(m, 64, mem::size_of::<&str>())?;
    let actions = mul(m, 32, mem::size_of::<usize>())?;
    let headers = mul(m, 3, mem::size_of::<Vec<usize>>())?;
    let bytes = add(m, pointers, actions)?;
    let bytes = add(m, bytes, headers)?;
    let workspace = arena.reserve_external_payload(bytes, m)?;
    let mut active = Vec::new();
    active
        .try_reserve_exact(32)
        .map_err(|_| exceeded("capacity active allocation"))?;
    let mut finished = Vec::new();
    finished
        .try_reserve_exact(32)
        .map_err(|_| exceeded("capacity union allocation"))?;
    visit(m, root_id, reservations, &mut active, &mut finished)?;
    let mut maximum_inverse = Vec::new();
    maximum_inverse
        .try_reserve_exact(finished.len())
        .map_err(|_| exceeded("capacity effect allocation"))?;
    for id in &finished {
        m.visit("capacity.distinct_decision_effects")?;
        let mut maximum = 0;
        if !shape::same_text(m, id, root_id)? {
            for edge in reservations {
                m.visit("capacity.incoming_effect_bound")?;
                if shape::same_text(m, edge.dependent_decision_id, id)?
                    && meter::cmp(m, &edge.maximum_inverse_actions.into(), &maximum.into())?.is_gt()
                {
                    maximum = edge.maximum_inverse_actions;
                }
            }
        }
        m.write_bytes(mem::size_of::<usize>())?;
        maximum_inverse.push(maximum);
    }
    let mut actions = root_inverse_and_replacement_actions;
    for maximum in maximum_inverse {
        m.visit("capacity.distinct_inverse_actions")?;
        actions = add(m, actions, maximum)?;
        if meter::cmp(m, &actions.into(), &128usize.into())?.is_gt() {
            return Err(exceeded("maximum appended correction actions"));
        }
    }
    // Even a dependency-free root obeys128 (the loop above may be empty).
    if meter::cmp(m, &actions.into(), &128usize.into())?.is_gt() {
        return Err(exceeded("maximum root correction actions"));
    }
    // Deterministic structural identity ordering; no semantic clone or BTree
    // ordering hides comparisons, and all physical moves are charged.
    for i in 1..finished.len() {
        let mut at = i;
        while w::compare(m, at, 0usize)?.is_gt() {
            let previous = w::usize_sub(m, at, 1)?;
            let order =
                shape::byte_order(m, finished[previous].as_bytes(), finished[at].as_bytes())?;
            if !order.is_gt() {
                break;
            }
            m.visit("capacity.union_sort_move")?;
            let bytes = mul(m, 2, mem::size_of::<&str>())?;
            m.scan_bytes(bytes)?;
            m.write_bytes(bytes)?;
            finished.swap(previous, at);
            at = previous;
        }
    }
    Ok(Union {
        decision_ids: finished,
        maximum_appended_actions: actions,
        _workspace: workspace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn union_counts_shared_and_none_prospective_decisions_once_and_refuses_cycles() {
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let rows = [
            Reservation {
                ancestor_decision_id: "root",
                dependent_decision_id: "family-a",
                maximum_inverse_actions: 1,
            },
            Reservation {
                ancestor_decision_id: "root",
                dependent_decision_id: "family-b",
                maximum_inverse_actions: 1,
            },
            Reservation {
                ancestor_decision_id: "family-a",
                dependent_decision_id: "shared",
                maximum_inverse_actions: 2,
            },
            Reservation {
                ancestor_decision_id: "family-b",
                dependent_decision_id: "shared",
                maximum_inverse_actions: 2,
            },
        ];
        let mut m = CoreMeter::new(65536).unwrap();
        let result = union(&arena, &mut m, "root", 4, &rows).unwrap();
        assert_eq!(
            result.decision_ids,
            vec!["family-a", "family-b", "root", "shared"]
        );
        assert_eq!(result.maximum_appended_actions, 8);
        drop(result);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let selected = [Reservation {
            ancestor_decision_id: "root",
            dependent_decision_id: "unclaimed-or-none-family",
            maximum_inverse_actions: 1,
        }];
        let result = union(&arena, &mut m, "root", 4, &selected).unwrap();
        assert_eq!(result.decision_ids.len(), 2);
        assert_eq!(result.maximum_appended_actions, 5);
        drop(result);
        let cycle = [
            Reservation {
                ancestor_decision_id: "root",
                dependent_decision_id: "family",
                maximum_inverse_actions: 1,
            },
            Reservation {
                ancestor_decision_id: "family",
                dependent_decision_id: "root",
                maximum_inverse_actions: 1,
            },
        ];
        assert!(union(&arena, &mut m, "root", 4, &cycle).is_err());
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
