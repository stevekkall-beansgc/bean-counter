//! Exact retained capacity-head/lineage association. Serialized maxima are
//! DATA to compare with the independently rederived prospective proof; they
//! never grant resource permission and this module cannot mint that proof.
use super::super::{
    arithmetic, body, data::*, meter, record_body::RecordBody, records::Records, shape,
    working_value::owning_reference, CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Result,
};
use num_bigint::BigInt;

pub(in crate::policy::workflow::transition_constructor) struct CurrentCapacity {
    pub reference: ExactRef,
    pub body: RecordBody,
    pub revision: BigInt,
    pub reserved: Vec<ExactRef>,
    pub retirements: Vec<ExactRef>,
    _ownership: ExternalBufferLease,
}
fn copy(m: &mut CoreMeter, value: &str) -> Result<String> {
    m.scan_bytes(value.len())?;
    m.write_bytes(value.len())?;
    Ok(value.to_owned())
}
fn copy_ref(m: &mut CoreMeter, value: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &value.kind)?,
        id: copy(m, &value.id)?,
        digest: copy(m, &value.digest)?,
    })
}
/// Compare the complete original structural reservation set before any
/// retirement. Wire record/byte/work maxima are deliberately NOT credited by
/// this check. A retired graph still needs its separate proved after-union.
pub(in crate::policy::workflow::transition_constructor) fn compare_original_structure(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    original: &super::original::FinancialReservations,
    current: &CurrentCapacity,
) -> Result<bool> {
    if m.check("capacity.original_graph_has_retirements", || {
        !current.retirements.is_empty()
    })? {
        return Ok(false);
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning original capacity comparison"))?;
    let expected = w::usize_add(m, original.families.len(), original.delayed_costs.len())?;
    let expected = w::usize_add(m, expected, original.app_costs.len())?;
    if !w::compare(m, current.reserved.len(), expected)?.is_eq() {
        return Err(shape::fail(
            "original capacity reservation set is incomplete",
        ));
    }
    let binding = owning_reference(m, &arena, &original.original_binding_ref)?;
    let root = r.get(
        m,
        &c.key,
        &original.original_root_ref,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    let ancestor = arena.string_value(&original.root_decision_id, m)?;
    let _scratch = arena.reserve_external_payload(5 * std::mem::size_of::<String>() + 512, m)?;
    for family in &original.families {
        m.visit("capacity.compare_original_family_edge")?;
        original_edge(
            m,
            r,
            c,
            current,
            ancestor.value(),
            &family.dependent_decision_id,
            "outcome_family",
            &family.target_ref,
            Some(&family.family_slot),
            binding.value(),
            &root["operation_key"],
            family.maximum_inverse_actions,
        )?;
    }
    for cost in &original.delayed_costs {
        m.visit("capacity.compare_original_supplier_edge")?;
        original_edge(
            m,
            r,
            c,
            current,
            ancestor.value(),
            &cost.dependent_decision_id,
            "delayed_cost",
            &cost.target_ref,
            None,
            binding.value(),
            &root["operation_key"],
            cost.maximum_inverse_actions,
        )?;
    }
    for cost in &original.app_costs {
        m.visit("capacity.compare_original_app_edge")?;
        original_edge(
            m,
            r,
            c,
            current,
            ancestor.value(),
            &cost.dependent_decision_id,
            "delayed_cost",
            &cost.target_ref,
            None,
            binding.value(),
            &root["operation_key"],
            cost.maximum_inverse_actions,
        )?;
    }
    let union = original.structural_union(&arena, m)?;
    compare_structural_fields(
        m,
        &current.body,
        union.decision_ids.len(),
        union.maximum_appended_actions,
    )?;
    Ok(true)
}
fn compare_structural_fields(
    m: &mut CoreMeter,
    capacity: &serde_json::Value,
    decisions: usize,
    actions: usize,
) -> Result<()> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning structural capacity scalars"))?;
    let affected = super::super::cost_postings::scalar(m, &arena, &decisions.into())?;
    let maximum = super::super::cost_postings::scalar(m, &arena, &actions.into())?;
    shape::equal(m, &capacity["affected_decisions"], affected.value())?;
    shape::equal(m, &capacity["maximum_actions"], maximum.value())
}
fn original_edge(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    current: &CurrentCapacity,
    ancestor: &serde_json::Value,
    dependent: &str,
    kind: &str,
    target: &ExactRef,
    family: Option<&str>,
    binding: &serde_json::Value,
    key: &serde_json::Value,
    actions: usize,
) -> Result<()> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning original dependency comparison"))?;
    let dependent = arena.string_value(dependent, m)?;
    let tuple = arena.array_from_values(&[ancestor, dependent.value(), binding], m)?;
    let id = body::semantic_id(m, "wdep_", "workflow-dependency", tuple.value())?;
    let mut selected = None;
    for reference in &current.reserved {
        m.visit("capacity.original_dependency_exact_set_member")?;
        if shape::same_text(m, &reference.id, &id)? {
            if selected.is_some() {
                return Err(shape::fail(
                    "duplicate original capacity dependency identity",
                ));
            }
            selected = Some(reference);
        }
    }
    let reference =
        selected.ok_or_else(|| shape::fail("missing original dependency reservation"))?;
    let schema = shape::text(m, &current.body, "schema")?;
    let local = shape::same_text(m, schema, "ledger-correction-capacity/2")?;
    let value = r.get(
        m,
        &c.key,
        reference,
        "workflow-dependency",
        if local {
            "ledger-workflow-dependency/2"
        } else {
            "ledger-workflow-dependency/1"
        },
    )?;
    let fields = &[
        "schema",
        "ancestor_decision_id",
        "dependent_decision_id",
        "kind",
        "target_ref",
        "original_terms_ref",
        "maximum_actions",
        "maximum_records",
        "maximum_bytes",
        "maximum_work",
        "operation_key",
    ];
    if local {
        shape::closed(
            m,
            &value,
            &[
                "schema",
                "ancestor_decision_id",
                "dependent_decision_id",
                "kind",
                "target_ref",
                "original_terms_ref",
                "maximum_actions",
                "operation_key",
            ],
            if family.is_some() {
                &["family_slot"]
            } else {
                &[]
            },
        )?;
    } else {
        shape::closed(
            m,
            &value,
            fields,
            if family.is_some() {
                &["family_slot"]
            } else {
                &[]
            },
        )?;
    }
    for (field, want) in [
        ("ancestor_decision_id", ancestor),
        ("dependent_decision_id", dependent.value()),
        ("original_terms_ref", binding),
        ("operation_key", key),
    ] {
        shape::equal(m, &value[field], want)?;
    }
    let actual_kind = shape::text(m, &value, "kind")?;
    if !shape::same_text(m, actual_kind, kind)? {
        return Err(shape::fail("original dependency kind"));
    }
    let target = owning_reference(m, &arena, target)?;
    shape::equal(m, &value["target_ref"], target.value())?;
    if let Some(family) = family {
        let actual = shape::text(m, &value, "family_slot")?;
        if !shape::same_text(m, actual, family)? {
            return Err(shape::fail("original dependency family slot"));
        }
    }
    let actions = super::super::cost_postings::scalar(m, &arena, &actions.into())?;
    shape::equal(m, &value["maximum_actions"], actions.value())?;
    Ok(())
}
/// A retained family/app operation affects its original work ancestor even
/// when the monetary producer itself is not a root correction. Resolve that
/// ancestor's actual held decision/capacity before numerical production; a
/// current unrelated cost/family head cannot stand in for the work root.
pub(in crate::policy::workflow::transition_constructor) fn retained_original_head(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    original_root: &ExactRef,
    root_id: &str,
    state: &EconomicStateData,
) -> Result<CurrentCapacity> {
    let mut selected = None;
    for head in &state.financial.decision_heads {
        m.visit("capacity.retained_ancestor_decision_head")?;
        if shape::same_text(m, &head.decision_id, root_id)? {
            if selected.is_some() {
                return Err(shape::fail("ambiguous original ancestor decision head"));
            }
            selected = Some(head);
        }
    }
    let head = selected
        .ok_or_else(|| shape::fail("original work ancestor held decision head is missing"))?;
    let value = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    let revision = shape::text(m, &value, "revision")?;
    if !shape::same_text(m, revision, &head.revision)? {
        return Err(shape::fail(
            "original ancestor head revision differs from body",
        ));
    }
    retained_root_head(m, r, c, original_root, &head.reference, state)
}
/// root_ref is ALWAYS the actual original revision1 root, independently of
/// today's root/head/CAS and independently of the stable capacity counter.
pub(in crate::policy::workflow::transition_constructor) fn retained_root_head(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    original_root: &ExactRef,
    current_root: &ExactRef,
    state: &EconomicStateData,
) -> Result<CurrentCapacity> {
    retained_root_head_mode(m, r, c, original_root, current_root, state, false)
}
pub(in crate::policy::workflow::transition_constructor) fn retained_local_root_head(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    original_root: &ExactRef,
    current_root: &ExactRef,
    state: &EconomicStateData,
) -> Result<CurrentCapacity> {
    retained_root_head_mode(m, r, c, original_root, current_root, state, true)
}
fn retained_root_head_mode(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    original_root: &ExactRef,
    current_root: &ExactRef,
    state: &EconomicStateData,
    local: bool,
) -> Result<CurrentCapacity> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning capacity head"))?;
    let original = r.get(
        m,
        &c.key,
        original_root,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    let first = shape::text(m, &original, "revision")?;
    if !shape::same_text(m, first, "1")? {
        return Err(shape::fail("capacity root is not original revision1"));
    }
    let current = r.get(
        m,
        &c.key,
        current_root,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    shape::equal(m, &current["decision_id"], &original["decision_id"])?;
    shape::equal(m, &current["subject_ref"], &original["subject_ref"])?;
    shape::equal(
        m,
        &current["original_terms_ref"],
        &original["original_terms_ref"],
    )?;
    let root_id = shape::text(m, &original, "decision_id")?;
    let mut found = None;
    for head in state.capacity_heads {
        m.visit("capacity.current_stable_root_head")?;
        if shape::same_text(m, &head.root_decision_id, root_id)? {
            if found.is_some() {
                return Err(shape::fail("duplicate current capacity root head"));
            }
            found = Some(head);
        }
    }
    let head = found.ok_or_else(|| shape::fail("current root capacity is missing"))?;
    let capacity = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-correction-capacity",
        if local {
            "ledger-correction-capacity/2"
        } else {
            "ledger-correction-capacity/1"
        },
    )?;
    if local {
        shape::closed(
            m,
            &capacity,
            &[
                "schema",
                "root_ref",
                "revision",
                "dependency_root",
                "affected_decisions",
                "maximum_actions",
                "reserved_dependency_refs",
                "retirement_refs",
            ],
            &[],
        )?;
    } else {
        shape::closed(
            m,
            &capacity,
            &[
                "schema",
                "root_ref",
                "revision",
                "dependency_root",
                "affected_decisions",
                "maximum_actions",
                "maximum_records",
                "maximum_bytes",
                "maximum_work",
                "reserved_dependency_refs",
                "retirement_refs",
            ],
            &[],
        )?;
    }
    let root = owning_reference(m, &arena, original_root)?;
    shape::equal(m, &capacity["root_ref"], root.value())?;
    let counter = shape::text(m, &capacity, "revision")?;
    if !shape::same_text(m, counter, &head.revision)? {
        return Err(shape::fail("capacity current head counter"));
    }
    let revision = arithmetic::integer(m, counter, false)?;
    if !meter::cmp(m, &revision, &0.into())?.is_gt() {
        return Err(shape::fail("positive capacity revision"));
    }
    let revision_value = super::super::cost_postings::scalar(m, &arena, &revision)?;
    let root_id_value = arena.string_value(root_id, m)?;
    let seed = arena.array_from_values(&[root_id_value.value(), revision_value.value()], m)?;
    let id = body::semantic_id(
        m,
        "wcapacity_",
        "workflow-correction-capacity",
        seed.value(),
    )?;
    if !shape::same_text(m, &id, &head.reference.id)? {
        return Err(shape::fail("stable capacity identity/counter"));
    }
    for (name, minimum, maximum) in [
        ("affected_decisions", 1u64, 32u64),
        ("maximum_actions", 0, 128),
        ("maximum_records", 0, 4096),
        ("maximum_bytes", 0, 8388608),
        ("maximum_work", 0, 65536),
    ] {
        if local && matches!(name, "maximum_records" | "maximum_bytes" | "maximum_work") {
            continue;
        }
        let raw = shape::text(m, &capacity, name)?;
        let value = arithmetic::integer(m, raw, false)?;
        if meter::cmp(m, &value, &minimum.into())?.is_lt()
            || meter::cmp(m, &value, &maximum.into())?.is_gt()
        {
            return Err(shape::fail("retained capacity exceeds original domain"));
        }
    }
    let reserved = shape::array(m, &capacity, "reserved_dependency_refs")?;
    let retired = shape::array(m, &capacity, "retirement_refs")?;
    if w::compare(m, reserved.len(), 128usize)?.is_gt()
        || w::compare(m, retired.len(), 128usize)?.is_gt()
    {
        return Err(shape::fail("capacity reservation/retirement count"));
    }
    let n = w::usize_add(m, reserved.len(), retired.len())?;
    let per = std::mem::size_of::<ExactRef>() + 64 + 128 + 71 + std::mem::size_of::<&ExactRef>();
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&n.into(), &per.into()])?;
    let bytes = n
        .checked_mul(per)
        .ok_or_else(|| shape::fail("capacity reference payload product"))?;
    let bytes = w::usize_add(
        m,
        bytes,
        std::mem::size_of::<CurrentCapacity>() + 64 + 128 + 71,
    )?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let mut reserved_refs = Vec::new();
    reserved_refs
        .try_reserve_exact(reserved.len())
        .map_err(|_| shape::fail("capacity reservation references"))?;
    for value in reserved {
        m.visit("capacity.retained_reservation_ref")?;
        let reference = shape::reference(m, value)?;
        let dependency = r.get(
            m,
            &c.key,
            &reference,
            "workflow-dependency",
            if local {
                "ledger-workflow-dependency/2"
            } else {
                "ledger-workflow-dependency/1"
            },
        )?;
        let _ = dependency;
        reserved_refs.push(reference);
    }
    let mut retired_refs = Vec::new();
    retired_refs
        .try_reserve_exact(retired.len())
        .map_err(|_| shape::fail("capacity retirement references"))?;
    for value in retired {
        m.visit("capacity.retained_retirement_ref")?;
        let reference = shape::reference(m, value)?;
        let retirement = r.get(
            m,
            &c.key,
            &reference,
            "workflow-capacity-retirement",
            "ledger-workflow-capacity-retirement/1",
        )?;
        shape::closed(
            m,
            &retirement,
            &[
                "schema",
                "dependency_ref",
                "closure_state_refs",
                "reason",
                "authority_ref",
                "operation_key",
            ],
            &[],
        )?;
        let reason = shape::text(m, &retirement, "reason")?;
        if !shape::same_text(m, reason, "no_future_or_active_dependency")? {
            return Err(shape::fail("capacity retirement reason"));
        }
        retired_refs.push(reference);
    }
    // The retained complete sets must already equal their canonical sorted form;
    // normalization here must not silently repair an originally invalid record.
    let mut borrowed = Vec::new();
    borrowed
        .try_reserve_exact(reserved_refs.len())
        .map_err(|_| shape::fail("capacity sorted reservation view"))?;
    for value in &reserved_refs {
        m.visit("capacity.sorted_reservation_view")?;
        borrowed.push(value);
    }
    let normalized = super::super::app_cost::reference_set(m, &arena, &borrowed)?;
    shape::equal(m, &capacity["reserved_dependency_refs"], normalized.value())?;
    let digest = super::super::records::hash(m, "workflow-dependency-set", normalized.value())?;
    let stored = shape::text(m, &capacity, "dependency_root")?;
    if !shape::same_text(m, stored, &digest)? {
        return Err(shape::fail("capacity dependency root digest"));
    }
    borrowed.clear();
    borrowed
        .try_reserve_exact(retired_refs.len())
        .map_err(|_| shape::fail("capacity sorted retirement view"))?;
    for value in &retired_refs {
        m.visit("capacity.sorted_retirement_view")?;
        borrowed.push(value);
    }
    let normalized = super::super::app_cost::reference_set(m, &arena, &borrowed)?;
    shape::equal(m, &capacity["retirement_refs"], normalized.value())?;
    let reference = copy_ref(m, &head.reference)?;
    Ok(CurrentCapacity {
        reference,
        body: capacity,
        revision,
        reserved: reserved_refs,
        retirements: retired_refs,
        _ownership: ownership,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    use serde_json::json;
    fn key() -> OperationKey {
        OperationKey {
            logical_store_id: "store".into(),
            scope: ["org".into(), "scope".into()],
            source: "source".into(),
            operation_id: "original".into(),
        }
    }
    fn context() -> DraftContextData {
        DraftContextData {
            key: key(),
            actor: "principal".into(),
            canonical_economic_namespace: "namespace".into(),
            installation_mode: "real_local".into(),
            received_at: "2026-10-17T00:00:00Z".into(),
            accepted_at: "2026-10-17T00:00:00Z".into(),
            authorities: vec![],
            source_observation_inputs: vec![],
        }
    }
    fn envelope(
        m: &mut CoreMeter,
        revision: &str,
    ) -> super::super::super::emitted_record::ConstructedEnvelope {
        let id = body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!(["decision", revision]),
        )
        .unwrap();
        body::record(m,&key(),"workflow-decision",id,json!({"schema":"ledger-workflow-decision-revision/1","decision_id":"decision","revision":revision,"subject_ref":{"kind":"workflow-work","id":"work","digest":format!("sha256:{}","1".repeat(64))},"original_terms_ref":{"kind":"workflow-binding","id":"binding","digest":format!("sha256:{}","2".repeat(64))}})).unwrap()
    }
    // Association-only unit: no origin/currentness/authentication is asserted.
    // The retained capacity revision belongs to the stable root decision, while
    // its full root Ref stays revision1 even after the financial head advances.
    #[test]
    fn original_root_ref_persists_while_capacity_counter_advances() {
        let mut build = CoreMeter::new(65536).unwrap();
        let first = envelope(&mut build, "1");
        let current = envelope(&mut build, "2");
        for (wrong, wrong_actions) in [(false, false), (true, false), (false, true)] {
            let digest = super::super::super::records::hash(
                &mut build,
                "workflow-dependency-set",
                &json!([]),
            )
            .unwrap();
            let id = body::semantic_id(
                &mut build,
                "wcapacity_",
                "workflow-correction-capacity",
                &json!(["decision", "3"]),
            )
            .unwrap();
            let capacity=body::record(&mut build,&key(),"workflow-correction-capacity",id,json!({"schema":"ledger-correction-capacity/1","root_ref":if wrong{&current.reference}else{&first.reference},"revision":"3","dependency_root":digest,"affected_decisions":"1","maximum_actions":if wrong_actions{"1"}else{"0"},"maximum_records":"0","maximum_bytes":"0","maximum_work":"0","reserved_dependency_refs":[],"retirement_refs":[]})).unwrap();
            let inputs = [
                EconomicRecordView {
                    reference: &first.reference,
                    canonical_body: &first.canonical_body,
                },
                EconomicRecordView {
                    reference: &current.reference,
                    canonical_body: &current.canonical_body,
                },
                EconomicRecordView {
                    reference: &capacity.reference,
                    canonical_body: &capacity.canonical_body,
                },
            ];
            let heads = [CapacityHead {
                root_decision_id: "decision".into(),
                revision: "3".into(),
                reference: capacity.reference.clone(),
            }];
            let financial = DraftStateData {
                decision_heads: vec![DecisionHead {
                    decision_id: "decision".into(),
                    revision: "2".into(),
                    reference: current.reference.clone(),
                }],
                ..DraftStateData::default()
            };
            let state = EconomicStateData {
                retained_assertion: None,
                financial: &financial,
                capacity_heads: &heads,
                supplier: None,
                app: None,
                current_party_authorizations: &[],
                observation: None,
                audit: None,
                initial_source_correspondences: &[],
            };
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut child = session.meter_mut().original_economic_meter().unwrap();
            let c = context();
            let mut records =
                Records::borrowed_app(&mut child, &arena, &c.key, &inputs, &[]).unwrap();
            let result = retained_original_head(
                &mut child,
                &mut records,
                &c,
                &first.reference,
                "decision",
                &state,
            );
            if wrong {
                assert!(result.is_err());
            } else {
                let result = result.unwrap();
                assert_eq!(result.revision, BigInt::from(3));
                assert_eq!(result.body["root_ref"], json!(first.reference));
                // A valid hash, in-range scalar and held head do not prove the stored
                // structural maxima. Compare the independently derived zero-edge union.
                let union =
                    super::super::topology::union(&arena, &mut child, "decision", 0, &[]).unwrap();
                let equality = compare_structural_fields(
                    &mut child,
                    &result.body,
                    union.decision_ids.len(),
                    union.maximum_appended_actions,
                );
                if wrong_actions {
                    assert!(equality.is_err());
                    session.invalidate();
                } else {
                    equality.unwrap();
                    session.meter_mut().finish_original_economic(child).unwrap();
                }
            }
        }
    }
}
