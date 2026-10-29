//! Intermediate Component target/family selection. This does not certify an
//! explanation: the caller must verify held heads and all other graph edges.
use super::*;
use ledgerlab_core::{canonical::audit_arena::{AuditedBytes, ExternalBufferLease}, policy::workflow::transition_constructor::ExactRef, validation_work::{ArithmeticOperation, OriginalAuditSession, RepresentationKind, ValidationVisit}};

pub(super) type Node = (i64, ExactRef, AuditedBytes, ExternalBufferLease);
pub(super) struct FamilySelection {
    pub(super) targets: Vec<usize>,
    /// Each pair identifies a target and its terminal family-state node.
    pub(super) family_heads: Vec<(usize, usize)>,
    pub(super) family_history: Vec<usize>,
    _owner: ExternalBufferLease,
}
pub(super) fn body(node: &Node, s: &mut OriginalAuditSession) -> Result<ledgerlab_core::canonical::audit_arena::AuditedValue> {
    s.parse_representation(node.2.bytes(), RepresentationKind::Record).map_err(core_error)
}
pub(super) fn matches_ref(value: &Value, reference: &ExactRef, s: &mut OriginalAuditSession) -> Result<bool> {
    let (actual, _lease) = current::source::from_value_local(value, s)?;
    local_original_work::same_ref(&actual, reference, s)
}
/// Select all declared families of every target that names this exact Work.
/// Every revision must occur exactly once in one contiguous prior chain. The
/// returned terminal nodes are DATA until the host compares the held heads.
pub(super) fn select(nodes: &[Node], subject: &ExactRef, s: &mut OriginalAuditSession) -> Result<FamilySelection> {
    let bytes = nodes.len().checked_mul(std::mem::size_of::<usize>() * 4).ok_or(ServiceError::Unavailable)?;
    let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut targets = Vec::new();
    let mut heads = Vec::new();
    let mut history = Vec::new();
    targets.try_reserve_exact(nodes.len()).map_err(|_| ServiceError::Unavailable)?;
    heads.try_reserve_exact(nodes.len()).map_err(|_| ServiceError::Unavailable)?;
    history.try_reserve_exact(nodes.len()).map_err(|_| ServiceError::Unavailable)?;
    require(targets.capacity() <= nodes.len() && heads.capacity() <= nodes.len() && history.capacity() <= nodes.len(), "INTEGRITY_FAILURE")?;
    for (ti, target) in nodes.iter().enumerate() {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        if target.1.kind != "workflow-target" { continue; }
        let v = body(target, s)?;
        request_wrapper::same_text(request_wrapper::text(v.value(), "schema", s)?, "ledger-workflow-component-target/1", s)?;
        if !matches_ref(request_wrapper::field(v.value(), "work_ref", s)?, subject, s)? { continue; }
        targets.push(ti);
        let families = request_wrapper::field(v.value(), "families", s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
        for (position, family) in families.iter().enumerate() {
            s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
            let slot = request_wrapper::text(family, "family_slot", s)?;
            for prior in &families[..position] {
                let p = request_wrapper::text(prior, "family_slot", s)?;
                s.semantic_work().arithmetic(ArithmeticOperation::Add, &[&slot.len().into(), &p.len().into()]).map_err(core_error)?;
                s.semantic_work().scan_bytes(slot.len().checked_add(p.len()).ok_or(ServiceError::Unavailable)?).map_err(core_error)?;
                require(slot != p, "INTEGRITY_FAILURE")?;
            }
            let mut expected = 1u64;
            let mut previous: Option<usize> = None;
            loop {
                let mut found = None;
                for (i, n) in nodes.iter().enumerate() {
                    s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
                    if n.1.kind != "workflow-family-state" { continue; }
                    let b = body(n, s)?;
                    let schema = request_wrapper::text(b.value(), "schema", s)?;
                    require(matches!(schema, "ledger-workflow-family-state/1" | "ledger-workflow-family-state/2"), "UNSUPPORTED_SCHEMA_PROFILE")?;
                    if !matches_ref(request_wrapper::field(b.value(), "target_ref", s)?, &target.1, s)? { continue; }
                    let actual_slot = request_wrapper::text(b.value(), "family_slot", s)?;
                    s.semantic_work().arithmetic(ArithmeticOperation::Add, &[&slot.len().into(), &actual_slot.len().into()]).map_err(core_error)?;
                    s.semantic_work().scan_bytes(slot.len().checked_add(actual_slot.len()).ok_or(ServiceError::Unavailable)?).map_err(core_error)?;
                    if slot != actual_slot { continue; }
                    let rev = super::acceptance::counter(request_wrapper::text(b.value(), "revision", s)?, s)?;
                    if rev != expected { continue; }
                    require(found.is_none(), "INTEGRITY_FAILURE")?;
                    if let Some(anchor) = b.value().get("anchor_work_ref") { require(matches_ref(anchor, subject, s)?, "INTEGRITY_FAILURE")?; }
                    match (previous, b.value().get("prior_ref")) {
                        (None, None) => (),
                        (Some(p), Some(r)) => require(matches_ref(r, &nodes[p].1, s)?, "INTEGRITY_FAILURE")?,
                        _ => return Err(ServiceError::IntegrityFailure),
                    }
                    found = Some(i);
                }
                let Some(index) = found else { break; };
                history.push(index);
                previous = Some(index);
                s.semantic_work().arithmetic(ArithmeticOperation::Add, &[&expected.into(), &1u64.into()]).map_err(core_error)?;
                expected = expected.checked_add(1).ok_or(ServiceError::Unavailable)?;
            }
            heads.push((ti, previous.ok_or(ServiceError::IntegrityFailure)?));
        }
    }
    require(!targets.is_empty(), "UNSUPPORTED_SCHEMA_PROFILE")?;
    // Ensure no extra family rows on selected targets (including skipped or
    // out-of-order revisions) can be mistaken for a complete chain.
    for (i, n) in nodes.iter().enumerate() {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        if n.1.kind != "workflow-family-state" { continue; }
        let b = body(n, s)?;
        for &ti in &targets {
            s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
            if matches_ref(request_wrapper::field(b.value(), "target_ref", s)?, &nodes[ti].1, s)? {
                let mut present = false;
                for &h in &history {
                    s.semantic_work().visit(ValidationVisit::MemberCompare).map_err(core_error)?;
                    s.semantic_work().arithmetic(ArithmeticOperation::Compare, &[&h.into(), &i.into()]).map_err(core_error)?;
                    if h == i { present = true; break; }
                }
                require(present, "INTEGRITY_FAILURE")?;
            }
        }
    }
    Ok(FamilySelection { targets, family_heads: heads, family_history: history, _owner: owner })
}
