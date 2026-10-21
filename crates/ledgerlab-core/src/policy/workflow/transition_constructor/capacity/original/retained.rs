//! Retained-original operands for the SAME reservation derivation. This uses
//! actual revision-one work/claim/decision/target bodies, never stored maxima
//! or a synthetic emitted envelope. Earlier origin authentication stays in the
//! chronological caller; every body is resolved again in this economic child.
use super::super::super::{
    data::DraftContextData, record_body::RecordBody, records::Records, work_keys,
    working_value::owning_reference,
};
use super::*;

/// Resolve the original work root for a retained family/app executor. The
/// current request's cost/family decision is never substituted for that root.
fn derive_for_work_with<T>(
    arena: &AuditArena,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected_operands: (&Value, &super::super::super::authority::Terms),
    finish: Derive<T>,
) -> Result<T> {
    let (work, terms) = selected_operands;
    let scratch =
        2 * (mem::size_of::<ExactRef>() + 64 + 128 + 71) + 2 * (mem::size_of::<String>() + 128);
    let _metadata = arena.reserve_external_payload(scratch, m)?;
    m.write_bytes(scratch)?;
    let payload = shape::field(m, work, "payload")?;
    let policy = super::super::super::working_value::parse_owned_reference(
        m,
        arena,
        &payload["policy_ref"],
    )?;
    let expected = program.root_policy_ref();
    for (actual, want) in [
        (policy.reference().kind.as_str(), expected.kind()),
        (policy.reference().id.as_str(), expected.id()),
        (policy.reference().digest.as_str(), expected.digest()),
    ] {
        if !shape::same_text(m, actual, want)? {
            return Err(crate::Error::new(
                "ORIGINAL_VERIFIER_UNAVAILABLE",
                "original root requires its own exact compiled program",
            ));
        }
    }
    let claim_id = shape::text(m, work, "claim_id")?;
    m.scan_bytes(64 + 128 + 71)?;
    let (claim_ref, claim) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    let work_ref =
        super::super::super::working_value::parse_owned_reference(m, arena, &claim["work_ref"])?;
    let selected = r.get(
        m,
        &c.key,
        work_ref.reference(),
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::equal(m, &selected, work)?;
    let decision_id = work_keys::decision(m, &claim_ref.id)?;
    let decision_id_value = arena.string_value(&decision_id, m)?;
    let one = arena.string_value("1", m)?;
    let seed = arena.array_from_values(&[decision_id_value.value(), one.value()], m)?;
    let revision_id = super::super::super::body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        seed.value(),
    )?;
    m.scan_bytes(64 + 128 + 71)?;
    let (decision_ref, decision) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &revision_id,
        "ledger-workflow-decision-revision/1",
    )?;
    for (field, want) in [
        ("decision_id", decision_id.as_str()),
        ("revision", "1"),
        ("kind", "work"),
        ("transition", "initial"),
    ] {
        let actual = shape::text(m, &decision, field)?;
        if !shape::same_text(m, actual, want)? {
            return Err(shape::fail("retained original work decision identity"));
        }
    }
    derive_retained_with(
        arena,
        m,
        r,
        c,
        program,
        (
            &decision_ref,
            &decision,
            work_ref.reference(),
            work,
            &claim_ref,
            terms,
        ),
        finish,
    )
}

