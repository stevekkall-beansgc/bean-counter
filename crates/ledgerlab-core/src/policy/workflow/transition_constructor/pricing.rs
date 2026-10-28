//! Selected exact pricing, invoked only by the shared draft constructor after
//! typed fact and authority-data validation. It supplies no public calculator.
use super::super::selected_program::SelectedProgram;
use super::{
    arithmetic::{self, Ratio},
    meter, shape, CoreMeter, Phase,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use num_traits::One;
use serde_json::Value;
pub(super) struct PricedInstruction {
    pub ordinal: String,
    pub component_key: String,
    pub root_key: String,
    pub role_slot: String,
    pub binding_slot: String,
    pub basis_components: Vec<String>,
    pub(super) prior_net_basis: bool,
    pub rational: Ratio,
    pub atoms: i128,
    pub predicate: bool,
    pub failed: bool,
    pub predicate_state: &'static str,
    // Row payload survives pop/iteration independently of the vector owner.
    pub(super) _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
/// The existing evaluator's actual result vector, with its allocation owner.
/// This is private pricing DATA, not eligibility or a different evaluator.
pub(super) struct PricedInstructions {
    rows: Vec<PricedInstruction>,
    _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl std::fmt::Debug for PricedInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PricedInstruction")
            .field("ordinal", &self.ordinal)
            .field("component_key", &self.component_key)
            .field("root_key", &self.root_key)
            .field("role_slot", &self.role_slot)
            .field("binding_slot", &self.binding_slot)
            .field("basis_components", &self.basis_components)
            .field("rational", &self.rational)
            .field("atoms", &self.atoms)
            .finish()
    }
}
impl std::fmt::Debug for PricedInstructions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.rows.fmt(f)
    }
}
impl PricedInstructions {
    pub(super) fn empty() -> Self {
        Self {
            rows: Vec::new(),
            _ownership: None,
        }
    }
}
impl std::ops::Deref for PricedInstructions {
    type Target = Vec<PricedInstruction>;
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}
impl std::ops::DerefMut for PricedInstructions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rows
    }
}
impl<'a> IntoIterator for &'a PricedInstructions {
    type Item = &'a PricedInstruction;
    type IntoIter = std::slice::Iter<'a, PricedInstruction>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}
pub(super) struct PricedIntoIter {
    rows: std::vec::IntoIter<PricedInstruction>,
    _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl Iterator for PricedIntoIter {
    type Item = PricedInstruction;
    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next()
    }
}
impl IntoIterator for PricedInstructions {
    type Item = PricedInstruction;
    type IntoIter = PricedIntoIter;
    fn into_iter(self) -> Self::IntoIter {
        PricedIntoIter {
            rows: self.rows.into_iter(),
            _ownership: self._ownership,
        }
    }
}
fn copied(m: &mut CoreMeter, value: &str) -> Result<String> {
    if m.representation_arena().is_some() {
        m.scan_bytes(value.len())?;
        m.write_bytes(value.len())?;
    }
    Ok(value.to_owned())
}
fn output_owner(
    m: &mut CoreMeter,
    n: usize,
) -> Result<Option<crate::canonical::audit_arena::ExternalBufferLease>> {
    use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
    let Some(arena) = m.representation_arena() else {
        return Ok(None);
    };
    let per = std::mem::size_of::<PricedInstruction>();
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&n.into(), &per.into()])?;
    let size = n
        .checked_mul(per)
        .ok_or_else(|| fail("pricing output dimension"))?;
    let size = w::usize_add(m, size, std::mem::size_of::<PricedInstructions>())?;
    let lease = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    Ok(Some(lease))
}
fn row_owner(
    m: &mut CoreMeter,
    instruction: &Value,
    rule: &Value,
    component: &Value,
    prior: &[PricedInstruction],
    component_recipe: bool,
) -> Result<Option<crate::canonical::audit_arena::ExternalBufferLease>> {
    use crate::validation_work as w;
    let Some(arena) = m.representation_arena() else {
        return Ok(None);
    };
    let mut size = std::mem::size_of::<PricedInstruction>();
    for (value, key) in [
        (instruction, "ordinal"),
        (rule, "component_key"),
        (component, "root_key"),
        (component, "role_slot"),
        (rule, "binding_slot"),
    ] {
        m.visit("pricing.output_string_dimension")?;
        let raw = value
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| fail("pricing typed output field"))?;
        size = w::usize_add(m, size, raw.len())?;
    }
    m.visit("pricing.output_basis_variant")?;
    if matches!(
        rule["formula"]["kind"].as_str(),
        Some("discount" | "percent")
    ) {
        m.visit("pricing.output_basis_dimension")?;
        let raw = rule["formula"]["basis"]["component_key"]
            .as_str()
            .ok_or_else(|| fail("pricing typed basis"))?;
        size = w::usize_add(m, size, raw.len())?;
        size = w::usize_add(m, size, std::mem::size_of::<String>())?;
    }
    if component_recipe
        && matches!(
            rule["formula"]["kind"].as_str(),
            Some("discount" | "percent")
        )
    {
        for row in prior {
            m.visit("pricing.component_basis_storage")?;
            size = w::usize_add(m, size, row.component_key.len())?;
            size = w::usize_add(m, size, std::mem::size_of::<String>())?;
        }
    }
    if matches!(
        rule["formula"]["kind"].as_str(),
        Some("closure_cap" | "closure_share")
    ) {
        let basis = shape::text(m, &rule["formula"], "basis_component_key")?;
        for row in prior {
            m.visit("pricing.Stage_basis_storage")?;
            if shape::same_text(m, &row.root_key, basis)? {
                size = w::usize_add(m, size, row.component_key.len())?;
                size = w::usize_add(m, size, std::mem::size_of::<String>())?;
            }
        }
    }
    // Output scalar limbs only. Arithmetic-kernel temporaries/native scratch
    // remain a separate containing proof; this lease never credits them.
    // Pinned num-bigint0.5.1 mul3 allocates x.len()+y.len()+1 digits.
    // Two512-bit operands therefore need <=17u64 digits (136 bytes),
    // including the extra carry digit retained after normalization. The
    // u32-digit build needs <=33*4 bytes and is covered by the same bound.
    size = w::usize_add(m, size, 2 * (std::mem::size_of::<BigInt>() + 136))?;
    let lease = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    Ok(Some(lease))
}
fn fail(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
fn convert_quantity(m: &mut CoreMeter, quantity: &Ratio, factor: &Value) -> Result<Ratio> {
    shape::closed(m, factor, &["numerator", "denominator"], &[])?;
    let n = shape::text(m, factor, "numerator")?;
    let d = shape::text(m, factor, "denominator")?;
    let factor = crate::money::ExactRatio::from_canonical_with_work(n, d, m)?;
    if !factor.is_positive_with_work(m)? {
        return Err(fail("positive unit normalization"));
    }
    let (numerator, denominator) = factor.into_parts();
    arithmetic::multiply(
        m,
        quantity,
        &Ratio {
            numerator,
            denominator,
        },
    )
}
fn normalized_quantity(
    program: &SelectedProgram,
    formula: &Value,
    facts: &Value,
    m: &mut CoreMeter,
) -> Result<Ratio> {
    let _scratch = m
        .representation_arena()
        .map(|a| a.reserve_external_payload(4096, m))
        .transpose()?;
    let field = shape::text(m, formula, "quantity_field")?;
    let raw_quantity = shape::text(m, facts, field)?;
    let quantity = arithmetic::decimal(m, raw_quantity)?;
    let from = shape::text(m, formula, "unit")?;
    let to = shape::text(m, formula, "normalize_to")?;
    let mut product = None;
    for doc in program.source_documents() {
        let r = doc.reference();
        let expected = program.product_ref();
        if shape::same_text(m, r.kind(), expected.kind())?
            && shape::same_text(m, r.id(), expected.id())?
            && shape::same_text(m, r.digest(), expected.digest())?
        {
            product = Some(doc.canonical_value());
            break;
        }
    }
    let product = product.ok_or_else(|| fail("original unit Product"))?;
    let units = shape::array(m, product, "units")?;
    let source = shape::named(m, units, "name", from)?;
    let target = shape::named(m, units, "name", to)?;
    shape::equal(m, &source["dimension"], &target["dimension"])?;
    m.visit("pricing.unit_terminal_target")?;
    if target.get("normalization").is_some() {
        return Err(fail("normalization requires terminal target"));
    }
    if shape::same_text(m, from, to)? {
        return Ok(quantity);
    }
    let normalization = shape::field(m, source, "normalization")?;
    let declared = shape::text(m, normalization, "to")?;
    if !shape::same_text(m, declared, to)? {
        return Err(fail("undeclared unit normalization"));
    }
    let factor = shape::field(m, normalization, "factor")?;
    convert_quantity(m, &quantity, factor)
}
pub(super) fn work(
    program: &SelectedProgram,
    activity: &str,
    inputs: (&Value, &Value),
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    work_operational(program, activity, inputs, None, m)
}
pub(super) fn work_operational(
    program: &SelectedProgram,
    activity: &str,
    inputs: (&Value, &Value),
    operational: Option<(&Value, &str)>,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    let (normalized_facts, context) = inputs;
    let condition =
        program
            .uses_conditional_work_recipe()
            .then_some((program, context, operational));
    let instructions = program.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .ok_or_else(|| fail("compiled instructions"))?;
    let components = program.compiled_plan().canonical_value()["component_slots"]
        .as_array()
        .ok_or_else(|| fail("compiled components"))?;
    if program.uses_app_owned_cost_recipe() {
        price_instructions_selected(
            instructions,
            components,
            activity,
            normalized_facts,
            m,
            (Some(CostSelection::AppDeferred), condition),
        )
    } else if program.uses_owner_local_cost_recipe() {
        price_instructions_selected(
            instructions,
            components,
            activity,
            normalized_facts,
            m,
            (Some(CostSelection::Deferred), condition),
        )
    } else {
        price_instructions_selected(
            instructions,
            components,
            activity,
            normalized_facts,
            m,
            (None, condition),
        )
    }
}
/// Real immutable Work status is separate from current authority and from
/// eligibility. Failed completion evaluates its predicate for explanation but
/// never evaluates completion-price arithmetic or produces a monetary effect.
pub(super) fn work_subject(
    program: &SelectedProgram,
    original: (&str, &Value),
    inputs: (&Value, &Value),
    operational: Option<(&Value, &str)>,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    let (activity, subject) = original;
    if !program.uses_work_lifecycle_recipe() {
        return work_operational(program, activity, inputs, operational, m);
    }
    let status = shape::text(m, subject, "status")?;
    if !matches!(status, "succeeded" | "failed") {
        return Err(fail("Work completion status"));
    }
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let components = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "component_slots",
    )?;
    price_instructions_selected_mode(
        instructions,
        components,
        activity,
        inputs.0,
        m,
        (
            Some(CostSelection::AppDeferred),
            Some((program, inputs.1, operational)),
        ),
        PriceMode::Work(status),
    )
}
pub(super) fn work_subject_exposure_bound(
    program: &SelectedProgram,
    original: (&str, &Value),
    inputs: (&Value, &Value),
    operational: Option<(&Value, &str)>,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    let (activity, subject) = original;
    if program.uses_work_lifecycle_recipe() {
        let status = shape::text(m, subject, "status")?;
        if shape::same_text(m, status, "failed")? {
            return work_subject(program, original, inputs, operational, m);
        }
    }
    work_exposure_bound(program, activity, inputs, m)
}
/// Conservative future positive exposure for the supported independent
/// Fixed/Unit producer. Eligibility is ignored ONLY in this DATA bound; actual
/// prices, targets and evaluations always use the original predicate.
pub(super) fn work_exposure_bound(
    program: &SelectedProgram,
    activity: &str,
    inputs: (&Value, &Value),
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    if !program.uses_policy_semantics_recipe() {
        return work(program, activity, inputs, m);
    }
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let components = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "component_slots",
    )?;
    price_instructions_selected_mode(
        instructions,
        components,
        activity,
        inputs.0,
        m,
        (
            Some(CostSelection::AppDeferred),
            Some((program, inputs.1, None)),
        ),
        PriceMode::Exposure,
    )
}
#[cfg(test)]
pub(super) fn price_instructions(
    instructions: &[Value],
    components: &[Value],
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    price_instructions_selected(
        instructions,
        components,
        activity,
        normalized_facts,
        m,
        (None, None),
    )
}
enum CostSelection<'a> {
    Deferred,
    #[cfg(test)]
    Final {
        supplier_slot: &'a str,
        supplier_component: Option<&'a str>,
        eligible_atoms: &'a BigInt,
    },
    AppDeferred,
    AppFinal {
        component_key: &'a str,
        eligible_atoms: &'a BigInt,
    },
}

