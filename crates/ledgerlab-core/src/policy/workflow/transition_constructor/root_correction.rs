//! Original-term work correction. The interpreter selects affected named roots
//! from the original plan and recomputes quantities through the original schema.
//! No saved action amount, current price, or caller capacity is a pricing input.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, arithmetic,
    authority::{self, EconomicMode},
    body, codec,
    data::*,
    inventory::entry,
    meter, outcome, pricing,
    records::Records,
    representation::{InvocationToken, ParsedDraftTimes},
    shape,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde_json::{Value, json};
mod causal;
mod dependent;
fn unsupported(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if shape::same(m, a, b)? {
        Ok(())
    } else {
        Err(shape::fail("original work correction association"))
    }
}
fn text<'a>(m: &mut CoreMeter, v: &'a Value, key: &str) -> Result<&'a str> {
    shape::text(m, v, key)
}
fn text_is(m: &mut CoreMeter, v: &Value, key: &str, wanted: &str) -> Result<bool> {
    let actual = text(m, v, key)?;
    shape::same_text(m, actual, wanted)
}
fn field<'a>(m: &mut CoreMeter, v: &'a Value, key: &str) -> Result<&'a Value> {
    shape::field(m, v, key)
}
fn counter(m: &mut CoreMeter, v: &Value, key: &str) -> Result<BigInt> {
    let raw = text(m, v, key)?;
    let n = arithmetic::integer(m, raw, false)?;
    if !meter::cmp(m, &n, &0.into())?.is_gt() {
        return Err(shape::fail("positive root revision"));
    }
    Ok(n)
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, Value)> {
    let reference = shape::reference(m, v)?;
    let value = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, value))
}
/// This owned plan is still DATA, not a capacity certificate or admitted bill.
/// Complete causal closure must consume it before any new record is returned.
struct RootPlan {
    work_ref: ExactRef,
    work: Value,
    claim_ref: ExactRef,
    original_ref: ExactRef,
    original: Value,
    current_ref: ExactRef,
    current: Value,
    next_revision: String,
    payload: Value,
    current_facts: Value,
    replacement_facts: Value,
    affected_roots: Vec<String>,
    old_prices: Vec<pricing::PricedInstruction>,
    new_prices: Vec<pricing::PricedInstruction>,
    operands: authority::ResolvedOperands,
    reverse: bool,
}
fn original_schema<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    reference: &Value,
) -> Result<&'a crate::policy::workflow::FactSchema> {
    for (r, schema) in p.fact_schemas() {
        m.visit("root.original_schema")?;
        if shape::same(m, &json!(r), reference)? {
            return Ok(schema);
        }
    }
    Err(shape::fail("original quantity schema missing"))
}
fn original_product<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    reference: &Value,
) -> Result<&'a Value> {
    for d in p.source_documents() {
        m.visit("root.original_product")?;
        if shape::same(m, &json!(d.reference()), reference)? {
            return Ok(d.canonical_value());
        }
    }
    Err(shape::fail("original product missing"))
}
fn original_head<'a>(
    m: &mut CoreMeter,
    state: &'a DraftStateData,
    id: &str,
) -> Result<&'a DecisionHead> {
    let mut found = None;
    for head in &state.decision_heads {
        m.visit("root.current_head")?;
        if shape::same_text(m, &head.decision_id, id)? {
            if found.is_some() {
                return Err(shape::fail("duplicate current root head"));
            }
            found = Some(head);
        }
    }
    found.ok_or_else(|| shape::fail("current root head missing"))
}
fn prepare(
    program: &SelectedProgram,
    c: &DraftContextData,
    request: &mut Value,
    r: &mut Records,
    state: &DraftStateData,
    m: &mut CoreMeter,
) -> Result<RootPlan> {
    entry::context(m, c)?;
    let reverse = text_is(m, request, "schema", "ledger-workflow-work-reverse/2")?;
    let required = if reverse {
        [
            "schema",
            "work_ref",
            "expected_revision",
            "reason",
            "evidence_refs",
            "dependent_reversals",
            "family_closures",
        ]
        .as_slice()
    } else {
        [
            "schema",
            "work_ref",
            "field",
            "replacement_quantity",
            "expected_revision",
            "evidence_refs",
            "fact_proofs",
            "dependent_reversals",
            "family_closures",
        ]
        .as_slice()
    };
    shape::closed(m, request, required, &[])?;
    if !reverse {
        equal(
            m,
            &request["schema"],
            &json!("ledger-workflow-quantity-correction/2"),
        )?;
    }
    if reverse {
        let reason = text(m, request, "reason")?;
        if !meter::cmp(m, &reason.len().into(), &0.into())?.is_gt()
            || meter::cmp(m, &reason.len().into(), &1024.into())?.is_gt()
        {
            return Err(shape::fail("work reversal reason length"));
        }
    }
    let expected = counter(m, request, "expected_revision")?;
    let (work_ref, work) = load(
        m,
        r,
        c,
        &request["work_ref"],
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::closed(
        m,
        &work,
        &[
            "schema",
            "key",
            "payload",
            "claim_id",
            "received_at",
            "accepted_at",
            "source_namespace",
        ],
        &[],
    )?;
    let payload = field(m, &work, "payload")?.clone();
    let claim_id = text(m, &work, "claim_id")?;
    let (claim_ref, claim) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    shape::closed(
        m,
        &claim,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "source_namespace",
            "business_operation_id",
            "completion",
            "work_ref",
            "facts_digest",
        ],
        &[],
    )?;
    for (name, wanted) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("source_namespace", work["source_namespace"].clone()),
        (
            "business_operation_id",
            payload["business_operation_id"].clone(),
        ),
        ("completion", json!("completion")),
        ("work_ref", json!(work_ref)),
    ] {
        equal(m, &claim[name], &wanted)?;
    }
    let claim_identity = body::semantic_id(
        m,
        "wclaim_",
        "workflow-work-claim",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            work["source_namespace"],
            payload["business_operation_id"],
            "completion"
        ]),
    )?;
    if !shape::same_text(m, &claim_ref.id, &claim_identity)? {
        return Err(shape::fail("permanent original work claim identity"));
    }
    outcome::original_claim_association(m, &claim, &work)?;
    let decision_id = body::semantic_id(
        m,
        "wdecision_",
        "workflow-decision",
        &json!([claim_id, "work"]),
    )?;
    let head = original_head(m, state, &decision_id)?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    shape::closed(
        m,
        &current,
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
    equal(m, &current["decision_id"], &json!(decision_id))?;
    equal(m, &current["subject_ref"], &json!(work_ref))?;
    equal(m, &current["kind"], &json!("work"))?;
    equal(m, &current["status"], &json!("active"))?;
    equal(m, &current["original_terms_ref"], &payload["binding_ref"])?;
    equal(m, &current["revision"], &json!(head.revision))?;
    let old_revision = counter(m, &current, "revision")?;
    let accepted =
        BigInt::from(crate::domain::Timestamp::parse_with_work(&c.accepted_at, m)?.micros());
    let received =
        BigInt::from(crate::domain::Timestamp::parse_with_work(&c.received_at, m)?.micros());
    if meter::cmp(m, &received, &accepted)?.is_gt() {
        return Err(shape::fail("root correction received after acceptance"));
    }
    for previous in [&current, &work] {
        let previous_at = BigInt::from(
            crate::domain::Timestamp::parse_with_work(text(m, previous, "accepted_at")?, m)?
                .micros(),
        );
        if meter::cmp(m, &accepted, &previous_at)?.is_lt() {
            return Err(Error::new(
                "CLOCK_REGRESSION",
                "root correction immutable history",
            ));
        }
    }
    if !meter::cmp(m, &old_revision, &expected)?.is_eq() {
        return Err(Error::new("REVISION_CONFLICT", "root expected revision"));
    }
    let stable_revision = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision_id, old_revision.to_string()]),
    )?;
    if !shape::same_text(m, &head.reference.id, &stable_revision)? {
        return Err(shape::fail("current root revision identity"));
    }
    let first_id = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision_id, "1"]),
    )?;
    let (original_ref, original) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &first_id,
        "ledger-workflow-decision-revision/1",
    )?;
    let (_, evaluation) = load(
        m,
        r,
        c,
        &current["evaluation_ref"],
        "workflow-evaluation",
        "ledger-workflow-evaluation-proof/1",
    )?;
    shape::closed(
        m,
        &evaluation,
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
    for (name, wanted) in [
        ("subject_ref", json!(work_ref)),
        ("policy_ref", payload["policy_ref"].clone()),
        (
            "compiled_plan_ref",
            json!(program.compiled_plan().reference()),
        ),
        (
            "compiler_ref",
            program.compiled_plan().canonical_value()["compiler_ref"].clone(),
        ),
        ("binding_ref", payload["binding_ref"].clone()),
        ("fact_schema_ref", payload["fact_schema_ref"].clone()),
    ] {
        equal(m, &evaluation[name], &wanted)?;
    }
    let schema = original_schema(m, program, &payload["fact_schema_ref"])?;
    let normalized = schema.normalize_value_with_work(&evaluation["facts"], m)?;
    equal(m, normalized.canonical_value(), &evaluation["facts"])?;
    let current_facts = normalized.canonical_value().clone();
    let mut replacement_facts = current_facts.clone();
    let activity_name = text(m, &payload, "activity")?;
    let old_prices = pricing::work(program, activity_name, &current_facts, m)?;
    let mut affected_roots: Vec<String> = vec![];
    if reverse {
        for price in &old_prices {
            let mut seen = false;
            for prior in &affected_roots {
                if shape::same_text(m, prior, &price.root_key)? {
                    seen = true;
                    break;
                }
            }
            if !seen {
                m.write_bytes(price.root_key.len())?;
                affected_roots.push(price.root_key.clone());
            }
        }
    } else {
        let raw_field = text(m, request, "field")?;
        m.write_bytes(raw_field.len())?;
        let quantity_field = raw_field.to_owned();
        let fields = shape::array(m, schema.canonical_value(), "fields")?;
        let definition = shape::named(m, fields, "name", &quantity_field)?;
        equal(m, &definition["required"], &json!(true))?;
        let kind = text(m, &definition["type"], "kind")?;
        if !shape::same_text(m, kind, "integer")? {
            return Err(unsupported("selected correction quantity type"));
        }
        // Required original schema normalization owns raw lexical acceptance,
        // inclusive scalar/unit bounds; canonical arithmetic follows it.
        replacement_facts[quantity_field.as_str()] = request["replacement_quantity"].clone();
        let normalized = schema.normalize_value_with_work(&replacement_facts, m)?;
        replacement_facts = normalized.canonical_value().clone();
        request["replacement_quantity"] = replacement_facts[quantity_field.as_str()].clone();
        let proof_inputs = shape::array(m, request, "fact_proofs")?;
        if !meter::cmp(m, &proof_inputs.len().into(), &0.into())?.is_eq()
            || !normalized.required_proof_fields().is_empty()
        {
            return Err(unsupported("attested quantity correction proof executor"));
        }
        let instructions = shape::array(
            m,
            program.compiled_plan().canonical_value(),
            "typed_instructions",
        )?;
        for instruction in instructions {
            m.visit("root.quantity_producer")?;
            if !text_is(m, instruction, "opcode", "evaluate_rule")? {
                continue;
            }
            let rule = field(m, instruction, "rule")?;
            if !text_is(m, rule, "activity", activity_name)? {
                continue;
            }
            let formula = field(m, rule, "formula")?;
            if !text_is(m, formula, "kind", "unit")?
                || !text_is(m, formula, "quantity_field", &quantity_field)?
            {
                continue;
            }
            let key = text(m, rule, "component_key")?;
            let components = shape::array(
                m,
                program.compiled_plan().canonical_value(),
                "component_slots",
            )?;
            let component = shape::named(m, components, "key", key)?;
            let root = text(m, component, "root_key")?;
            let mut seen = false;
            for prior in &affected_roots {
                if shape::same_text(m, prior, root)? {
                    seen = true;
                    break;
                }
            }
            if !seen {
                m.write_bytes(root.len())?;
                affected_roots.push(root.to_owned());
            }
        }
        if affected_roots.is_empty() {
            return Err(shape::fail(
                "quantity field has no original monetary producer",
            ));
        }
    }
    let product = original_product(m, program, &payload["product_ref"])?;
    let activities = shape::array(m, product, "activities")?;
    let activity = shape::named(m, activities, "name", activity_name)?;
    let mut current_activity = activity.clone();
    current_activity["source_slot"] = json!(c.authorities[0].source_slot);
    let mut authority_payload = payload.clone();
    authority_payload["evidence_refs"] = request["evidence_refs"].clone();
    let owner = InvocationToken::new();
    let times = ParsedDraftTimes::parse(m, &owner, &c.received_at, &c.accepted_at)?;
    let mode = if reverse {
        EconomicMode::WorkReverse
    } else {
        EconomicMode::QuantityCorrect
    };
    let operands = authority::resolve_economic(
        m,
        r,
        c,
        &authority_payload,
        &current_activity,
        product,
        &owner,
        &times,
        program,
        mode,
        &[],
    )?;
    equal(
        m,
        &operands.slot_terms["correction_source_slot"],
        &json!(c.authorities[0].source_slot),
    )?;
    equal(
        m,
        &evaluation["context"],
        &operands.binding["accepted_context"],
    )?;
    outcome::verify_original_actions(
        m,
        r,
        c,
        program,
        &operands,
        &work_ref,
        &work,
        &claim_ref,
        &original_ref,
        &original,
    )?;
    if !reverse {
        entry::quantities(
            m,
            &operands.binding,
            &operands.slot_terms,
            activity_name,
            &replacement_facts,
        )?;
    }
    let new_prices = if reverse {
        vec![]
    } else {
        pricing::work(program, activity_name, &replacement_facts, m)?
    };
    let next_revision = meter::add(m, &old_revision, &1.into())?.to_string();
    Ok(RootPlan {
        work_ref,
        work,
        claim_ref,
        original_ref,
        original,
        current_ref: head.reference.clone(),
        current,
        next_revision,
        payload,
        current_facts,
        replacement_facts,
        affected_roots,
        old_prices,
        new_prices,
        operands,
        reverse,
    })
}
struct ActiveRootAction {
    price_index: usize,
    reference: ExactRef,
    body: Value,
    affected: bool,
}
fn is_affected(m: &mut CoreMeter, plan: &RootPlan, key: &str) -> Result<bool> {
    for root in &plan.affected_roots {
        m.visit("root.affected_root")?;
        if shape::same_text(m, root, key)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn active_actions(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    plan: &RootPlan,
) -> Result<Vec<ActiveRootAction>> {
    let references = shape::array(m, &plan.current, "active_action_refs")?;
    if meter::cmp(m, &references.len().into(), &128.into())?.is_gt() {
        return Err(shape::fail("root action bound"));
    }
    let mut actual: Vec<(ExactRef, Value)> = vec![];
    for v in references {
        m.visit("root.current_action_operand")?;
        let (reference, body) = load(m, r, c, v, "workflow-action", "ledger-workflow-action/1")?;
        shape::closed(
            m,
            &body,
            &[
                "schema",
                "effect_id",
                "decision_id",
                "revision",
                "posting_role",
                "book",
                "component_slot",
                "root_component_slot",
                "role_ref",
                "slot_terms_ref",
                "funding_leg",
                "amount",
                "basis_refs",
                "operation_key",
            ],
            &[],
        )?;
        for prior in &actual {
            if shape::same_ref(m, &reference, &prior.0)? {
                return Err(shape::fail("duplicate current active action"));
            }
        }
        actual.push((reference, body));
    }
    let mut out = Vec::<ActiveRootAction>::new();
    for (index, price) in plan.old_prices.iter().enumerate() {
        m.visit("root.reprice_current_instruction")?;
        if meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq() {
            continue;
        }
        let mappings = shape::array(m, &plan.operands.instantiation, "component_map")?;
        let mapping = shape::named(m, mappings, "template_key", &price.component_key)?;
        let slot = text(m, mapping, "slot_id")?;
        let rootmap = shape::named(m, mappings, "template_key", &price.root_key)?;
        let root_slot = text(m, rootmap, "slot_id")?;
        let catalog_components = shape::array(m, &plan.operands.catalog, "components")?;
        let catalog = shape::named(m, catalog_components, "slot_id", slot)?;
        equal(m, &catalog["root_slot"], &json!(root_slot))?;
        equal(m, &catalog["role_slot"], &json!(price.role_slot))?;
        equal(m, &catalog["book"], &json!("retail"))?;
        let mut found = None;
        for (reference, body) in &actual {
            if text_is(m, body, "component_slot", slot)? {
                if found.is_some() {
                    return Err(shape::fail("two active actions for one original component"));
                }
                found = Some((reference, body));
            }
        }
        let (reference, body) =
            found.ok_or_else(|| shape::fail("repriced current action missing"))?;
        let effect = body::semantic_id(
            m,
            "weffect_",
            "workflow-effect",
            &json!([
                c.key.scope,
                plan.operands.binding["agreement_id"],
                slot,
                plan.claim_ref.id,
                "self",
                "original"
            ]),
        )?;
        let revision = counter(m, body, "revision")?;
        let root_revision = counter(m, &plan.current, "revision")?;
        if meter::cmp(m, &revision, &root_revision)?.is_gt() {
            return Err(shape::fail("future active action revision"));
        }
        let posting = if meter::cmp(m, &revision, &1.into())?.is_eq() {
            "original"
        } else {
            "replacement"
        };
        let id = body::semantic_id(
            m,
            "waction_",
            "workflow-action",
            &json!([effect, revision.to_string(), posting]),
        )?;
        if !shape::same_text(m, &reference.id, &id)? {
            return Err(shape::fail("current root action identity"));
        }
        let amount = json!({"currency":"USD","scale":18,"atoms":price.atoms.to_string()});
        for (name, wanted) in [
            ("effect_id", json!(effect)),
            ("decision_id", plan.current["decision_id"].clone()),
            ("posting_role", json!(posting)),
            ("root_component_slot", json!(root_slot)),
            ("role_ref", json!(plan.operands.role_ref)),
            ("slot_terms_ref", json!(plan.operands.slot_terms_ref)),
            (
                "funding_leg",
                plan.operands.slot_terms["funding_leg"].clone(),
            ),
            ("book", json!("retail")),
            ("amount", amount),
        ] {
            equal(m, &body[name], &wanted)?;
        }
        equal(
            m,
            &body["operation_key"]["logical_store_id"],
            &json!(c.key.logical_store_id),
        )?;
        equal(m, &body["operation_key"]["scope"], &json!(c.key.scope))?;
        let mut basis = vec![];
        for key in &price.basis_components {
            for prior in &out {
                if shape::same_text(m, &plan.old_prices[prior.price_index].component_key, key)? {
                    basis.push(prior.reference.clone());
                }
            }
        }
        let basis = body::ref_set(m, basis)?;
        equal(m, &body["basis_refs"], &json!(basis))?;
        let affected = is_affected(m, plan, &price.root_key)?;
        out.push(ActiveRootAction {
            price_index: index,
            reference: reference.clone(),
            body: body.clone(),
            affected,
        });
    }
    if !meter::cmp(m, &out.len().into(), &actual.len().into())?.is_eq() {
        return Err(shape::fail("unpriced current root action"));
    }
    Ok(out)
}
struct RootEffects {
    actions: Vec<DraftEnvelope>,
    steps: Vec<Value>,
    active: Vec<ExactRef>,
}
fn monetary_effects(
    m: &mut CoreMeter,
    c: &DraftContextData,
    plan: &RootPlan,
    old: &[ActiveRootAction],
) -> Result<RootEffects> {
    let mut actions = vec![];
    let mut steps = vec![];
    let mut active = vec![];
    let mut components: Vec<(String, ExactRef)> = vec![];
    for prior in old.iter().rev() {
        if !prior.affected {
            continue;
        }
        m.visit("root.inverse")?;
        let old_raw = text(m, &prior.body["amount"], "atoms")?;
        let old_atoms = arithmetic::integer(m, old_raw, true)?;
        let negative = meter::neg(m, &old_atoms)?;
        let id = body::semantic_id(
            m,
            "waction_",
            "workflow-action",
            &json!([prior.body["effect_id"], plan.next_revision, "inverse"]),
        )?;
        let mut inverse = prior.body.clone();
        inverse["revision"] = json!(plan.next_revision);
        inverse["posting_role"] = json!("inverse");
        inverse["amount"] = json!({"currency":"USD","scale":18,"atoms":negative.to_string()});
        inverse["operation_key"] = json!(c.key);
        inverse["inverse_of"] = json!(prior.reference);
        let action = body::record(m, &c.key, "workflow-action", id, inverse)?;
        let price = &plan.old_prices[prior.price_index];
        steps.push(json!({"instruction":price.ordinal,"component_slot":prior.body["component_slot"],"predicate":"true","basis_refs":prior.body["basis_refs"],"rational_numerator":negative.to_string(),"rational_denominator":"1","rounded":{"currency":"USD","scale":18,"atoms":negative.to_string()},"disposition":"inverse","action_ref":action.reference}));
        actions.push(action);
    }
    for price in &plan.new_prices {
        m.visit("root.replacement_instruction")?;
        let affected = is_affected(m, plan, &price.root_key)?;
        if !affected {
            for prior in old {
                if shape::same_text(
                    m,
                    &plan.old_prices[prior.price_index].component_key,
                    &price.component_key,
                )? {
                    active.push(prior.reference.clone());
                    components.push((price.component_key.clone(), prior.reference.clone()));
                }
            }
            continue;
        }
        let mappings = shape::array(m, &plan.operands.instantiation, "component_map")?;
        let mapping = shape::named(m, mappings, "template_key", &price.component_key)?;
        let slot = text(m, mapping, "slot_id")?;
        let rootmap = shape::named(m, mappings, "template_key", &price.root_key)?;
        let root_slot = text(m, rootmap, "slot_id")?;
        let mut basis = vec![];
        for key in &price.basis_components {
            for (name, reference) in &components {
                if shape::same_text(m, name, key)? {
                    basis.push(reference.clone());
                }
            }
        }
        let basis = body::ref_set(m, basis)?;
        let amount = json!({"currency":"USD","scale":18,"atoms":price.atoms.to_string()});
        let nonzero = !meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq();
        let mut step = json!({"instruction":price.ordinal,"component_slot":slot,"predicate":"true","basis_refs":basis,"rational_numerator":price.rational.numerator.to_string(),"rational_denominator":price.rational.denominator.to_string(),"rounded":amount,"disposition":if nonzero{"posted"}else{"zero"}});
        if nonzero {
            let effect = body::semantic_id(
                m,
                "weffect_",
                "workflow-effect",
                &json!([
                    c.key.scope,
                    plan.operands.binding["agreement_id"],
                    slot,
                    plan.claim_ref.id,
                    "self",
                    "original"
                ]),
            )?;
            let id = body::semantic_id(
                m,
                "waction_",
                "workflow-action",
                &json!([effect, plan.next_revision, "replacement"]),
            )?;
            let action = body::record(
                m,
                &c.key,
                "workflow-action",
                id,
                json!({"schema":"ledger-workflow-action/1","effect_id":effect,"decision_id":plan.current["decision_id"],"revision":plan.next_revision,"posting_role":"replacement","book":"retail","component_slot":slot,"root_component_slot":root_slot,"role_ref":plan.operands.role_ref,"slot_terms_ref":plan.operands.slot_terms_ref,"funding_leg":plan.operands.slot_terms["funding_leg"],"amount":amount,"basis_refs":basis,"operation_key":c.key}),
            )?;
            step["action_ref"] = json!(action.reference);
            components.push((price.component_key.clone(), action.reference.clone()));
            active.push(action.reference.clone());
            actions.push(action);
        }
        steps.push(step);
    }
    // A reverse has no replacement instructions, so no retained action survives.
    let active = body::ref_set(m, active)?;
    Ok(RootEffects {
        actions,
        steps,
        active,
    })
}
/// Select one actual original nominated slot. Other rows are neither treated
/// as proof nor silently consumed: the orchestrator must reject unused rows.
fn slot_context(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    slot: &str,
    introduced: &[DraftEnvelope],
) -> Result<DraftContextData> {
    if meter::cmp(m, &c.authorities.len().into(), &32.into())?.is_gt() {
        return Err(shape::fail("authority slot bound"));
    }
    let mut selected = None;
    for frame in &c.authorities {
        m.visit("root.nominated_slot")?;
        if shape::same_text(m, &frame.source_slot, slot)? {
            if selected.is_some() {
                return Err(shape::fail("duplicate authority source slot"));
            }
            selected = Some(frame);
        }
    }
    let frame = selected.ok_or_else(|| {
        Error::new(
            "AUTHORITY_REQUIRED",
            "original correction source nomination missing",
        )
    })?;
    let observation = r.get(
        m,
        &c.key,
        &frame.authority_observation_ref,
        "workflow-authority-prefix-observation",
        "ledger-workflow-authority-prefix-observation/1",
    )?;
    let verifier = shape::reference(m, &observation["verifier_ref"])?;
    let mut pending = vec![];
    for input in &c.source_observation_inputs {
        m.visit("root.pending_source_operand")?;
        if !shape::same_ref(m, &input.reference, &frame.authority_observation_ref)?
            && !shape::same_ref(m, &input.reference, &verifier)?
        {
            continue;
        }
        let mut known = false;
        for record in introduced {
            if shape::same_ref(m, &record.reference, &input.reference)? {
                known = true;
                break;
            }
        }
        if !known {
            m.scan_bytes(input.canonical_body.len())?;
            m.write_bytes(input.canonical_body.len())?;
            pending.push(input.clone());
        }
    }
    Ok(DraftContextData {
        key: c.key.clone(),
        actor: c.actor.clone(),
        canonical_economic_namespace: c.canonical_economic_namespace.clone(),
        installation_mode: c.installation_mode.clone(),
        received_at: c.received_at.clone(),
        accepted_at: c.accepted_at.clone(),
        authorities: vec![frame.clone()],
        source_observation_inputs: pending,
    })
}
fn root_context(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    request: &Value,
) -> Result<DraftContextData> {
    let (_, work) = load(
        m,
        r,
        c,
        &request["work_ref"],
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let (_, binding) = load(
        m,
        r,
        c,
        &work["payload"]["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/1",
    )?;
    let bindings = shape::array(m, &binding, "bindings")?;
    if !meter::cmp(m, &bindings.len().into(), &1.into())?.is_eq() {
        return Err(unsupported("selected retail binding count"));
    }
    let terms_ref = shape::reference(m, &bindings[0]["binding_ref"])?;
    let terms = r.get_schemas(
        m,
        &c.key,
        &terms_ref,
        "workflow-slot-terms",
        &[
            "ledger-workflow-slot-binding-terms/2",
            "ledger-workflow-slot-binding-terms/3",
        ],
    )?;
    let slot = text(m, &terms, "correction_source_slot")?;
    slot_context(m, r, c, slot, &[])
}
/// Both entry modes use this same original-term interpreter. The return value
/// remains noncommittable DATA until candidate resource/capacity/funding and
/// host custody/atomicity gates are independently completed.
pub(super) fn execute<'inv>(
    program: &SelectedProgram,
    c: &DraftContextData,
    mut request: Value,
    inputs: &[RecordInput],
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction> {
    let mut pool = inputs.to_vec();
    pool.extend_from_slice(&c.source_observation_inputs);
    let mut r = Records::new(m, &pool)?;
    if let Some((owner, evidence)) = intrinsic {
        r = r.with_intrinsic_evidence(m, &c.key, owner, evidence)?;
    }
    let local = root_context(m, &mut r, c, &request)?;
    let evidence = causal::references(m, &request["evidence_refs"], 0, 32)?;
    request["evidence_refs"] = json!(evidence);
    let plan = prepare(program, &local, &mut request, &mut r, state, m)?;
    dependent::evidence_membership(m, &mut r, &local, &plan.operands, &evidence)?;
    let active = active_actions(m, &mut r, &local, &plan)?;
    let families = causal::prepare(m, &mut r, &local, program, &plan, &mut request, state)?;
    let mut introduced = plan.operands.introduced.clone();
    let mut closures = vec![];
    let mut checks = vec![];
    let mut used_slots = vec![local.authorities[0].source_slot.clone()];
    for family in families {
        m.visit("root.close_original_family")?;
        let closed = dependent::build(m, &mut r, c, program, &plan, family, &introduced)?;
        introduced.extend(closed.introduced.iter().cloned());
        checks.push(closed.check(m, &plan)?);
        let mut seen = false;
        for previous in &used_slots {
            if shape::same_text(m, previous, &closed.source_slot)? {
                seen = true;
                break;
            }
        }
        if !seen {
            used_slots.push(closed.source_slot.clone());
        }
        closures.push(closed);
    }
    // No extra nomination can silently escape its contextual validation.
    for supplied in &c.authorities {
        let mut seen = false;
        for used in &used_slots {
            if shape::same_text(m, &supplied.source_slot, used)? {
                seen = true;
                break;
            }
        }
        if !seen {
            return Err(shape::fail("unused correction authority nomination"));
        }
    }
    if !meter::cmp(m, &c.authorities.len().into(), &used_slots.len().into())?.is_eq() {
        return Err(shape::fail("duplicate correction authority nomination"));
    }
    for supplied in &c.source_observation_inputs {
        let mut seen = false;
        for consumed in &introduced {
            if shape::same_ref(m, &supplied.reference, &consumed.reference)? {
                seen = true;
                break;
            }
        }
        if !seen {
            return Err(shape::fail("unused correction pending observation"));
        }
    }
    let authority = plan.operands.authority_document(m, Some(&checks))?;
    let effects = monetary_effects(m, c, &plan, &active)?;
    let mut total = BigInt::from(0);
    for price in &plan.new_prices {
        total = meter::add(m, &total, &price.atoms.into())?;
    }
    entry::exposure(m, &plan.operands.binding, &plan.operands.slot_terms, &total)?;
    let mut records = introduced;
    for closed in &closures {
        records.push(closed.authority.clone());
    }
    records.push(authority.clone());
    let mut steps = vec![];
    let mut heads = ProposedHeads {
        decision: vec![],
        family: vec![],
        chain: None,
    };
    for closed in closures {
        records.extend(closed.records.iter().cloned());
        steps.extend(closed.steps.iter().cloned());
        if let Some(decision) = &closed.decision {
            let body = codec::parse(m, &decision.canonical_body, 262144)?;
            heads.decision.push((
                text(m, &body, "decision_id")?.to_owned(),
                text(m, &body, "revision")?.to_owned(),
                decision.reference.clone(),
            ));
            records.push(decision.clone());
        }
        let (family, revision) = closed.finalize(m, c, &plan, &authority.reference)?;
        heads.family.push((
            closed.family.target_ref.clone(),
            text(m, &closed.family.prior, "family_slot")?.to_owned(),
            revision,
            family.reference.clone(),
        ));
        records.push(family);
    }
    records.extend(effects.actions);
    steps.extend(effects.steps.iter().cloned());
    let evaluation = body::document(
        m,
        "workflow-evaluation",
        json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":plan.work_ref,"policy_ref":plan.payload["policy_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"compiler_ref":program.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":plan.payload["binding_ref"],"fact_schema_ref":plan.payload["fact_schema_ref"],"facts":plan.replacement_facts,"context":plan.operands.binding["accepted_context"],"steps":effects.steps}),
    )?;
    let id = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([plan.current["decision_id"], plan.next_revision]),
    )?;
    let mut root_actions = vec![];
    // Only root postings belong to the root revision; dependent inverses are
    // explained in their own immutable decision/evaluation records.
    for record in &records {
        if shape::same_text(m, &record.reference.kind, "workflow-action")? {
            let value = codec::parse(m, &record.canonical_body, 262144)?;
            if shape::same(m, &value["decision_id"], &plan.current["decision_id"])? {
                root_actions.push(record.reference.clone());
            }
        }
    }
    let root_actions = body::ref_set(m, root_actions)?;
    let decision = body::record(
        m,
        &c.key,
        "workflow-decision",
        id,
        json!({"schema":"ledger-workflow-decision-revision/1","decision_id":plan.current["decision_id"],"revision":plan.next_revision,"prior_ref":plan.current_ref,"subject_ref":plan.work_ref,"kind":"work","transition":if plan.reverse{"work_reverse"}else{"quantity_correct"},"status":if plan.reverse{"reversed"}else{"active"},"original_terms_ref":plan.payload["binding_ref"],"authority_ref":authority.reference,"evaluation_ref":evaluation.reference,"actions_appended":root_actions,"active_action_refs":effects.active,"dependency_refs":plan.current["dependency_refs"],"operation_key":c.key,"accepted_at":c.accepted_at}),
    )?;
    heads.decision.push((
        text(m, &plan.current, "decision_id")?.to_owned(),
        plan.next_revision.clone(),
        decision.reference.clone(),
    ));
    records.push(evaluation);
    records.push(decision);
    let prior_ref = state
        .chain_head
        .as_ref()
        .ok_or_else(|| shape::fail("root correction chain head"))?;
    let prior = r.get(
        m,
        &c.key,
        prior_ref,
        "workflow-chain-state",
        "ledger-workflow-chain-state/1",
    )?;
    let (revision, admitted) = entry::chain_head_with_links(
        m,
        &mut r,
        &local,
        &plan.payload,
        prior_ref,
        &prior,
        &state.links,
    )?;
    let revision = meter::add(m, &revision, &1.into())?.to_string();
    let id = body::semantic_id(
        m,
        "wchain_",
        "workflow-chain-state",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            plan.payload["customer"],
            plan.payload["chain"],
            revision
        ]),
    )?;
    let chain = body::record(
        m,
        &c.key,
        "workflow-chain-state",
        id,
        json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"customer":plan.payload["customer"],"chain":plan.payload["chain"],"binding_ref":plan.payload["binding_ref"],"accepted_agreement_ref":local.authorities[0].accepted_agreement_ref,"activation_ref":local.authorities[0].activation_ref,"revision":revision,"admitted_events":admitted.to_string(),"links_root":prior["links_root"],"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":prior_ref}),
    )?;
    heads.chain = Some(chain.reference.clone());
    records.push(chain);
    normalize_nomination_sets(m, &mut request)?;
    Ok(Construction {
        normalized_payload: codec::encode(m, &request, 262144)?,
        new_envelopes: records,
        evaluation_steps: steps,
        proposed_heads: heads,
        claim_introductions: vec![],
        link_introductions: vec![],
        prospective_obligations: vec![],
        consumed_refs: body::ref_set(m, r.consumed())?,
        history_edges: vec![],
    })
}
fn normalize_nomination_sets(m: &mut CoreMeter, p: &mut Value) -> Result<()> {
    for field in ["dependent_reversals", "family_closures"] {
        let values = shape::array(m, p, field)?;
        let mut ordered: Vec<(Vec<u8>, Value)> = vec![];
        for value in values {
            m.visit("causal.normalize_nomination")?;
            let bytes = codec::encode(m, value, 262144)?;
            let mut at = ordered.len();
            for (i, (existing, _)) in ordered.iter().enumerate() {
                m.visit("causal.nomination_compare")?;
                m.scan_bytes(existing.len())?;
                m.scan_bytes(bytes.len())?;
                if existing >= &bytes {
                    at = i;
                    break;
                }
            }
            for _ in at..ordered.len() {
                m.visit("causal.nomination_move")?;
            }
            ordered.insert(at, (bytes, value.clone()));
        }
        p[field] = json!(ordered.into_iter().map(|(_, v)| v).collect::<Vec<_>>());
    }
    Ok(())
}
