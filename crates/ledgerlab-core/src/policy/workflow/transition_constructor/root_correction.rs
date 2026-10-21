//! Original-term work correction. The interpreter selects affected named roots
//! from the original plan and recomputes quantities through the original schema.
//! No saved action amount, current price, or caller capacity is a pricing input.
use super::super::selected_program::SelectedProgram;
use super::emitted_record::ConstructedEnvelope;
use super::record_body::RecordBody;
use super::records::SuppliedRecords;
use super::{
    arithmetic,
    authority::{self, EconomicMode},
    body, codec,
    data::*,
    inventory::entry,
    meter, outcome, pricing,
    records::Records,
    representation::{InvocationToken, ParsedDraftTimes},
    shape, CoreMeter,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde_json::{json, Value};
mod causal;
mod context;
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
) -> Result<(ExactRef, RecordBody)> {
    let reference = shape::reference(m, v)?;
    let value = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, value))
}
/// This owned plan is still DATA, not a capacity certificate or admitted bill.
/// Complete causal closure must consume it before any new record is returned.
struct RootPlan {
    work_ref: ExactRef,
    work: RecordBody,
    claim_ref: ExactRef,
    original_ref: ExactRef,
    original: RecordBody,
    current_ref: ExactRef,
    current: RecordBody,
    next_revision: String,
    payload: super::working_value::WorkingValue,
    replacement_facts: super::working_value::WorkingValue,
    affected_roots: Vec<String>,
    old_prices: pricing::PricedInstructions,
    new_prices: pricing::PricedInstructions,
    operands: authority::ResolvedOperands,
    reverse: bool,
    _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
fn original_schema<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    reference: &Value,
) -> Result<&'a crate::policy::workflow::FactSchema> {
    for (r, schema) in p.fact_schemas() {
        m.visit("root.original_schema")?;
        let same = if let Some(arena) = m.representation_arena() {
            let original = super::working_value::owning_definition(m, &arena, r)?;
            shape::same(m, original.value(), reference)?
        } else {
            shape::same(m, &json!(r), reference)?
        };
        if same {
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
        let same = if let Some(arena) = m.representation_arena() {
            let original = super::working_value::owning_definition(m, &arena, d.reference())?;
            shape::same(m, original.value(), reference)?
        } else {
            shape::same(m, &json!(d.reference()), reference)?
        };
        if same {
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
    request: &mut super::working_value::WorkingValue,
    r: &mut Records,
    state: &DraftStateData,
    m: &mut CoreMeter,
) -> Result<RootPlan> {
    entry::context_selected(m, c, program)?;
    let (metadata, maximum_roots) = if let Some(arena) = m.representation_arena() {
        use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
        let components = shape::array(
            m,
            program.compiled_plan().canonical_value(),
            "component_slots",
        )?;
        let row = std::mem::size_of::<String>() + 128;
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&components.len().into(), &row.into()],
        )?;
        let roots = components
            .len()
            .checked_mul(row)
            .ok_or_else(|| shape::fail("root plan affected components"))?;
        // Four retained exact Ref payloads plus captured-next revision. Five
        // simultaneously live fixed hash/ID outputs and revision formatting
        // controls overlap the retained plan. Native arithmetic scratch is
        // independently unfinished; this lease grants no such credit.
        let fixed = std::mem::size_of::<RootPlan>()
            + 4 * (64 + 128 + 71)
            + 65
            + 5 * (std::mem::size_of::<String>() + 128)
            + 2 * std::mem::size_of::<String>()
            + 128;
        let size = w::usize_add(m, roots, fixed)?;
        let owner = arena.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        (Some(owner), components.len())
    } else {
        (None, 0)
    };
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
    let original_payload = field(m, &work, "payload")?;
    let payload = super::working_value::WorkingValue::copy(m, original_payload)?;
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
    let claim_identity = if let Some(arena) = m.representation_arena() {
        let key = super::cost_postings::operation_key(m, &arena, c)?;
        let completion = arena.string_value("completion", m)?;
        let work_reference = super::working_value::owning_reference(m, &arena, &work_ref)?;
        for (name, wanted) in [
            ("logical_store_id", &key.value()["logical_store_id"]),
            ("scope", &key.value()["scope"]),
            ("source_namespace", &work["source_namespace"]),
            ("business_operation_id", &payload["business_operation_id"]),
            ("completion", completion.value()),
            ("work_ref", work_reference.value()),
        ] {
            m.visit("root.owning_claim_field")?;
            equal(m, &claim[name], wanted)?;
        }
        let namespace = text(m, &work, "source_namespace")?;
        super::work_keys::claim(m, &c.key, namespace, &payload)?
    } else {
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

        body::semantic_id(
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
        )?
    };
    if !shape::same_text(m, &claim_ref.id, &claim_identity)? {
        return Err(shape::fail("permanent original work claim identity"));
    }
    outcome::original_claim_association(m, &claim, &work)?;
    let decision_id = if m.representation_arena().is_some() {
        super::work_keys::decision(m, claim_id)?
    } else {
        body::semantic_id(
            m,
            "wdecision_",
            "workflow-decision",
            &json!([claim_id, "work"]),
        )?
    };
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
    if let Some(arena) = m.representation_arena() {
        let id = arena.string_value(&decision_id, m)?;
        let subject = super::working_value::owning_reference(m, &arena, &work_ref)?;
        let kind = arena.string_value("work", m)?;
        let active = arena.string_value("active", m)?;
        let revision = arena.string_value(&head.revision, m)?;
        for (name, wanted) in [
            ("decision_id", id.value()),
            ("subject_ref", subject.value()),
            ("kind", kind.value()),
            ("status", active.value()),
            ("original_terms_ref", &payload["binding_ref"]),
            ("revision", revision.value()),
        ] {
            m.visit("root.owning_current_decision_field")?;
            equal(m, &current[name], wanted)?;
        }
    } else {
        equal(m, &current["decision_id"], &json!(decision_id))?;
        equal(m, &current["subject_ref"], &json!(work_ref))?;
        equal(m, &current["kind"], &json!("work"))?;
        equal(m, &current["status"], &json!("active"))?;
        equal(m, &current["original_terms_ref"], &payload["binding_ref"])?;
        equal(m, &current["revision"], &json!(head.revision))?;
    }
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
    let stable_revision = if let Some(arena) = m.representation_arena() {
        let id = arena.string_value(&decision_id, m)?;
        let revision = super::cost_postings::scalar(m, &arena, &old_revision)?;
        let tuple = arena.array_from_values(&[id.value(), revision.value()], m)?;
        body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?
    } else {
        body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!([decision_id, old_revision.to_string()]),
        )?
    };
    if !shape::same_text(m, &head.reference.id, &stable_revision)? {
        return Err(shape::fail("current root revision identity"));
    }
    let first_id = if let Some(arena) = m.representation_arena() {
        let id = arena.string_value(&decision_id, m)?;
        let one = arena.string_value("1", m)?;
        let tuple = arena.array_from_values(&[id.value(), one.value()], m)?;
        body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?
    } else {
        body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!([decision_id, "1"]),
        )?
    };
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
    if let Some(arena) = m.representation_arena() {
        let subject = super::working_value::owning_reference(m, &arena, &work_ref)?;
        let plan = super::working_value::owning_definition(
            m,
            &arena,
            program.compiled_plan().reference(),
        )?;
        for (name, wanted) in [
            ("subject_ref", subject.value()),
            ("policy_ref", &payload["policy_ref"]),
            ("compiled_plan_ref", plan.value()),
            (
                "compiler_ref",
                &program.compiled_plan().canonical_value()["compiler_ref"],
            ),
            ("binding_ref", &payload["binding_ref"]),
            ("fact_schema_ref", &payload["fact_schema_ref"]),
        ] {
            m.visit("root.owning_current_evaluation_field")?;
            equal(m, &evaluation[name], wanted)?;
        }
    } else {
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
    }
    let schema = original_schema(m, program, &payload["fact_schema_ref"])?;
    let normalized =
        super::working_value::NormalizedInput::normalize(m, schema, &evaluation["facts"])?;
    equal(m, normalized.canonical_value(), &evaluation["facts"])?;
    let current_facts = normalized.canonical_value();
    let mut replacement_facts = super::working_value::WorkingValue::copy(m, current_facts)?;
    let activity_name = text(m, &payload, "activity")?;
    let old_prices = pricing::work(program, activity_name, current_facts, m)?;
    let mut affected_roots: Vec<String> = vec![];
    if metadata.is_some() {
        affected_roots
            .try_reserve_exact(maximum_roots)
            .map_err(|_| shape::fail("root affected allocation"))?;
    }
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
                if metadata.is_some() {
                    m.scan_bytes(price.root_key.len())?;
                }
                m.write_bytes(price.root_key.len())?;
                affected_roots.push(price.root_key.clone());
            }
        }
    } else {
        let raw_field = text(m, request, "field")?;
        let _field_owner = if let Some(arena) = m.representation_arena() {
            let size = crate::validation_work::usize_add(
                m,
                std::mem::size_of::<String>(),
                raw_field.len(),
            )?;
            let owner = arena.reserve_external_payload(size, m)?;
            m.write_bytes(std::mem::size_of::<String>())?;
            Some(owner)
        } else {
            None
        };
        if metadata.is_some() {
            m.scan_bytes(raw_field.len())?;
        }
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
        replacement_facts.replace(m, &quantity_field, &request["replacement_quantity"])?;
        let normalized =
            super::working_value::NormalizedInput::normalize(m, schema, &replacement_facts)?;
        replacement_facts =
            super::working_value::WorkingValue::copy(m, normalized.canonical_value())?;
        request.replace(
            m,
            "replacement_quantity",
            &replacement_facts[quantity_field.as_str()],
        )?;
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
                if metadata.is_some() {
                    m.scan_bytes(root.len())?;
                }
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
    let current_activity = super::working_value::WorkingValue::copy(m, activity)?;
    let source_slot = if let Some(arena) = m.representation_arena() {
        super::working_value::WorkingValue::Owned(
            arena.string_value(&c.authorities[0].source_slot, m)?,
        )
    } else {
        super::working_value::WorkingValue::Legacy(json!(c.authorities[0].source_slot))
    };
    let current_activity = current_activity.with_fields(m, &[("source_slot", &source_slot)])?;
    let authority_payload =
        payload.with_fields(m, &[("evidence_refs", &request["evidence_refs"])])?;
    let owner = InvocationToken::new();
    let times = ParsedDraftTimes::parse(m, &owner, &c.received_at, &c.accepted_at)?;
    let mode = if reverse {
        EconomicMode::WorkReverse
    } else {
        EconomicMode::QuantityCorrect
    };
    let original_namespace = text(m, &work, "source_namespace")?;
    let operands = if m.representation_arena().is_some() {
        authority::resolve_economic_original(
            m,
            r,
            c,
            &authority_payload,
            &current_activity,
            product,
            (&owner, &times, program, mode, &[], original_namespace),
        )?
    } else {
        authority::resolve_economic(
            m,
            r,
            c,
            &authority_payload,
            &current_activity,
            product,
            (&owner, &times, program, mode, &[]),
        )?
    };
    equal(
        m,
        &operands.slot_terms["correction_source_slot"],
        &source_slot,
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
        (&work, &claim_ref, &original_ref, &original),
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
        pricing::PricedInstructions::empty()
    } else {
        pricing::work(program, activity_name, &replacement_facts, m)?
    };
    let next = meter::add(m, &old_revision, &1.into())?;
    let next_revision = if let Some(arena) = m.representation_arena() {
        let scalar = super::cost_postings::scalar(m, &arena, &next)?;
        let raw = scalar
            .value()
            .as_str()
            .ok_or_else(|| shape::fail("root next revision scalar"))?;
        m.scan_bytes(raw.len())?;
        m.write_bytes(raw.len())?;
        raw.to_owned()
    } else {
        next.to_string()
    };
    if metadata.is_some() {
        for v in [
            &head.reference.kind,
            &head.reference.id,
            &head.reference.digest,
        ] {
            m.scan_bytes(v.len())?;
            m.write_bytes(v.len())?;
        }
    }
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
        replacement_facts,
        affected_roots,
        old_prices,
        new_prices,
        operands,
        reverse,
        _ownership: metadata,
    })
}
struct ActiveRootAction {
    price_index: usize,
    reference: ExactRef,
    body: RecordBody,
    affected: bool,
}
struct ActiveRootActions {
    rows: Vec<ActiveRootAction>,
    _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl std::ops::Deref for ActiveRootActions {
    type Target = [ActiveRootAction];
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
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
) -> Result<ActiveRootActions> {
    if m.representation_arena().is_some() {
        return active_actions_owned(m, r, c, plan);
    }
    let references = shape::array(m, &plan.current, "active_action_refs")?;
    if meter::cmp(m, &references.len().into(), &128.into())?.is_gt() {
        return Err(shape::fail("root action bound"));
    }
    let mut actual: Vec<(ExactRef, RecordBody)> = vec![];
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
    Ok(ActiveRootActions {
        rows: out,
        _ownership: None,
    })
}
/// Reconstruct retained active postings with the SAME original/replacement
/// producer. The actual captured action key remains distinct from today's
/// correcting source; full current/original authority and origin checks stay
/// in their existing callers. No retained amount is a pricing operand here.
fn active_actions_owned(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    plan: &RootPlan,
) -> Result<ActiveRootActions> {
    use super::{
        cost_postings::{self, OriginalCostPosting},
        working_value::WorkingKey,
    };
    use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
    use std::mem;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("retained active posting owner"))?;
    let references = shape::array(m, &plan.current, "active_action_refs")?;
    if w::compare(m, references.len(), 128usize)?.is_gt() {
        return Err(shape::fail("root action bound"));
    }
    let rows = w::usize_add(m, references.len(), plan.old_prices.len())?;
    let per = mem::size_of::<ActiveRootAction>() + 64 + 128 + 71 + mem::size_of::<&ExactRef>();
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Multiply,
        &[&rows.into(), &per.into()],
    )?;
    let size = rows
        .checked_mul(per)
        .ok_or_else(|| shape::fail("active root metadata dimension"))?;
    let size = w::usize_add(
        m,
        size,
        mem::size_of::<ActiveRootActions>()
            + 3 * mem::size_of::<Vec<()>>()
            + mem::size_of::<ActiveRootAction>()
            + mem::size_of::<ExactRef>()
            + 64
            + 128
            + 71,
    )?;
    let owner = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    let mut actual: Vec<(ExactRef, RecordBody)> = Vec::new();
    actual
        .try_reserve_exact(references.len())
        .map_err(|_| shape::fail("current root operand rows"))?;
    for value in references {
        m.visit("root.owning_current_action_operand")?;
        let (reference, body) = load(
            m,
            r,
            c,
            value,
            "workflow-action",
            "ledger-workflow-action/1",
        )?;
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
        for (old, _) in &actual {
            m.visit("root.owning_distinct_active_action")?;
            if shape::same_ref(m, old, &reference)? {
                return Err(shape::fail("duplicate current active action"));
            }
        }
        actual.push((reference, body));
    }
    let decision = text(m, &plan.current, "decision_id")?;
    let root_revision = counter(m, &plan.current, "revision")?;
    let key = cost_postings::operation_key(m, &arena, c)?;
    let retail = arena.string_value("retail", m)?;
    let mut out: Vec<ActiveRootAction> = Vec::new();
    out.try_reserve_exact(plan.old_prices.len())
        .map_err(|_| shape::fail("repriced current root rows"))?;
    for (index, price) in plan.old_prices.iter().enumerate() {
        m.visit("root.owning_reprice_current_instruction")?;
        if meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq() {
            continue;
        }
        let mappings = shape::array(m, &plan.operands.instantiation, "component_map")?;
        let mapping = shape::named(m, mappings, "template_key", &price.component_key)?;
        let root = shape::named(m, mappings, "template_key", &price.root_key)?;
        let slot = text(m, mapping, "slot_id")?;
        let catalogs = shape::array(m, &plan.operands.catalog, "components")?;
        let catalog = shape::named(m, catalogs, "slot_id", slot)?;
        equal(m, &catalog["root_slot"], &root["slot_id"])?;
        let role = arena.string_value(&price.role_slot, m)?;
        equal(m, &catalog["role_slot"], role.value())?;
        equal(m, &catalog["book"], retail.value())?;
        let mut found = None;
        for (reference, body) in &actual {
            m.visit("root.owning_current_component")?;
            if text_is(m, body, "component_slot", slot)? {
                if found.is_some() {
                    return Err(shape::fail("two active actions for one original component"));
                }
                found = Some((reference, body));
            }
        }
        let (reference, current) =
            found.ok_or_else(|| shape::fail("repriced current action missing"))?;
        let revision = counter(m, current, "revision")?;
        if meter::cmp(m, &revision, &root_revision)?.is_gt() {
            return Err(shape::fail("future active action revision"));
        }
        let replacement = !meter::cmp(m, &revision, &1.into())?.is_eq();
        equal(
            m,
            &current["operation_key"]["logical_store_id"],
            &key.value()["logical_store_id"],
        )?;
        equal(m, &current["operation_key"]["scope"], &key.value()["scope"])?;
        let captured = WorkingKey::parse(m, &current["operation_key"])?;
        let count = price.basis_components.len();
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&count.into(), &mem::size_of::<&ExactRef>().into()],
        )?;
        let size = count
            .checked_mul(mem::size_of::<&ExactRef>())
            .ok_or_else(|| shape::fail("current root basis controls"))?;
        let size = w::usize_add(m, size, mem::size_of::<Vec<&ExactRef>>())?;
        let _basis = arena.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        let mut basis = Vec::new();
        basis
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("current root basis rows"))?;
        for name in &price.basis_components {
            m.visit("root.owning_current_basis_name")?;
            for prior in &out {
                m.visit("root.owning_current_basis_prefix")?;
                if shape::same_text(m, &plan.old_prices[prior.price_index].component_key, name)? {
                    basis.push(&prior.reference);
                }
            }
        }
        let basis = super::app_cost::reference_set(m, &arena, &basis)?;
        let expected = cost_postings::original_cost_posting_key(
            m,
            &captured,
            OriginalCostPosting {
                binding: &plan.operands.binding,
                original_work_claim: &plan.claim_ref,
                component_slot: &mapping["slot_id"],
                root_slot: &root["slot_id"],
                book: retail.value(),
                terms_ref: &plan.operands.slot_terms_ref,
                terms: &plan.operands.slot_terms,
                role_ref: &plan.operands.role_ref,
                decision_id: decision,
                revision: &revision,
                replacement,
                basis_refs: Some(basis.value()),
            },
            price,
        )?;
        let expected = expected
            .action
            .ok_or_else(|| shape::fail("nonzero active price lacks action"))?;
        if !shape::same_ref(m, reference, &expected.reference)? {
            return Err(shape::fail("current root action identity"));
        }
        let bytes = r.canonical(m, reference)?;
        if !shape::same_bytes(m, bytes, &expected.canonical_body)? {
            return Err(shape::fail("repriced current root action differs"));
        }
        let affected = is_affected(m, plan, &price.root_key)?;
        let mut copied = ExactRef {
            kind: String::new(),
            id: String::new(),
            digest: String::new(),
        };
        for (target, raw) in [
            (&mut copied.kind, &reference.kind),
            (&mut copied.id, &reference.id),
            (&mut copied.digest, &reference.digest),
        ] {
            m.scan_bytes(raw.len())?;
            m.write_bytes(raw.len())?;
            *target = raw.to_owned();
        }
        out.push(ActiveRootAction {
            price_index: index,
            reference: copied,
            body: current.clone(),
            affected,
        });
    }
    if !w::compare(m, out.len(), actual.len())?.is_eq() {
        return Err(shape::fail("unpriced current root action"));
    }
    Ok(ActiveRootActions {
        rows: out,
        _ownership: Some(owner),
    })
}
struct RootEffects {
    actions: Vec<ConstructedEnvelope>,
    steps: Vec<Value>,
    active: Vec<ExactRef>,
    owned_steps: Option<crate::canonical::audit_arena::AuditedValue>,
    owned_active: Option<crate::canonical::audit_arena::AuditedValue>,
    _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
fn monetary_effects(
    m: &mut CoreMeter,
    c: &DraftContextData,
    plan: &RootPlan,
    old: &[ActiveRootAction],
) -> Result<RootEffects> {
    if m.representation_arena().is_some() {
        return monetary_effects_owned(m, c, plan, old);
    }
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
        let mut inverse = (*prior.body).clone();
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
        owned_steps: None,
        owned_active: None,
        _ownership: None,
    })
}
fn monetary_effects_owned(
    m: &mut CoreMeter,
    c: &DraftContextData,
    plan: &RootPlan,
    old: &[ActiveRootAction],
) -> Result<RootEffects> {
    use super::cost_postings::{self, OriginalCostPosting};
    use crate::canonical::audit_arena::AuditedValue;
    use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
    use std::mem;
    fn mul(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
        ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
        a.checked_mul(b)
            .ok_or_else(|| shape::fail("root posting metadata"))
    }
    fn copy(m: &mut CoreMeter, v: &str) -> Result<String> {
        m.scan_bytes(v.len())?;
        m.write_bytes(v.len())?;
        Ok(v.to_owned())
    }
    fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
        Ok(ExactRef {
            kind: copy(m, &r.kind)?,
            id: copy(m, &r.id)?,
            digest: copy(m, &r.digest)?,
        })
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("root posting owner"))?;
    let count = w::usize_add(m, old.len(), plan.new_prices.len())?;
    // Each action, active Ref and component Ref has its own exact strings;
    // components borrow their immutable original instruction names. Basis
    // and final-step workspaces borrow references only. No Value DTO clone.
    let row = mem::size_of::<ConstructedEnvelope>()
        + 64
        + 128
        + 71
        + 2 * (mem::size_of::<ExactRef>() + 64 + 128 + 71)
        + mem::size_of::<&str>()
        + mem::size_of::<AuditedValue>()
        + mem::size_of::<&Value>()
        + mem::size_of::<&ExactRef>();
    let size = mul(m, count, row)?;
    let size = w::usize_add(
        m,
        size,
        mem::size_of::<RootEffects>() + 4 * mem::size_of::<Vec<()>>(),
    )?;
    let ownership = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    let mut actions = Vec::new();
    actions
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("root action allocation"))?;
    let mut steps = Vec::new();
    steps
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("root step allocation"))?;
    let mut active = Vec::new();
    active
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("root active Ref allocation"))?;
    let mut components: Vec<(&str, ExactRef)> = Vec::new();
    components
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("root component allocation"))?;
    let revision = arithmetic::integer(m, &plan.next_revision, false)?;
    let decision = text(m, &plan.current, "decision_id")?;
    for prior in old.iter().rev() {
        m.visit("root.owning_inverse_selection")?;
        if !prior.affected {
            continue;
        }
        let inverse = cost_postings::exact_root_inverse(
            m,
            c,
            &prior.reference,
            &prior.body,
            &plan.old_prices[prior.price_index],
            &revision,
        )?;
        let action = inverse
            .action
            .ok_or_else(|| shape::fail("exact root inverse lacks nonzero action"))?;
        actions.push(action);
        steps.push(inverse.step);
    }
    let constants = m.parse_owned_representation(
        &arena,
        br#"{"book":"retail"}"#,
        crate::validation_work::RepresentationKind::SourceDefinition,
    )?;
    for price in &plan.new_prices {
        m.visit("root.owning_replacement_instruction")?;
        if !is_affected(m, plan, &price.root_key)? {
            for prior in old {
                m.visit("root.owning_preserved_component")?;
                if shape::same_text(
                    m,
                    &plan.old_prices[prior.price_index].component_key,
                    &price.component_key,
                )? {
                    active.push(reference(m, &prior.reference)?);
                    components.push((&price.component_key, reference(m, &prior.reference)?));
                }
            }
            continue;
        }
        let mappings = shape::array(m, &plan.operands.instantiation, "component_map")?;
        let mapping = shape::named(m, mappings, "template_key", &price.component_key)?;
        let root = shape::named(m, mappings, "template_key", &price.root_key)?;
        let selected = {
            let controls = mul(m, count, mem::size_of::<&ExactRef>())?;
            let controls = w::usize_add(m, controls, mem::size_of::<Vec<&ExactRef>>())?;
            m.write_bytes(controls)?;
            let mut basis = Vec::new();
            basis
                .try_reserve_exact(count)
                .map_err(|_| shape::fail("root basis allocation"))?;
            for key in &price.basis_components {
                m.visit("root.owning_basis_component")?;
                for (name, r) in &components {
                    m.visit("root.owning_basis_prefix")?;
                    if shape::same_text(m, name, key)? {
                        basis.push(r);
                    }
                }
            }
            super::app_cost::reference_set(m, &arena, &basis)?
        };
        let posting = cost_postings::original_cost_posting(
            m,
            c,
            OriginalCostPosting {
                binding: &plan.operands.binding,
                original_work_claim: &plan.claim_ref,
                component_slot: &mapping["slot_id"],
                root_slot: &root["slot_id"],
                book: &constants.value()["book"],
                terms_ref: &plan.operands.slot_terms_ref,
                terms: &plan.operands.slot_terms,
                role_ref: &plan.operands.role_ref,
                decision_id: decision,
                revision: &revision,
                replacement: true,
                basis_refs: Some(selected.value()),
            },
            price,
        )?;
        if let Some(action) = posting.action {
            active.push(reference(m, &action.reference)?);
            components.push((&price.component_key, reference(m, &action.reference)?));
            actions.push(action);
        }
        steps.push(posting.step);
    }
    let mut borrowed = Vec::new();
    borrowed
        .try_reserve_exact(steps.len())
        .map_err(|_| shape::fail("root evaluation fields"))?;
    for step in &steps {
        m.visit("root.owning_evaluation_step")?;
        borrowed.push(step.value());
    }
    let owned_steps = arena.array_from_values(&borrowed, m)?;
    let mut active_borrowed = Vec::new();
    active_borrowed
        .try_reserve_exact(active.len())
        .map_err(|_| shape::fail("root active set fields"))?;
    for r in &active {
        m.visit("root.owning_active_set")?;
        active_borrowed.push(r);
    }
    let owned_active = super::app_cost::reference_set(m, &arena, &active_borrowed)?;
    // The legacy Draft exposes Value steps. Complete construction retains only
    // the actual owning serialized explanation, matching other RP01 producers.
    Ok(RootEffects {
        actions,
        steps: Vec::new(),
        active,
        owned_steps: Some(owned_steps),
        owned_active: Some(owned_active),
        _ownership: Some(ownership),
    })
}
/// Select one actual original nominated slot. Other rows are neither treated
/// as proof nor silently consumed: the orchestrator must reject unused rows.
fn slot_context(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    slot: &str,
    introduced: &[ConstructedEnvelope],
) -> Result<context::RootContext> {
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
    if frame.authority_observation_ref.is_none() {
        return context::selected_local(m, c, frame);
    }
    let observation = r.get(
        m,
        &c.key,
        frame.required_observation_ref()?,
        "workflow-authority-prefix-observation",
        "ledger-workflow-authority-prefix-observation/1",
    )?;
    if m.representation_arena().is_some() {
        return context::selected(m, c, frame, &observation["verifier_ref"], introduced);
    }
    let verifier = shape::reference(m, &observation["verifier_ref"])?;
    let mut pending = vec![];
    for input in &c.source_observation_inputs {
        m.visit("root.pending_source_operand")?;
        if !shape::same_ref(m, &input.reference, frame.required_observation_ref()?)?
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
    Ok(context::RootContext::legacy(DraftContextData {
        key: c.key.clone(),
        actor: c.actor.clone(),
        canonical_economic_namespace: c.canonical_economic_namespace.clone(),
        installation_mode: c.installation_mode.clone(),
        received_at: c.received_at.clone(),
        accepted_at: c.accepted_at.clone(),
        authorities: vec![frame.clone()],
        source_observation_inputs: pending,
    }))
}
fn root_context(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    request: &Value,
) -> Result<context::RootContext> {
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
        if program.uses_app_owned_cost_recipe() {
            "ledger-workflow-binding/2"
        } else {
            "ledger-workflow-binding/1"
        },
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
        if program.uses_app_cost_causal_recipe() {
            &["ledger-workflow-slot-binding-terms/7"]
        } else if program.uses_app_owned_cost_recipe() {
            &["ledger-workflow-slot-binding-terms/6"]
        } else {
            &[
                "ledger-workflow-slot-binding-terms/2",
                "ledger-workflow-slot-binding-terms/3",
            ]
        },
    )?;
    let slot = text(m, &terms, "correction_source_slot")?;
    slot_context(m, r, c, slot, &[])
}
/// Both entry modes use this same original-term interpreter. The return value
/// remains noncommittable DATA until candidate resource/capacity/funding and
/// host custody/atomicity gates are independently completed.
pub(super) fn execute<'inv, 'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    request: super::working_value::WorkingValue,
    inputs: SuppliedRecords<'raw>,
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction<'inv, 'raw>> {
    execute_with_state(program, c, request, inputs, (state, None), m, intrinsic)
}
pub(super) fn execute_complete<'inv, 'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    request: super::working_value::WorkingValue,
    inputs: SuppliedRecords<'raw>,
    state: &EconomicStateData,
    m: &mut CoreMeter,
) -> Result<Construction<'inv, 'raw>> {
    execute_with_state(
        program,
        c,
        request,
        inputs,
        (state.financial, Some(state)),
        m,
        None,
    )
}
fn execute_with_state<'inv, 'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    mut request: super::working_value::WorkingValue,
    inputs: SuppliedRecords<'raw>,
    selected_operands: (&DraftStateData, Option<&EconomicStateData>),
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction<'inv, 'raw>> {
    let (state, complete_state) = selected_operands;
    let mut r = inputs.resolve(m, &c.key, &c.source_observation_inputs)?;
    if let Some((owner, evidence)) = intrinsic {
        r = r.with_intrinsic_evidence(m, &c.key, owner, evidence)?;
    }
    let local = root_context(program, m, &mut r, c, &request)?;
    let evidence_owner = if let Some(arena) = m.representation_arena() {
        use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
        let values = shape::array(m, &request, "evidence_refs")?;
        if w::compare(m, values.len(), 32usize)?.is_gt() {
            return Err(shape::fail("root evidence reference count"));
        }
        let row =
            std::mem::size_of::<ExactRef>() + 64 + 128 + 71 + std::mem::size_of::<&ExactRef>();
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&values.len().into(), &row.into()],
        )?;
        let size = values
            .len()
            .checked_mul(row)
            .ok_or_else(|| shape::fail("root evidence metadata"))?;
        let size = w::usize_add(m, size, 2 * std::mem::size_of::<Vec<ExactRef>>())?;
        let owner = arena.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        Some(owner)
    } else {
        None
    };
    let evidence = if evidence_owner.is_some() {
        let values = shape::array(m, &request, "evidence_refs")?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(values.len())
            .map_err(|_| shape::fail("root evidence Ref rows"))?;
        for value in values {
            m.visit("root.owning_evidence_reference")?;
            let reference = shape::reference(m, value)?;
            for previous in &refs {
                m.visit("root.owning_distinct_evidence")?;
                if shape::same_ref(m, previous, &reference)? {
                    return Err(shape::fail("duplicate causal evidence"));
                }
            }
            refs.push(reference);
        }
        refs
    } else {
        causal::references(m, &request["evidence_refs"], 0, 32)?
    };
    if let Some(arena) = m.representation_arena() {
        let mut borrowed = Vec::new();
        borrowed
            .try_reserve_exact(evidence.len())
            .map_err(|_| shape::fail("root evidence fields"))?;
        for reference in &evidence {
            m.visit("root.owning_evidence_set")?;
            borrowed.push(reference);
        }
        let value = super::app_cost::reference_set(m, &arena, &borrowed)?;
        request.replace(m, "evidence_refs", value.value())?;
    } else {
        request.replace(m, "evidence_refs", &json!(evidence))?;
    }
    let _evidence_owner = evidence_owner;
    let plan = prepare(program, &local, &mut request, &mut r, state, m)?;
    let mut financial_reservations = None;
    let original_reservations = if let Some(arena) = m.representation_arena() {
        if program.uses_local_execution_recipe() {
            financial_reservations = Some(super::capacity::original::derive_financial_retained(
                &arena,
                m,
                &mut r,
                &local,
                program,
                &plan.original_ref,
                (
                    &plan.original,
                    &plan.work_ref,
                    &plan.work,
                    &plan.claim_ref,
                    &plan.operands.terms,
                ),
            )?);
            None
        } else {
            Some(super::capacity::original::derive_retained(
                &arena,
                m,
                &mut r,
                &local,
                program,
                &plan.original_ref,
                (
                    &plan.original,
                    &plan.work_ref,
                    &plan.work,
                    &plan.claim_ref,
                    &plan.operands.terms,
                ),
            )?)
        }
    } else {
        None
    };
    dependent::evidence_membership(m, &mut r, &local, &plan.operands, &evidence)?;
    let active = active_actions(m, &mut r, &local, &plan)?;
    let current_capacity = if program.uses_app_owned_cost_recipe() {
        let complete = complete_state
            .ok_or_else(|| shape::fail("app root capacity requires complete economic state"))?;
        Some(if program.uses_local_execution_recipe() {
            super::capacity::current::retained_local_root_head(
                m,
                &mut r,
                &local,
                &plan.original_ref,
                &plan.current_ref,
                complete,
            )?
        } else {
            super::capacity::current::retained_root_head(
                m,
                &mut r,
                &local,
                &plan.original_ref,
                &plan.current_ref,
                complete,
            )?
        })
    } else {
        None
    };
    let app_closures = if program.uses_app_owned_cost_recipe() {
        let complete = complete_state
            .ok_or_else(|| shape::fail("app root closure requires complete economic state"))?;
        Some(super::app_cost::causal::prepare(
            m,
            &mut r,
            &local,
            program,
            &plan.work_ref,
            &plan.current_ref,
            (
                if plan.reverse {
                    "work.reverse"
                } else {
                    "quantity.correct"
                },
                &plan.payload,
                &plan.operands.terms,
                &mut request,
                complete,
                current_capacity
                    .as_ref()
                    .ok_or_else(|| shape::fail("app closure capacity head missing"))?,
            ),
        )?)
    } else {
        None
    };
    let app_reversals = match &app_closures {
        Some(app) => app.used_reversals.as_slice(),
        None => &[],
    };
    let families = causal::prepare(
        m,
        &mut r,
        &local,
        program,
        &plan,
        &mut request,
        (state, app_reversals),
    )?;
    if let Some(app) = &app_closures {
        let app_count =
            crate::validation_work::usize_add(m, app.finalized.len(), app.pending.len())?;
        let combined = crate::validation_work::usize_add(m, app_count, families.len())?;
        if crate::validation_work::compare(m, combined, 31usize)?.is_gt() {
            return Err(shape::fail("combined family/app closure bound"));
        }
    }
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
    // Native Ref/control payloads of the FOUR common root output records,
    // their one retained authority clone, and the root/chain head copies.
    // Dependent/introduced aggregation remains independently unqualified.
    let root_output_owner = if let Some(arena) = m.representation_arena() {
        let size = 5 * (std::mem::size_of::<ConstructedEnvelope>() + 64 + 128 + 71)
            + std::mem::size_of::<(String, String, ExactRef)>()
            + 128
            + 65
            + 64
            + 128
            + 71
            + std::mem::size_of::<ExactRef>()
            + 64
            + 128
            + 71;
        // Each terminal family contributes its owning body row and held-head
        // metadata. The revision parser accepts at most64 digits, so adding
        // one needs at most65; target/slot/Ref payloads retain their existing
        // checked identifier bounds. Dependent monetary rows remain separate.
        let per_family = std::mem::size_of::<ConstructedEnvelope>()
            + 64
            + 128
            + 71
            + std::mem::size_of::<(ExactRef, String, String, ExactRef)>()
            + 2 * (64 + 128 + 71)
            + 128
            + 65;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            crate::validation_work::ArithmeticOperation::Multiply,
            &[&closures.len().into(), &per_family.into()],
        )?;
        let family = closures
            .len()
            .checked_mul(per_family)
            .ok_or_else(|| shape::fail("root terminal family metadata"))?;
        let size = crate::validation_work::usize_add(m, size, family)?;
        let owner = arena.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        Some(owner)
    } else {
        None
    };
    let authority = if let Some(app) = &app_closures {
        let base = plan.operands.app_authority_value(m, &checks)?;
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning parent app closure"))?;
        let n = crate::validation_work::usize_add(m, app.finalized.len(), app.pending.len())?;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            crate::validation_work::ArithmeticOperation::Multiply,
            &[&n.into(), &std::mem::size_of::<&Value>().into()],
        )?;
        let size = n
            .checked_mul(std::mem::size_of::<&Value>())
            .ok_or_else(|| shape::fail("parent app check metadata"))?;
        let _metadata = arena.reserve_external_payload(size, m)?;
        let mut finalized = Vec::new();
        finalized
            .try_reserve_exact(app.finalized.len())
            .map_err(|_| shape::fail("parent finalized check fields"))?;
        for closed in &app.finalized {
            m.visit("root.finalized_app_authority_check")?;
            finalized.push(closed.check.value());
        }
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(app.pending.len())
            .map_err(|_| shape::fail("parent pending check fields"))?;
        for closed in &app.pending {
            m.visit("root.pending_app_authority_check")?;
            pending.push(closed.check.value());
        }
        let finalized = arena.array_from_values(&finalized, m)?;
        let pending = arena.array_from_values(&pending, m)?;
        let value = super::working_value::WorkingValue::copy(m, base.value())?.with_fields(
            m,
            &[
                ("app_cost_closure_checks", finalized.value()),
                ("app_pending_cost_closure_checks", pending.value()),
            ],
        )?;
        let mut value = value.into_owned()?;
        super::intent::sort_owned_set(&mut value, "app_cost_closure_checks", false, m, &arena)?;
        super::intent::sort_owned_set(
            &mut value,
            "app_pending_cost_closure_checks",
            false,
            m,
            &arena,
        )?;
        body::document_borrowed(m, "workflow-economic-authority", value.value())?
    } else {
        plan.operands.authority_document(m, Some(&checks))?
    };
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
    if let Some(app) = &app_closures {
        for closed in &app.finalized {
            m.visit("root.app_finalized_result")?;
            records.push(closed.authority.clone());
            if let Some(inverse) = &closed.inverse {
                records.push(inverse.clone());
            }
            records.push(closed.evaluation.clone());
            records.push(closed.decision.clone());
            records.push(closed.revision.clone());
        }
        for closed in &app.pending {
            m.visit("root.app_pending_authority_result")?;
            records.push(closed.authority.clone());
        }
    }
    if root_output_owner.is_some() {
        for raw in [
            &authority.reference.kind,
            &authority.reference.id,
            &authority.reference.digest,
        ] {
            m.scan_bytes(raw.len())?;
            m.write_bytes(raw.len())?;
        }
    }
    records.push(authority.clone());
    let mut steps = vec![];
    let mut heads = ProposedHeads {
        decision: vec![],
        family: vec![],
        chain: None,
    };
    if let Some(app) = &app_closures {
        for closed in &app.finalized {
            let value = closed.decision.decoded(m, 4194304)?;
            heads.decision.push((
                text(m, &value, "decision_id")?.to_owned(),
                text(m, &value, "revision")?.to_owned(),
                closed.decision.reference.clone(),
            ));
        }
    }
    for closed in closures {
        records.extend(closed.records.iter().cloned());
        steps.extend(closed.steps.iter().cloned());
        if let Some(decision) = &closed.decision {
            let body = decision.decoded(m, 262144)?;
            heads.decision.push((
                text(m, &body, "decision_id")?.to_owned(),
                text(m, &body, "revision")?.to_owned(),
                decision.reference.clone(),
            ));
            records.push(decision.clone());
        }
        let (family, revision) = closed.finalize(m, c, &plan, &authority.reference)?;
        if root_output_owner.is_some() {
            for raw in [
                &closed.family.target_ref.kind,
                &closed.family.target_ref.id,
                &closed.family.target_ref.digest,
                text(m, &closed.family.prior, "family_slot")?,
                &family.reference.kind,
                &family.reference.id,
                &family.reference.digest,
            ] {
                m.scan_bytes(raw.len())?;
                m.write_bytes(raw.len())?;
            }
        }
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
    let evaluation = if let Some(explanation) = &effects.owned_steps {
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning root evaluation"))?;
        let schema = m.parse_owned_representation(
            &arena,
            br#""ledger-workflow-evaluation-proof/1""#,
            crate::validation_work::RepresentationKind::SourceDefinition,
        )?;
        let subject = super::working_value::owning_reference(m, &arena, &plan.work_ref)?;
        let compiled = program.compiled_plan().canonical_value();
        let plan_ref = super::working_value::owning_definition(
            m,
            &arena,
            program.compiled_plan().reference(),
        )?;
        let value = arena.object_from_fields(
            &[
                ("schema", schema.value()),
                ("subject_ref", subject.value()),
                ("policy_ref", &plan.payload["policy_ref"]),
                ("compiled_plan_ref", plan_ref.value()),
                ("compiler_ref", &compiled["compiler_ref"]),
                ("binding_ref", &plan.payload["binding_ref"]),
                ("fact_schema_ref", &plan.payload["fact_schema_ref"]),
                ("facts", &plan.replacement_facts),
                ("context", &plan.operands.binding["accepted_context"]),
                ("steps", explanation.value()),
            ],
            m,
        )?;
        body::document_borrowed(m, "workflow-evaluation", value.value())?
    } else {
        let evaluation = body::document(
            m,
            "workflow-evaluation",
            json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":plan.work_ref,"policy_ref":plan.payload["policy_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"compiler_ref":program.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":plan.payload["binding_ref"],"fact_schema_ref":plan.payload["fact_schema_ref"],"facts":plan.replacement_facts,"context":plan.operands.binding["accepted_context"],"steps":effects.steps}),
        )?;
        evaluation
    };
    let decision = if let Some(active) = &effects.owned_active {
        use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
        use std::mem;
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning corrected root decision"))?;
        let dependencies = shape::array(m, &plan.current, "dependency_refs")?;
        let ref_count = w::usize_add(m, dependencies.len(), 1)?;
        let ref_row = mem::size_of::<ExactRef>() + 64 + 128 + 71;
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&ref_count.into(), &ref_row.into()],
        )?;
        let refs = ref_count
            .checked_mul(ref_row)
            .ok_or_else(|| shape::fail("root dependency Ref metadata"))?;
        let count = w::usize_add(m, records.len(), dependencies.len())?;
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&count.into(), &mem::size_of::<&ExactRef>().into()],
        )?;
        let borrowed = count
            .checked_mul(mem::size_of::<&ExactRef>())
            .ok_or_else(|| shape::fail("root decision Ref workspace"))?;
        let size = w::usize_add(m, refs, borrowed)?;
        let size = w::usize_add(
            m,
            size,
            3 * mem::size_of::<Vec<()>>() + 4 * mem::size_of::<String>() + 128 + 71 + 64 + 71,
        )?;
        let _metadata = arena.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        let mut root = Vec::new();
        root.try_reserve_exact(records.len())
            .map_err(|_| shape::fail("root appended action set"))?;
        for record in &records {
            m.visit("root.owning_appended_action")?;
            if shape::same_text(m, &record.reference.kind, "workflow-action")? {
                let value = record.decoded(m, 262144)?;
                if shape::same(m, &value["decision_id"], &plan.current["decision_id"])? {
                    root.push(&record.reference);
                }
            }
        }
        let root = super::app_cost::reference_set(m, &arena, &root)?;
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(dependencies.len())
            .map_err(|_| shape::fail("root surviving dependencies"))?;
        for value in dependencies {
            m.visit("root.owning_original_dependency")?;
            let reference = shape::reference(m, value)?;
            let excluded = if let Some(app) = &app_closures {
                app.excludes(m, &reference)?
            } else {
                false
            };
            if !excluded {
                retained.push(reference);
            }
        }
        let mut borrowed = Vec::new();
        borrowed
            .try_reserve_exact(retained.len())
            .map_err(|_| shape::fail("root dependency set fields"))?;
        for r in &retained {
            m.visit("root.owning_surviving_dependency")?;
            borrowed.push(r);
        }
        let dependencies = super::app_cost::reference_set(m, &arena, &borrowed)?;
        let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-decision-revision/1","kind":"work","reverse":"work_reverse","correct":"quantity_correct","reversed":"reversed","active":"active"}"#,crate::validation_work::RepresentationKind::SourceDefinition)?;
        let revision = arena.string_value(&plan.next_revision, m)?;
        let seed = arena.array_from_values(&[&plan.current["decision_id"], revision.value()], m)?;
        let id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", seed.value())?;
        let prior = super::working_value::owning_reference(m, &arena, &plan.current_ref)?;
        let subject = super::working_value::owning_reference(m, &arena, &plan.work_ref)?;
        let authority_ref =
            super::working_value::owning_reference(m, &arena, &authority.reference)?;
        let evaluation_ref =
            super::working_value::owning_reference(m, &arena, &evaluation.reference)?;
        let key = super::cost_postings::operation_key(m, &arena, c)?;
        let at = arena.string_value(&c.accepted_at, m)?;
        let transition = if plan.reverse {
            &constants.value()["reverse"]
        } else {
            &constants.value()["correct"]
        };
        let status = if plan.reverse {
            &constants.value()["reversed"]
        } else {
            &constants.value()["active"]
        };
        let value = arena.object_from_fields(
            &[
                ("schema", &constants.value()["schema"]),
                ("decision_id", &plan.current["decision_id"]),
                ("revision", revision.value()),
                ("prior_ref", prior.value()),
                ("subject_ref", subject.value()),
                ("kind", &constants.value()["kind"]),
                ("transition", transition),
                ("status", status),
                ("original_terms_ref", &plan.payload["binding_ref"]),
                ("authority_ref", authority_ref.value()),
                ("evaluation_ref", evaluation_ref.value()),
                ("actions_appended", root.value()),
                ("active_action_refs", active.value()),
                ("dependency_refs", dependencies.value()),
                ("operation_key", key.value()),
                ("accepted_at", at.value()),
            ],
            m,
        )?;
        body::record_borrowed(m, &c.key, "workflow-decision", id, value.value())?
    } else {
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
                let value = record.decoded(m, 262144)?;
                if shape::same(m, &value["decision_id"], &plan.current["decision_id"])? {
                    root_actions.push(record.reference.clone());
                }
            }
        }
        let root_actions = body::ref_set(m, root_actions)?;
        let dependencies = if let Some(app) = &app_closures {
            let mut dependencies = vec![];
            for value in shape::array(m, &plan.current, "dependency_refs")? {
                let reference = shape::reference(m, value)?;
                if !app.excludes(m, &reference)? {
                    dependencies.push(reference);
                }
            }
            json!(body::ref_set(m, dependencies)?)
        } else {
            plan.current["dependency_refs"].clone()
        };

        body::record(
            m,
            &c.key,
            "workflow-decision",
            id,
            json!({"schema":"ledger-workflow-decision-revision/1","decision_id":plan.current["decision_id"],"revision":plan.next_revision,"prior_ref":plan.current_ref,"subject_ref":plan.work_ref,"kind":"work","transition":if plan.reverse{"work_reverse"}else{"quantity_correct"},"status":if plan.reverse{"reversed"}else{"active"},"original_terms_ref":plan.payload["binding_ref"],"authority_ref":authority.reference,"evaluation_ref":evaluation.reference,"actions_appended":root_actions,"active_action_refs":effects.active,"dependency_refs":dependencies,"operation_key":c.key,"accepted_at":c.accepted_at}),
        )?
    };
    if root_output_owner.is_some() {
        for raw in [
            text(m, &plan.current, "decision_id")?,
            &plan.next_revision,
            &decision.reference.kind,
            &decision.reference.id,
            &decision.reference.digest,
        ] {
            m.scan_bytes(raw.len())?;
            m.write_bytes(raw.len())?;
        }
    }
    heads.decision.push((
        text(m, &plan.current, "decision_id")?.to_owned(),
        plan.next_revision.clone(),
        decision.reference.clone(),
    ));
    records.push(evaluation);
    if let Some(app) = &app_closures {
        for pending in &app.pending {
            m.visit("root.pending_app_retirement_result")?;
            records.push(pending.retire(m, c, &decision)?);
        }
    }
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
    let chain = if let Some(arena) = m.representation_arena() {
        let revision = meter::add(m, &revision, &1.into())?;
        let revision = super::cost_postings::scalar(m, &arena, &revision)?;
        let admitted = super::cost_postings::scalar(m, &arena, &admitted)?;
        let key = super::cost_postings::operation_key(m, &arena, c)?;
        let tuple = arena.array_from_values(
            &[
                &key.value()["logical_store_id"],
                &key.value()["scope"],
                &plan.payload["customer"],
                &plan.payload["chain"],
                revision.value(),
            ],
            m,
        )?;
        let id = body::semantic_id(m, "wchain_", "workflow-chain-state", tuple.value())?;
        let agreement = super::working_value::owning_reference(
            m,
            &arena,
            &local.authorities[0].accepted_agreement_ref,
        )?;
        let activation = super::working_value::owning_reference(
            m,
            &arena,
            &local.authorities[0].activation_ref,
        )?;
        let prior_ref = super::working_value::owning_reference(m, &arena, prior_ref)?;
        let schema = arena.string_value("ledger-workflow-chain-state/1", m)?;
        let at = arena.string_value(&c.accepted_at, m)?;
        let value = arena.object_from_fields(
            &[
                ("schema", schema.value()),
                ("logical_store_id", &key.value()["logical_store_id"]),
                ("scope", &key.value()["scope"]),
                ("customer", &plan.payload["customer"]),
                ("chain", &plan.payload["chain"]),
                ("binding_ref", &plan.payload["binding_ref"]),
                ("accepted_agreement_ref", agreement.value()),
                ("activation_ref", activation.value()),
                ("revision", revision.value()),
                ("admitted_events", admitted.value()),
                ("links_root", &prior["links_root"]),
                ("operation_key", key.value()),
                ("accepted_at", at.value()),
                ("prior_ref", prior_ref.value()),
            ],
            m,
        )?;
        body::record_borrowed(m, &c.key, "workflow-chain-state", id, value.value())?
    } else {
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

        body::record(
            m,
            &c.key,
            "workflow-chain-state",
            id,
            json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"customer":plan.payload["customer"],"chain":plan.payload["chain"],"binding_ref":plan.payload["binding_ref"],"accepted_agreement_ref":local.authorities[0].accepted_agreement_ref,"activation_ref":local.authorities[0].activation_ref,"revision":revision,"admitted_events":admitted.to_string(),"links_root":prior["links_root"],"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":prior_ref}),
        )?
    };
    if root_output_owner.is_some() {
        for raw in [
            &chain.reference.kind,
            &chain.reference.id,
            &chain.reference.digest,
        ] {
            m.scan_bytes(raw.len())?;
            m.write_bytes(raw.len())?;
        }
    }
    heads.chain = Some(chain.reference.clone());
    records.push(chain);
    normalize_nomination_sets(m, &mut request)?;
    let (app_effects, app_heads_owner) = if let Some(app) = &app_closures {
        let count = app.finalized.len();
        let per = std::mem::size_of::<AppHeadProposal>() + 3 * (64 + 128 + 71) + 155;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            crate::validation_work::ArithmeticOperation::Multiply,
            &[&count.into(), &per.into()],
        )?;
        let size = count
            .checked_mul(per)
            .ok_or_else(|| shape::fail("root app head effect metadata"))?;
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning root app head effects"))?;
        let owner = arena.reserve_external_payload(size, m)?;
        let mut app_heads = Vec::new();
        app_heads
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("root app head effects vector"))?;
        for closed in &app.finalized {
            m.visit("root.app_head_cas_effect")?;
            app_heads.push(closed.head_proposal(m)?);
        }
        (
            Some(AppEffects {
                claims: Vec::new(),
                heads: app_heads,
            }),
            Some(owner),
        )
    } else {
        (None, None)
    };
    Ok(Construction {
        original_reservations,
        financial_reservations,
        economic: EconomicConstruction {
            terms: plan.operands.terms,
            original_work: OriginalWorkPayload::Retained(plan.work),
        },
        normalized_payload: request.bytes(m)?,
        new_envelopes: records,
        reused_result: None,
        evaluation_steps: steps,
        proposed_heads: heads,
        claim_introductions: vec![],
        link_introductions: vec![],
        prospective_obligations: vec![],
        records: r,
        history_edges: vec![],
        app_effects,
        _app_causal: app_closures,
        current_capacity,
        candidate_heads: None,
        candidate_introductions: None,
        financial_observation: None,
        funding_effects: None,
        control_owners: [
            app_heads_owner,
            effects._ownership,
            root_output_owner,
            None,
            None,
        ],
    })
}
fn normalize_nomination_sets(
    m: &mut CoreMeter,
    p: &mut super::working_value::WorkingValue,
) -> Result<()> {
    for key in ["dependent_reversals", "family_closures"] {
        p.normalize_set(m, key, false)?;
    }
    Ok(())
}
pub(super) fn normalize_object_field(m: &mut CoreMeter, p: &mut Value, field: &str) -> Result<()> {
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
    Ok(())
}
