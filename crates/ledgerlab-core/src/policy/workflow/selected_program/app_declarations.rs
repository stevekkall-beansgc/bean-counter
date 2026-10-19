//! Source-derived app cost declarations in the SAME selected compiler. This
//! validates original policy promises; it grants no evidence/origin/admission.
use super::super::{
    transition_constructor::{arithmetic, meter, CoreMeter},
    FactSchema, Product,
};
use super::{
    compiler::same_literal as same,
    model::{Formula, Policy},
    validate::name,
};
use crate::{validation_work as w, Error, Result};
fn fail(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
pub(super) fn validate(
    m: &mut CoreMeter,
    p: &Policy,
    product: &Product,
    _schemas: &[FactSchema],
) -> Result<()> {
    if !p.delayed_costs.is_empty() {
        return Err(fail("app capability cannot promise supplier delayed costs"));
    }
    let declarations = p
        .app_delayed_costs
        .as_ref()
        .ok_or_else(|| fail("app declarations required"))?;
    if w::compare(m, declarations.len(), 1usize)?.is_lt()
        || w::compare(m, declarations.len(), 64usize)?.is_gt()
    {
        return Err(fail("app declaration bounds"));
    }
    for (index, declaration) in declarations.iter().enumerate() {
        m.visit("app.original_declaration")?;
        for value in [
            &declaration.component_key,
            &declaration.activity,
            &declaration.resource_funding_leg,
            &declaration.usage_field,
            &declaration.usage_unit,
            &declaration.work_source_slot,
            &declaration.cost_source_slot,
            &declaration.correction_source_slot,
        ] {
            name(m, value)?;
        }
        for (actual, expected) in [
            (&declaration.cost_basis, "app_attributed_final_amount"),
            (
                &declaration.comparable_quantity_rule,
                "when_present_lte_original_observed",
            ),
            (&declaration.allocation, "one_call_one_component"),
            (&declaration.on_missing, "wait"),
        ] {
            if !same(m, actual, expected)? {
                return Err(fail("closed original app cost basis"));
            }
        }
        let anchored = same(m, &p.schema, "ledger-workflow-policy/4")?;
        if anchored {
            let anchor = declaration
                .correction_anchor
                .as_deref()
                .ok_or_else(|| fail("original correction anchor required"))?;
            if !same(m, anchor, "initial_cost_accepted_at")? {
                return Err(fail("original app correction anchor"));
            }
            if let Some(clause) = &declaration.causal_reversal {
                for (actual, expected) in [
                    (
                        &clause.root_relation,
                        "original_work_delayed_cost_dependency",
                    ),
                    (&clause.mode, "exact_inverse_and_permanent_close"),
                    (
                        &clause.standalone_deadline_effect,
                        "does_not_expire_preaccepted_causal_reversal",
                    ),
                ] {
                    if !same(m, actual, expected)? {
                        return Err(fail("closed app causal reversal clause"));
                    }
                }
            }
        } else if declaration.correction_anchor.is_some() || declaration.causal_reversal.is_some() {
            return Err(fail("new timing requires new original compiler recipe"));
        }
        for previous in &declarations[..index] {
            m.visit("app.original_declaration_uniqueness")?;
            if same(m, &previous.component_key, &declaration.component_key)?
                && same(m, &previous.activity, &declaration.activity)?
            {
                return Err(fail("duplicate app component/activity declaration"));
            }
        }
        arithmetic::integer(m, &declaration.finalize_within_us, false)?;
        arithmetic::integer(m, &declaration.correction_within_us, false)?;
        let amount = &declaration.maximum_final_amount;
        if !same(m, &amount.currency, "USD")? || !w::compare(m, amount.scale, 18u8)?.is_eq() {
            return Err(fail("app maximum amount units"));
        }
        let maximum = arithmetic::integer(m, &amount.atoms, false)?;
        let amount_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
        if meter::cmp(m, &maximum, &0.into())?.is_lt()
            || meter::cmp(m, &maximum, &amount_limit)?.is_gt()
        {
            return Err(fail("app maximum final amount"));
        }
        let activity = product
            .activity_with_work(&declaration.activity, m)?
            .ok_or_else(|| fail("app original activity"))?;
        if !same(m, activity.source_slot(), &declaration.work_source_slot)? {
            return Err(fail("app original work source nomination"));
        }
        let schema = product
            .fact_schema_with_work(activity.fact_schema_id(), m)?
            .ok_or_else(|| fail("app original quantity schema"))?;
        let unit = schema.quantity_unit_with_work(&declaration.usage_field, m)?;
        if !same(m, unit, &declaration.usage_unit)? {
            return Err(fail("app original quantity unit"));
        }
        let mut producers = 0usize;
        for rule in &p.rules {
            m.visit("app.original_component_producer")?;
            if !same(m, &rule.component_key, &declaration.component_key)?
                || !same(m, &rule.activity, &declaration.activity)?
            {
                continue;
            }
            let Formula::AppFinalCost { evidence_slot, .. } = &rule.formula else {
                return Err(fail("app declaration names a non-app producer"));
            };
            if !same(m, evidence_slot, &declaration.cost_source_slot)?
                || !same(m, &rule.dependency, "on_missing_wait")?
            {
                return Err(fail("app final evidence nomination/missing rule"));
            }
            producers = w::usize_add(m, producers, 1)?;
            let mut selected = None;
            for slot in &p.binding_slots {
                m.visit("app.original_retail_binding_slot")?;
                if same(m, &slot.name, &rule.binding_slot)? {
                    if selected.is_some() {
                        return Err(fail("ambiguous app retail binding slot"));
                    }
                    selected = Some(slot);
                }
            }
            let slot = selected.ok_or_else(|| fail("app original retail binding missing"))?;
            if !same(m, &slot.book, "retail")?
                || same(m, &slot.funding_leg, &declaration.resource_funding_leg)?
            {
                return Err(fail(
                    "app resource funding must be separate from customer charge funding",
                ));
            }
        }
        if !w::compare(m, producers, 1usize)?.is_eq() {
            return Err(fail(
                "app declaration requires exactly one final-cost producer",
            ));
        }
    }
    for rule in &p.rules {
        if !matches!(rule.formula, Formula::AppFinalCost { .. }) {
            continue;
        }
        let mut found = 0usize;
        for declaration in declarations {
            m.visit("app.required_original_reservation")?;
            if same(m, &declaration.component_key, &rule.component_key)?
                && same(m, &declaration.activity, &rule.activity)?
            {
                found = w::usize_add(m, found, 1)?;
            }
        }
        if !w::compare(m, found, 1usize)?.is_eq() {
            return Err(fail("app producer lacks its exact original declaration"));
        }
    }
    Ok(())
}
