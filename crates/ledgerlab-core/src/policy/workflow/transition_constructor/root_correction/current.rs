//! Exact retained policy-root explanation. The immediate predecessor is an
//! operand, not a recursive authority/history walk or a source of new prices.
use super::super::{cost_postings, working_value::WorkingKey};
use super::*;
use crate::canonical::audit_arena::AuditedValue;
use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
use std::mem;

pub(super) fn verify(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    plan: &RootPlan,
    evaluation: &Value,
) -> Result<()> {
    let revision = counter(m, &plan.current, "revision")?;
    if meter::cmp(m, &revision, &1.into())?.is_eq() {
        if !shape::same_ref(m, &plan.current_ref, &plan.original_ref)? {
            return Err(shape::fail("initial current root differs from original"));
        }
        // The original verifier above reconstructs the same complete initial
        // explanation, including nonmatch/Unknown/rounded-zero rows.
        return Ok(());
    }
    if !text_is(m, &plan.current, "transition", "quantity_correct")? {
        return Err(shape::fail("current policy root transition"));
    }
    let (prior_ref, prior) = load(
        m,
        r,
        c,
        &plan.current["prior_ref"],
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    shape::closed(
        m,
        &prior,
        &[
            "schema",
            "decision_id",
            "revision",
            "subject_ref",
            "kind",
            "transition",
            "status",
            "original_terms_ref",
            "authority_ref",
            "evaluation_ref",
            "actions_appended",
            "active_action_refs",
            "dependency_refs",
            "operation_key",
            "accepted_at",
        ],
        &["prior_ref"],
    )?;
    for name in ["decision_id", "subject_ref", "kind", "original_terms_ref"] {
        equal(m, &prior[name], &plan.current[name])?;
    }
    let previous = counter(m, &prior, "revision")?;
    let next = meter::add(m, &previous, &1.into())?;
    if !meter::cmp(m, &next, &revision)?.is_eq() {
        return Err(shape::fail("current root immediate predecessor revision"));
    }
    let transition = if meter::cmp(m, &previous, &1.into())?.is_eq() {
        "initial"
    } else {
        "quantity_correct"
    };
    if !text_is(m, &prior, "transition", transition)? {
        return Err(shape::fail("current root predecessor transition"));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning retained root explanation"))?;
    let previous_scalar = cost_postings::scalar(m, &arena, &previous)?;
    let tuple = arena.array_from_values(&[&prior["decision_id"], previous_scalar.value()], m)?;
    let identity = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
    if !shape::same_text(m, &prior_ref.id, &identity)? {
        return Err(shape::fail("retained root predecessor identity"));
    }
    if !text_is(m, &prior, "status", "active")? {
        return Err(shape::fail("current policy predecessor status"));
    }
    let before = crate::domain::Timestamp::parse_with_work(text(m, &prior, "accepted_at")?, m)?;
    let now = crate::domain::Timestamp::parse_with_work(text(m, &plan.current, "accepted_at")?, m)?;
    if meter::cmp(m, &before.micros().into(), &now.micros().into())?.is_gt() {
        return Err(shape::fail("current root predecessor time"));
    }
    let (_, old_eval) = load(
        m,
        r,
        c,
        &prior["evaluation_ref"],
        "workflow-evaluation",
        "ledger-workflow-evaluation-proof/1",
    )?;
    shape::closed(
        m,
        &old_eval,
        &[
            "schema",
            "subject_ref",
            "policy_ref",
            "compiled_plan_ref",
            "compiler_ref",
            "binding_ref",
            "fact_schema_ref",
            "facts",
            "context",
            "steps",
        ],
        &[],
    )?;
    for name in [
        "schema",
        "subject_ref",
        "policy_ref",
        "compiled_plan_ref",
        "compiler_ref",
        "binding_ref",
        "fact_schema_ref",
        "context",
    ] {
        equal(m, &old_eval[name], &evaluation[name])?;
    }
    let schema = original_schema(m, program, &plan.payload["fact_schema_ref"])?;
    let normalized =
        super::super::working_value::NormalizedInput::normalize(m, schema, &old_eval["facts"])?;
    equal(m, normalized.canonical_value(), &old_eval["facts"])?;
    let activity = text(m, &plan.payload, "activity")?;
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let mut quantity = None;
    for instruction in instructions {
        m.visit("root.retained_quantity_field")?;
        if !text_is(m, instruction, "opcode", "evaluate_rule")? {
            continue;
        }
        let rule = shape::field(m, instruction, "rule")?;
        if !text_is(m, rule, "activity", activity)?
            || !text_is(m, &rule["formula"], "kind", "unit")?
        {
            continue;
        }
        let field = text(m, &rule["formula"], "quantity_field")?;
        if quantity.replace(field).is_some() {
            return Err(shape::fail(
                "retained correction requires one Unit producer",
            ));
        }
    }
    let quantity = quantity
        .ok_or_else(|| shape::fail("retained correction lacks original quantity producer"))?;
    for field in shape::array(m, schema.canonical_value(), "fields")? {
        let name = text(m, field, "name")?;
        if shape::same_text(m, name, quantity)? {
            continue;
        }
        for facts in [&old_eval["facts"], &evaluation["facts"]] {
            match (plan.payload["facts"].get(name), facts.get(name)) {
                (Some(original), Some(current)) => equal(m, original, current)?,
                (None, None) => {
                    m.visit("root.retained_unchanged_missing_fact")?;
                }
                _ => return Err(shape::fail("retained correction changed nonquantity fact")),
            }
        }
    }
    // A correction replaces only its named quantity proof. Equal-valued,
    // different assertions for other original fields are not substitutes.
    let (_, original_authority) = load(
        m,
        r,
        c,
        &plan.original["authority_ref"],
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    let (_, current_authority) = load(
        m,
        r,
        c,
        &plan.current["authority_ref"],
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    let current_proofs = shape::array(m, &current_authority, "verified_fact_refs")?;
    for value in shape::array(m, &original_authority, "verified_fact_refs")? {
        let (reference, fact) = load(
            m,
            r,
            c,
            value,
            "workflow-verified-fact",
            "ledger-workflow-verified-fact/1",
        )?;
        let field = text(m, &fact, "field")?;
        if shape::same_text(m, field, quantity)? {
            continue;
        }
        let mut retained = false;
        for current in current_proofs {
            let selected = shape::reference(m, current)?;
            if shape::same_ref(m, &reference, &selected)? {
                retained = true;
            }
        }
        if !retained {
            return Err(shape::fail(
                "retained correction replaced unrelated original proof",
            ));
        }
    }
    let prices = pricing::work(
        program,
        activity,
        (normalized.canonical_value(), &evaluation["context"]),
        m,
    )?;
    let old = active_actions_owned_for(m, r, c, plan, (&prior, &prices))?;
    // New admission remains one independent Fixed/Unit root plus an app whose
    // own decision is separate. A root correction affects this Unit producer;
    // it never creates a replacement target/family or reprices app charges.
    for price in &plan.old_prices {
        if !price.basis_components.is_empty() {
            return Err(shape::fail(
                "retained policy correction requires independent root",
            ));
        }
    }
    let count = w::usize_add(m, old.len(), plan.old_prices.len())?;
    let row = mem::size_of::<AuditedValue>()
        + mem::size_of::<(ConstructedEnvelope, bool)>()
        + 2 * mem::size_of::<&ExactRef>()
        + 64
        + 128
        + 71;
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Multiply,
        &[&count.into(), &row.into()],
    )?;
    let bytes = count
        .checked_mul(row)
        .ok_or_else(|| shape::fail("retained root explanation dimensions"))?;
    let bytes = w::usize_add(m, bytes, 4 * mem::size_of::<Vec<()>>())?;
    let _owner = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut steps = Vec::new();
    steps
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("retained root step rows"))?;
    let mut actions = Vec::new();
    actions
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("retained root action rows"))?;
    let key = WorkingKey::parse(m, &plan.current["operation_key"])?;
    for action in old.iter().rev() {
        let posting = cost_postings::exact_root_inverse_key(
            m,
            &key,
            &action.reference,
            &action.body,
            &prices[action.price_index],
            &revision,
        )?;
        let action = posting
            .action
            .ok_or_else(|| shape::fail("retained root inverse action missing"))?;
        compare_action(m, r, c, &action)?;
        actions.push((action, false));
        steps.push(posting.step);
    }
    let mappings = shape::array(m, &plan.operands.instantiation, "component_map")?;
    let retail = arena.string_value("retail", m)?;
    let empty = arena.array_from_values(&[], m)?;
    let decision_id = text(m, &plan.current, "decision_id")?;
    for price in &plan.old_prices {
        let mapping = shape::named(m, mappings, "template_key", &price.component_key)?;
        let root = shape::named(m, mappings, "template_key", &price.root_key)?;
        let posting = cost_postings::original_cost_posting_key(
            m,
            &key,
            cost_postings::OriginalCostPosting {
                binding: &plan.operands.binding,
                original_work_claim: &plan.claim_ref,
                component_slot: &mapping["slot_id"],
                root_slot: &root["slot_id"],
                book: retail.value(),
                terms_ref: &plan.operands.slot_terms_ref,
                terms: &plan.operands.slot_terms,
                role_ref: &plan.operands.role_ref,
                decision_id,
                revision: &revision,
                replacement: true,
                basis_refs: Some(empty.value()),
            },
            price,
        )?;
        if let Some(action) = posting.action {
            compare_action(m, r, c, &action)?;
            actions.push((action, true));
        }
        steps.push(posting.step);
    }
    let mut views = Vec::new();
    views
        .try_reserve_exact(steps.len())
        .map_err(|_| shape::fail("retained root step views"))?;
    for step in &steps {
        views.push(step.value());
    }
    let expected = arena.array_from_values(&views, m)?;
    equal(m, &evaluation["steps"], expected.value())?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(actions.len())
        .map_err(|_| shape::fail("retained root action views"))?;
    for (action, _) in &actions {
        refs.push(&action.reference);
    }
    let expected = super::super::app_cost::reference_set(m, &arena, &refs)?;
    equal(m, &plan.current["actions_appended"], expected.value())?;
    refs.clear();
    for (action, active) in &actions {
        if *active {
            refs.push(&action.reference);
        }
    }
    let expected = super::super::app_cost::reference_set(m, &arena, &refs)?;
    equal(m, &plan.current["active_action_refs"], expected.value())
}
fn compare_action(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    action: &ConstructedEnvelope,
) -> Result<()> {
    r.get(
        m,
        &c.key,
        &action.reference,
        "workflow-action",
        "ledger-workflow-action/1",
    )?;
    let bytes = r.canonical(m, &action.reference)?;
    if !shape::same_bytes(m, bytes, &action.canonical_body)? {
        return Err(shape::fail("retained root explained action bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
