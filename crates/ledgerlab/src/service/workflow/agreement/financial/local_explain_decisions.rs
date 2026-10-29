//! Intermediate family decision lineage selection; held-head and full graph
//! verification remain the caller's obligations.
use super::*;
use super::local_explain_family::{body, matches_ref, FamilySelection, Node};
use ledgerlab_core::{canonical::audit_arena::ExternalBufferLease, validation_work::{ArithmeticOperation, OriginalAuditSession, ValidationVisit}};

pub(super) struct DecisionSelection {
    /// Terminal family node and its selected current decision node.
    pub(super) heads: Vec<(usize, usize)>,
    /// Exact family decision revisions, each emitted once.
    pub(super) history: Vec<usize>,
    /// Exact outcome claims referenced by family states.
    pub(super) claims: Vec<usize>,
    _owner: ExternalBufferLease,
}
fn find(nodes: &[Node], value: &Value, kind: &str, s: &mut OriginalAuditSession) -> Result<usize> {
    let (reference, _owner) = current::source::from_value_local(value, s)?;
    request_wrapper::same_text(&reference.kind, kind, s)?;
    let mut result = None;
    for (i, row) in nodes.iter().enumerate() {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        if local_original_work::same_ref(&row.1, &reference, s)? {
            require(result.is_none(), "INTEGRITY_FAILURE")?;
            result = Some(i);
        }
    }
    result.ok_or(ServiceError::IntegrityFailure)
}
fn insert_unique(out: &mut Vec<usize>, i: usize, s: &mut OriginalAuditSession) -> Result<()> {
    for &old in out.iter() {
        s.semantic_work().visit(ValidationVisit::MemberCompare).map_err(core_error)?;
        s.semantic_work().arithmetic(ArithmeticOperation::Compare, &[&old.into(), &i.into()]).map_err(core_error)?;
        if old == i { return Ok(()); }
    }
    out.push(i);
    Ok(())
}
/// Starting with authenticated family-state DATA, follow only literal claim and
/// decision refs. Returns indices, never proof of a current decision head.
pub(super) fn select(nodes: &[Node], families: &FamilySelection, s: &mut OriginalAuditSession) -> Result<DecisionSelection> {
    let bytes = nodes.len().checked_mul(std::mem::size_of::<usize>() * 4).ok_or(ServiceError::Unavailable)?;
    let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut heads = Vec::new(); let mut history = Vec::new(); let mut claims = Vec::new();
    heads.try_reserve_exact(nodes.len()).map_err(|_| ServiceError::Unavailable)?;
    history.try_reserve_exact(nodes.len()).map_err(|_| ServiceError::Unavailable)?;
    claims.try_reserve_exact(nodes.len()).map_err(|_| ServiceError::Unavailable)?;
    require(heads.capacity() <= nodes.len() && history.capacity() <= nodes.len() && claims.capacity() <= nodes.len(), "INTEGRITY_FAILURE")?;
    for &fi in &families.family_history {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        let family = body(nodes.get(fi).ok_or(ServiceError::IntegrityFailure)?, s)?;
        let claim = family.value().get("claim_ref");
        let decision = family.value().get("decision_ref");
        match (claim, decision) {
            (None, None) => continue,
            (Some(c), Some(d)) => {
                let ci = find(nodes, c, "workflow-outcome-claim", s)?;
                let claim_body = body(&nodes[ci], s)?;
                let claim_schema = request_wrapper::text(claim_body.value(), "schema", s)?;
                require(matches!(claim_schema, "ledger-workflow-outcome-claim/1" | "ledger-workflow-outcome-claim/2"), "UNSUPPORTED_SCHEMA_PROFILE")?;
                let target_ref = request_wrapper::field(family.value(), "target_ref", s)?;
                let ti = find(nodes, target_ref, "workflow-target", s)?;
                let target = body(&nodes[ti], s)?;
                require(matches_ref(request_wrapper::field(claim_body.value(), "target_ref", s)?, &nodes[ti].1, s)?, "INTEGRITY_FAILURE")?;
                request_wrapper::same_text(request_wrapper::text(claim_body.value(), "family_slot", s)?, request_wrapper::text(family.value(), "family_slot", s)?, s)?;
                let (binding, _binding_owner) = current::source::from_value_local(request_wrapper::field(target.value(), "binding_ref", s)?, s)?;
                let payload = request_wrapper::field(claim_body.value(), "original_payload", s)?;
                require(matches_ref(request_wrapper::field(payload, "binding_ref", s)?, &binding, s)?, "INTEGRITY_FAILURE")?;
                insert_unique(&mut claims, ci, s)?;
                let mut current = find(nodes, d, "workflow-decision", s)?;
                let original_head = body(&nodes[current], s)?;
                let expected_id = request_wrapper::text(original_head.value(), "decision_id", s)?;
                let mut last_revision = None;
                loop {
                    let node = &nodes[current];
                    let v = body(node, s)?;
                    request_wrapper::same_text(request_wrapper::text(v.value(), "schema", s)?, "ledger-workflow-decision-revision/1", s)?;
                    request_wrapper::same_text(request_wrapper::text(v.value(), "kind", s)?, "outcome", s)?;
                    request_wrapper::same_text(request_wrapper::text(v.value(), "decision_id", s)?, expected_id, s)?;
                    require(matches_ref(request_wrapper::field(v.value(), "original_terms_ref", s)?, &binding, s)?, "INTEGRITY_FAILURE")?;
                    require(matches_ref(request_wrapper::field(v.value(), "subject_ref", s)?, &nodes[ci].1, s)?, "INTEGRITY_FAILURE")?;
                    let rev = super::acceptance::counter(request_wrapper::text(v.value(), "revision", s)?, s)?;
                    if let Some(last) = last_revision {
                        s.semantic_work().arithmetic(ArithmeticOperation::Add, &[&rev.into(), &1u64.into()]).map_err(core_error)?;
                        require(rev.checked_add(1) == Some(last), "INTEGRITY_FAILURE")?;
                    }
                    insert_unique(&mut history, current, s)?;
                    match v.value().get("prior_ref") {
                        None => { require(rev == 1, "INTEGRITY_FAILURE")?; break; }
                        Some(prior) => { require(rev > 1, "INTEGRITY_FAILURE")?; last_revision = Some(rev); current = find(nodes, prior, "workflow-decision", s)?; }
                    }
                }
            }
            _ => return Err(ServiceError::IntegrityFailure),
        }
    }
    // Reject an unlinked extra revision for any selected claim.
    for (i, node) in nodes.iter().enumerate() {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        if node.1.kind != "workflow-decision" { continue; }
        let decision = body(node, s)?;
        let subject = request_wrapper::field(decision.value(), "subject_ref", s)?;
        for &ci in &claims {
            s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
            if matches_ref(subject, &nodes[ci].1, s)? {
                let mut seen = false;
                for &j in &history {
                    s.semantic_work().visit(ValidationVisit::MemberCompare).map_err(core_error)?;
                    s.semantic_work().arithmetic(ArithmeticOperation::Compare, &[&i.into(), &j.into()]).map_err(core_error)?;
                    if i == j { seen = true; break; }
                }
                require(seen, "INTEGRITY_FAILURE")?;
            }
        }
    }
    for &(_, fi) in &families.family_heads {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        let family = body(&nodes[fi], s)?;
        if let Some(d) = family.value().get("decision_ref") {
            heads.push((fi, find(nodes, d, "workflow-decision", s)?));
        }
    }
    Ok(DecisionSelection { heads, history, claims, _owner: owner })
}
