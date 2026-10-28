//! Selected semantic relations; data validation supplies no host authentication.
use super::super::{
    product::{BindingRelation, MissingDependency, Primitive},
    template::TemplateBook,
    transition_constructor::{arithmetic, meter, CoreMeter},
    FactSchema, Product, SlotTemplate,
};
use super::model::{Amount, Effect, Formula, Policy, Rule};
use crate::{Error, Result};
use num_bigint::BigInt;
use std::cmp::Ordering;
fn bad(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn closure_ceiling(m: &mut CoreMeter, ceiling: &Amount) -> Result<()> {
    if !super::compiler::same_literal(m, &ceiling.currency, "USD")?
        || !crate::validation_work::compare(m, ceiling.scale, 18u8)?.is_eq()
    {
        return Err(bad("closure ceiling currency/scale"));
    }
    let n = arithmetic::integer(m, &ceiling.atoms, false)?;
    let maximum = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, &n, &0.into())?.is_lt() || meter::cmp(m, &n, &maximum)?.is_gt() {
        return Err(bad("closure ceiling range"));
    }
    Ok(())
}
fn cost_component(
    m: &mut CoreMeter,
    r: &Rule,
    book: TemplateBook,
    slot_book: &str,
    root: &str,
    key: &str,
    selected_operands: (&str, &Amount),
) -> Result<()> {
    let (evidence_slot, ceiling) = selected_operands;
    name(m, evidence_slot)?;
    if !super::compiler::same_literal(m, &r.phase, "cost")?
        || !super::compiler::same_literal(m, root, key)?
    {
        return Err(bad("cost phase/direct root"));
    }
    let expected = match book {
        TemplateBook::Retail => "retail",
        TemplateBook::Supplier => "supplier",
        _ => return Err(bad("payable cost component book")),
    };
    if !super::compiler::same_literal(m, expected, slot_book)? {
        return Err(bad("cost slot book"));
    }
    if !super::compiler::same_literal(m, &ceiling.currency, "USD")?
        || !crate::validation_work::compare(m, ceiling.scale, 18u8)?.is_eq()
    {
        return Err(bad("cost ceiling currency/scale"));
    }
    let n = arithmetic::integer(m, &ceiling.atoms, false)?;
    let max = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, &n, &0.into())?.is_lt() || meter::cmp(m, &n, &max)?.is_gt() {
        return Err(bad("cost ceiling amount"));
    }
    Ok(())
}
fn delayed_costs(m: &mut CoreMeter, p: &Policy) -> Result<()> {
    use super::compiler::same_literal as same;
    if crate::validation_work::compare(m, p.delayed_costs.len(), 64usize)?.is_gt() {
        return Err(bad("delayed cost declarations"));
    }
    for (i, declaration) in p.delayed_costs.iter().enumerate() {
        m.visit("cost.delayed_declaration")?;
        name(m, &declaration.component_key)?;
        name(m, &declaration.evidence_source_slot)?;
        if !same(m, &declaration.on_missing, "wait")? {
            return Err(bad("delayed cost missing rule"));
        }
        arithmetic::integer(m, &declaration.finalize_within_us, false)?;
        for previous in &p.delayed_costs[..i] {
            if same(m, &previous.component_key, &declaration.component_key)? {
                return Err(bad("duplicate delayed cost component"));
            }
        }
        let mut producer = None;
        for rule in &p.rules {
            m.visit("cost.delayed_supplier_producer")?;
            if !same(m, &rule.component_key, &declaration.component_key)? {
                continue;
            }
            if producer.is_some() {
                return Err(bad("ambiguous delayed cost producer"));
            }
            match &rule.formula {
                Formula::SupplierFinalCost { evidence_slot, .. } => {
                    if !same(m, evidence_slot, &declaration.evidence_source_slot)? {
                        return Err(bad("delayed cost evidence nomination"));
                    }
                    producer = Some(rule);
                }
                _ => return Err(bad("delayed cost must name supplier final producer")),
            }
        }
        if producer.is_none() {
            return Err(bad("missing delayed supplier producer"));
        }
    }
    for rule in &p.rules {
        match &rule.formula {
            Formula::SupplierFinalCost { .. } => {
                let mut count = 0usize;
                for declaration in &p.delayed_costs {
                    m.visit("cost.required_delayed_reservation")?;
                    if same(m, &declaration.component_key, &rule.component_key)? {
                        count = crate::validation_work::usize_add(m, count, 1)?;
                    }
                }
                if !crate::validation_work::compare(m, count, 1usize)?.is_eq() {
                    return Err(bad("supplier producer needs exact delayed declaration"));
                }
            }
            Formula::FinalCost {
                supplier_slot,
                evidence_slot,
                ..
            } => {
                let mut count = 0usize;
                for supplier in &p.rules {
                    m.visit("cost.original_pass_through_supplier_join")?;
                    if !same(m, &supplier.activity, &rule.activity)?
                        || !same(m, &supplier.binding_slot, supplier_slot)?
                    {
                        continue;
                    }
                    if let Formula::SupplierFinalCost {
                        evidence_slot: source,
                        ..
                    } = &supplier.formula
                    {
                        if !same(m, source, evidence_slot)? {
                            return Err(bad("pass through and supplier evidence differ"));
                        }
                        count = crate::validation_work::usize_add(m, count, 1)?;
                    }
                }
                if !crate::validation_work::compare(m, count, 1usize)?.is_eq() {
                    return Err(bad(
                        "pass through needs unique original supplier obligation",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub(super) fn name(m: &mut CoreMeter, s: &str) -> Result<()> {
    m.visit("name.split")?;
    let (namespace, local) = s.split_once(':').ok_or_else(|| bad("qualified name"))?;
    for (part, maximum) in [(namespace, 63), (local, 64)] {
        if !m.check("name.empty", || !part.is_empty())?
            || !m.check("name.length", || part.len() <= maximum)?
            || !m.check("name.first", || part.as_bytes()[0].is_ascii_alphabetic())?
        {
            return Err(bad("name syntax"));
        }
        for b in part.bytes() {
            if !m.check("name.character", || {
                b.is_ascii_alphanumeric() || b"_.-".contains(&b)
            })? {
                return Err(bad("name character"));
            }
        }
    }
    Ok(())
}
pub(super) fn policy(
    m: &mut CoreMeter,
    p: &Policy,
    product: &Product,
    template: &SlotTemplate,
    schemas: &[FactSchema],
    selected_recipes: (bool, bool, bool, bool),
) -> Result<()> {
    let (owner_local_cost, independent_field_issuers, stage_lifecycle, component_lifecycle) =
        selected_recipes;
    name(m, &p.policy_id)?;
    if !m.check("policy.bounds", || {
        !p.rules.is_empty()
            && p.rules.len() <= 64
            && p.families.len() <= 64
            && (if owner_local_cost || stage_lifecycle || component_lifecycle {
                !p.binding_slots.is_empty() && p.binding_slots.len() <= 16
            } else {
                p.binding_slots.len() == 1
            })
    })? {
        return Err(bad("policy count"));
    }
    let version = arithmetic::integer(m, &p.version, false)?;
    if meter::cmp(m, &version, &BigInt::from(0))? != Ordering::Greater
        || meter::cmp(m, &version, &BigInt::from(u64::MAX))? == Ordering::Greater
    {
        return Err(bad("version"));
    }
    for (i, s) in p.binding_slots.iter().enumerate() {
        for n in [&s.name, &s.funding_leg, &s.correction_source_slot] {
            name(m, n)?;
        }
        if !m.check("slot.book", || {
            s.book == "retail"
                || (owner_local_cost || stage_lifecycle) && s.book == "supplier"
                || component_lifecycle && s.book == "cost_observation"
        })? {
            return Err(bad("selected book"));
        }
        for prior in &p.binding_slots[..i] {
            if m.check("slot.unique", || prior.name == s.name)? {
                return Err(bad("duplicate binding slot"));
            }
        }
        for a in &s.allowed_activities {
            name(m, a)?;
            m.visit("slot.activity_lookup")?;
            product
                .activity_with_work(a, m)?
                .ok_or_else(|| bad("slot activity"))?;
        }
    }
    if stage_lifecycle {
        stages(m, p, product, template)?;
    }
    for (i, r) in p.rules.iter().enumerate() {
        for n in [
            &r.rule_id,
            &r.activity,
            &r.component_name,
            &r.component_key,
            &r.binding_slot,
        ] {
            name(m, n)?;
        }
        for prior in &p.rules[..i] {
            if m.check("rule.identity_unique", || prior.rule_id == r.rule_id)?
                || m.check("rule.effect_unique", || {
                    prior.activity == r.activity && prior.component_key == r.component_key
                })?
            {
                return Err(bad("duplicate rule/effect"));
            }
        }
        m.visit("rule.activity_lookup")?;
        let a = product
            .activity_with_work(&r.activity, m)?
            .ok_or_else(|| bad("rule activity"))?;
        let mut stage_closure = false;
        if stage_lifecycle {
            for stage in &p.stages {
                m.visit("rule.original_stage_activity")?;
                stage_closure |=
                    super::compiler::same_literal(m, &stage.closure_activity, &r.activity)?;
            }
        }
        if !m.check("rule.primitive", || {
            a.primitive() == Primitive::Work || stage_closure && a.primitive() == Primitive::Outcome
        })? {
            return Err(bad("rule requires work"));
        }
        let c = template.component_named_with_work(&r.component_key, &r.component_name, m)?;
        let slot = p
            .binding_slots
            .iter()
            .find(|s| s.name == r.binding_slot)
            .ok_or_else(|| bad("binding slot"))?;
        if !m.check("rule.role_book_funding", || {
            (c.book() == TemplateBook::Retail && slot.book == "retail"
                || (owner_local_cost || stage_lifecycle)
                    && c.book() == TemplateBook::Supplier
                    && slot.book == "supplier"
                || component_lifecycle
                    && c.book() == TemplateBook::CostObservation
                    && slot.book == "cost_observation"
                    && matches!(r.formula, Formula::ObserveCost { .. }))
                && (owner_local_cost
                    || stage_lifecycle
                    || a.funding_leg() == Some(slot.funding_leg.as_str()))
                && slot.allowed_activities.contains(&r.activity)
                && (r.dependency == "on_missing_refuse"
                    || (p.schema == "ledger-workflow-policy/3"
                        || p.schema == "ledger-workflow-policy/4")
                        && (matches!(r.formula, Formula::AppFinalCost { .. })
                            || component_lifecycle
                                && matches!(r.formula, Formula::ObserveCost { .. }))
                        && r.dependency == "on_missing_wait")
        })? {
            return Err(bad("rule role/book/funding/dependency"));
        }
        match &r.formula {
            Formula::ClosureCap {
                stage,
                basis_component_key,
                ceiling,
            } => {
                let mut original_stage = false;
                for original in &p.stages {
                    m.visit("cap.original_stage")?;
                    if super::compiler::same_literal(m, &original.name, stage)?
                        && super::compiler::same_literal(
                            m,
                            &original.closure_activity,
                            &r.activity,
                        )?
                        && super::compiler::same_literal(
                            m,
                            &original.closure_component_key,
                            basis_component_key,
                        )?
                    {
                        original_stage = true;
                    }
                }
                if !stage_lifecycle
                    || p.mode != "closure_share"
                    || !stage_closure
                    || r.phase != "cap"
                    || c.book() != TemplateBook::Retail
                    || c.root_key() != basis_component_key
                    || c.key() == c.root_key()
                    || !original_stage
                {
                    return Err(bad("closure cap stage/root/phase"));
                }
                closure_ceiling(m, ceiling)?;
            }
            Formula::ClosureShare {
                basis_component_key,
                percent,
                ceiling,
                ..
            } => {
                let mut original_base = false;
                for base in &p.rules {
                    m.visit("share.original_basis")?;
                    if super::compiler::same_literal(m, &base.component_key, basis_component_key)?
                        && super::compiler::same_literal(m, &base.activity, &r.activity)?
                        && matches!(base.formula, Formula::Fixed { .. } | Formula::Unit { .. })
                    {
                        original_base = true;
                    }
                }
                if !stage_lifecycle
                    || p.mode != "closure_share"
                    || !stage_closure
                    || r.phase != "share"
                    || c.book() != TemplateBook::Supplier
                    || c.root_key() != c.key()
                    || !original_base
                {
                    return Err(bad("closure share stage/root/phase"));
                }
                percentage(m, percent)?;
                closure_ceiling(m, ceiling)?;
            }
            Formula::Fixed { amount } | Formula::Unit { rate: amount, .. } => {
                let mut stage_premium = false;
                if stage_lifecycle
                    && p.mode == "closure_share"
                    && r.phase == "premium"
                    && matches!(r.formula, Formula::Fixed { .. })
                {
                    for stage in &p.stages {
                        m.visit("Stage.exact_original_premium_root")?;
                        if super::compiler::same_literal(m, &stage.closure_activity, &r.activity)?
                            && super::compiler::same_literal(
                                m,
                                &stage.closure_component_key,
                                &r.component_key,
                            )?
                        {
                            stage_premium = true;
                        }
                    }
                }
                if !m.check("base.phase_root", || {
                    (r.phase == "base" || stage_premium) && c.root_key() == c.key()
                })? {
                    return Err(bad("base phase/root"));
                }
                let ratio = arithmetic::decimal(m, amount)?;
                if meter::cmp(m, &ratio.numerator, &BigInt::from(0))? == Ordering::Less {
                    return Err(bad("negative base"));
                }
            }
            Formula::Percent { basis, percent } => {
                if !component_lifecycle || r.phase != "premium" {
                    return Err(bad("component percentage recipe/phase"));
                }
                component_modifier_basis(m, p, r, i, template, basis)?;
                percentage(m, percent)?;
            }
            Formula::ObserveCost { evidence_slot } => {
                if !component_lifecycle
                    || r.phase != "cost"
                    || slot.book != "cost_observation"
                    || c.root_key() != c.key()
                {
                    return Err(bad("nonpayable observation recipe/book/root"));
                }
                name(m, evidence_slot)?;
            }
            Formula::Discount { basis, amount, .. } if component_lifecycle => {
                if r.phase != "discount" {
                    return Err(bad("component discount phase"));
                }
                component_modifier_basis(m, p, r, i, template, basis)?;
                modifier_amount(m, amount)?;
            }
            Formula::Discount { basis, amount, .. } => {
                if !m.check("modifier.phase_root", || {
                    r.phase == "discount"
                        && c.root_key() == basis.component_key
                        && c.root_key() != c.key()
                })? {
                    return Err(bad("discount phase/root"));
                }
                let mut found = false;
                for base in &p.rules {
                    m.visit("basis.candidate")?;
                    if m.check("basis.identity", || {
                        base.component_key == basis.component_key
                            && base.activity == r.activity
                            && base.binding_slot == r.binding_slot
                            && base.phase == "base"
                    })? {
                        found = true;
                    }
                }
                if !found {
                    return Err(bad("missing same activity/slot base"));
                }
                if amount.kind() != "percent" {
                    return Err(bad("legacy percentage discount"));
                }
                percentage(m, amount.value())?;
            }
            Formula::AppFinalCost {
                evidence_slot,
                multiplier,
                markup,
                ceiling,
            } => {
                if !super::compiler::same_literal(m, &p.schema, "ledger-workflow-policy/3")?
                    && !super::compiler::same_literal(m, &p.schema, "ledger-workflow-policy/4")?
                {
                    return Err(bad("app cost recipe"));
                }
                cost_component(
                    m,
                    r,
                    c.book(),
                    slot.book.as_str(),
                    c.root_key(),
                    c.key(),
                    (evidence_slot, ceiling),
                )?;
                if !super::compiler::same_literal(m, &slot.book, "retail")? {
                    return Err(bad("app cost retail book"));
                }
                let n = arithmetic::integer(m, &multiplier.numerator, false)?;
                let d = arithmetic::integer(m, &multiplier.denominator, false)?;
                if meter::cmp(m, &n, &0.into())?.is_lt() || !meter::cmp(m, &d, &0.into())?.is_gt() {
                    return Err(bad("app cost original multiplier"));
                }
                let value = arithmetic::decimal(m, markup)?;
                if meter::cmp(m, &value.numerator, &0.into())?.is_lt() {
                    return Err(bad("negative app cost markup"));
                }
            }
            Formula::FinalCost {
                evidence_slot,
                supplier_slot,
                multiplier,
                markup,
                ceiling,
            } => {
                if !owner_local_cost {
                    return Err(bad("final_cost recipe"));
                }
                cost_component(
                    m,
                    r,
                    c.book(),
                    slot.book.as_str(),
                    c.root_key(),
                    c.key(),
                    (evidence_slot, ceiling),
                )?;
                if slot.book != "retail" {
                    return Err(bad("retail final_cost book"));
                }
                name(m, supplier_slot)?;
                let mut supplier = None;
                for original in &p.binding_slots {
                    m.visit("cost.original_supplier_binding_slot")?;
                    if super::compiler::same_literal(m, &original.name, supplier_slot)? {
                        if supplier.is_some() {
                            return Err(bad("duplicate supplier binding slot"));
                        }
                        supplier = Some(original);
                    }
                }
                let supplier =
                    supplier.ok_or_else(|| bad("missing original supplier binding slot"))?;
                if supplier.book != "supplier" || !supplier.allowed_activities.contains(&r.activity)
                {
                    return Err(bad("supplier slot activity/book"));
                }
                let n = arithmetic::integer(m, &multiplier.numerator, false)?;
                let d = arithmetic::integer(m, &multiplier.denominator, false)?;
                if meter::cmp(m, &n, &0.into())?.is_lt() || !meter::cmp(m, &d, &0.into())?.is_gt() {
                    return Err(bad("final_cost multiplier"));
                }
                let markup = arithmetic::decimal(m, markup)?;
                if meter::cmp(m, &markup.numerator, &0.into())?.is_lt() {
                    return Err(bad("negative final_cost markup"));
                }
            }
            Formula::SupplierFinalCost {
                evidence_slot,
                ceiling,
            } => {
                if !owner_local_cost {
                    return Err(bad("supplier_final_cost recipe"));
                }
                cost_component(
                    m,
                    r,
                    c.book(),
                    slot.book.as_str(),
                    c.root_key(),
                    c.key(),
                    (evidence_slot, ceiling),
                )?;
                if slot.book != "supplier" {
                    return Err(bad("supplier_final_cost book"));
                }
            }
        }
    }
    if owner_local_cost {
        delayed_costs(m, p)?;
    }
    if (p.schema == "ledger-workflow-policy/3" || p.schema == "ledger-workflow-policy/4")
        && !((stage_lifecycle || component_lifecycle)
            && p.app_delayed_costs
                .as_ref()
                .is_none_or(|rows| rows.is_empty()))
    {
        super::app_declarations::validate(m, p, product, schemas)?;
    }
    for (i, f) in p.families.iter().enumerate() {
        for n in [
            &f.family_name,
            &f.family_key,
            &f.trigger_activity,
            &f.target_link_slot,
            &f.source_slot,
            &f.code_field,
            &f.correction_source_slot,
        ] {
            name(m, n)?;
        }
        for prior in &p.families[..i] {
            if m.check("family.unique", || {
                prior.family_key == f.family_key
                    || !component_lifecycle && prior.selector == f.selector
            })? {
                return Err(bad("duplicate selected target family"));
            }
        }
        if let Some(close) = &f.no_success_close {
            name(m, &close.close_source_slot)?;
            if p.schema != "ledger-workflow-policy/2"
                && p.schema != "ledger-workflow-policy/3"
                && p.schema != "ledger-workflow-policy/4"
            {
                return Err(bad("old family dictionary cannot nominate close"));
            }
        }
        let tf = template.family_named_with_work(&f.family_key, &f.family_name, m)?;
        let a = product
            .activity_with_work(&f.trigger_activity, m)?
            .ok_or_else(|| bad("family trigger"))?;
        let target = product
            .activity_with_work(&f.selector.target_activity, m)?
            .ok_or_else(|| bad("family target"))?;
        if !m.check("family.roles", || {
            a.primitive() == Primitive::Outcome
                && target.primitive() == Primitive::Work
                && a.source_slot() == f.source_slot
                && tf.target_component_key() == f.selector.component_key
                && f.selector.basis == "booked_original_net"
                && f.selector.path.first() == Some(&f.target_link_slot)
        })? {
            return Err(bad("family selected identity/role"));
        }
        let mut target_exists = false;
        for rule in &p.rules {
            if m.check("family.target_producer", || {
                rule.phase == "base"
                    && rule.activity == f.selector.target_activity
                    && rule.binding_slot == f.selector.binding_slot
                    && rule.component_key == f.selector.component_key
            })? {
                target_exists = true;
            }
        }
        if !target_exists {
            return Err(bad("missing family root producer"));
        }
        let duration = arithmetic::integer(m, &f.window.occurrence_us, false)?;
        if meter::cmp(m, &duration, &BigInt::from(0))? != Ordering::Greater {
            return Err(bad("positive occurrence window required"));
        }
        if component_lifecycle {
            product.validate_selector_path_with_work(
                &f.trigger_activity,
                &f.selector.path,
                &f.selector.target_activity,
                m,
            )?;
        }
        for link in &f.selector.path {
            m.visit("path.link_lookup")?;
            let l = product
                .link_with_work(link, m)?
                .ok_or_else(|| bad("path link"))?;
            let mut priced_child = false;
            if component_lifecycle && l.minimum() == 0 {
                for rule in &p.rules {
                    m.visit("path.optional_report_instruction")?;
                    priced_child |= super::super::transition_constructor::shape::same_text(
                        m,
                        &rule.activity,
                        l.child_activity(),
                    )?;
                }
            }
            if !m.check("path.selected", || {
                l.maximum() == 1
                    && l.binding_relation() == BindingRelation::SameBinding
                    && ((l.minimum() == 1 && l.on_missing() == MissingDependency::Refuse)
                        || (component_lifecycle
                            && l.minimum() == 0
                            && l.on_missing() == MissingDependency::Wait
                            && l.child_primitive() == Primitive::Work
                            && !priced_child))
            })? {
                return Err(bad("bounded monetary link"));
            }
        }
        let mut found_schema = None;
        for schema in schemas {
            let id = schema.document_id_with_work(m)?;
            if m.check("family.schema_compare", || id == a.fact_schema_id())? {
                found_schema = Some(schema);
                break;
            }
        }
        let sr = found_schema.ok_or_else(|| bad("family schema"))?;
        let fields = sr.canonical_value()["fields"]
            .as_array()
            .ok_or_else(|| bad("family fields"))?;
        let code = fields
            .iter()
            .find(|v| v["name"] == f.code_field)
            .ok_or_else(|| bad("code field"))?;
        if !m.check("family.code_provenance", || {
            code["required"] == true
                && code["type"]["kind"] == "enum"
                && code["provenance"]["kind"] == "attested"
                && (independent_field_issuers || code["provenance"]["source_slot"] == f.source_slot)
                && code["provenance"]["evidence_kind"] == "outcome"
        })? {
            return Err(bad("family code provenance"));
        }
        let values = code["type"]["values"]
            .as_array()
            .ok_or_else(|| bad("enum codes"))?;
        if !m.check("family.codes_count", || values.len() == f.codes.len())? {
            return Err(bad("family enum coverage"));
        }
        for (index, c) in f.codes.iter().enumerate() {
            name(m, &c.code)?;
            for prior in &f.codes[..index] {
                if m.check("family.code_unique", || prior.code == c.code)? {
                    return Err(bad("duplicate family code"));
                }
            }
            if !m.check("family.code_enum", || values.iter().any(|v| v == &c.code))? {
                return Err(bad("family code enum"));
            }
            match &c.effect {
                Effect::None => {}
                Effect::Premium { amount } | Effect::Discount { amount } => {
                    if !component_lifecycle && amount.kind() != "percent" {
                        return Err(bad("legacy percentage family effect"));
                    }
                    modifier_amount(m, amount)?
                }
            }
        }
        let mut occurrence = 0;
        for field in fields {
            m.visit("family.occurrence_field")?;
            if m.check("family.occurrence_kind", || {
                field["provenance"]["evidence_kind"] == "occurrence"
            })? {
                occurrence += 1;
                if !m.check("family.occurrence_provenance", || {
                    field["required"] == true
                        && field["type"]["kind"] == "timestamp"
                        && field["provenance"]["kind"] == "attested"
                        && (independent_field_issuers
                            || field["provenance"]["source_slot"] == f.source_slot)
                })? {
                    return Err(bad("occurrence provenance"));
                }
            }
        }
        if occurrence == 0 {
            return Err(bad("required occurrence field"));
        }
        for amount in [&f.maximum_premium, &f.maximum_discount] {
            if !m.check("family.amount_domain", || {
                amount.currency == p.currency && amount.scale == p.scale
            })? {
                return Err(bad("family amount domain"));
            }
            let atoms = arithmetic::integer(m, &amount.atoms, false)?;
            if meter::cmp(m, &atoms, &BigInt::from(999999999999999999999999999999i128))?
                == Ordering::Greater
            {
                return Err(bad("family amount bound"));
            }
        }
    }
    Ok(())
}
fn stages(m: &mut CoreMeter, p: &Policy, product: &Product, template: &SlotTemplate) -> Result<()> {
    use super::compiler::same_literal as same;
    if crate::validation_work::compare(m, p.stages.len(), 16usize)?.is_gt() {
        return Err(bad("Stage count"));
    }
    if !p.stages.is_empty() && !same(m, &p.mode, "closure_share")? {
        return Err(bad("Stage policy mode"));
    }
    if same(m, &p.mode, "closure_share")?
        && (!p.families.is_empty()
            || !p.delayed_costs.is_empty()
            || p.app_delayed_costs.as_ref().is_some_and(|v| !v.is_empty()))
    {
        return Err(bad(
            "closure share excludes component families and delayed costs",
        ));
    }
    for (i, stage) in p.stages.iter().enumerate() {
        m.visit("compile.stage_definition")?;
        for value in [
            &stage.name,
            &stage.authorization_source_slot,
            &stage.closure_activity,
            &stage.closure_component_key,
            &stage.closure_source_slot,
        ] {
            name(m, value)?;
        }
        // Evidence kind is a closed literal rather than a qualified Name.
        if !same(m, &stage.closure_evidence_kind, "outcome")? {
            return Err(bad("Stage closure evidence"));
        }
        for old in &p.stages[..i] {
            if same(m, &stage.name, &old.name)?
                || same(m, &stage.closure_activity, &old.closure_activity)?
            {
                return Err(bad("duplicate Stage identity/activity"));
            }
        }
        let activity = product
            .activity_with_work(&stage.closure_activity, m)?
            .ok_or_else(|| bad("Stage closure activity"))?;
        if activity.primitive() != Primitive::Outcome
            || !same(m, activity.source_slot(), &stage.closure_source_slot)?
        {
            return Err(bad("Stage closure source/primitive"));
        }
        if stage.expected_slots.is_empty()
            || crate::validation_work::compare(m, stage.expected_slots.len(), 32usize)?.is_gt()
        {
            return Err(bad("Stage expected count"));
        }
        if !same(m, &stage.window.anchor, "verified_predecessor_occurred_at")? {
            return Err(bad("Stage occurrence anchor"));
        }
        for text in [
            &stage.window.start_offset_us,
            &stage.window.occurrence_us,
            &stage.window.receipt_grace_us,
            &stage.window.acceptance_grace_us,
            &stage.window.activation_deadline_us,
            &stage.window.correction_us,
        ] {
            arithmetic::integer(m, text, false)?;
        }
        let mut anchor = false;
        for (j, expected) in stage.expected_slots.iter().enumerate() {
            for value in [&expected.key, &expected.activity, &expected.source_slot] {
                name(m, value)?;
            }
            for old in &stage.expected_slots[..j] {
                if same(m, &expected.key, &old.key)? {
                    return Err(bad("duplicate expected Stage key"));
                }
            }
            let work = product
                .activity_with_work(&expected.activity, m)?
                .ok_or_else(|| bad("Stage expected activity"))?;
            if work.primitive() != Primitive::Work
                || !same(m, work.source_slot(), &expected.source_slot)?
            {
                return Err(bad("Stage expected Work source"));
            }
            if expected.component_keys.is_empty()
                || crate::validation_work::compare(m, expected.component_keys.len(), 128usize)?
                    .is_gt()
            {
                return Err(bad("Stage expected components"));
            }
            for component in &expected.component_keys {
                name(m, component)?;
                let mut found = false;
                for rule in &p.rules {
                    m.visit("Stage.expected_component_producer")?;
                    if same(m, &rule.activity, &expected.activity)?
                        && same(m, &rule.component_key, component)?
                    {
                        template.component_named_with_work(component, &rule.component_name, m)?;
                        found = true;
                    }
                }
                if !found {
                    return Err(bad("Stage expected component producer"));
                }
            }
            let last = stage
                .window
                .anchor_path
                .last()
                .ok_or_else(|| bad("Stage anchor path"))?;
            let link = product
                .link_with_work(last, m)?
                .ok_or_else(|| bad("Stage anchor link"))?;
            for target in link.predecessor_activities() {
                if same(m, target, &expected.activity)? {
                    product.validate_selector_path_with_work(
                        &stage.closure_activity,
                        &stage.window.anchor_path,
                        &expected.activity,
                        m,
                    )?;
                    anchor = true;
                }
            }
        }
        if !anchor {
            return Err(bad("Stage anchor expected member path"));
        }
        let mut cap = 0usize;
        let mut share = 0usize;
        let mut base = false;
        for rule in &p.rules {
            m.visit("Stage.closure_rule")?;
            if !same(m, &rule.activity, &stage.closure_activity)? {
                continue;
            }
            match &rule.formula {
                Formula::ClosureCap { stage: name, .. } if same(m, name, &stage.name)? => {
                    cap = crate::validation_work::usize_add(m, cap, 1)?;
                }
                Formula::ClosureShare { .. } => {
                    share = crate::validation_work::usize_add(m, share, 1)?;
                }
                Formula::Fixed { .. } | Formula::Unit { .. } => {
                    base |= same(m, &rule.component_key, &stage.closure_component_key)?;
                }
                _ => return Err(bad("closure share formula matrix")),
            }
        }
        if !base
            || !crate::validation_work::compare(m, cap, 1usize)?.is_eq()
            || crate::validation_work::compare(m, share, 1usize)?.is_gt()
        {
            return Err(bad("Stage closure base/cap/share"));
        }
    }
    Ok(())
}
fn percentage(m: &mut CoreMeter, text: &str) -> Result<()> {
    let r = arithmetic::decimal(m, text)?;
    if meter::cmp(m, &r.numerator, &BigInt::from(0))? == Ordering::Less {
        return Err(bad("negative percent"));
    }
    // Original S12 §6 FixedOrPercent Decimal0to100 and compile step3.
    let upper = meter::mul(m, &r.denominator, &BigInt::from(100))?;
    if meter::cmp(m, &r.numerator, &upper)? == Ordering::Greater {
        return Err(bad("percent outside [0,100]"));
    }
    Ok(())
}

fn modifier_amount(m: &mut CoreMeter, value: &super::model::Percent) -> Result<()> {
    if value.kind() == "percent" {
        return percentage(m, value.value());
    }
    let ratio = arithmetic::decimal(m, value.value())?;
    if meter::cmp(m, &ratio.numerator, &BigInt::from(0))?.is_lt() {
        return Err(bad("negative fixed modifier"));
    }
    Ok(())
}

fn component_modifier_basis(
    m: &mut CoreMeter,
    p: &Policy,
    consumer: &super::model::Rule,
    consumer_index: usize,
    template: &SlotTemplate,
    basis: &super::model::Basis,
) -> Result<()> {
    let current =
        template.component_named_with_work(&consumer.component_key, &consumer.component_name, m)?;
    let consumer_phase = super::model::instruction_phase(consumer, true);
    let mut producer = None;
    for (index, rule) in p.rules.iter().enumerate() {
        m.visit("component.named_basis_rule")?;
        if !super::compiler::same_literal(m, &rule.component_key, &basis.component_key)?
            || !super::compiler::same_literal(m, &rule.activity, &consumer.activity)?
            || !super::compiler::same_literal(m, &rule.binding_slot, &consumer.binding_slot)?
        {
            continue;
        }
        if producer.replace((index, rule)).is_some() {
            return Err(bad("ambiguous named component producer"));
        }
    }
    let (producer_index, rule) = producer.ok_or_else(|| bad("missing named component producer"))?;
    let phase = super::model::instruction_phase(rule, true);
    if phase > consumer_phase
        || phase == consumer_phase && producer_index >= consumer_index
        || matches!(
            rule.formula,
            Formula::ObserveCost { .. } | Formula::Discount { .. }
        )
    {
        return Err(bad("named basis must be earlier monetary producer"));
    }
    let original =
        template.component_named_with_work(&rule.component_key, &rule.component_name, m)?;
    if current.book() != TemplateBook::Retail
        || original.book() != TemplateBook::Retail
        || current.root_key() != original.root_key()
        || current.root_key() == current.key()
    {
        return Err(bad("named modifier original direct-root association"));
    }
    let selected = match basis.view.as_str() {
        "booked_base" => {
            rule.phase == "base"
                && matches!(rule.formula, Formula::Fixed { .. } | Formula::Unit { .. })
        }
        "booked_premium" => rule.phase == "premium",
        "prior_net" => matches!(rule.phase.as_str(), "base" | "premium"),
        _ => false,
    };
    if !selected {
        return Err(bad("named component basis view"));
    }
    Ok(())
}