// Concrete borrowed inputs to the same instruction evaluator. They own no
// grants, prices or identity capability and never outlive the accepted terms.
type SelectedCondition<'a> = (&'a SelectedProgram, &'a Value, Option<(&'a Value, &'a str)>);
type PricingSelections<'a> = (Option<CostSelection<'a>>, Option<SelectedCondition<'a>>);

/// Actual attributable amount and original WorkRecord facts enter the SAME
/// original instruction evaluator. There is no supplier selection or payable.
/// Evidence/source/lineage eligibility is established by the app executor.
#[cfg(test)]
pub(super) fn app_final_cost(
    program: &SelectedProgram,
    activity: &str,
    component_key: &str,
    original_inputs: (&Value, &Value),
    eligible_atoms: &BigInt,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    app_final_cost_operational(
        program,
        activity,
        component_key,
        (original_inputs, None),
        eligible_atoms,
        m,
    )
}
pub(super) fn app_final_cost_operational(
    program: &SelectedProgram,
    activity: &str,
    component_key: &str,
    original_inputs: ((&Value, &Value), Option<(&Value, &str)>),
    eligible_atoms: &BigInt,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    app_final_cost_mode(
        program,
        activity,
        component_key,
        original_inputs,
        eligible_atoms,
        m,
        PriceMode::Actual,
    )
}
pub(super) fn app_final_cost_work(
    program: &SelectedProgram,
    original: (&str, &Value),
    component_key: &str,
    original_inputs: ((&Value, &Value), Option<(&Value, &str)>),
    eligible_atoms: &BigInt,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    if !program.uses_work_lifecycle_recipe() {
        return app_final_cost_operational(
            program,
            original.0,
            component_key,
            original_inputs,
            eligible_atoms,
            m,
        );
    }
    let status = shape::text(m, original.1, "status")?;
    if !shape::same_text(m, status, "succeeded")? {
        return Err(fail("app cost requires successful original Work"));
    }
    app_final_cost_mode(
        program,
        original.0,
        component_key,
        original_inputs,
        eligible_atoms,
        m,
        PriceMode::Work(status),
    )
}
fn app_final_cost_mode(
    program: &SelectedProgram,
    activity: &str,
    component_key: &str,
    original_inputs: ((&Value, &Value), Option<(&Value, &str)>),
    eligible_atoms: &BigInt,
    m: &mut CoreMeter,
    mode: PriceMode<'_>,
) -> Result<PricedInstructions> {
    let (original_inputs, operational) = original_inputs;
    if !program.uses_app_owned_cost_recipe() {
        return Err(Error::new(
            "ORIGINAL_VERIFIER_UNAVAILABLE",
            "original app cost compiler recipe",
        ));
    }
    let (original_facts, context) = original_inputs;
    let condition =
        program
            .uses_conditional_work_recipe()
            .then_some((program, context, operational));
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let components = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "component_slots",
    )?;
    let maximum = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, eligible_atoms, &0.into())?.is_lt()
        || meter::cmp(m, eligible_atoms, &maximum)?.is_gt()
    {
        return Err(fail("eligible attributable amount"));
    }
    price_instructions_selected_mode(
        instructions,
        components,
        activity,
        original_facts,
        m,
        (
            Some(CostSelection::AppFinal {
                component_key,
                eligible_atoms,
            }),
            condition,
        ),
        mode,
    )
}
fn price_instructions_selected(
    instructions: &[Value],
    components: &[Value],
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
    selections: PricingSelections<'_>,
) -> Result<PricedInstructions> {
    price_instructions_selected_mode(
        instructions,
        components,
        activity,
        normalized_facts,
        m,
        selections,
        PriceMode::Actual,
    )
}
/// Stage's fixed original predecessor net enters the SAME instruction engine.
/// This value is derived from authenticated named member actions by its caller.
pub(super) fn stage(
    program: &SelectedProgram,
    activity: &str,
    facts: &Value,
    original: (&Value, &Value, &str),
    prior: &BigInt,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(fail("Stage pricing recipe"));
    }
    let plan = program.compiled_plan().canonical_value();
    let instructions = shape::array(m, plan, "typed_instructions")?;
    let components = shape::array(m, plan, "component_slots")?;
    price_instructions_selected_mode(
        instructions,
        components,
        activity,
        facts,
        m,
        (
            None,
            Some((program, original.0, Some((original.1, original.2)))),
        ),
        PriceMode::Stage(prior),
    )
}
enum PriceMode<'a> {
    Stage(&'a BigInt),
    Actual,
    Exposure,
    Work(&'a str),
}
fn price_instructions_selected_mode(
    instructions: &[Value],
    components: &[Value],
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
    selections: PricingSelections<'_>,
    mode: PriceMode<'_>,
) -> Result<PricedInstructions> {
    let exposure_bound = matches!(mode, PriceMode::Exposure);
    let work_status = match mode {
        PriceMode::Work(status) => Some(status),
        _ => None,
    };
    let failed = matches!(work_status, Some("failed"));
    let (selection, condition) = selections;
    let component_recipe = condition.is_some_and(|(p, _, _)| p.uses_component_lifecycle_recipe());
    m.phase(Phase::Pricing);
    let scale = arithmetic::integer(m, "1000000000000000000", false)?;
    let atom_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    let scale = Ratio {
        numerator: scale,
        denominator: BigInt::one(),
    };
    let output_ownership = output_owner(m, instructions.len())?;
    let mut output: Vec<PricedInstruction> = Vec::new();
    if output_ownership.is_some() {
        output
            .try_reserve_exact(instructions.len())
            .map_err(|_| fail("pricing output allocation"))?;
    }
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
        if let Some(selection) = &selection {
            let kind = shape::text(m, &rule["formula"], "kind")?;
            let retail = shape::same_text(m, kind, "final_cost")?;
            let supplier = shape::same_text(m, kind, "supplier_final_cost")?;
            match selection {
                CostSelection::AppDeferred => {
                    if shape::same_text(m, kind, "app_final_cost")? {
                        continue;
                    }
                    if retail || supplier {
                        return Err(fail("AP formula in app-only program"));
                    }
                }
                CostSelection::AppFinal { component_key, .. } => {
                    if !shape::same_text(m, kind, "app_final_cost")? {
                        continue;
                    }
                    let actual = shape::text(m, rule, "component_key")?;
                    if !shape::same_text(m, actual, component_key)? {
                        continue;
                    }
                }
                CostSelection::Deferred => {
                    if retail || supplier {
                        continue;
                    }
                }
                #[cfg(test)]
                CostSelection::Final {
                    supplier_slot,
                    supplier_component,
                    ..
                } => {
                    if !retail && !supplier {
                        continue;
                    }
                    if supplier {
                        if let Some(component) = supplier_component {
                            let actual = shape::text(m, rule, "component_key")?;
                            if !shape::same_text(m, actual, component)? {
                                continue;
                            }
                        }
                    }
                    let selected = if retail {
                        shape::text(m, &rule["formula"], "supplier_slot")?
                    } else {
                        shape::text(m, rule, "binding_slot")?
                    };
                    if !shape::same_text(m, selected, supplier_slot)? {
                        continue;
                    }
                }
            }
        }
        let formula_kind = shape::text(m, &rule["formula"], "kind")?;
        if component_recipe && shape::same_text(m, formula_kind, "observe_cost")? {
            // Dedicated nonpayable consumer handles assertion/unknown/observed
            // proof shapes; this monetary table must never synthesize a price.
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
        let row_ownership = row_owner(m, instruction, rule, component, &output, component_recipe)?;
        let mut basis_components = Vec::new();
        m.visit("pricing.formula_variant")?;
        let predicate_state = if exposure_bound {
            let kind = shape::text(m, formula, "kind")?;
            if !shape::same_text(m, kind, "unit")?
                && !shape::same_text(m, kind, "fixed")?
                && !(component_recipe && matches!(kind, "percent" | "discount"))
            {
                return Err(fail(
                    "future exposure requires independent positive producer",
                ));
            }
            "true"
        } else {
            match condition {
                Some((program, context, operational)) => {
                    let slot = shape::text(m, rule, "binding_slot")?;
                    let contexts = (
                        context,
                        operational.map(|(binding, source)| (binding, source, slot)),
                    );
                    if let Some(status) = work_status {
                        program.evaluate_work_rule_condition_operational(
                            m,
                            &rule["when"],
                            activity,
                            normalized_facts,
                            contexts,
                            status,
                        )?
                    } else {
                        program.evaluate_rule_condition_operational(
                            m,
                            &rule["when"],
                            activity,
                            normalized_facts,
                            contexts,
                        )?
                    }
                }
                None => "true",
            }
        };
        let predicate = predicate_state == "true";
        let mut reduction_limit = None;
        let rational = if failed
            || !predicate
            || component_recipe && exposure_bound && formula["kind"].as_str() == Some("discount")
        {
            Ratio {
                numerator: BigInt::from(0),
                denominator: BigInt::one(),
            }
        } else {
            match formula["kind"]
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
                    let quantity = if formula.get("normalize_to").is_some() {
                        let (program, _, _) = condition.ok_or_else(|| {
                            fail("unit normalization needs exact selected Product")
                        })?;
                        normalized_quantity(program, formula, normalized_facts, m)?
                    } else if condition
                        .is_some_and(|(program, _, _)| program.uses_policy_semantics_recipe())
                    {
                        let raw = shape::text(m, normalized_facts, field)?;
                        arithmetic::decimal(m, raw)?
                    } else {
                        let quantity = arithmetic::integer(
                            m,
                            normalized_facts[field]
                                .as_str()
                                .ok_or_else(|| fail("normalized integer fact"))?,
                            true,
                        )?;
                        Ratio {
                            numerator: quantity,
                            denominator: BigInt::one(),
                        }
                    };
                    if condition.is_some_and(|(program, _, _)| program.uses_work_lifecycle_recipe())
                        && !meter::cmp(m, &quantity.numerator, &BigInt::from(0))?.is_gt()
                    {
                        return Err(fail("successful selected quantity must be positive"));
                    }
                    let rate = arithmetic::decimal(
                        m,
                        formula["rate"].as_str().ok_or_else(|| fail("unit rate"))?,
                    )?;
                    let amount = arithmetic::multiply(m, &quantity, &rate)?;
                    arithmetic::multiply(m, &amount, &scale)?
                }
                "percent" | "discount" if component_recipe => {
                    let (ratio, limit) = component_modifier(
                        m,
                        formula,
                        rule,
                        &output,
                        &scale,
                        &mut basis_components,
                    )?;
                    reduction_limit = limit;
                    ratio
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
                    if row_ownership.is_some() {
                        basis_components
                            .try_reserve_exact(1)
                            .map_err(|_| fail("pricing basis allocation"))?;
                    }
                    basis_components.push(copied(m, base)?);
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
                "closure_cap" | "closure_share" => {
                    let PriceMode::Stage(prior_net) = mode else {
                        return Err(fail("Stage formula requires fixed original member net"));
                    };
                    let kind = shape::text(m, formula, "kind")?;
                    let base = shape::text(m, formula, "basis_component_key")?;
                    let mut net = BigInt::from(0);
                    let mut count = 0usize;
                    for previous in &output {
                        m.visit("pricing.Stage_named_booked_basis")?;
                        if shape::same_text(m, &previous.root_key, base)? {
                            net = meter::add(m, &net, &previous.atoms.into())?;
                            count = crate::validation_work::usize_add(m, count, 1)?;
                        }
                    }
                    if count == 0 || meter::cmp(m, &net, &0.into())?.is_lt() {
                        return Err(fail("Stage named nonnegative booked basis"));
                    }
                    if row_ownership.is_some() {
                        basis_components
                            .try_reserve_exact(count)
                            .map_err(|_| fail("Stage pricing basis storage"))?;
                    }
                    for previous in &output {
                        if shape::same_text(m, &previous.root_key, base)? {
                            basis_components.push(copied(m, &previous.component_key)?);
                        }
                    }
                    if shape::same_text(m, kind, "closure_cap")? {
                        let raw = shape::text(m, &formula["ceiling"], "atoms")?;
                        let ceiling = arithmetic::integer(m, raw, false)?;
                        if meter::cmp(m, prior_net, &0.into())?.is_lt()
                            || meter::cmp(m, &ceiling, prior_net)?.is_lt()
                        {
                            return Err(Error::new(
                                "CAP_BELOW_BOOKED",
                                "Stage cap below original prior booked net",
                            ));
                        }
                        let total = meter::add(m, prior_net, &net)?;
                        let excess = meter::sub(m, &total, &ceiling)?;
                        let credit = if meter::cmp(m, &excess, &0.into())?.is_gt() {
                            meter::neg(m, &excess)?
                        } else {
                            0.into()
                        };
                        arithmetic::rational(m, credit, 1.into())?
                    } else {
                        let raw = shape::text(m, formula, "percent")?;
                        let percent = arithmetic::decimal(m, raw)?;
                        let denominator = meter::mul(m, &percent.denominator, &100.into())?;
                        let ratio = arithmetic::rational(m, percent.numerator, denominator)?;
                        arithmetic::multiply(
                            m,
                            &Ratio {
                                numerator: net,
                                denominator: 1.into(),
                            },
                            &ratio,
                        )?
                    }
                }
                "app_final_cost" => {
                    let atoms = match &selection {
                        Some(CostSelection::AppFinal { eligible_atoms, .. }) => *eligible_atoms,
                        _ => {
                            return Err(Error::new(
                                "WAITING_FINAL_COST",
                                "actual authenticated app final amount required",
                            ))
                        }
                    };
                    let eligible = Ratio {
                        numerator: atoms.clone(),
                        denominator: BigInt::one(),
                    };
                    customer_final_cost_rational(m, formula, &eligible, &scale)?
                }
                #[cfg(not(test))]
                "supplier_final_cost" | "final_cost" => {
                    return Err(Error::new(
                        "WAITING_FINAL_COST",
                        "exact final supplier evidence required",
                    ))
                }
                #[cfg(test)]
                "supplier_final_cost" | "final_cost" => {
                    let atoms = match &selection {
                        #[cfg(test)]
                        Some(CostSelection::Final { eligible_atoms, .. }) => *eligible_atoms,
                        _ => {
                            return Err(Error::new(
                                "WAITING_FINAL_COST",
                                "exact final supplier evidence required",
                            ))
                        }
                    };
                    let supplier = Ratio {
                        numerator: atoms.clone(),
                        denominator: BigInt::one(),
                    };
                    if shape::same_text(
                        m,
                        formula["kind"].as_str().unwrap_or(""),
                        "supplier_final_cost",
                    )? {
                        supplier
                    } else {
                        customer_final_cost_rational(m, formula, &supplier, &scale)?
                    }
                }
                _ => return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE", "selected formula")),
            }
        };
        let mut atoms = arithmetic::rounded(m, &rational, &atom_limit)?;
        if let Some(limit) = reduction_limit {
            let minimum =
                i128::checked_neg(limit).ok_or_else(|| fail("component reduction bound"))?;
            if crate::validation_work::compare(m, atoms, minimum)?.is_lt() {
                return Err(fail("discount exceeds selected original component net"));
            }
        }
        if matches!(mode, PriceMode::Stage(_)) && formula["kind"].as_str() == Some("closure_share")
        {
            let raw = shape::text(m, &formula["ceiling"], "atoms")?;
            let ceiling = arithmetic::integer(m, raw, false)?;
            if meter::cmp(m, &atoms.into(), &ceiling)?.is_gt() {
                use num_traits::ToPrimitive;
                atoms = ceiling
                    .to_i128()
                    .ok_or_else(|| fail("Stage share ceiling atoms"))?;
            }
        }
        let final_selection = matches!(selection, Some(CostSelection::AppFinal { .. }));
        #[cfg(test)]
        let final_selection =
            final_selection || matches!(selection, Some(CostSelection::Final { .. }));
        if final_selection {
            let ceiling = &formula["ceiling"];
            let currency = shape::text(m, ceiling, "currency")?;
            if !shape::same_text(m, currency, "USD")?
                || !shape::same(m, &ceiling["scale"], &serde_json::json!(18))?
            {
                return Err(fail("cost ceiling units"));
            }
            let ceiling = shape::text(m, ceiling, "atoms")?;
            let ceiling = arithmetic::integer(m, ceiling, false)?;
            if meter::cmp(m, &BigInt::from(atoms), &ceiling)?.is_gt() {
                return Err(Error::new(
                    "EXPOSURE_CEILING_EXCEEDED",
                    "original cost formula ceiling",
                ));
            }
        }
        if component_recipe && !exposure_bound {
            let root = shape::text(m, component, "root_key")?;
            let mut net = BigInt::from(atoms);
            for prior in &output {
                m.visit("pricing.component_nonnegative_root")?;
                if shape::same_text(m, &prior.root_key, root)? {
                    net = meter::add(m, &net, &BigInt::from(prior.atoms))?;
                }
            }
            if meter::cmp(m, &net, &BigInt::from(0))?.is_lt() {
                return Err(fail("component root net is negative"));
            }
        }
        output.push(PricedInstruction {
            predicate,
            failed,
            predicate_state,
            ordinal: copied(
                m,
                instruction["ordinal"]
                    .as_str()
                    .ok_or_else(|| fail("ordinal"))?,
            )?,
            component_key: copied(m, key)?,
            root_key: copied(
                m,
                component["root_key"]
                    .as_str()
                    .ok_or_else(|| fail("root key"))?,
            )?,
            role_slot: copied(
                m,
                component["role_slot"]
                    .as_str()
                    .ok_or_else(|| fail("role slot"))?,
            )?,
            binding_slot: copied(
                m,
                rule["binding_slot"]
                    .as_str()
                    .ok_or_else(|| fail("binding slot"))?,
            )?,
            basis_components,
            prior_net_basis: component_recipe
                && formula["basis"]["view"].as_str() == Some("prior_net"),
            rational,
            atoms,
            _ownership: row_ownership,
        });
    }
    Ok(PricedInstructions {
        rows: output,
        _ownership: output_ownership,
    })
}

