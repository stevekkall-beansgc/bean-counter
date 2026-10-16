use super::super::super::selected_program::SelectedProgram;
use super::super::{CoreMeter, arithmetic, data::*, meter, shape};
use super::check::*;
use crate::Result;
use serde_json::{Value, json};
/// Complete initial catalog/template projection, without equating policy binding
/// names with economic role names.
pub(in crate::policy::workflow::transition_constructor) fn validate(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    b: &Value,
    catalog: &Value,
    inst: &Value,
    t: &Value,
    role: &Value,
    current_grant: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::slots::validate", |m| {
            validate_data(m, program, b, catalog, inst, t, role, current_grant)
        });
    }
    #[cfg(not(test))]
    validate_data(m, program, b, catalog, inst, t, role, current_grant)
}
fn validate_data(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    b: &Value,
    catalog: &Value,
    inst: &Value,
    t: &Value,
    role: &Value,
    current_grant: &Value,
) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        catalog,
        &[
            "schema",
            "agreement_id",
            "customer",
            "revision",
            "components",
            "families"
        ],
        &[]
    )?;
    invoke!(
        shape::closed,
        m,
        inst,
        &[
            "schema",
            "agreement_id",
            "customer",
            "revision",
            "template_ref",
            "catalog_ref",
            "component_map",
            "family_map"
        ],
        &[]
    )?;
    invoke!(number, m, catalog, "revision", true)?;
    invoke!(number, m, inst, "revision", true)?;
    eq(m, catalog, "revision", &json!("1"))?;
    eq(m, inst, "revision", &json!("1"))?;
    let plan = program.compiled_plan().canonical_value();
    let template_ref = invoke!(shape::field, m, inst, "template_ref")?;
    let mut template = None;
    let mut policy = None;
    for d in program.source_documents() {
        m.visit("inventory.source_lookup")?;
        if invoke!(shape::same, m, &json!(d.reference()), template_ref)? {
            template = Some(d.canonical_value());
        }
        if invoke!(
            shape::same,
            m,
            &json!(d.reference()),
            &data_field!(m, &b, "policy_ref")
        )? {
            policy = Some(d.canonical_value());
        }
    }
    let template = template.ok_or_else(|| shape::fail("instantiation original template"))?;
    let policy = policy.ok_or_else(|| shape::fail("original policy"))?;
    invoke!(copy, m, policy, "slot_template_ref", inst, "template_ref")?;
    for (kind, mapping, items) in [
        ("components", "component_map", "components"),
        ("families", "family_map", "families"),
    ] {
        let original = invoke!(shape::array, m, template, kind)?;
        let maps = invoke!(shape::array, m, inst, mapping)?;
        let targets = invoke!(shape::array, m, catalog, items)?;
        super::binding::ordered(m, maps)?;
        super::binding::ordered(m, targets)?;
        invoke!(count, m, maps, original.len())?;
        invoke!(count, m, targets, original.len())?;
        for (i, map) in maps.iter().enumerate() {
            invoke!(shape::closed, m, map, &["template_key", "slot_id"], &[])?;
            invoke!(name, m, map, "slot_id")?;
            for prior in &maps[..i] {
                if invoke!(
                    shape::same,
                    m,
                    &data_field!(m, &map, "template_key"),
                    &data_field!(m, &prior, "template_key")
                )? || invoke!(
                    shape::same,
                    m,
                    &data_field!(m, &map, "slot_id"),
                    &data_field!(m, &prior, "slot_id")
                )? {
                    return Err(shape::fail("ambiguous slot mapping"));
                }
            }
            let old = invoke!(
                shape::named,
                m,
                original,
                "key",
                invoke!(shape::text, m, map, "template_key")?
            )?;
            let target = invoke!(
                shape::named,
                m,
                targets,
                "slot_id",
                invoke!(shape::text, m, map, "slot_id")?
            )?;
            if kind == "components" {
                invoke!(
                    shape::closed,
                    m,
                    target,
                    &["slot_id", "book", "root_slot", "role_slot"],
                    &[]
                )?;
                invoke!(copy, m, target, "book", old, "book")?;
                invoke!(copy, m, target, "role_slot", old, "role_slot")?;
                let root = invoke!(
                    shape::named,
                    m,
                    maps,
                    "template_key",
                    invoke!(shape::text, m, old, "root_key")?
                )?;
                invoke!(copy, m, target, "root_slot", root, "slot_id")?;
            } else {
                invoke!(
                    shape::closed,
                    m,
                    target,
                    &["slot_id", "target_component_slot", "role_slot"],
                    &[]
                )?;
                invoke!(copy, m, target, "role_slot", old, "role_slot")?;
                let comps = invoke!(shape::array, m, inst, "component_map")?;
                let targetmap = invoke!(
                    shape::named,
                    m,
                    comps,
                    "template_key",
                    invoke!(shape::text, m, old, "target_component_key")?
                )?;
                invoke!(
                    copy,
                    m,
                    target,
                    "target_component_slot",
                    targetmap,
                    "slot_id"
                )?;
            }
        }
    }
    let terms_schema = shape::text(m, t, "schema")?;
    let original_work_scope =
        shape::same_text(m, terms_schema, "ledger-workflow-slot-binding-terms/3")?;
    let mut required = vec![
        "schema",
        "scope",
        "agreement_id",
        "customer",
        "slot",
        "book",
        "role_slot",
        "role_ref",
        "funding_leg",
        "allowed_activities",
        "quantity_ceilings",
        "exposure_ceiling",
        "source_grant_refs",
        "correction_source_slot",
        "causal_reversal_clauses",
    ];
    if original_work_scope {
        required.push("exposure_scope");
    }
    shape::closed(m, t, &required, &[])?;
    if original_work_scope {
        eq(m, t, "exposure_scope", &json!("original_work"))?;
        eq(m, t, "book", &json!("retail"))?;
    } else {
        eq(
            m,
            t,
            "schema",
            &json!("ledger-workflow-slot-binding-terms/2"),
        )?;
    }
    let policy_slots = invoke!(shape::array, m, policy, "binding_slots")?;
    let selected = invoke!(
        shape::named,
        m,
        policy_slots,
        "name",
        invoke!(shape::text, m, t, "slot")?
    )?;
    for k in ["book", "funding_leg", "correction_source_slot"] {
        invoke!(copy, m, t, k, selected, k)?;
    }
    let allowed = invoke!(shape::array, m, t, "allowed_activities")?;
    bounds(m, allowed.len(), 1, 16)?;
    super::binding::ordered(m, allowed)?;
    for activity in allowed {
        qualified(
            m,
            activity
                .as_str()
                .ok_or_else(|| shape::fail("activity name"))?,
        )?;
    }
    invoke!(set, m, allowed)?;
    invoke!(
        shape::subset,
        m,
        allowed,
        invoke!(shape::array, m, selected, "allowed_activities")?
    )?;
    let authority_refs = invoke!(shape::array, m, b, "authority_refs")?;
    let grants = invoke!(shape::array, m, t, "source_grant_refs")?;
    super::binding::refs(m, &json!(grants), 1, 32)?;
    invoke!(set, m, grants)?;
    invoke!(shape::subset, m, grants, authority_refs)?;
    if !meter::cmp(m, &grants.len().into(), &0usize.into())?.is_gt() {
        return Err(shape::fail("slot grant nominations"));
    }
    invoke!(
        shape::closed,
        m,
        role,
        &[
            "schema",
            "scope",
            "agreement_id",
            "customer",
            "role_slot",
            "provider",
            "cost_originator",
            "bearer",
            "payer",
            "beneficiary",
            "recipient"
        ],
        &[]
    )?;
    for k in ["bearer", "payer"] {
        invoke!(copy, m, role, k, b, "customer")?;
    }
    for k in [
        "provider",
        "cost_originator",
        "bearer",
        "payer",
        "beneficiary",
        "recipient",
    ] {
        invoke!(name, m, role, k)?;
    }
    invoke!(qualified, m, invoke!(shape::text, m, role, "role_slot")?)?;
    // Every original instruction using this slot must resolve to this declared role.
    for instruction in invoke!(shape::array, m, plan, "typed_instructions")? {
        m.visit("inventory.instruction")?;
        if invoke!(
            shape::same,
            m,
            data_field!(m, &instruction, "opcode"),
            &json!("evaluate_rule")
        )? {
            let rule = &data_field!(m, &instruction, "rule");
            if invoke!(
                shape::same,
                m,
                &data_field!(m, &rule, "binding_slot"),
                &data_field!(m, &t, "slot")
            )? {
                let comp = invoke!(
                    shape::named,
                    m,
                    invoke!(shape::array, m, plan, "component_slots")?,
                    "key",
                    invoke!(shape::text, m, rule, "component_key")?
                )?;
                invoke!(copy, m, comp, "role_slot", t, "role_slot")?;
                invoke!(copy, m, comp, "book", t, "book")?;
                invoke!(
                    shape::contains,
                    m,
                    allowed,
                    &data_field!(m, &rule, "activity")
                )?;
                let formula = data_field!(m, rule, "formula");
                if invoke!(
                    shape::same,
                    m,
                    data_field!(m, formula, "kind"),
                    &json!("unit")
                )? {
                    for declarations in [
                        invoke!(shape::array, m, b, "quantity_ceilings")?,
                        invoke!(shape::array, m, t, "quantity_ceilings")?,
                    ] {
                        let mut found = false;
                        for q in declarations {
                            m.visit("inventory.unit_quantity_declaration")?;
                            if invoke!(
                                shape::same,
                                m,
                                data_field!(m, q, "activity"),
                                data_field!(m, rule, "activity")
                            )? && invoke!(
                                shape::same,
                                m,
                                data_field!(m, q, "field"),
                                data_field!(m, formula, "quantity_field")
                            )? {
                                invoke!(copy, m, q, "unit", formula, "unit")?;
                                found = true;
                            }
                        }
                        if !found {
                            return Err(shape::fail("missing original unit quantity ceiling"));
                        }
                    }
                }
            }
        }
    }
    for nomination in invoke!(shape::array, m, b, "source_slots")? {
        invoke!(
            shape::closed,
            m,
            nomination,
            &["name", "sources", "evidence_kinds"],
            &[]
        )?;
        invoke!(name, m, nomination, "name")?;
        invoke!(set, m, invoke!(shape::array, m, nomination, "sources")?)?;
        invoke!(
            set,
            m,
            invoke!(shape::array, m, nomination, "evidence_kinds")?
        )?;
    }
    // Every accepted quantity declaration is typed against the original activity's
    // immutable schema, and each unit instruction has a declared upper bound.
    for quantities in [
        invoke!(shape::array, m, b, "quantity_ceilings")?,
        invoke!(shape::array, m, t, "quantity_ceilings")?,
    ] {
        bounds(m, quantities.len(), 0, 64)?;
        super::binding::ordered(m, quantities)?;
        for (i, q) in quantities.iter().enumerate() {
            invoke!(
                shape::closed,
                m,
                q,
                &["activity", "field", "unit", "maximum"],
                &[]
            )?;
            for prior in &quantities[..i] {
                if invoke!(
                    shape::same,
                    m,
                    &data_field!(m, &q, "activity"),
                    &data_field!(m, &prior, "activity")
                )? && invoke!(
                    shape::same,
                    m,
                    &data_field!(m, &q, "field"),
                    &data_field!(m, &prior, "field")
                )? {
                    return Err(shape::fail("ambiguous quantity ceiling"));
                }
            }
            let activity = invoke!(
                shape::named,
                m,
                invoke!(shape::array, m, plan, "activity_table")?,
                "activity",
                invoke!(shape::text, m, q, "activity")?
            )?;
            let mut schema = None;
            for d in program.source_documents() {
                if invoke!(
                    shape::same,
                    m,
                    &json!(d.reference()),
                    &data_field!(m, &activity, "fact_schema_ref")
                )? {
                    schema = Some(d.canonical_value());
                }
            }
            let schema = schema.ok_or_else(|| shape::fail("quantity schema"))?;
            let field = invoke!(
                shape::named,
                m,
                invoke!(shape::array, m, schema, "fields")?,
                "name",
                invoke!(shape::text, m, q, "field")?
            )?;
            invoke!(
                eq,
                m,
                &data_field!(m, &field, "type"),
                "kind",
                &json!("integer")
            )?;
            invoke!(copy, m, q, "unit", &data_field!(m, &field, "type"), "unit")?;
            invoke!(eq, m, field, "required", &json!(true))?;
            let maximum = invoke!(decimal, m, q, "maximum")?;
            let max = invoke!(
                arithmetic::integer,
                m,
                invoke!(shape::text, m, &data_field!(m, &field, "type"), "max")?,
                true
            )?;
            let minimum = invoke!(
                arithmetic::integer,
                m,
                invoke!(shape::text, m, data_field!(m, field, "type"), "min")?,
                true
            )?;
            let lower = meter::mul(m, &minimum, &maximum.denominator)?;
            let upper = meter::mul(m, &max, &maximum.denominator)?;
            if invoke!(meter::cmp, m, &maximum.numerator, &lower)?.is_lt() {
                return Err(shape::fail("quantity below original minimum"));
            }
            if invoke!(meter::cmp, m, &maximum.numerator, &upper)?.is_gt() {
                return Err(shape::fail("quantity exceeds original schema"));
            }
        }
    }
    invoke!(amount, m, &data_field!(m, &t, "exposure_ceiling"))?;
    let exposures = invoke!(shape::array, m, b, "exposure_ceilings")?;
    invoke!(count, m, exposures, 1)?;
    invoke!(
        shape::closed,
        m,
        &exposures[0],
        &["binding_slot", "maximum"],
        &[]
    )?;
    invoke!(copy, m, &exposures[0], "binding_slot", t, "slot")?;
    invoke!(amount, m, &exposures[0]["maximum"])?;
    // The exact preaccepted inverse clauses name existing roots/families and their
    // immutable correction nomination; they never mint an economic capability.
    let clauses = invoke!(shape::array, m, t, "causal_reversal_clauses")?;
    bounds(m, clauses.len(), 0, 32)?;
    super::binding::ordered(m, clauses)?;
    for clause in clauses {
        invoke!(
            shape::closed,
            m,
            clause,
            &[
                "root_component_slot",
                "family_slot",
                "correction_source_slot",
                "mode",
                "standalone_deadline_effect"
            ],
            &[]
        )?;
        invoke!(
            eq,
            m,
            clause,
            "mode",
            &json!("exact_inverse_and_permanent_close")
        )?;
        invoke!(
            eq,
            m,
            clause,
            "standalone_deadline_effect",
            &json!("does_not_expire_preaccepted_causal_reversal")
        )?;
        let family = invoke!(
            shape::named,
            m,
            invoke!(shape::array, m, catalog, "families")?,
            "slot_id",
            invoke!(shape::text, m, clause, "family_slot")?
        )?;
        invoke!(
            copy,
            m,
            clause,
            "root_component_slot",
            family,
            "target_component_slot"
        )?;
        let nomination = invoke!(
            shape::named,
            m,
            invoke!(shape::array, m, b, "source_slots")?,
            "name",
            invoke!(shape::text, m, clause, "correction_source_slot")?
        )?;
        invoke!(
            shape::contains,
            m,
            invoke!(shape::array, m, current_grant, "source_slots")?,
            &data_field!(m, &nomination, "name")
        )?;
    }
    Ok(())
}
