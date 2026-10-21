//! Owning canonical body of the SAME normalized typed selected policy.
//! No policy validation, pricing, normalization or authority is implemented here.
use super::super::transition_constructor::CoreMeter;
use super::{
    model::*,
    normalize::{amount_value, code_value, declaration_value},
    owning_plan::{array, reference, vector},
};
use crate::{
    canonical::audit_arena::{AuditArena, AuditedValue},
    validation_work::{self as w},
    Error, Result,
};
use serde_json::Value;
fn fail() -> Error {
    Error::new(
        "UNSUPPORTED_SCHEMA_PROFILE",
        "owned selected policy framing",
    )
}
fn object(
    m: &mut CoreMeter,
    a: &AuditArena,
    strings: &[(&str, &str)],
    children: &[(&str, &Value)],
) -> Result<AuditedValue> {
    let n = w::usize_add(m, strings.len(), children.len())?;
    let mut values = vector(m, a, strings.len())?;
    let mut fields = vector(m, a, n)?;
    for (_, text) in strings {
        m.visit("compile.policy_owned_string")?;
        values.push(a.string_value(text, m)?);
    }
    for ((key, _), value) in strings.iter().zip(values.iter()) {
        m.visit("compile.policy_owned_field")?;
        fields.push((*key, value.value()));
    }
    for field in children {
        m.visit("compile.policy_owned_child")?;
        fields.push(*field);
    }
    a.object_from_fields(&fields, m)
}
fn names(m: &mut CoreMeter, a: &AuditArena, strings: &[String]) -> Result<AuditedValue> {
    let mut rows = vector(m, a, strings.len())?;
    for text in strings {
        m.visit("compile.policy_owned_name")?;
        rows.push(a.string_value(text, m)?);
    }
    array(m, a, &rows)
}
fn percent(m: &mut CoreMeter, a: &AuditArena, p: &Percent) -> Result<AuditedValue> {
    object(m, a, &[("kind", &p.kind), ("percent", &p.percent)], &[])
}
fn formula(m: &mut CoreMeter, a: &AuditArena, f: &Formula) -> Result<AuditedValue> {
    match f {
        Formula::Fixed { amount } => object(m, a, &[("kind", "fixed"), ("amount", amount)], &[]),
        Formula::Unit {
            quantity_field,
            unit,
            rate,
        } => object(
            m,
            a,
            &[
                ("kind", "unit"),
                ("quantity_field", quantity_field),
                ("unit", unit),
                ("rate", rate),
            ],
            &[],
        ),
        Formula::Discount {
            basis,
            mode,
            amount,
        } => {
            let basis = object(
                m,
                a,
                &[
                    ("component_key", &basis.component_key),
                    ("view", &basis.view),
                ],
                &[],
            )?;
            let amount = percent(m, a, amount)?;
            object(
                m,
                a,
                &[("kind", "discount"), ("mode", mode)],
                &[("basis", basis.value()), ("amount", amount.value())],
            )
        }
        Formula::FinalCost {
            evidence_slot,
            supplier_slot,
            multiplier,
            markup,
            ceiling,
        } => {
            let multiplier = object(
                m,
                a,
                &[
                    ("numerator", &multiplier.numerator),
                    ("denominator", &multiplier.denominator),
                ],
                &[],
            )?;
            let ceiling = amount_value(a, m, ceiling)?;
            object(
                m,
                a,
                &[
                    ("kind", "final_cost"),
                    ("evidence_slot", evidence_slot),
                    ("supplier_slot", supplier_slot),
                    ("markup", markup),
                ],
                &[
                    ("multiplier", multiplier.value()),
                    ("ceiling", ceiling.value()),
                ],
            )
        }
        Formula::AppFinalCost {
            evidence_slot,
            multiplier,
            markup,
            ceiling,
        } => {
            let multiplier = object(
                m,
                a,
                &[
                    ("numerator", &multiplier.numerator),
                    ("denominator", &multiplier.denominator),
                ],
                &[],
            )?;
            let ceiling = amount_value(a, m, ceiling)?;
            object(
                m,
                a,
                &[
                    ("kind", "app_final_cost"),
                    ("evidence_slot", evidence_slot),
                    ("markup", markup),
                ],
                &[
                    ("multiplier", multiplier.value()),
                    ("ceiling", ceiling.value()),
                ],
            )
        }
        Formula::SupplierFinalCost {
            evidence_slot,
            ceiling,
        } => {
            let ceiling = amount_value(a, m, ceiling)?;
            object(
                m,
                a,
                &[
                    ("kind", "supplier_final_cost"),
                    ("evidence_slot", evidence_slot),
                ],
                &[("ceiling", ceiling.value())],
            )
        }
    }
}
pub(super) fn build(m: &mut CoreMeter, p: &Policy) -> Result<AuditedValue> {
    let a = m.representation_arena().ok_or_else(fail)?;
    // The selected compiler has already refused stages before this producer.
    if !w::compare(m, p.stages.len(), 0usize)?.is_eq() {
        return Err(fail());
    }
    let mut bindings = vector(m, &a, p.binding_slots.len())?;
    for b in &p.binding_slots {
        let activities = names(m, &a, &b.allowed_activities)?;
        bindings.push(object(
            m,
            &a,
            &[
                ("name", &b.name),
                ("book", &b.book),
                ("funding_leg", &b.funding_leg),
                ("correction_source_slot", &b.correction_source_slot),
            ],
            &[("allowed_activities", activities.value())],
        )?);
    }
    let bindings = array(m, &a, &bindings)?;
    let mut rules = vector(m, &a, p.rules.len())?;
    for r in &p.rules {
        let predicate = match r.when {
            Predicate::True => object(m, &a, &[("op", "true")], &[])?,
        };
        let formula = formula(m, &a, &r.formula)?;
        rules.push(object(
            m,
            &a,
            &[
                ("rule_id", &r.rule_id),
                ("activity", &r.activity),
                ("component_name", &r.component_name),
                ("component_key", &r.component_key),
                ("binding_slot", &r.binding_slot),
                ("phase", &r.phase),
                ("dependency", &r.dependency),
            ],
            &[("when", predicate.value()), ("formula", formula.value())],
        )?);
    }
    let rules = array(m, &a, &rules)?;
    let mut families = vector(m, &a, p.families.len())?;
    for f in &p.families {
        let path = names(m, &a, &f.selector.path)?;
        let selector = object(
            m,
            &a,
            &[
                ("target_activity", &f.selector.target_activity),
                ("binding_slot", &f.selector.binding_slot),
                ("component_key", &f.selector.component_key),
                ("basis", &f.selector.basis),
            ],
            &[("path", path.value())],
        )?;
        let mut codes = vector(m, &a, f.codes.len())?;
        for code in &f.codes {
            codes.push(code_value(&a, m, code)?);
        }
        let codes = array(m, &a, &codes)?;
        let anchor_path = names(m, &a, &f.window.anchor_path)?;
        let window = &f.window;
        let window = object(
            m,
            &a,
            &[
                ("anchor", &window.anchor),
                ("start_offset_us", &window.start_offset_us),
                ("occurrence_us", &window.occurrence_us),
                ("receipt_grace_us", &window.receipt_grace_us),
                ("acceptance_grace_us", &window.acceptance_grace_us),
                ("activation_deadline_us", &window.activation_deadline_us),
                ("correction_us", &window.correction_us),
            ],
            &[("anchor_path", anchor_path.value())],
        )?;
        let premium = amount_value(&a, m, &f.maximum_premium)?;
        let discount = amount_value(&a, m, &f.maximum_discount)?;
        let close = if let Some(close) = &f.no_success_close {
            Some(object(
                m,
                &a,
                &[("close_source_slot", &close.close_source_slot)],
                &[],
            )?)
        } else {
            None
        };
        let mut children = vector(m, &a, 7)?;
        children.push(("selector", selector.value()));
        children.push(("codes", codes.value()));
        children.push(("window", window.value()));
        children.push(("maximum_premium", premium.value()));
        children.push(("maximum_discount", discount.value()));
        if let Some(close) = &close {
            children.push(("no_success_close", close.value()));
        }
        families.push(object(
            m,
            &a,
            &[
                ("family_name", &f.family_name),
                ("family_key", &f.family_key),
                ("trigger_activity", &f.trigger_activity),
                ("target_link_slot", &f.target_link_slot),
                ("source_slot", &f.source_slot),
                ("code_field", &f.code_field),
                ("correction_source_slot", &f.correction_source_slot),
            ],
            &children,
        )?);
    }
    let families = array(m, &a, &families)?;
    let mut delayed = vector(m, &a, p.delayed_costs.len())?;
    for d in &p.delayed_costs {
        delayed.push(object(
            m,
            &a,
            &[
                ("component_key", &d.component_key),
                ("evidence_source_slot", &d.evidence_source_slot),
                ("finalize_within_us", &d.finalize_within_us),
                ("on_missing", &d.on_missing),
            ],
            &[],
        )?);
    }
    let delayed = array(m, &a, &delayed)?;
    let app = if let Some(declarations) = &p.app_delayed_costs {
        let mut rows = vector(m, &a, declarations.len())?;
        for d in declarations {
            rows.push(declaration_value(&a, m, d)?);
        }
        Some(array(m, &a, &rows)?)
    } else {
        None
    };
    let previous = if let Some(r) = &p.previous_policy_ref {
        Some(reference(m, &a, r)?)
    } else {
        None
    };
    let product = reference(m, &a, &p.product_ref)?;
    let context = reference(m, &a, &p.context_schema_ref)?;
    let template = reference(m, &a, &p.slot_template_ref)?;
    let compiler = reference(m, &a, &p.compiler_ref)?;
    let empty = a.array_from_values(&[], m)?;
    let scale = Value::from(p.scale);
    let mut children = vector(m, &a, 13)?;
    for field in [
        ("product_ref", product.value()),
        ("context_schema_ref", context.value()),
        ("slot_template_ref", template.value()),
        ("compiler_ref", compiler.value()),
        ("scale", &scale),
        ("binding_slots", bindings.value()),
        ("rules", rules.value()),
        ("families", families.value()),
        ("stages", empty.value()),
        ("delayed_costs", delayed.value()),
    ] {
        children.push(field);
    }
    if let Some(app) = &app {
        children.push(("app_delayed_costs", app.value()));
    }
    if let Some(previous) = &previous {
        children.push(("previous_policy_ref", previous.value()));
    }
    object(
        m,
        &a,
        &[
            ("schema", &p.schema),
            ("policy_id", &p.policy_id),
            ("version", &p.version),
            ("currency", &p.currency),
            ("rounding", &p.rounding),
            ("mode", &p.mode),
        ],
        &children,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical, validation_work::OriginalAuditSession};
    use serde_json::json;
    // Framing comparison only: this deliberately does not validate/authorize a
    // policy or manufacture an executable support/program for the new recipe.
    #[test]
    fn owning_policy_body_preserves_all_typed_formula_and_optional_fields() {
        let source:Value=canonical::parse(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let mut raw = source["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["schema"] == "ledger-workflow-policy/1")
            .unwrap()
            .clone();
        raw["schema"] = json!("ledger-workflow-policy/4");
        raw["previous_policy_ref"] = source["root_policy_ref"].clone();
        raw["families"][0]["no_success_close"] = json!({"close_source_slot":"closer"});
        raw["app_delayed_costs"] = json!([{"component_key":"app","activity":"work","resource_funding_leg":"platform","usage_field":"tokens","usage_unit":"token","work_source_slot":"worker","cost_source_slot":"cost","correction_source_slot":"corrector","finalize_within_us":"10","correction_within_us":"20","correction_anchor":"initial_cost_accepted_at","causal_reversal":{"root_relation":"original_work_delayed_cost_dependency","mode":"exact_inverse_and_permanent_close","standalone_deadline_effect":"does_not_expire_preaccepted_causal_reversal"},"maximum_final_amount":{"currency":"USD","scale":18,"atoms":"300"},"on_missing":"wait","cost_basis":"app_attributed_final_amount","comparable_quantity_rule":"when_present_lte_original_observed","allocation":"one_call_one_component"}]);
        raw["delayed_costs"] = json!([{"component_key":"supplier","evidence_source_slot":"supplier_cost","finalize_within_us":"10","on_missing":"wait"}]);
        let rule = raw["rules"][0].clone();
        for formula in [
            json!({"kind":"final_cost","evidence_slot":"cost","supplier_slot":"supplier","multiplier":{"numerator":"1","denominator":"1"},"markup":"0.1","ceiling":{"currency":"USD","scale":18,"atoms":"300"}}),
            json!({"kind":"supplier_final_cost","evidence_slot":"supplier","ceiling":{"currency":"USD","scale":18,"atoms":"300"}}),
            json!({"kind":"app_final_cost","evidence_slot":"app","multiplier":{"numerator":"1","denominator":"1"},"markup":"0.2","ceiling":{"currency":"USD","scale":18,"atoms":"300"}}),
        ] {
            let mut next = rule.clone();
            next["formula"] = formula;
            raw["rules"].as_array_mut().unwrap().push(next);
        }
        let typed: Policy = serde_json::from_value(raw).unwrap();
        let expected = serde_json::to_value(&typed).unwrap();
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let output = build(session.meter_mut(), &typed).unwrap();
        assert_eq!(output.value(), &expected);
        drop(output);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