fn component_modifier(
    m: &mut CoreMeter,
    formula: &Value,
    rule: &Value,
    prior: &[PricedInstruction],
    scale: &Ratio,
    basis_components: &mut Vec<String>,
) -> Result<(Ratio, Option<i128>)> {
    let base = shape::text(m, &formula["basis"], "component_key")?;
    let view = shape::text(m, &formula["basis"], "view")?;
    let slot = shape::text(m, rule, "binding_slot")?;
    let mut producer = None;
    for row in prior {
        m.visit("pricing.component_named_producer")?;
        if shape::same_text(m, &row.component_key, base)?
            && shape::same_text(m, &row.binding_slot, slot)?
            && producer.replace(row).is_some()
        {
            return Err(fail("ambiguous named monetary producer"));
        }
    }
    let producer = producer.ok_or_else(|| fail("named monetary producer unavailable"))?;
    let prior_net = shape::same_text(m, view, "prior_net")?;
    let kind = shape::text(m, formula, "kind")?;
    let discount = shape::same_text(m, kind, "discount")?;
    let sequential = if discount {
        let mode = shape::text(m, formula, "mode")?;
        shape::same_text(m, mode, "sequential")?
    } else {
        false
    };
    let count = crate::validation_work::usize_add(m, prior.len(), 1)?;
    basis_components
        .try_reserve_exact(count)
        .map_err(|_| fail("component basis allocation"))?;
    basis_components.push(copied(m, base)?);
    let mut net = BigInt::from(producer.atoms);
    let mut selected_net = net.clone();
    if prior_net {
        net = 0.into();
        for row in prior {
            m.visit("pricing.explicit_prior_root_net")?;
            if shape::same_text(m, &row.root_key, &producer.root_key)?
                && shape::same_text(m, &row.binding_slot, slot)?
            {
                net = meter::add(m, &net, &BigInt::from(row.atoms))?;
                if !shape::same_text(m, &row.component_key, base)? {
                    basis_components.push(copied(m, &row.component_key)?);
                }
            }
        }
        selected_net = net.clone();
    } else if discount {
        for row in prior {
            m.visit("pricing.named_prior_reduction")?;
            if row.atoms >= 0
                || row.prior_net_basis
                || !shape::same_text(m, &row.binding_slot, slot)?
            {
                continue;
            }
            let Some(target) = row.basis_components.first() else {
                continue;
            };
            if shape::same_text(m, target, base)? {
                selected_net = meter::add(m, &selected_net, &BigInt::from(row.atoms))?;
                if sequential {
                    basis_components.push(copied(m, &row.component_key)?);
                }
            }
        }
        if sequential {
            net = selected_net.clone();
        }
    }
    if meter::cmp(m, &net, &BigInt::from(0))?.is_lt()
        || meter::cmp(m, &selected_net, &BigInt::from(0))?.is_lt()
    {
        return Err(fail("named component nonnegative basis"));
    }
    let value = if discount {
        &formula["amount"]
    } else {
        formula
    };
    let fixed = if discount {
        let kind = shape::text(m, value, "kind")?;
        shape::same_text(m, kind, "fixed")?
    } else {
        false
    };
    let mut amount = if fixed {
        let raw = shape::text(m, value, "amount")?;
        let decimal = arithmetic::decimal(m, raw)?;
        arithmetic::multiply(m, &decimal, scale)?
    } else {
        let raw = shape::text(m, value, "percent")?;
        let percentage = arithmetic::decimal(m, raw)?;
        let denominator = meter::mul(m, &percentage.denominator, &BigInt::from(100))?;
        let ratio = arithmetic::rational(m, percentage.numerator, denominator)?;
        let basis = arithmetic::rational(m, net, 1.into())?;
        arithmetic::multiply(m, &basis, &ratio)?
    };
    if discount {
        amount.numerator = meter::neg(m, &amount.numerator)?;
    }
    use num_traits::ToPrimitive;
    let limit = if discount {
        Some(
            selected_net
                .to_i128()
                .ok_or_else(|| fail("component reduction limit overflow"))?,
        )
    } else {
        None
    };
    Ok((amount, limit))
}

