//! Customer-local positive-obligation bounds for ONE original work. This is
//! neither a cash pool nor a payable/pricing engine. Exact original pricing is
//! reused; discounts cannot buy contingent premium headroom. A closed family
//! cannot coexist with replacement postings after a root correction.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, arithmetic, authority::Terms, data::ExactRef, inventory::entry, meter, pricing,
    shape, targets,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde_json::{Value, json};
pub(super) struct RootExposureBound {
    pub root_key: String,
    pub original_positive: BigInt,
    pub original_net: BigInt,
    pub simultaneous_family_premium: BigInt,
    pub corrected_positive: BigInt,
    pub positive_bound: BigInt,
}
pub(super) struct WorkExposureBound {
    pub work_claim_ref: ExactRef,
    pub binding_slot: String,
    pub roots: Vec<RootExposureBound>,
    pub maximum_positive: BigInt,
}
fn fail(s: &str) -> Error {
    shape::fail(s)
}
fn same(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn schema<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    r: &Value,
) -> Result<&'a crate::policy::workflow::FactSchema> {
    for (reference, schema) in p.fact_schemas() {
        m.visit("funding.original_fact_schema")?;
        if shape::same(m, &json!(reference), r)? {
            return Ok(schema);
        }
    }
    Err(fail("funding original schema missing"))
}
/// Derive a maximum permitted normalized integer vector from original schema
/// AND both accepted quantity ceilings. It is an abstract prospective operand,
/// not a new assertion, claim, proof, request, or substituted billed quantity.
fn maximum_facts(
    m: &mut CoreMeter,
    p: &SelectedProgram,
    payload: &Value,
    t: &Terms,
) -> Result<Value> {
    let activity = shape::text(m, payload, "activity")?;
    let original_schema = schema(m, p, &payload["fact_schema_ref"])?;
    let mut facts = payload["facts"].clone();
    let mut fields: Vec<String> = vec![];
    for instruction in shape::array(m, p.compiled_plan().canonical_value(), "typed_instructions")? {
        m.visit("funding.original_quantity_producer")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_rule")? {
            continue;
        }
        let rule = &instruction["rule"];
        let declared_activity = shape::text(m, rule, "activity")?;
        if !shape::same_text(m, declared_activity, activity)? {
            continue;
        }
        let formula = &rule["formula"];
        let kind = shape::text(m, formula, "kind")?;
        if !shape::same_text(m, kind, "unit")? {
            continue;
        }
        let field = shape::text(m, formula, "quantity_field")?;
        let mut seen = false;
        for prior in &fields {
            if shape::same_text(m, prior, field)? {
                seen = true;
                break;
            }
        }
        if seen {
            continue;
        }
        fields.push(field.to_owned());
        let declarations = shape::array(m, original_schema.canonical_value(), "fields")?;
        let declaration = shape::named(m, declarations, "name", field)?;
        same(m, &declaration["required"], &json!(true))?;
        same(m, &declaration["type"]["kind"], &json!("integer"))?;
        let raw = shape::text(m, &declaration["type"], "max")?;
        let mut maximum = arithmetic::integer(m, raw, true)?;
        for limits in [
            &t.binding["quantity_ceilings"],
            &t.slot_terms["quantity_ceilings"],
        ] {
            for bound in limits
                .as_array()
                .ok_or_else(|| fail("funding quantity ceiling array"))?
            {
                m.visit("funding.accepted_quantity_ceiling")?;
                let name = shape::text(m, bound, "activity")?;
                let quantity = shape::text(m, bound, "field")?;
                if !shape::same_text(m, name, activity)? || !shape::same_text(m, quantity, field)? {
                    continue;
                }
                same(m, &bound["unit"], &declaration["type"]["unit"])?;
                let raw = shape::text(m, bound, "maximum")?;
                let bound = arithmetic::decimal(m, raw)?;
                if !meter::cmp(m, &bound.numerator, &0.into())?.is_gt() {
                    return Err(fail("nonpositive future quantity ceiling"));
                }
                let integral = meter::div(m, &bound.numerator, &bound.denominator)?;
                if meter::cmp(m, &integral, &maximum)?.is_lt() {
                    maximum = integral;
                }
            }
        }
        let raw_min = shape::text(m, &declaration["type"], "min")?;
        let minimum = arithmetic::integer(m, raw_min, true)?;
        if !meter::cmp(m, &maximum, &0.into())?.is_gt()
            || meter::cmp(m, &maximum, &minimum)?.is_lt()
        {
            return Err(fail("no permitted positive future quantity"));
        }
        facts[field] = json!(maximum.to_string());
    }
    let normalized = original_schema.normalize_value_with_work(&facts, m)?;
    entry::quantities(
        m,
        &t.binding,
        &t.slot_terms,
        activity,
        normalized.canonical_value(),
    )?;
    Ok(normalized.canonical_value().clone())
}
fn root_position(
    m: &mut CoreMeter,
    roots: &[RootExposureBound],
    key: &str,
) -> Result<Option<usize>> {
    for (i, root) in roots.iter().enumerate() {
        m.visit("funding.original_root")?;
        if shape::same_text(m, &root.root_key, key)? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}
/// The caller must already resolve the exact accepted terms/program/work and
/// validate the additive exposure declaration. This result is pure derived DATA
/// and has no constructor for a store balance or host authorization capability.
pub(super) fn derive_per_work_bound(
    m: &mut CoreMeter,
    p: &SelectedProgram,
    payload: &Value,
    claim: &ExactRef,
    t: &Terms,
) -> Result<WorkExposureBound> {
    same(
        m,
        &t.slot_terms["schema"],
        &json!("ledger-workflow-slot-binding-terms/3"),
    )?;
    same(m, &t.slot_terms["exposure_scope"], &json!("original_work"))?;
    same(m, &t.slot_terms["book"], &json!("retail"))?;
    let activity = shape::text(m, payload, "activity")?;
    let binding_slot = shape::text(m, &t.slot_terms, "slot")?.to_owned();
    let original = pricing::work(p, activity, &payload["facts"], m)?;
    let maximum = maximum_facts(m, p, payload, t)?;
    let corrected = pricing::work(p, activity, &maximum, m)?;
    let mut roots: Vec<RootExposureBound> = vec![];
    for price in original {
        m.visit("funding.original_component")?;
        if !shape::same_text(m, &price.binding_slot, &binding_slot)? {
            return Err(fail("funding component binding slot"));
        }
        let index = match root_position(m, &roots, &price.root_key)? {
            Some(i) => i,
            None => {
                let index = roots.len();
                roots.push(RootExposureBound {
                    root_key: price.root_key.clone(),
                    original_positive: 0.into(),
                    original_net: 0.into(),
                    simultaneous_family_premium: 0.into(),
                    corrected_positive: 0.into(),
                    positive_bound: 0.into(),
                });
                index
            }
        };
        let atoms = BigInt::from(price.atoms);
        roots[index].original_net = meter::add(m, &roots[index].original_net, &atoms)?;
        if meter::cmp(m, &atoms, &0.into())?.is_gt() {
            roots[index].original_positive =
                meter::add(m, &roots[index].original_positive, &atoms)?;
        }
    }
    for price in corrected {
        m.visit("funding.corrected_component")?;
        let index = root_position(m, &roots, &price.root_key)?
            .ok_or_else(|| fail("future price introduced an undeclared root"))?;
        let atoms = BigInt::from(price.atoms);
        if meter::cmp(m, &atoms, &0.into())?.is_gt() {
            roots[index].corrected_positive =
                meter::add(m, &roots[index].corrected_positive, &atoms)?;
        }
    }
    for instruction in shape::array(m, p.compiled_plan().canonical_value(), "typed_instructions")? {
        m.visit("funding.original_family_alternatives")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_family")? {
            continue;
        }
        let family = &instruction["family"];
        same(m, &family["selector"]["binding_slot"], &json!(binding_slot))?;
        if !shape::same(
            m,
            &family["selector"]["target_activity"],
            &payload["activity"],
        )? {
            continue;
        }
        let key = shape::text(m, &family["selector"], "component_key")?;
        let index = root_position(m, &roots, key)?
            .ok_or_else(|| fail("family positive reserve lacks original root"))?;
        if meter::cmp(m, &roots[index].original_net, &0.into())?.is_lt() {
            return Err(fail("original named root has excessive aggregate discount"));
        }
        // MAX over exclusive codes happens in the existing production helper;
        // SUM over distinct families happens here. Discounts never offset this.
        let component_map = shape::array(m, &t.instantiation, "component_map")?;
        let component = shape::named(m, component_map, "template_key", key)?;
        let families = shape::array(m, &t.instantiation, "family_map")?;
        let family_key = shape::text(m, family, "family_key")?;
        let mapped = shape::named(m, families, "template_key", family_key)?;
        let mut found = false;
        for clause in shape::array(m, &t.slot_terms, "causal_reversal_clauses")? {
            m.visit("funding.future_permanent_closure_clause")?;
            if shape::same(m, &clause["root_component_slot"], &component["slot_id"])?
                && shape::same(m, &clause["family_slot"], &mapped["slot_id"])?
            {
                if found {
                    return Err(fail("duplicate exposure causal clause"));
                }
                same(
                    m,
                    &clause["correction_source_slot"],
                    &family["correction_source_slot"],
                )?;
                same(
                    m,
                    &clause["mode"],
                    &json!("exact_inverse_and_permanent_close"),
                )?;
                same(
                    m,
                    &clause["standalone_deadline_effect"],
                    &json!("does_not_expire_preaccepted_causal_reversal"),
                )?;
                found = true;
            }
        }
        if !found {
            return Err(fail(
                "exposure branch requires original permanent closure clause",
            ));
        }
        let (premium, _discount) = targets::maxima(m, family, &roots[index].original_net)?;
        roots[index].simultaneous_family_premium =
            meter::add(m, &roots[index].simultaneous_family_premium, &premium)?;
    }
    let mut total = BigInt::from(0);
    for root in &mut roots {
        m.visit("funding.simultaneously_permitted_branch")?;
        let original = meter::add(
            m,
            &root.original_positive,
            &root.simultaneous_family_premium,
        )?;
        root.positive_bound = if meter::cmp(m, &original, &root.corrected_positive)?.is_gt() {
            original
        } else {
            root.corrected_positive.clone()
        };
        total = meter::add(m, &total, &root.positive_bound)?;
    }
    entry::exposure(m, &t.binding, &t.slot_terms, &total)?;
    Ok(WorkExposureBound {
        work_claim_ref: claim.clone(),
        binding_slot,
        roots,
        maximum_positive: total,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical;
    use crate::policy::workflow::selected_program::{
        SelectedCompilerSupport, SelectedSourceInput, compile_selected_program,
    };
    #[test]
    fn original_work_bound_covers_future_quantity_without_summing_exclusive_or_closed_branches() {
        // Ordinary immutable source inputs, not a saved economic result/oracle.
        let source:Value=canonical::parse(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let inputs = SelectedSourceInput {
            documents: source["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
        };
        let support = SelectedCompilerSupport::builtin().unwrap();
        let mut compile = CoreMeter::for_compile(1_048_576).unwrap();
        let program = compile_selected_program(inputs, &support, &mut compile).unwrap();
        let raw: Value =
            serde_json::from_slice(include_bytes!("fixtures/prerequisite-input-data.json"))
                .unwrap();
        let get = |name: &str| -> (ExactRef, Value) {
            let row = raw["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["name"] == name)
                .unwrap();
            let bytes = row["canonical_body_hex"]
                .as_str()
                .unwrap()
                .as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            (
                serde_json::from_value(row["reference"].clone()).unwrap(),
                serde_json::from_slice(&bytes).unwrap(),
            )
        };
        let (terms_ref, mut terms) = get("A.slot_terms");
        terms["schema"] = json!("ledger-workflow-slot-binding-terms/3");
        terms["exposure_scope"] = json!("original_work");
        let (_, binding) = get("A.binding");
        let (_, instantiation) = get("A.instantiation");
        let (_, catalog) = get("A.catalog");
        let unit = program.compiled_plan().canonical_value()["typed_instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["rule"]["formula"]["kind"] == "unit")
            .unwrap();
        let field = unit["rule"]["formula"]["quantity_field"].as_str().unwrap();
        let activity = &unit["rule"]["activity"];
        let schema_ref = program
            .source_documents()
            .iter()
            .find(|d| d.canonical_value()["schema"] == "ledger-workflow-product/1")
            .unwrap()
            .canonical_value()["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| &a["name"] == activity)
            .unwrap()["fact_schema"]
            .clone();
        let payload = json!({"activity":activity,"fact_schema_ref":schema_ref,"facts":{field:"3"}});
        let claim = ExactRef {
            kind: "workflow-work-claim".into(),
            id: format!("wclaim_{}", "1".repeat(64)),
            digest: format!("sha256:{}", "2".repeat(64)),
        };
        let mut terms = Terms {
            binding,
            accepted: json!({}),
            activation: json!({}),
            catalog,
            instantiation,
            slot_terms: terms,
            slot_terms_ref: terms_ref,
            role_ref: claim.clone(),
        };
        let mut m = CoreMeter::new(65_536).unwrap();
        let bound = derive_per_work_bound(&mut m, &program, &payload, &claim, &terms).unwrap();
        assert_eq!(
            bound.maximum_positive,
            BigInt::from(37) * BigInt::from(10u8).pow(18)
        );
        assert_eq!(bound.roots.len(), 1);
        assert_eq!(
            bound.roots[0].original_net,
            BigInt::from(999) * BigInt::from(10u8).pow(15)
        );
        assert_eq!(
            bound.roots[0].simultaneous_family_premium,
            BigInt::from(1998) * BigInt::from(10u8).pow(14)
        );
        // Each independent original work has its own aggregate; deriving one
        // bound does not create/spend/reset a shared lifetime balance.
        let mut independent = claim.clone();
        independent.id = format!("wclaim_{}", "3".repeat(64));
        assert_eq!(
            derive_per_work_bound(&mut m, &program, &payload, &independent, &terms)
                .unwrap()
                .maximum_positive,
            bound.maximum_positive
        );
        terms.slot_terms["exposure_ceiling"]["atoms"] = json!("1000000000000000000");
        assert!(derive_per_work_bound(&mut m, &program, &payload, &claim, &terms).is_err());
    }
}
