//! Selected exact pricing, invoked only by the shared draft constructor after
//! typed fact and authority-data validation. It supplies no public calculator.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, Phase,
    arithmetic::{self, Ratio},
    meter, shape,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use num_traits::One;
use serde_json::Value;
#[derive(Clone, Debug)]
pub(super) struct PricedInstruction {
    pub ordinal: String,
    pub component_key: String,
    pub root_key: String,
    pub role_slot: String,
    pub binding_slot: String,
    pub basis_components: Vec<String>,
    pub rational: Ratio,
    pub atoms: i128,
}
fn fail(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
pub(super) fn work(
    program: &SelectedProgram,
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
) -> Result<Vec<PricedInstruction>> {
    let instructions = program.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .ok_or_else(|| fail("compiled instructions"))?;
    let components = program.compiled_plan().canonical_value()["component_slots"]
        .as_array()
        .ok_or_else(|| fail("compiled components"))?;
    price_instructions(instructions, components, activity, normalized_facts, m)
}
pub(super) fn price_instructions(
    instructions: &[Value],
    components: &[Value],
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
) -> Result<Vec<PricedInstruction>> {
    m.phase(Phase::Pricing);
    let scale = arithmetic::integer(m, "1000000000000000000", false)?;
    let atom_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    let scale = Ratio {
        numerator: scale,
        denominator: BigInt::one(),
    };
    let mut output: Vec<PricedInstruction> = Vec::new();
    for instruction in instructions {
        if !shape::same(
            m,
            &instruction["opcode"],
            &serde_json::json!("evaluate_rule"),
        )? {
            continue;
        }
        let rule = &instruction["rule"];
        if !shape::same(m, &rule["activity"], &serde_json::json!(activity))? {
            continue;
        }
        m.visit("pricing.instruction")?;
        let key = rule["component_key"]
            .as_str()
            .ok_or_else(|| fail("component key"))?;
        let mut component = None;
        for candidate in components {
            if shape::same(m, &candidate["key"], &serde_json::json!(key))? {
                component = Some(candidate);
                break;
            }
        }
        let component = component.ok_or_else(|| fail("compiled component missing"))?;
        let formula = &rule["formula"];
        let mut basis_components = Vec::new();
        m.visit("pricing.formula_variant")?;
        let rational = match formula["kind"]
            .as_str()
            .ok_or_else(|| fail("formula kind"))?
        {
            "fixed" => {
                let amount = arithmetic::decimal(
                    m,
                    formula["amount"]
                        .as_str()
                        .ok_or_else(|| fail("fixed amount"))?,
                )?;
                arithmetic::multiply(m, &amount, &scale)?
            }
            "unit" => {
                let field = formula["quantity_field"]
                    .as_str()
                    .ok_or_else(|| fail("quantity field"))?;
                m.visit("pricing.fact_lookup")?;
                let quantity = arithmetic::integer(
                    m,
                    normalized_facts[field]
                        .as_str()
                        .ok_or_else(|| fail("normalized integer fact"))?,
                    true,
                )?;
                let rate = arithmetic::decimal(
                    m,
                    formula["rate"].as_str().ok_or_else(|| fail("unit rate"))?,
                )?;
                let quantity = Ratio {
                    numerator: quantity,
                    denominator: BigInt::one(),
                };
                let amount = arithmetic::multiply(m, &quantity, &rate)?;
                arithmetic::multiply(m, &amount, &scale)?
            }
            "discount" => {
                let base = formula["basis"]["component_key"]
                    .as_str()
                    .ok_or_else(|| fail("basis key"))?;
                let binding_slot = rule["binding_slot"]
                    .as_str()
                    .ok_or_else(|| fail("binding slot"))?;
                let mut prior = None;
                for candidate in &output {
                    if shape::same_text(m, &candidate.component_key, base)?
                        && shape::same_text(m, &candidate.binding_slot, binding_slot)?
                    {
                        prior = Some(candidate);
                        break;
                    }
                }
                let prior = prior.ok_or_else(|| fail("same work named basis missing"))?;
                basis_components.push(base.to_owned());
                let percentage = arithmetic::decimal(
                    m,
                    formula["amount"]["percent"]
                        .as_str()
                        .ok_or_else(|| fail("percentage"))?,
                )?;
                let denominator = meter::mul(m, &percentage.denominator, &BigInt::from(100))?;
                let negative = meter::neg(m, &percentage.numerator)?;
                let percent = arithmetic::rational(m, negative, denominator)?;
                arithmetic::multiply(
                    m,
                    &Ratio {
                        numerator: BigInt::from(prior.atoms),
                        denominator: BigInt::one(),
                    },
                    &percent,
                )?
            }
            _ => return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE", "selected formula")),
        };
        let atoms = arithmetic::rounded(m, &rational, &atom_limit)?;
        output.push(PricedInstruction {
            ordinal: instruction["ordinal"]
                .as_str()
                .ok_or_else(|| fail("ordinal"))?
                .to_owned(),
            component_key: key.to_owned(),
            root_key: component["root_key"]
                .as_str()
                .ok_or_else(|| fail("root key"))?
                .to_owned(),
            role_slot: component["role_slot"]
                .as_str()
                .ok_or_else(|| fail("role slot"))?
                .to_owned(),
            binding_slot: rule["binding_slot"]
                .as_str()
                .ok_or_else(|| fail("binding slot"))?
                .to_owned(),
            basis_components,
            rational,
            atoms,
        });
    }
    Ok(output)
}

/// One original family alternative, evaluated only against its frozen named
/// component net. This is shared by initial and replacement outcome records;
/// no current policy, aggregate invoice, or current statement enters the basis.
pub(super) struct PricedOutcome {
    pub ordinal: String,
    pub root_key: String,
    pub family_key: String,
    pub rational: Ratio,
    pub atoms: i128,
    pub none: bool,
}
macro_rules! paid_call {
    ($f:path,$m:expr,$a:expr $(,)?) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr $(,)?) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr $(,)?) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