/// SAME original customer cost formula for every eligible cost-basis mode.
/// Its caller supplies an actually validated attributable amount; this helper
/// does not select evidence, grant charge authority or produce an AP payable.
/// Rounding remains at the one original instruction site after this rational.
pub(super) fn customer_final_cost_rational(
    m: &mut CoreMeter,
    formula: &Value,
    eligible: &Ratio,
    scale: &Ratio,
) -> Result<Ratio> {
    let numerator = shape::text(m, &formula["multiplier"], "numerator")?;
    let denominator = shape::text(m, &formula["multiplier"], "denominator")?;
    let numerator = arithmetic::integer(m, numerator, false)?;
    let denominator = arithmetic::integer(m, denominator, false)?;
    if meter::cmp(m, &numerator, &0.into())?.is_lt()
        || !meter::cmp(m, &denominator, &0.into())?.is_gt()
    {
        return Err(fail("original pass through multiplier"));
    }
    let multiplier = arithmetic::rational(m, numerator, denominator)?;
    let amount = arithmetic::multiply(m, eligible, &multiplier)?;
    let markup = shape::text(m, formula, "markup")?;
    let markup = arithmetic::decimal(m, markup)?;
    if meter::cmp(m, &markup.numerator, &0.into())?.is_lt() {
        return Err(fail("negative original cost markup"));
    }
    let markup = arithmetic::multiply(m, &markup, scale)?;
    add_ratio(m, &amount, &markup)
}

