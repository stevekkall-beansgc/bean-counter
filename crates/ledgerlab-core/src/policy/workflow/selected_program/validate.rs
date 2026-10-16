//! Selected semantic relations; data validation supplies no host authentication.
use super::super::{
    FactSchema, Product, SlotTemplate,
    product::{BindingRelation, MissingDependency, Primitive},
    template::TemplateBook,
    transition_constructor::{CoreMeter, arithmetic, meter},
};
use super::model::{Effect, Formula, Policy};
use crate::{Error, Result};
use num_bigint::BigInt;
use std::cmp::Ordering;
fn bad(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
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
) -> Result<()> {
    name(m, &p.policy_id)?;
    if !m.check("policy.bounds", || {
        !p.rules.is_empty()
            && p.rules.len() <= 64
            && p.families.len() <= 64
            && p.binding_slots.len() == 1
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
        if !m.check("slot.book", || s.book == "retail")? {
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
        if !m.check("rule.primitive", || a.primitive() == Primitive::Work)? {
            return Err(bad("rule requires work"));
        }
        let c = template.component_named_with_work(&r.component_key, &r.component_name, m)?;
        let slot = p
            .binding_slots
            .iter()
            .find(|s| s.name == r.binding_slot)
            .ok_or_else(|| bad("binding slot"))?;
        if !m.check("rule.role_book_funding", || {
            c.book() == TemplateBook::Retail
                && a.funding_leg() == Some(slot.funding_leg.as_str())
                && slot.allowed_activities.contains(&r.activity)
                && r.dependency == "on_missing_refuse"
        })? {
            return Err(bad("rule role/book/funding/dependency"));
        }
        match &r.formula {
            Formula::Fixed { amount } | Formula::Unit { rate: amount, .. } => {
                if !m.check("base.phase_root", || {
                    r.phase == "base" && c.root_key() == c.key()
                })? {
                    return Err(bad("base phase/root"));
                }
                let ratio = arithmetic::decimal(m, amount)?;
                if meter::cmp(m, &ratio.numerator, &BigInt::from(0))? == Ordering::Less {
                    return Err(bad("negative base"));
                }
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
                percentage(m, &amount.percent)?;
            }
        }
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
                prior.family_key == f.family_key || prior.selector == f.selector
            })? {
                return Err(bad("duplicate selected target family"));
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
        for link in &f.selector.path {
            m.visit("path.link_lookup")?;
            let l = product
                .link_with_work(link, m)?
                .ok_or_else(|| bad("path link"))?;
            if !m.check("path.selected", || {
                l.maximum() == 1
                    && l.minimum() == 1
                    && l.binding_relation() == BindingRelation::SameBinding
                    && l.on_missing() == MissingDependency::Refuse
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
                && code["provenance"]["source_slot"] == f.source_slot
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
                    percentage(m, &amount.percent)?
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
                        && field["provenance"]["source_slot"] == f.source_slot
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