/// Explicit correction with no replacement code or facts. Select only the
/// original compiled family instruction; do not invent a none-code attestation.
pub(super) fn explicit_none(
    program: &SelectedProgram,
    family_key: &str,
    m: &mut CoreMeter,
) -> Result<PricedOutcome> {
    m.phase(Phase::Pricing);
    let mut selected = None;
    for instruction in paid_call!(
        shape::array,
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions"
    )? {
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, instruction, "opcode")?,
            "evaluate_family"
        )? {
            continue;
        }
        let family = paid_call!(shape::field, m, instruction, "family")?;
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, family, "family_key")?,
            family_key
        )? {
            continue;
        }
        m.visit("pricing.unique_family")?;
        if selected.is_some() {
            return Err(fail("ambiguous original family"));
        }
        selected = Some((instruction, family));
    }
    let (instruction, family) = selected.ok_or_else(|| fail("original family missing"))?;
    Ok(PricedOutcome {
        ordinal: paid_call!(shape::text, m, instruction, "ordinal")?.to_owned(),
        root_key: paid_call!(shape::text, m, &family["selector"], "component_key")?.to_owned(),
        family_key: family_key.to_owned(),
        rational: Ratio {
            numerator: 0.into(),
            denominator: 1.into(),
        },
        atoms: 0,
        none: true,
    })
}
pub(super) fn outcome(
    program: &SelectedProgram,
    family_key: &str,
    code: &str,
    original_net: &BigInt,
    m: &mut CoreMeter,
) -> Result<PricedOutcome> {
    m.phase(Phase::Pricing);
    if meter::cmp(m, original_net, &0.into())?.is_lt() {
        return Err(fail("negative frozen family net"));
    }
    let mut selected = None;
    for instruction in program.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .ok_or_else(|| fail("original instructions"))?
    {
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, instruction, "opcode")?,
            "evaluate_family"
        )? {
            continue;
        }
        let family = paid_call!(shape::field, m, instruction, "family")?;
        if paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, family, "family_key")?,
            family_key
        )? {
            m.visit("pricing.unique_family")?;
            if selected.is_some() {
                return Err(fail("ambiguous original family"));
            }
            selected = Some((instruction, family));
        }
    }
    let (instruction, family) = selected.ok_or_else(|| fail("original family missing"))?;
    let choice = paid_call!(
        shape::named,
        m,
        paid_call!(shape::array, m, family, "codes")?,
        "code",
        code
    )?;
    let effect = paid_call!(shape::field, m, choice, "effect")?;
    let kind = paid_call!(shape::text, m, effect, "kind")?;
    let none = paid_call!(shape::same_text, m, kind, "none")?;
    let mut rational = Ratio {
        numerator: 0.into(),
        denominator: 1.into(),
    };
    if !none {
        let discount = paid_call!(shape::same_text, m, kind, "discount")?;
        if !discount && !paid_call!(shape::same_text, m, kind, "premium")? {
            return Err(fail("original family effect"));
        }
        let formula = paid_call!(shape::field, m, effect, "amount")?;
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, formula, "kind")?,
            "percent"
        )? {
            return Err(Error::new(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "selected family percent",
            ));
        }
        let scalar = paid_call!(shape::text, m, formula, "percent")?;
        let percent = arithmetic::decimal(m, scalar)?;
        let hundred = meter::mul(m, &percent.denominator, &100.into())?;
        if meter::cmp(m, &percent.numerator, &0.into())?.is_lt()
            || meter::cmp(m, &percent.numerator, &hundred)?.is_gt()
        {
            return Err(fail("original family percent interval"));
        }
        let numerator = if discount {
            meter::neg(m, &percent.numerator)?
        } else {
            percent.numerator
        };
        let percent = arithmetic::rational(m, numerator, hundred)?;
        rational = arithmetic::multiply(
            m,
            &Ratio {
                numerator: original_net.clone(),
                denominator: 1.into(),
            },
            &percent,
        )?;
    }
    let atom_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    let atoms = arithmetic::rounded(m, &rational, &atom_limit)?;
    if !none {
        let bound_key = if meter::cmp(m, &atoms.into(), &0.into())?.is_lt() {
            "maximum_discount"
        } else {
            "maximum_premium"
        };
        let declaration = paid_call!(shape::field, m, family, bound_key)?;
        let scalar = paid_call!(shape::text, m, declaration, "atoms")?;
        let limit = arithmetic::integer(m, scalar, false)?;
        let magnitude = meter::absolute(m, &atoms.into())?;
        if meter::cmp(m, &magnitude, &limit)?.is_gt() {
            return Err(fail("original family amount ceiling"));
        }
        if paid_call!(shape::same_text, m, bound_key, "maximum_discount")?
            && meter::cmp(m, &magnitude, original_net)?.is_gt()
        {
            return Err(fail("discount exceeds frozen original net"));
        }
    }
    Ok(PricedOutcome {
        ordinal: paid_call!(shape::text, m, instruction, "ordinal")?.to_owned(),
        root_key: paid_call!(shape::text, m, &family["selector"], "component_key")?.to_owned(),
        family_key: family_key.to_owned(),
        rational,
        atoms,
        none,
    })
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn explicit_no_replacement_uses_original_ordinal_without_a_code() {
        let (program, _, _, _) = super::super::first_work_tests::test_inputs();
        let mut meter = CoreMeter::new(65536).unwrap();
        let price = explicit_none(&program, "fixture.measure:assessment", &mut meter).unwrap();
        let choice = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &999000000000000000_i128.into(),
            &mut meter,
        )
        .unwrap();
        assert_eq!(price.ordinal, choice.ordinal);
        assert_eq!(price.root_key, choice.root_key);
        assert_eq!(price.rational.numerator, 0.into());
        assert_eq!(price.rational.denominator, 1.into());
        assert!(price.none);
        assert_eq!(price.atoms, 0);
        assert!(explicit_none(&program, "absent:family", &mut meter).is_err());
    }
    #[test]
    fn original_family_none_zero_and_signed_alternatives_remain_distinct() {
        // The production selected compiler supplies the program. No expected
        // economic output is supplied as a constructor/pricing operand.
        let (program, _, _, _) = super::super::first_work_tests::test_inputs();
        let mut m = CoreMeter::new(65536).unwrap();
        let net = arithmetic::integer(&mut m, "999000000000000000", false).unwrap();
        let success = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &net,
            &mut m,
        )
        .unwrap();
        assert_eq!(success.atoms, 199800000000000000);
        assert!(!success.none);
        let rebate = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:rebate",
            &net,
            &mut m,
        )
        .unwrap();
        assert_eq!(rebate.atoms, -99900000000000000);
        assert!(!rebate.none);
        let none = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:none",
            &net,
            &mut m,
        )
        .unwrap();
        assert_eq!(none.atoms, 0);
        assert!(none.none);
        let zero = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &0.into(),
            &mut m,
        )
        .unwrap();
        assert_eq!(zero.atoms, 0);
        assert!(!zero.none);
        assert!(
            outcome(
                &program,
                "fixture.measure:assessment",
                "fixture.measure:unknown",
                &net,
                &mut m
            )
            .is_err()
        );
        assert!(
            outcome(
                &program,
                "fixture.measure:assessment",
                "fixture.measure:success",
                &(-1).into(),
                &mut m
            )
            .is_err()
        );
    }
}