fn add_ratio(m: &mut CoreMeter, a: &Ratio, b: &Ratio) -> Result<Ratio> {
    let left = meter::mul(m, &a.numerator, &b.denominator)?;
    let right = meter::mul(m, &b.numerator, &a.denominator)?;
    let n = meter::add(m, &left, &right)?;
    let d = meter::mul(m, &a.denominator, &b.denominator)?;
    arithmetic::rational(m, n, d)
}

/// One original family alternative, evaluated only against its frozen named
/// component net. This is shared by initial and replacement outcome records;
/// no current policy, aggregate invoice, or current statement enters the basis.
pub(super) struct PricedOutcome {
    pub ordinal: String,
    pub _root_key: String,
    pub _family_key: String,
    pub rational: Ratio,
    pub atoms: i128,
    pub none: bool,
    pub(super) _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
fn outcome_owner(
    m: &mut CoreMeter,
    instruction: &Value,
    family: &Value,
    key: &str,
) -> Result<Option<crate::canonical::audit_arena::ExternalBufferLease>> {
    use crate::validation_work as w;
    let Some(arena) = m.representation_arena() else {
        return Ok(None);
    };
    let ordinal = shape::text(m, instruction, "ordinal")?;
    let component = shape::text(m, &family["selector"], "component_key")?;
    let mut size = std::mem::size_of::<PricedOutcome>()
        + 3 * std::mem::size_of::<&str>()
        + 2 * (std::mem::size_of::<BigInt>() + 136);
    for raw in [ordinal, component, key] {
        m.visit("pricing.family_output_dimension")?;
        size = w::usize_add(m, size, raw.len())?;
    }
    let lease = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    Ok(Some(lease))
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
    let output_ownership = outcome_owner(m, instruction, family, family_key)?;
    let ordinal = paid_call!(shape::text, m, instruction, "ordinal")?;
    let ordinal = copied(m, ordinal)?;
    let root = paid_call!(shape::text, m, &family["selector"], "component_key")?;
    let root = copied(m, root)?;
    let key = copied(m, family_key)?;
    Ok(PricedOutcome {
        ordinal,
        _root_key: root,
        _family_key: key,
        _ownership: output_ownership,
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
    let output_ownership = outcome_owner(m, instruction, family, family_key)?;
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
        let fixed_component = program.uses_component_lifecycle_recipe() && {
            let kind = shape::text(m, formula, "kind")?;
            shape::same_text(m, kind, "fixed")?
        };
        if fixed_component {
            let raw = shape::text(m, formula, "amount")?;
            let decimal = arithmetic::decimal(m, raw)?;
            if meter::cmp(m, &decimal.numerator, &0.into())?.is_lt() {
                return Err(fail("negative family fixed amount"));
            }
            let scale = arithmetic::rational(m, 1_000_000_000_000_000_000i128.into(), 1.into())?;
            rational = arithmetic::multiply(m, &decimal, &scale)?;
            if discount {
                rational.numerator = meter::neg(m, &rational.numerator)?;
            }
        } else {
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
    let ordinal = paid_call!(shape::text, m, instruction, "ordinal")?;
    let ordinal = copied(m, ordinal)?;
    let root = paid_call!(shape::text, m, &family["selector"], "component_key")?;
    let root = copied(m, root)?;
    let key = copied(m, family_key)?;
    Ok(PricedOutcome {
        ordinal,
        _root_key: root,
        _family_key: key,
        _ownership: output_ownership,
        rational,
        atoms,
        none,
    })
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn stage_cap_share_uses_booked_atoms_then_rounds_and_limits_supplier_share() {
        use serde_json::json;
        let arena = crate::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session =
            crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let instructions = json!([
            {"opcode":"evaluate_rule","ordinal":"0","rule":{"activity":"test:close","component_key":"test:premium","binding_slot":"test:retail","formula":{"kind":"fixed","amount":"2"}}},
            {"opcode":"evaluate_rule","ordinal":"1","rule":{"activity":"test:close","component_key":"test:cap","binding_slot":"test:retail","formula":{"kind":"closure_cap","basis_component_key":"test:premium","ceiling":{"atoms":"3150000000000000000"}}}},
            {"opcode":"evaluate_rule","ordinal":"2","rule":{"activity":"test:close","component_key":"test:share","binding_slot":"test:supplier","formula":{"kind":"closure_share","basis_component_key":"test:premium","percent":"25","ceiling":{"atoms":"1000000000000000000"}}}}
        ]);
        let components = json!([
            {"key":"test:premium","root_key":"test:premium","role_slot":"test:retail"},
            {"key":"test:cap","root_key":"test:premium","role_slot":"test:retail"},
            {"key":"test:share","root_key":"test:share","role_slot":"test:supplier"}
        ]);
        let prior = BigInt::from(1_350_000_000_000_000_000i128);
        let result = price_instructions_selected_mode(
            instructions.as_array().unwrap(),
            components.as_array().unwrap(),
            "test:close",
            &json!({}),
            session.meter_mut(),
            (None, None),
            PriceMode::Stage(&prior),
        )
        .unwrap();
        assert_eq!(
            result.iter().map(|v| v.atoms).collect::<Vec<_>>(),
            [
                2_000_000_000_000_000_000i128,
                -200_000_000_000_000_000,
                450_000_000_000_000_000
            ]
        );
        let zero = BigInt::from(0);
        let bound = price_instructions_selected_mode(
            instructions.as_array().unwrap(),
            components.as_array().unwrap(),
            "test:close",
            &json!({}),
            session.meter_mut(),
            (None, None),
            PriceMode::Stage(&zero),
        )
        .unwrap();
        assert_eq!(bound[2].atoms, 500_000_000_000_000_000);
        let too_high = BigInt::from(4_000_000_000_000_000_000i128);
        assert_eq!(
            price_instructions_selected_mode(
                instructions.as_array().unwrap(),
                components.as_array().unwrap(),
                "test:close",
                &json!({}),
                session.meter_mut(),
                (None, None),
                PriceMode::Stage(&too_high)
            )
            .unwrap_err()
            .code,
            "CAP_BELOW_BOOKED"
        );
        let mut limited = instructions.clone();
        limited[2]["rule"]["formula"]["ceiling"]["atoms"] = json!("400000000000000000");
        let result = price_instructions_selected_mode(
            limited.as_array().unwrap(),
            components.as_array().unwrap(),
            "test:close",
            &json!({}),
            session.meter_mut(),
            (None, None),
            PriceMode::Stage(&prior),
        )
        .unwrap();
        assert_eq!(result[2].atoms, 400_000_000_000_000_000);
    }

    #[test]
    fn declared_unit_ratio_is_exact_until_the_single_monetary_round() {
        let mut m = CoreMeter::new(65_536).unwrap();
        let factor = serde_json::json!({"numerator":"1","denominator":"60"});
        let quantity = arithmetic::decimal(&mut m, "90").unwrap();
        let converted = convert_quantity(&mut m, &quantity, &factor).unwrap();
        assert_eq!(converted.numerator, BigInt::from(3));
        assert_eq!(converted.denominator, BigInt::from(2));
        let rate = arithmetic::decimal(&mut m, "0.1").unwrap();
        let amount = arithmetic::multiply(&mut m, &converted, &rate).unwrap();
        let scale = arithmetic::decimal(&mut m, "1000000000000000000").unwrap();
        let atoms = arithmetic::multiply(&mut m, &amount, &scale).unwrap();
        let limit = arithmetic::integer(&mut m, "999999999999999999999999999999", false).unwrap();
        assert_eq!(
            arithmetic::rounded(&mut m, &atoms, &limit).unwrap(),
            150_000_000_000_000_000
        );
        let one = arithmetic::decimal(&mut m, "1").unwrap();
        let converted = convert_quantity(&mut m, &one, &factor).unwrap();
        for (raw, expected) in [("0.00000000000000003", 1), ("0.00000000000000009", 2)] {
            let rate = arithmetic::decimal(&mut m, raw).unwrap();
            let amount = arithmetic::multiply(&mut m, &converted, &rate).unwrap();
            let atoms = arithmetic::multiply(&mut m, &amount, &scale).unwrap();
            assert_eq!(
                arithmetic::rounded(&mut m, &atoms, &limit).unwrap(),
                expected
            );
        }
        assert!(convert_quantity(
            &mut m,
            &one,
            &serde_json::json!({"numerator":"2","denominator":"120"})
        )
        .is_err());
    }
    #[test]
    fn final_supplier_and_retail_share_original_kernel_without_unit_rate_proxy() {
        let input:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/owner-local-cost-01/fixture-input.json")).unwrap();
        let policy = &input["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["definition"] == "Policy2")
            .unwrap()["body"];
        let instructions:Vec<Value>=policy["rules"].as_array().unwrap().iter().enumerate().map(|(i,r)|serde_json::json!({"opcode":"evaluate_rule","ordinal":i.to_string(),"rule":r})).collect();
        let components:Vec<Value>=policy["rules"].as_array().unwrap().iter().map(|r|serde_json::json!({"key":r["component_key"],"root_key":r["component_key"],"role_slot":"test:caller-selected-role"})).collect();
        let mut m = CoreMeter::new(65536).unwrap();
        let deferred = price_instructions_selected(
            &instructions,
            &components,
            "support:complete",
            &serde_json::json!({}),
            &mut m,
            (Some(CostSelection::Deferred), None),
        )
        .unwrap();
        assert_eq!(deferred.len(), 1);
        assert_eq!(deferred[0].atoms, 1_000_000_000_000_000_000);
        let eligible = BigInt::from(2_000_000_000_000_000_000i128);
        let finalized = price_instructions_selected(
            &instructions,
            &components,
            "support:complete",
            &Value::Null,
            &mut m,
            (
                Some(CostSelection::Final {
                    supplier_slot: "support:supplier",
                    supplier_component: None,
                    eligible_atoms: &eligible,
                }),
                None,
            ),
        )
        .unwrap();
        assert_eq!(finalized.len(), 2);
        for price in &finalized {
            assert_eq!(price.atoms, 2_000_000_000_000_000_000);
        }
        let mut changed = instructions.clone();
        for instruction in &mut changed {
            if instruction["rule"]["formula"]["kind"] == "final_cost" {
                instruction["rule"]["formula"]["multiplier"] =
                    serde_json::json!({"numerator":"3","denominator":"2"});
                instruction["rule"]["formula"]["markup"] = serde_json::json!("0.1");
            }
        }
        let eligible = BigInt::from(1_600_000_000_000_000_000i128);
        let next = price_instructions_selected(
            &changed,
            &components,
            "support:complete",
            &Value::Null,
            &mut m,
            (
                Some(CostSelection::Final {
                    supplier_slot: "support:supplier",
                    supplier_component: None,
                    eligible_atoms: &eligible,
                }),
                None,
            ),
        )
        .unwrap();
        let retail = next
            .iter()
            .find(|p| p.binding_slot == "support:retail")
            .unwrap();
        let supplier = next
            .iter()
            .find(|p| p.binding_slot == "support:supplier")
            .unwrap();
        assert_eq!(retail.atoms, 2_500_000_000_000_000_000);
        assert_eq!(supplier.atoms, 1_600_000_000_000_000_000);
        let eligible = BigInt::from(3_000_000_000_000_000_000i128);
        assert_eq!(
            price_instructions_selected(
                &changed,
                &components,
                "support:complete",
                &Value::Null,
                &mut m,
                (
                    Some(CostSelection::Final {
                        supplier_slot: "support:supplier",
                        supplier_component: None,
                        eligible_atoms: &eligible
                    }),
                    None
                )
            )
            .err()
            .unwrap()
            .code,
            "EXPOSURE_CEILING_EXCEEDED"
        );
    }
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
        assert_eq!(price._root_key, choice._root_key);
        assert_eq!(price.rational.numerator, 0.into());
        assert_eq!(price.rational.denominator, 1.into());
        assert!(price.none);
        assert_eq!(price.atoms, 0);
        assert!(explicit_none(&program, "absent:family", &mut meter).is_err());
        // Same selected family evaluator/none branch with independent row
        // owners; no constructor, source-origin or capacity eligibility.
        let arena = crate::canonical::audit_arena::AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let mut original =
            crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child = original.meter_mut().original_economic_meter().unwrap();
        let own_none = explicit_none(&program, "fixture.measure:assessment", &mut child).unwrap();
        let own_choice = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &999000000000000000_i128.into(),
            &mut child,
        )
        .unwrap();
        assert_eq!(own_none.ordinal, price.ordinal);
        assert_eq!(own_none.rational, price.rational);
        assert_eq!(own_choice.ordinal, choice.ordinal);
        assert_eq!(own_choice.rational, choice.rational);
        assert_eq!(own_choice.atoms, choice.atoms);
        assert!(own_none._ownership.is_some() && own_choice._ownership.is_some());
        drop(own_none);
        drop(own_choice);
        original
            .meter_mut()
            .finish_original_economic(child)
            .unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes, before);
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
        assert!(outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:unknown",
            &net,
            &mut m
        )
        .is_err());
        assert!(outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &(-1).into(),
            &mut m
        )
        .is_err());
    }
}

