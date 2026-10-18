//! Same original cost-price interpreter, exact current-price comparison and
//! inverse/replacement record construction. No invoice total or saved amount
//! enters replacement pricing. Authority/capacity completes in the enclosing
//! factory, never through an expected-output or caller-verified parameter.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    CoreMeter, arithmetic, body, data::*, emitted_record::ConstructedEnvelope, meter, pricing,
    record_body::RecordBody, records::Records, shape, working_value::owning_reference,
};
use super::{plan::CostPlan, records};
use crate::{
    Result,
    canonical::audit_arena::{AuditArena, AuditedValue, ExternalBufferLease},
    validation_work::{self as w, ArithmeticOperation, RepresentationKind, ValidationWork},
};
use num_bigint::BigInt;
use serde_json::{Value, json};
use super::super::cost_postings::{scalar,amount,operation_key as key,exact_inverse,original_cost_posting,OriginalCostPosting};

pub(super) struct CostEvaluation {
    pub new_claim: Option<ConstructedEnvelope>,
    pub claim_ref: ExactRef,
    pub actions: Vec<ConstructedEnvelope>,
    pub evaluation: ConstructedEnvelope,
    pub decision: ConstructedEnvelope,
    pub lineage: records::CostLineage,
    _ownership: ExternalBufferLease,
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn extend(
    m: &mut CoreMeter,
    value: AuditedValue,
    extra: &[(&str, &Value)],
) -> Result<AuditedValue> {
    super::super::working_value::WorkingValue::Owned(value)
        .with_fields(m, extra)?
        .into_owned()
}
fn price_slot<'a>(
    m: &mut CoreMeter,
    inst: &'a Value,
    price: &pricing::PricedInstruction,
) -> Result<&'a Value> {
    let rows = shape::array(m, inst, "component_map")?;
    shape::named(m, rows, "template_key", &price.component_key)
}
fn posting_terms(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    plan: &CostPlan<'_>,
    price: &pricing::PricedInstruction,
    component: &Value,
) -> Result<(ExactRef, RecordBody, ExactRef)> {
    let bindings = shape::array(m, &plan.selected.binding, "bindings")?;
    let row = shape::named(m, bindings, "slot", &price.binding_slot)?;
    let reference = shape::reference(m, &row["binding_ref"])?;
    let book = shape::text(m, component, "book")?;
    let supplier = shape::same_text(m, book, "supplier")?;
    let schema = if supplier {
        "ledger-workflow-slot-binding-terms/4"
    } else {
        "ledger-workflow-slot-binding-terms/3"
    };
    let terms = r.get(m, &c.key, &reference, "workflow-slot-terms", schema)?;
    for field in ["agreement_id", "customer"] {
        eq(m, &terms[field], &plan.selected.binding[field])?;
    }
    eq(m, &terms["scope"], &json!(c.key.scope))?;
    eq(m, &terms["slot"], &json!(price.binding_slot))?;
    eq(m, &terms["book"], &component["book"])?;
    eq(m, &terms["role_slot"], &json!(price.role_slot))?;
    eq(m, &component["role_slot"], &terms["role_slot"])?;
    let activities = shape::array(m, &terms, "allowed_activities")?;
    if !shape::member(m, activities, &plan.selected.work["payload"]["activity"])? {
        return Err(shape::fail("cost posting activity absent original terms"));
    }
    if supplier {
        eq(
            m,
            &json!(reference),
            &plan.selected.original["supplier_terms_ref"],
        )?;
    } else {
        eq(m, &terms["exposure_scope"], &json!("original_work"))?;
    }
    let role_ref = shape::reference(m, &terms["role_ref"])?;
    let role = r.get(
        m,
        &c.key,
        &role_ref,
        "workflow-roles",
        "ledger-workflow-role-assignment/1",
    )?;
    for field in ["scope", "agreement_id", "customer", "role_slot"] {
        eq(m, &role[field], &terms[field])?;
    }
    let role_refs = shape::array(m, &plan.selected.binding, "roles_refs")?;
    if !shape::member(m, role_refs, &json!(role_ref))? {
        return Err(shape::fail("cost posting role absent original binding"));
    }
    let funding = shape::array(m, &plan.selected.binding, "funding_legs")?;
    let funding_name = shape::text(m, &terms, "funding_leg")?;
    let funding = shape::named(m, funding, "name", funding_name)?;
    eq(
        m,
        &funding["mode"],
        &json!(if supplier {
            "platform"
        } else {
            "customer_local"
        }),
    )?;
    eq(m, &funding["payer"], &role["payer"])?;
    if !supplier {
        eq(
            m,
            &funding["resource_owner"],
            &plan.selected.binding["customer"],
        )?;
        eq(m, &role["payer"], &plan.selected.binding["customer"])?;
    }
    Ok((reference, terms, role_ref))
}
fn verify_active(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &CostPlan<'_>,
    inst: &Value,
    catalog: &Value,
) -> Result<Vec<(ExactRef, RecordBody, pricing::PricedInstruction)>> {
    let Some(old) = &plan.current else {
        return Ok(Vec::new());
    };
    let eligible = super::operands::amount(m, &old.evidence["amount"])?;
    let activity = shape::text(m, &plan.selected.work["payload"], "activity")?;
    let supplier = shape::text(m, &plan.instruction["rule"], "binding_slot")?;
    let key = shape::text(m, &plan.instruction["rule"], "component_key")?;
    let prices = pricing::final_cost(program, activity, supplier, key, &eligible, m)?;
    let refs = shape::array(m, &old.decision, "active_action_refs")?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(refs.len())
        .map_err(|_| shape::fail("cost original inverse vector"))?;
    let (_, evaluation) = super::operands::load(
        m,
        r,
        c,
        &old.decision["evaluation_ref"],
        "workflow-evaluation",
        "ledger-workflow-evaluation-proof/1",
    )?;
    eq(m, &evaluation["subject_ref"], &json!(old.claim_ref))?;
    eq(
        m,
        &evaluation["compiled_plan_ref"],
        &json!(program.compiled_plan().reference()),
    )?;
    eq(
        m,
        &evaluation["binding_ref"],
        &json!(plan.selected.binding_ref),
    )?;
    eq(
        m,
        &evaluation["schema"],
        &json!("ledger-workflow-evaluation-proof/1"),
    )?;
    eq(
        m,
        &evaluation["policy_ref"],
        &plan.selected.work["payload"]["policy_ref"],
    )?;
    eq(
        m,
        &evaluation["fact_schema_ref"],
        &plan.selected.work["payload"]["fact_schema_ref"],
    )?;
    eq(
        m,
        &evaluation["compiler_ref"],
        &program.compiled_plan().canonical_value()["compiler_ref"],
    )?;
    eq(
        m,
        &evaluation["context"],
        &plan.selected.binding["accepted_context"],
    )?;
    eq(
        m,
        &evaluation["facts"],
        &plan.selected.work["payload"]["facts"],
    )?;
    for price in prices {
        m.visit("cost.recompute_current_price")?;
        let slot = price_slot(m, inst, &price)?;
        let slot_id = shape::text(m, slot, "slot_id")?;
        let catalog_rows = shape::array(m, catalog, "components")?;
        let component = shape::named(m, catalog_rows, "slot_id", slot_id)?;
        let (terms_ref, terms, role_ref) = posting_terms(m, r, c, plan, &price, component)?;
        let mappings = shape::array(m, inst, "component_map")?;
        let root_mapping = shape::named(m, mappings, "template_key", &price.root_key)?;
        let effect = body::semantic_id(
            m,
            "weffect_",
            "workflow-effect",
            &json!([
                c.key.scope,
                plan.selected.binding["agreement_id"],
                slot_id,
                plan.original_work_claim_ref.id,
                "self",
                "original"
            ]),
        )?;
        let mut found = None;
        for value in refs {
            let (reference, action) = super::operands::load(
                m,
                r,
                c,
                value,
                "workflow-action",
                "ledger-workflow-action/1",
            )?;
            if !shape::same(m, &action["component_slot"], &json!(slot_id))? {
                continue;
            }
            if found.is_some() {
                return Err(shape::fail("duplicate current cost component action"));
            }
            eq(m, &action["effect_id"], &json!(effect))?;
            eq(m, &action["decision_id"], &old.decision["decision_id"])?;
            eq(m, &action["revision"], &old.decision["revision"])?;
            eq(m, &action["book"], &component["book"])?;
            eq(m, &action["slot_terms_ref"], &json!(terms_ref))?;
            eq(m, &action["role_ref"], &json!(role_ref))?;
            eq(m, &action["schema"], &json!("ledger-workflow-action/1"))?;
            eq(m, &action["root_component_slot"], &root_mapping["slot_id"])?;
            eq(m, &action["funding_leg"], &terms["funding_leg"])?;
            eq(m, &action["basis_refs"], &json!([]))?;
            let original = shape::same(m, &old.decision["transition"], &json!("cost_finalize"))?;
            if !original {
                eq(m, &old.decision["transition"], &json!("cost_correct"))?;
            }
            eq(
                m,
                &action["posting_role"],
                &json!(if original { "original" } else { "replacement" }),
            )?;
            if action.get("inverse_of").is_some() {
                return Err(shape::fail("current cost active action cannot be inverse"));
            }
            let actual = super::operands::amount(m, &action["amount"])?;
            if !meter::cmp(m, &actual, &price.atoms.into())?.is_eq() {
                return Err(shape::fail(
                    "current cost action differs from original formula",
                ));
            }
            found = Some((reference, action));
        }
        let nonzero = !meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq();
        if m.check("cost.current_nonzero_action", || nonzero != found.is_some())? {
            return Err(shape::fail("current cost zero/action mismatch"));
        }
        let mut step = None;
        for candidate in shape::array(m, &evaluation, "steps")? {
            if !shape::same(m, &candidate["instruction"], &json!(price.ordinal))?
                || shape::same(m, &candidate["disposition"], &json!("inverse"))?
            {
                continue;
            }
            if step.is_some() {
                return Err(shape::fail("duplicate current cost evaluation step"));
            }
            step = Some(candidate);
        }
        let step = step
            .ok_or_else(|| shape::fail("current cost evaluation omitted original instruction"))?;
        eq(
            m,
            &step["rational_numerator"],
            &json!(price.rational.numerator.to_string()),
        )?;
        eq(
            m,
            &step["rational_denominator"],
            &json!(price.rational.denominator.to_string()),
        )?;
        let actual = super::operands::amount(m, &step["rounded"])?;
        if !meter::cmp(m, &actual, &price.atoms.into())?.is_eq() {
            return Err(shape::fail("cost evaluation original rounded amount"));
        }
        if let Some((reference, action)) = found {
            eq(m, &step["action_ref"], &json!(reference))?;
            output.push((reference, action, price));
        } else if step.get("action_ref").is_some() {
            return Err(shape::fail("cost zero evaluation has action"));
        }
    }
    if !w::compare(m, output.len(), refs.len())?.is_eq() {
        return Err(shape::fail("current cost includes unmatched active action"));
    }
    Ok(output)
}

