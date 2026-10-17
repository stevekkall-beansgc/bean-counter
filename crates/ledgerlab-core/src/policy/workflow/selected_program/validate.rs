//! Selected semantic relations; data validation supplies no host authentication.
use super::super::{
    FactSchema, Product, SlotTemplate,
    product::{BindingRelation, MissingDependency, Primitive},
    template::TemplateBook,
    transition_constructor::{CoreMeter, arithmetic, meter},
};
use super::model::{Effect, Formula, Policy,Rule,Amount};
use crate::{Error, Result};
use num_bigint::BigInt;
use std::cmp::Ordering;
fn bad(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn cost_component(m:&mut CoreMeter,r:&Rule,book:TemplateBook,slot_book:&str,root:&str,key:&str,evidence_slot:&str,ceiling:&Amount)->Result<()> {
    name(m,evidence_slot)?;
    if !super::compiler::same_literal(m,&r.phase,"cost")? || !super::compiler::same_literal(m,root,key)? {return Err(bad("cost phase/direct root"));}
    let expected=match book{TemplateBook::Retail=>"retail",TemplateBook::Supplier=>"supplier",_=>return Err(bad("payable cost component book"))};
    if !super::compiler::same_literal(m,expected,slot_book)?{return Err(bad("cost slot book"));}
    if !super::compiler::same_literal(m,&ceiling.currency,"USD")? || !crate::validation_work::compare(m,ceiling.scale,18u8)?.is_eq(){return Err(bad("cost ceiling currency/scale"));}
    let n=arithmetic::integer(m,&ceiling.atoms,false)?;
    let max=arithmetic::integer(m,"999999999999999999999999999999",false)?;
    if meter::cmp(m,&n,&0.into())?.is_lt() || meter::cmp(m,&n,&max)?.is_gt(){return Err(bad("cost ceiling amount"));}
    Ok(())
}
fn delayed_costs(m:&mut CoreMeter,p:&Policy)->Result<()> {
    use super::compiler::same_literal as same;
    if crate::validation_work::compare(m,p.delayed_costs.len(),64usize)?.is_gt(){return Err(bad("delayed cost declarations"));}
    for (i,declaration) in p.delayed_costs.iter().enumerate(){
        m.visit("cost.delayed_declaration")?;
        name(m,&declaration.component_key)?;name(m,&declaration.evidence_source_slot)?;
        if !same(m,&declaration.on_missing,"wait")?{return Err(bad("delayed cost missing rule"));}
        arithmetic::integer(m,&declaration.finalize_within_us,false)?;
        for previous in &p.delayed_costs[..i]{if same(m,&previous.component_key,&declaration.component_key)?{return Err(bad("duplicate delayed cost component"));}}
        let mut producer=None;
        for rule in &p.rules {
            m.visit("cost.delayed_supplier_producer")?;
            if !same(m,&rule.component_key,&declaration.component_key)?{continue;}
            if producer.is_some(){return Err(bad("ambiguous delayed cost producer"));}
            match &rule.formula{
                Formula::SupplierFinalCost{evidence_slot,..}=>{
                    if !same(m,evidence_slot,&declaration.evidence_source_slot)?{return Err(bad("delayed cost evidence nomination"));}
                    producer=Some(rule);
                },
                _=>return Err(bad("delayed cost must name supplier final producer")),
            }
        }
        if producer.is_none(){return Err(bad("missing delayed supplier producer"));}
    }
    for rule in &p.rules {
        match &rule.formula{
            Formula::SupplierFinalCost{..}=>{
                let mut count=0usize;
                for declaration in &p.delayed_costs{m.visit("cost.required_delayed_reservation")?;if same(m,&declaration.component_key,&rule.component_key)?{count=crate::validation_work::usize_add(m,count,1)?;}}
                if !crate::validation_work::compare(m,count,1usize)?.is_eq(){return Err(bad("supplier producer needs exact delayed declaration"));}
            },
            Formula::FinalCost{supplier_slot,evidence_slot,..}=>{
                let mut count=0usize;
                for supplier in &p.rules{
                    m.visit("cost.original_pass_through_supplier_join")?;
                    if !same(m,&supplier.activity,&rule.activity)? || !same(m,&supplier.binding_slot,supplier_slot)?{continue;}
                    if let Formula::SupplierFinalCost{evidence_slot:source,..}=&supplier.formula{
                        if !same(m,source,evidence_slot)?{return Err(bad("pass through and supplier evidence differ"));}
                        count=crate::validation_work::usize_add(m,count,1)?;
                    }
                }
                if !crate::validation_work::compare(m,count,1usize)?.is_eq(){return Err(bad("pass through needs unique original supplier obligation"));}
            },
            _=>{},
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
    owner_local_cost:bool,
) -> Result<()> {
    name(m, &p.policy_id)?;
    if !m.check("policy.bounds", || {
        !p.rules.is_empty()
            && p.rules.len() <= 64
            && p.families.len() <= 64
            && (if owner_local_cost{!p.binding_slots.is_empty()&&p.binding_slots.len()<=16}else{p.binding_slots.len()==1})
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
        if !m.check("slot.book", || s.book == "retail" || owner_local_cost&&s.book=="supplier")? {
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
            (c.book() == TemplateBook::Retail&&slot.book=="retail" || owner_local_cost&&c.book()==TemplateBook::Supplier&&slot.book=="supplier")
                && (owner_local_cost||a.funding_leg() == Some(slot.funding_leg.as_str()))
                && slot.allowed_activities.contains(&r.activity)
                && (r.dependency == "on_missing_refuse" || (p.schema=="ledger-workflow-policy/3"||p.schema=="ledger-workflow-policy/4") && matches!(r.formula,Formula::AppFinalCost{..}) && r.dependency=="on_missing_wait")
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
            Formula::AppFinalCost{evidence_slot,multiplier,markup,ceiling}=>{
                if !super::compiler::same_literal(m,&p.schema,"ledger-workflow-policy/3")? && !super::compiler::same_literal(m,&p.schema,"ledger-workflow-policy/4")?{return Err(bad("app cost recipe"));}
                cost_component(m,r,c.book(),slot.book.as_str(),c.root_key(),c.key(),evidence_slot,ceiling)?;
                if !super::compiler::same_literal(m,&slot.book,"retail")?{return Err(bad("app cost retail book"));}
                let n=arithmetic::integer(m,&multiplier.numerator,false)?;
                let d=arithmetic::integer(m,&multiplier.denominator,false)?;
                if meter::cmp(m,&n,&0.into())?.is_lt()||!meter::cmp(m,&d,&0.into())?.is_gt(){return Err(bad("app cost original multiplier"));}
                let value=arithmetic::decimal(m,markup)?;
                if meter::cmp(m,&value.numerator,&0.into())?.is_lt(){return Err(bad("negative app cost markup"));}
            },
            Formula::FinalCost{evidence_slot,supplier_slot,multiplier,markup,ceiling}=>{
                if !owner_local_cost{return Err(bad("final_cost recipe"));}
                cost_component(m,r,c.book(),slot.book.as_str(),c.root_key(),c.key(),evidence_slot,ceiling)?;
                if slot.book!="retail"{return Err(bad("retail final_cost book"));}
                name(m,supplier_slot)?;
                let mut supplier=None;
                for original in &p.binding_slots{
                    m.visit("cost.original_supplier_binding_slot")?;
                    if super::compiler::same_literal(m,&original.name,supplier_slot)?{
                        if supplier.is_some(){return Err(bad("duplicate supplier binding slot"));}
                        supplier=Some(original);
                    }
                }
                let supplier=supplier.ok_or_else(||bad("missing original supplier binding slot"))?;
                if supplier.book!="supplier" || !supplier.allowed_activities.contains(&r.activity){return Err(bad("supplier slot activity/book"));}
                let n=arithmetic::integer(m,&multiplier.numerator,false)?;
                let d=arithmetic::integer(m,&multiplier.denominator,false)?;
                if meter::cmp(m,&n,&0.into())?.is_lt() || !meter::cmp(m,&d,&0.into())?.is_gt(){return Err(bad("final_cost multiplier"));}
                let markup=arithmetic::decimal(m,markup)?;
                if meter::cmp(m,&markup.numerator,&0.into())?.is_lt(){return Err(bad("negative final_cost markup"));}
            },
            Formula::SupplierFinalCost{evidence_slot,ceiling}=>{
                if !owner_local_cost{return Err(bad("supplier_final_cost recipe"));}
                cost_component(m,r,c.book(),slot.book.as_str(),c.root_key(),c.key(),evidence_slot,ceiling)?;
                if slot.book!="supplier"{return Err(bad("supplier_final_cost book"));}
            },
        }
    }
    if owner_local_cost{delayed_costs(m,p)?;}
    if p.schema=="ledger-workflow-policy/3" || p.schema=="ledger-workflow-policy/4" {super::app_declarations::validate(m,p,product,schemas)?;}
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
        if let Some(close)=&f.no_success_close {
            name(m,&close.close_source_slot)?;
            if p.schema!="ledger-workflow-policy/2" && p.schema!="ledger-workflow-policy/3" && p.schema!="ledger-workflow-policy/4" {return Err(bad("old family dictionary cannot nominate close"));}
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