#[cfg(test)]
pub(super) mod component_tests {
    use super as pricing;
    use super::arithmetic;
    use crate::{
        canonical::{self, audit_arena::AuditArena},
        policy::workflow::{
            document_digest,
            selected_program::{
                compile_selected_publication, load_local_selected_execution,
                SelectedCompilerSupport, SelectedSourceInput,
            },
        },
        validation_work::OriginalAuditSession,
    };
    use serde_json::{json, Value};
    /// Synthetic source authoring for this one pure compiler/math vector.
    /// It uses the existing actual source normalizers, not a sealed candidate.
    pub(in crate::policy::workflow::transition_constructor) fn component_input(
        support: &SelectedCompilerSupport,
        session: &mut OriginalAuditSession,
    ) -> SelectedSourceInput {
        let mut data: Value = serde_json::from_slice(include_bytes!(
            "../selected_program/fixtures/local-first-policy-input.json"
        ))
        .unwrap();
        let docs = data["documents"].as_array_mut().unwrap();
        let template = docs
            .iter_mut()
            .find(|d| d["schema"] == "ledger-workflow-slot-template/1")
            .unwrap();
        template["components"] = json!([
            {"key":"support:service","aliases":["support:service"],"book":"retail","root_key":"support:service","role_slot":"support:retail-role"},
            {"key":"support:reduction","aliases":["support:reduction"],"book":"retail","root_key":"support:service","role_slot":"support:retail-role"},
            {"key":"support:priority","aliases":["support:priority"],"book":"retail","root_key":"support:priority","role_slot":"support:retail-role"},
            {"key":"support:observation","aliases":["support:observation"],"book":"cost_observation","root_key":"support:observation","role_slot":"support:retail-role"}
        ]);
        template["families"] = json!([
            {"key":"support:quality","aliases":["support:quality"],"role_slot":"support:retail-role","target_component_key":"support:service"},
            {"key":"support:other-quality","aliases":["support:other-quality"],"role_slot":"support:retail-role","target_component_key":"support:service"}
        ]);
        let template_digest = document_digest("ledger-workflow-slot-template/1", template).unwrap();
        let template_ref = json!({"kind":"workflow-slot-template","id":format!("wdoc_{}",&template_digest[7..]),"digest":template_digest});
        let policy = docs
            .iter_mut()
            .find(|d| d["schema"] == "ledger-workflow-policy/4")
            .unwrap();
        policy["slot_template_ref"] = template_ref;
        policy["app_delayed_costs"] = json!([]);
        let original_slot = policy["binding_slots"][0].clone();
        let mut observation_slot = original_slot.clone();
        observation_slot["name"] = json!("support:observed");
        observation_slot["book"] = json!("cost_observation");
        observation_slot["correction_source_slot"] = json!("support:observer-corrector");
        policy["binding_slots"] = json!([original_slot, observation_slot]);
        let original_rule = policy["rules"][0].clone();
        let rule = |key: &str, slot: &str, phase: &str, formula: Value, dependency: &str| {
            let mut v = original_rule.clone();
            v["rule_id"] = json!(key);
            v["component_key"] = json!(key);
            v["component_name"] = json!(key);
            v["binding_slot"] = json!(slot);
            v["phase"] = json!(phase);
            v["formula"] = formula;
            v["dependency"] = json!(dependency);
            v
        };
        policy["rules"] = json!([
            rule(
                "support:service",
                "support:retail",
                "base",
                json!({"kind":"unit","quantity_field":"support:quantity","unit":"support:request","rate":"0.5"}),
                "on_missing_refuse"
            ),
            rule(
                "support:reduction",
                "support:retail",
                "discount",
                json!({"kind":"discount","basis":{"component_key":"support:service","view":"booked_base"},"mode":"additive","amount":{"kind":"percent","percent":"20"}}),
                "on_missing_refuse"
            ),
            rule(
                "support:priority",
                "support:retail",
                "base",
                json!({"kind":"fixed","amount":"0.2"}),
                "on_missing_refuse"
            ),
            rule(
                "support:observation",
                "support:observed",
                "cost",
                json!({"kind":"observe_cost","evidence_slot":"support:observer"}),
                "on_missing_wait"
            )
        ]);
        let mut first = policy["families"][0].clone();
        first["codes"] = json!([
            {"code":"support:none","effect":{"kind":"none"}},
            {"code":"support:success","effect":{"kind":"premium","amount":{"kind":"percent","percent":"25"}}},
            {"code":"support:discount","effect":{"kind":"premium","amount":{"kind":"percent","percent":"10"}}}
        ]);
        let mut second = first.clone();
        second["family_key"] = json!("support:other-quality");
        second["family_name"] = json!("support:other-quality");
        second["codes"] = json!([
            {"code":"support:none","effect":{"kind":"none"}},
            {"code":"support:success","effect":{"kind":"none"}},
            {"code":"support:discount","effect":{"kind":"discount","amount":{"kind":"percent","percent":"10"}}}
        ]);
        policy["families"] = json!([first, second]);
        let normalized =
            crate::policy::workflow::selected_program::stage_fixture_normalized_source(
                &data, support, session,
            )
            .unwrap();
        SelectedSourceInput {
            documents: normalized["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .chain(std::iter::once(support.canonical_bytes().to_vec()))
                .collect(),
            root_policy_ref: serde_json::from_value(normalized["root_policy_ref"].clone()).unwrap(),
        }
    }
    #[test]
    fn component_actual_compile_reload_and_frozen_family_math() {
        use crate::validation_work::ResourceLimits;
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut author = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support =
            SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut author)
                .unwrap();
        let input = component_input(&support, &mut author);
        let mut compile = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let program = compile_selected_publication(input, &support, &mut compile).unwrap();
        assert!(program.uses_component_lifecycle_recipe());
        assert!(!program.uses_stage_lifecycle_recipe());
        assert!(!program.uses_supplier_scoped_consent_recipe());
        let plan = program.compiled_plan();
        let raw = plan.canonical_bytes().to_vec();
        let reference = plan.reference().clone();
        let source = SelectedSourceInput {
            documents: program
                .source_documents()
                .iter()
                .map(|d| d.canonical_bytes().to_vec())
                .collect(),
            root_policy_ref: program.root_policy_ref().clone(),
        };
        let mut reload = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let loaded = load_local_selected_execution(source, &raw, &reference, &mut reload).unwrap();
        assert!(loaded.uses_component_lifecycle_recipe());
        assert_eq!(loaded.execution_registry(), program.execution_registry());
        assert_eq!(loaded.compiled_plan().canonical_bytes(), raw);
        let mut calculate = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = calculate.meter_mut().local_economic_meter().unwrap();
        let context = json!({});
        let work = json!({"status":"succeeded","activity":"support:complete"});
        let prices = pricing::work_subject(
            &loaded,
            ("support:complete", &work),
            (&json!({"support:quantity":"2"}), &context),
            None,
            &mut m,
        )
        .unwrap();
        let atoms = |key: &str| {
            prices
                .iter()
                .find(|p| p.component_key == key)
                .unwrap()
                .atoms
        };
        let scale = 10i128.pow(18);
        assert_eq!(
            atoms("support:service") + atoms("support:reduction"),
            8 * scale / 10
        );
        assert_eq!(atoms("support:priority"), 2 * scale / 10);
        assert!(!prices
            .iter()
            .any(|p| p.component_key == "support:observation"));
        let net = (8 * scale / 10).into();
        let f =
            pricing::outcome(&loaded, "support:quality", "support:success", &net, &mut m).unwrap();
        let g = pricing::outcome(
            &loaded,
            "support:other-quality",
            "support:discount",
            &net,
            &mut m,
        )
        .unwrap();
        let total =
            prices.iter().map(|p| p.atoms).sum::<i128>() + 3 * scale / 10 + f.atoms + g.atoms;
        assert_eq!(total, 142 * scale / 100);
        let corrected_f =
            pricing::outcome(&loaded, "support:quality", "support:discount", &net, &mut m).unwrap();
        assert_eq!(total - f.atoms + corrected_f.atoms, 130 * scale / 100);
        let revised = pricing::work_subject(
            &loaded,
            ("support:complete", &work),
            (&json!({"support:quantity":"1"}), &context),
            None,
            &mut m,
        )
        .unwrap();
        assert_eq!(
            revised.iter().map(|p| p.atoms).sum::<i128>() + 3 * scale / 10,
            90 * scale / 100
        );
        let zero = arithmetic::integer(&mut m, "0", false).unwrap();
        assert_eq!(
            pricing::outcome(&loaded, "support:quality", "support:none", &zero, &mut m)
                .unwrap()
                .atoms,
            0
        );
        calculate.meter_mut().finish_local_economic(m).unwrap();
    }
}