fn reserve_items(
    m: &mut CoreMeter,
    arena: &AuditArena,
    count: usize,
    item: usize,
) -> Result<ExternalBufferLease> {
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Multiply,
        &[&count.into(), &item.into()],
    )?;
    let bytes = count
        .checked_mul(item)
        .ok_or_else(|| shape::fail("cost producer metadata overflow"))?;
    arena.reserve_external_payload(bytes, m)
}
fn value_array(
    m: &mut CoreMeter,
    arena: &AuditArena,
    values: &[AuditedValue],
) -> Result<AuditedValue> {
    let _metadata = reserve_items(m, arena, values.len(), std::mem::size_of::<&Value>())?;
    let mut borrowed = Vec::new();
    borrowed
        .try_reserve_exact(values.len())
        .map_err(|_| shape::fail("cost evaluation array metadata"))?;
    for value in values {
        m.visit("cost.borrow_evaluation_step")?;
        borrowed.push(value.value());
    }
    arena.array_from_values(&borrowed, m)
}
/// One connected monetary producer. The enclosing executor supplies the actual
/// newly constructed authority and its derived immutable dependency set; this
/// function cannot mint custody, capacity or a complete EconomicCandidate.
/// Initial and correction paths share the SAME original cost-price interpreter.
pub(super) fn evaluate(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &CostPlan<'_>,
    authority: &ConstructedEnvelope,
    dependencies: &[ExactRef],
) -> Result<CostEvaluation> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning cost evaluation"))?;
    let activity = shape::text(m, &plan.selected.work["payload"], "activity")?;
    let supplier_slot = shape::text(m, &plan.instruction["rule"], "binding_slot")?;
    let supplier_component = shape::text(m, &plan.instruction["rule"], "component_key")?;
    let prices = pricing::final_cost(
        program,
        activity,
        supplier_slot,
        supplier_component,
        &plan.proof.eligible,
        m,
    )?;
    let old_count = match &plan.current {
        None => 0,
        Some(old) => shape::array(m, &old.decision, "active_action_refs")?.len(),
    };
    let total = w::usize_add(m, prices.len(), old_count)?;
    if w::compare(m, total, 128usize)?.is_gt() {
        return Err(shape::fail("cost appended action bound"));
    }
    // All control vectors and their worst-case bounded Ref/String payloads are
    // reserved before allocation. Canonical bodies/Value trees retain their
    // separate owning artifact leases; upstream pricing remains in the composed
    // full-call ledger and is not claimed covered by this vector reservation.
    let item = w::usize_add(
        m,
        std::mem::size_of::<ConstructedEnvelope>(),
        std::mem::size_of::<AuditedValue>(),
    )?;
    let item = w::usize_add(
        m,
        item,
        std::mem::size_of::<(ExactRef, RecordBody, pricing::PricedInstruction)>(),
    )?;
    let item = w::usize_add(m, item, 3 * (std::mem::size_of::<ExactRef>() + 263))?;
    let item = w::usize_add(m, item, 5 * 128 + 2 * std::mem::size_of::<BigInt>())?;
    let ownership = reserve_items(m, &arena, total, item)?;
    let (_, inst) = super::operands::load(
        m,
        r,
        c,
        &plan.selected.binding["slot_instantiation_ref"],
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let (_, catalog) = super::operands::load(
        m,
        r,
        c,
        &plan.selected.binding["slot_catalog_ref"],
        "workflow-slot-catalog",
        "ledger-workflow-slot-catalog/1",
    )?;
    let old = verify_active(m, r, c, program, plan, &inst, &catalog)?;
    let authority_value = authority.decoded(m, 65536)?;
    eq(
        m,
        &authority_value["schema"],
        &json!("ledger-workflow-economic-authority/4"),
    )?;
    eq(
        m,
        &authority_value["original_binding_ref"],
        &json!(plan.selected.binding_ref),
    )?;
    eq(
        m,
        &authority_value["original_invocation_ref"],
        &json!(plan.selected.original_ref),
    )?;
    eq(
        m,
        &authority_value["operation"],
        &json!(if plan.current.is_some() {
            "cost.correct"
        } else {
            "cost.finalize"
        }),
    )?;
    let constants=m.parse_owned_representation(&arena,br#"{"action":"ledger-workflow-action/1","evaluation":"ledger-workflow-evaluation-proof/1","decision":"ledger-workflow-decision-revision/2","inverse":"inverse","replacement":"replacement","original":"original","posted":"posted","zero":"zero","true":"true","one":"1","kind":"cost","status":"active","initial":"cost_finalize","correct":"cost_correct"}"#,RepresentationKind::SourceDefinition)?;
    let revision = scalar(m, &arena, &plan.next_revision)?;
    let operation_key = key(m, &arena, c)?;
    let decision_id = arena.string_value(&plan.decision_id, m)?;
    let new_claim = if plan.current.is_none() {
        let component = arena.string_value(&plan.supplier_component_slot, m)?;
        Some(records::claim(
            m,
            c,
            &plan.selected,
            &plan.proof,
            component.value(),
        )?)
    } else {
        None
    };
    let (claim_ref, claim_body) = match (&new_claim, &plan.current) {
        (Some(claim), None) => (claim.reference.clone(), claim.decoded(m, 4194304)?),
        (None, Some(old)) => (old.claim_ref.clone(), old.claim.clone()),
        _ => return Err(shape::fail("cost claim producer partition")),
    };
    let mut actions = Vec::new();
    actions
        .try_reserve_exact(total)
        .map_err(|_| shape::fail("cost actions capacity"))?;
    let mut steps = Vec::new();
    steps
        .try_reserve_exact(total)
        .map_err(|_| shape::fail("cost steps capacity"))?;
    let mut active = Vec::new();
    active
        .try_reserve_exact(prices.len())
        .map_err(|_| shape::fail("cost active capacity"))?;
    // Reverse the original actual posting order, never arbitrary Ref/JCS order.
    // verify_active returns the original compiled instruction order.
    for (reference,prior,price) in old.iter().rev(){
        m.visit("cost.inverse_original_action")?;
        let inverse=exact_inverse(m,c,reference,prior,price,&plan.next_revision)?;
        let action=inverse.action.ok_or_else(||shape::fail("missing exact inverse posting"))?;
        steps.push(inverse.step);actions.push(action);
    }
    for price in &prices {
        m.visit("cost.original_price_posting")?;
        let mapping=price_slot(m,&inst,price)?;
        let slot=shape::text(m,mapping,"slot_id")?;
        let component_map=shape::array(m,&inst,"component_map")?;
        let root_mapping=shape::named(m,component_map,"template_key",&price.root_key)?;
        let catalog_rows=shape::array(m,&catalog,"components")?;
        let component=shape::named(m,catalog_rows,"slot_id",slot)?;
        let (terms_ref,terms,role_ref)=posting_terms(m,r,c,plan,price,component)?;
        let posting=original_cost_posting(m,c,OriginalCostPosting{
            binding:&plan.selected.binding,original_work_claim:&plan.original_work_claim_ref,
            component_slot:&mapping["slot_id"],root_slot:&root_mapping["slot_id"],book:&component["book"],
            terms_ref:&terms_ref,terms:&terms,role_ref:&role_ref,decision_id:&plan.decision_id,
            revision:&plan.next_revision,replacement:plan.current.is_some(),basis_refs:None,
        },price)?;
        if let Some(action)=posting.action{active.push(action.reference.clone());actions.push(action);}
        steps.push(posting.step);
    }
    let subject = owning_reference(m, &arena, &claim_ref)?;
    let binding = owning_reference(m, &arena, &plan.selected.binding_ref)?;
    let compiled_ref = program.compiled_plan().reference();
    let compiled_kind = arena.string_value(compiled_ref.kind(), m)?;
    let compiled_id = arena.string_value(compiled_ref.id(), m)?;
    let compiled_digest = arena.string_value(compiled_ref.digest(), m)?;
    let compiled = arena.object_from_fields(
        &[
            ("kind", compiled_kind.value()),
            ("id", compiled_id.value()),
            ("digest", compiled_digest.value()),
        ],
        m,
    )?;
    let step_values = value_array(m, &arena, &steps)?;
    let payload = &plan.selected.work["payload"];
    let value = arena.object_from_fields(
        &[
            ("schema", &constants.value()["evaluation"]),
            ("subject_ref", subject.value()),
            ("policy_ref", &payload["policy_ref"]),
            ("compiled_plan_ref", compiled.value()),
            (
                "compiler_ref",
                &program.compiled_plan().canonical_value()["compiler_ref"],
            ),
            ("binding_ref", binding.value()),
            ("fact_schema_ref", &payload["fact_schema_ref"]),
            ("facts", &payload["facts"]),
            ("context", &plan.selected.binding["accepted_context"]),
            ("steps", step_values.value()),
        ],
        m,
    )?;
    let evaluation = body::document_borrowed(m, "workflow-evaluation", value.value())?;
    let mut appended = Vec::new();
    appended
        .try_reserve_exact(actions.len())
        .map_err(|_| shape::fail("cost appended refs"))?;
    for action in &actions {
        m.visit("cost.appended_action_ref")?;
        appended.push(action.reference.clone());
    }
    let appended = body::ref_set(m, appended)?;
    let active = body::ref_set(m, active)?;
    let appended = records::reference_array(m, &arena, &appended)?;
    let active = records::reference_array(m, &arena, &active)?;
    // A derived closure owns this set. Its producer is still a candidate gate;
    // this monetary slice neither fabricates it nor treats it as origin proof.
    if w::compare(m, dependencies.len(), 128usize)?.is_gt() {
        return Err(shape::fail("cost dependency ref bound"));
    }
    let dependencies = records::reference_array(m, &arena, dependencies)?;
    let authority_ref = owning_reference(m, &arena, &authority.reference)?;
    let evaluation_ref = owning_reference(m, &arena, &evaluation.reference)?;
    let accepted = arena.string_value(&c.accepted_at, m)?;
    let mut value = arena.object_from_fields(
        &[
            ("schema", &constants.value()["decision"]),
            ("decision_id", decision_id.value()),
            ("revision", revision.value()),
            ("subject_ref", subject.value()),
            ("kind", &constants.value()["kind"]),
            (
                "transition",
                if plan.current.is_some() {
                    &constants.value()["correct"]
                } else {
                    &constants.value()["initial"]
                },
            ),
            ("status", &constants.value()["status"]),
            ("original_terms_ref", binding.value()),
            ("authority_ref", authority_ref.value()),
            ("evaluation_ref", evaluation_ref.value()),
            ("actions_appended", appended.value()),
            ("active_action_refs", active.value()),
            ("dependency_refs", dependencies.value()),
            ("operation_key", operation_key.value()),
            ("accepted_at", accepted.value()),
        ],
        m,
    )?;
    if let Some(old) = &plan.current {
        let prior = owning_reference(m, &arena, &old.decision_ref)?;
        value = extend(m, value, &[("prior_ref", prior.value())])?;
    }
    let tuple = arena.array_from_values(&[decision_id.value(), revision.value()], m)?;
    let id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
    let decision = body::record_borrowed(m, &c.key, "workflow-decision", id, value.value())?;
    let prior = plan.current.as_ref().map(|old| records::PreviousLineage {
        revision_ref: &old.revision_ref,
        revision: &old.revision,
        owner_ref: &old.owner_ref,
        owner: &old.owner,
        consumption_ref: &old.consumption_ref,
        consumption: &old.consumption,
    });
    let lineage = records::advance(
        m,
        c,
        &plan.selected,
        &plan.proof,
        &claim_ref,
        &claim_body,
        &decision,
        &actions,
        prior,
    )?;
    Ok(CostEvaluation {
        new_claim,
        claim_ref,
        actions,
        evaluation,
        decision,
        lineage,
        _ownership: ownership,
    })
}