fn derive_retained_with<T>(
    arena: &AuditArena,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected_operands: (
        &ExactRef,
        &Value,
        &ExactRef,
        &Value,
        &ExactRef,
        &super::super::super::authority::Terms,
    ),
    finish: Derive<T>,
) -> Result<T> {
    let (decision_ref, decision, work_ref, work, claim_ref, terms) = selected_operands;
    let payload = shape::field(m, work, "payload")?;
    let claim = r.get(
        m,
        &c.key,
        claim_ref,
        "workflow-work-claim",
        "ledger-workflow-work-claim/1",
    )?;
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let mut maximum = 0usize;
    for instruction in instructions {
        m.visit("capacity.retained_target_dimension")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_family")? {
            continue;
        }
        if shape::same(
            m,
            &instruction["family"]["selector"]["target_activity"],
            &payload["activity"],
        )? {
            maximum = add(m, maximum, 1)?;
        }
    }
    if w::compare(m, maximum, 128usize)?.is_gt() {
        return Err(shape::fail("retained original target dimension"));
    }
    let per = mem::size_of::<(ExactRef, RecordBody)>() + 64 + 128 + 71;
    let bytes = mul(m, maximum, per)?;
    let bytes = add(
        m,
        bytes,
        mem::size_of::<Vec<(ExactRef, RecordBody)>>() + 128 + mem::size_of::<String>(),
    )?;
    let _metadata = arena.reserve_external_payload(bytes, m)?;
    // Includes vector controls and each bounded locator clone before any
    // allocation/refusal. Semantic field/Ref checks still execute below.
    m.write_bytes(bytes)?;
    let mut targets = Vec::new();
    targets
        .try_reserve_exact(maximum)
        .map_err(|_| shape::fail("retained original target views"))?;
    let work_value = owning_reference(m, arena, work_ref)?;
    let claim_value = owning_reference(m, arena, claim_ref)?;
    let decision_value = owning_reference(m, arena, decision_ref)?;
    let original_key = shape::field(m, work, "key")?;
    let source_scope = shape::field(m, original_key, "scope")?;
    let scope = work_keys::scope(m, &c.key)?;
    shape::equal(m, source_scope, scope.value())?;
    for instruction in instructions {
        m.visit("capacity.retained_original_target")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_family")? {
            continue;
        }
        let selector = shape::field(m, &instruction["family"], "selector")?;
        if !shape::same(m, &selector["target_activity"], &payload["activity"])? {
            continue;
        }
        let basis = shape::text(m, selector, "basis")?;
        if !shape::same_text(m, basis, "booked_original_net")? {
            return Err(shape::fail("retained original target basis"));
        }
        let component = shape::text(m, selector, "component_key")?;
        let mappings = shape::array(m, &terms.instantiation, "component_map")?;
        let mapping = shape::named(m, mappings, "template_key", component)?;
        let slot = shape::text(m, mapping, "slot_id")?;
        let components = shape::array(m, &terms.catalog, "components")?;
        let declared = shape::named(m, components, "slot_id", slot)?;
        let root = shape::text(m, declared, "root_slot")?;
        if !shape::same_text(m, root, slot)? {
            return Err(shape::fail("retained family target must be direct root"));
        }
        let target_id = work_keys::target(
            m,
            &c.key,
            &terms.binding["agreement_id"],
            &claim_ref.id,
            slot,
        )?;
        let mut selected = false;
        for (prior, _) in &targets {
            let prior: &ExactRef = prior;
            m.visit("capacity.retained_target_unique")?;
            if shape::same_text(m, &prior.id, &target_id)? {
                selected = true;
                break;
            }
        }
        if selected {
            continue;
        }
        // Records::by_identity returns one exact full Ref only when the
        // identity is unambiguous, and then runs the actual typed body lookup.
        // Its bounded String clone has prepaid storage above and scans here.
        m.scan_bytes(64 + 128 + 71)?;
        let (reference, target) = r.by_identity(
            m,
            &c.key,
            "workflow-target",
            &target_id,
            "ledger-workflow-component-target/1",
        )?;
        for (field, want) in [
            ("work_ref", work_value.value()),
            ("work_claim_ref", claim_value.value()),
            ("original_decision_ref", decision_value.value()),
            ("binding_ref", &payload["binding_ref"]),
            ("policy_ref", &payload["policy_ref"]),
            ("product_ref", &payload["product_ref"]),
            ("scope", source_scope),
            ("agreement_id", &terms.binding["agreement_id"]),
        ] {
            let actual = shape::field(m, &target, field)?;
            shape::equal(m, actual, want)?;
        }
        let actual_slot = shape::text(m, &target, "root_component_slot")?;
        if !shape::same_text(m, actual_slot, slot)? {
            return Err(shape::fail("retained original target component"));
        }
        let plan = program.compiled_plan().reference();
        let expected = super::super::super::working_value::parse_owned_reference(
            m,
            arena,
            &target["compiled_plan_ref"],
        )?;
        for (actual, want) in [
            (expected.reference().kind.as_str(), plan.kind()),
            (expected.reference().id.as_str(), plan.id()),
            (expected.reference().digest.as_str(), plan.digest()),
        ] {
            if !shape::same_text(m, actual, want)? {
                return Err(shape::fail("retained original target program"));
            }
        }
        targets.push((reference, target));
    }
    finish(
        arena,
        m,
        r,
        c,
        program,
        payload,
        (
            decision_ref,
            decision,
            &targets,
            work_ref,
            work,
            claim_ref,
            &claim,
            terms,
        ),
    )
}

type Derive<T> = fn(
    &AuditArena,
    &mut CoreMeter,
    &mut Records,
    &DraftContextData,
    &SelectedProgram,
    &Value,
    (
        &ExactRef,
        &Value,
        &[(ExactRef, RecordBody)],
        &ExactRef,
        &Value,
        &ExactRef,
        &Value,
        &super::super::super::authority::Terms,
    ),
) -> Result<T>;
pub(in crate::policy::workflow::transition_constructor) fn derive_for_work(
    a: &AuditArena,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &SelectedProgram,
    w: &Value,
    t: &super::super::super::authority::Terms,
) -> Result<OriginalReservations> {
    derive_for_work_with(a, m, r, c, p, (w, t), super::derive_data)
}
pub(in crate::policy::workflow::transition_constructor) fn derive_financial_for_work(
    a: &AuditArena,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &SelectedProgram,
    w: &Value,
    t: &super::super::super::authority::Terms,
) -> Result<FinancialReservations> {
    derive_for_work_with(a, m, r, c, p, (w, t), super::derive_financial_data)
}
pub(in crate::policy::workflow::transition_constructor) fn derive_retained(
    a: &AuditArena,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &SelectedProgram,
    d: &ExactRef,
    selected_operands: (
        &Value,
        &ExactRef,
        &Value,
        &ExactRef,
        &super::super::super::authority::Terms,
    ),
) -> Result<OriginalReservations> {
    let (dv, w, wv, cl, t) = selected_operands;
    derive_retained_with(a, m, r, c, p, (d, dv, w, wv, cl, t), super::derive_data)
}
pub(in crate::policy::workflow::transition_constructor) fn derive_financial_retained(
    a: &AuditArena,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &SelectedProgram,
    d: &ExactRef,
    selected_operands: (
        &Value,
        &ExactRef,
        &Value,
        &ExactRef,
        &super::super::super::authority::Terms,
    ),
) -> Result<FinancialReservations> {
    let (dv, w, wv, cl, t) = selected_operands;
    derive_retained_with(
        a,
        m,
        r,
        c,
        p,
        (d, dv, w, wv, cl, t),
        super::derive_financial_data,
    )
}