#[cfg(test)]

#[test]
fn component_observation_nomination_requires_selected_component_program() {
    use crate::{canonical::audit_arena::AuditArena, validation_work::{OriginalAuditSession, ResourceLimits}};
    use serde_json::{json, Value};
    use crate::policy::workflow::{AcceptanceTerms, OwnedAcceptanceTerms, selected_program::{compile_selected_publication, SelectedCompilerSupport}};
    let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let mut author = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let support = SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut author).unwrap();
    let input = component_tests::component_input(&support, &mut author);
    let mut compile = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let program = compile_selected_publication(input, &support, &mut compile).unwrap();
    let mut context_session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let context = program.context_schema_with_work(context_session.work()).unwrap();
    let fixture: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    let mut body = fixture["records"].as_array().unwrap().iter().find(|row| row["name"] == "A.terms").unwrap()["body"].clone();
    body["schema"] = json!("ledger-workflow-acceptance-terms/2");
    body["binding_inputs"]["accepted_context"] = json!({"fact_schema_ref":program.context_schema_ref(),"facts":{}});
    body["binding_inputs"]["source_slots"][0]["evidence_kinds"] = json!(["cost_observation"]);
    let raw = serde_json::to_vec(&body).unwrap();
    assert!(AcceptanceTerms::parse(&raw, context).is_err());
    let parsed = AcceptanceTerms::parse_component(&raw, context, &program).unwrap();
    let mut normal = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    assert!(OwnedAcceptanceTerms::parse(&raw, context, &mut normal).is_err());
    let mut selected = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let owned = OwnedAcceptanceTerms::parse_component(&raw, context, &program, &mut selected).unwrap();
    assert_eq!(owned.canonical_bytes(), parsed.canonical_bytes());
    assert_eq!(owned.reference(), parsed.reference());
}
