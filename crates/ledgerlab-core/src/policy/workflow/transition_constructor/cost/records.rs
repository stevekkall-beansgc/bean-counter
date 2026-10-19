//! Immutable cost lineage construction after the shared economic evaluation.
//! These private functions do not accept expected money or a preverified flag.
//! They retain exact evidence, permanent claim and original invocation identity;
//! completed capacity/authority is required by the enclosing candidate factory.
use super::super::{
    body, data::*, emitted_record::ConstructedEnvelope, meter, shape,
    working_value::owning_reference, CoreMeter,
};
use super::{
    operands::{self, FinalEvidence, InvocationSelection},
    InvocationAmounts,
};
use crate::{
    canonical::audit_arena::{AuditArena, AuditedValue, ExternalBufferLease},
    validation_work::{self as w, RepresentationKind},
    Result,
};
use num_bigint::BigInt;
use serde_json::{json, Value};

pub(in crate::policy::workflow::transition_constructor) struct CostLineage {
    pub allocation: ConstructedEnvelope,
    pub owner: ConstructedEnvelope,
    pub consumption: ConstructedEnvelope,
    pub invocation: ConstructedEnvelope,
    pub revision: ConstructedEnvelope,
    _ownership: ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) struct PreviousLineage<'a> {
    pub revision_ref: &'a ExactRef,
    pub revision: &'a Value,
    pub owner_ref: &'a ExactRef,
    pub owner: &'a Value,
    pub consumption_ref: &'a ExactRef,
    pub consumption: &'a Value,
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn string(m: &mut CoreMeter, arena: &AuditArena, n: &BigInt) -> Result<AuditedValue> {
    // Every caller has established an Amount/Version bound before conversion.
    // Headers, transient decimal buffer and returned String overlap are held.
    let _scratch = arena.reserve_external_payload(256, m)?;
    m.scan_bytes(64)?;
    m.write_bytes(64)?;
    let raw = n.to_string();
    if w::compare(m, raw.len(), 64usize)?.is_gt() {
        return Err(shape::fail("cost record scalar encoding"));
    }
    arena.string_value(&raw, m)
}
fn amount(
    m: &mut CoreMeter,
    arena: &AuditArena,
    n: &BigInt,
    constants: &Value,
) -> Result<AuditedValue> {
    super::nonnegative(m, n)?;
    let atoms = string(m, arena, n)?;
    arena.object_from_fields(
        &[
            ("currency", &constants["currency"]),
            ("scale", &constants["scale"]),
            ("atoms", atoms.value()),
        ],
        m,
    )
}
fn record(
    m: &mut CoreMeter,
    c: &DraftContextData,
    id: String,
    v: &AuditedValue,
) -> Result<ConstructedEnvelope> {
    body::record_borrowed(m, &c.key, "workflow-cost", id, v.value())
}
fn with(m: &mut CoreMeter, value: AuditedValue, key: &str, extra: &Value) -> Result<AuditedValue> {
    super::super::working_value::WorkingValue::Owned(value)
        .with_fields(m, &[(key, extra)])?
        .into_owned()
}
fn owned_key(m: &mut CoreMeter, arena: &AuditArena, key: &OperationKey) -> Result<AuditedValue> {
    let store = arena.string_value(&key.logical_store_id, m)?;
    let source = arena.string_value(&key.source, m)?;
    let operation = arena.string_value(&key.operation_id, m)?;
    let first = arena.string_value(&key.scope[0], m)?;
    let second = arena.string_value(&key.scope[1], m)?;
    let scope = arena.array_from_values(&[first.value(), second.value()], m)?;
    arena.object_from_fields(
        &[
            ("logical_store_id", store.value()),
            ("scope", scope.value()),
            ("source", source.value()),
            ("operation_id", operation.value()),
        ],
        m,
    )
}
pub(super) fn reference_array(
    m: &mut CoreMeter,
    arena: &AuditArena,
    refs: &[ExactRef],
) -> Result<AuditedValue> {
    let item = w::usize_add(
        m,
        std::mem::size_of::<AuditedValue>(),
        std::mem::size_of::<&Value>(),
    )?;
    crate::validation_work::ValidationWork::arithmetic(
        m,
        crate::validation_work::ArithmeticOperation::Multiply,
        &[&refs.len().into(), &item.into()],
    )?;
    let bytes = refs
        .len()
        .checked_mul(item)
        .ok_or_else(|| shape::fail("cost owned reference array"))?;
    let bytes = w::usize_add(m, bytes, 2 * std::mem::size_of::<Vec<AuditedValue>>())?;
    let _metadata = arena.reserve_external_payload(bytes, m)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(refs.len())
        .map_err(|_| shape::fail("cost owned references"))?;
    for reference in refs {
        m.visit("cost.owned_reference")?;
        values.push(owning_reference(m, arena, reference)?);
    }
    let mut borrowed = Vec::new();
    borrowed
        .try_reserve_exact(values.len())
        .map_err(|_| shape::fail("cost borrowed reference array"))?;
    for value in &values {
        m.visit("cost.borrow_owned_reference")?;
        borrowed.push(value.value());
    }
    arena.array_from_values(&borrowed, m)
}
/// The global original provider-line claim excludes scope/agreement/current
/// labels. Host atomic indices enforce this key and the separate once-only
/// original WorkClaim/component key; neither is replaced by operation identity.
pub(in crate::policy::workflow::transition_constructor) fn claim(
    m: &mut CoreMeter,
    c: &DraftContextData,
    selected: &InvocationSelection,
    proof: &FinalEvidence,
    component_slot: &Value,
) -> Result<ConstructedEnvelope> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning cost claim"))?;
    let line = &proof.assertion["subject"]["line_key"];
    let tuple = arena.array_from_values(
        &[
            &line["cost_authority_id"],
            &line["provider_account_identity"],
            &line["original_document_id"],
            &line["original_line_id"],
        ],
        m,
    )?;
    let id = body::semantic_id(m, "wcostclaim_", "workflow-cost-claim", tuple.value())?;
    let constants = m.parse_owned_representation(
        &arena,
        br#"{"schema":"ledger-workflow-cost-claim/1"}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let binding = owning_reference(m, &arena, &selected.binding_ref)?;
    let work = owning_reference(m, &arena, &selected.work_ref)?;
    let invocation = owning_reference(m, &arena, &selected.original_ref)?;
    let evidence = owning_reference(m, &arena, &proof.assertion_ref)?;
    let key = owned_key(m, &arena, &c.key)?;
    let value = arena.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            ("line_key", line),
            ("owner_ref", &selected.original["cost_authority_ref"]),
            ("original_binding_ref", binding.value()),
            ("original_work_ref", work.value()),
            ("original_invocation_ref", invocation.value()),
            ("supplier_component_slot", component_slot),
            ("original_evidence_ref", evidence.value()),
            ("operation_key", key.value()),
        ],
        m,
    )?;
    record(m, c, id, &value)
}
/// Produce all permanent line/allocation/high-water/invocation updates together
/// from one evaluated COST decision. Only the exact current lineage may advance;
/// an independent source/label/work cannot receive prior high-water credit.
pub(in crate::policy::workflow::transition_constructor) fn advance(
    m: &mut CoreMeter,
    c: &DraftContextData,
    selected: &InvocationSelection,
    proof: &FinalEvidence,
    claim_reference: &ExactRef,
    claim_value: &Value,
    decision: &ConstructedEnvelope,
    actions: &[ConstructedEnvelope],
    prior: Option<PreviousLineage<'_>>,
) -> Result<CostLineage> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning cost lineage"))?;
    let metadata = w::usize_add(m, std::mem::size_of::<CostLineage>(), 4096)?;
    let ownership = arena.reserve_external_payload(metadata, m)?;
    let constants=m.parse_owned_representation(&arena,br#"{"allocation":"ledger-cost-local-allocation/1","owner":"ledger-cost-line-owner/1","consumption":"ledger-cost-obligation-consumption/1","revision":"ledger-workflow-cost-revision/1","currency":"USD","scale":18,"zero":"0","completed":"completed"}"#,RepresentationKind::SourceDefinition)?;
    let d = decision.decoded(m, 4194304)?;
    eq(
        m,
        &d["schema"],
        &json!("ledger-workflow-decision-revision/2"),
    )?;
    eq(m, &d["kind"], &json!("cost"))?;
    eq(m, &d["subject_ref"], &json!(claim_reference))?;
    eq(m, &d["original_terms_ref"], &json!(selected.binding_ref))?;
    let revision =
        super::super::super::facts::counter_with_work(shape::text(m, &d, "revision")?, m)?;
    if !w::compare(m, revision, 0u64)?.is_gt() {
        return Err(shape::fail("positive cost decision revision"));
    }
    let cost_revision = string(m, &arena, &revision.into())?;
    let eligible = operands::amount(m, &proof.assertion["amount"])?;
    let previous = match &prior {
        None => BigInt::from(0),
        Some(old) => operands::amount(m, &old.consumption["high_water"])?,
    };
    let current = InvocationAmounts {
        authorized: meter::add(m, &selected.amounts.authorized, &0.into())?,
        consumed: meter::add(m, &selected.amounts.consumed, &0.into())?,
        held: meter::add(m, &selected.amounts.held, &0.into())?,
        released: meter::add(m, &selected.amounts.released, &0.into())?,
    };
    let transition = super::high_water(m, current, &previous, &eligible)?;
    super::conserved(m, &transition.invocation)?;
    let previous_invocation_revision = super::super::super::facts::counter_with_work(
        shape::text(m, &selected.current, "revision")?,
        m,
    )?;
    let next_invocation_revision = meter::add(m, &previous_invocation_revision.into(), &1.into())?;
    let next_invocation_revision = string(m, &arena, &next_invocation_revision)?;
    for (key, want) in [
        ("original_binding_ref", json!(selected.binding_ref)),
        ("original_work_ref", json!(selected.work_ref)),
        ("original_invocation_ref", json!(selected.original_ref)),
    ] {
        eq(m, &claim_value[key], &want)?;
    }
    eq(
        m,
        &claim_value["line_key"],
        &proof.assertion["subject"]["line_key"],
    )?;
    let original_decision = match &prior {
        None => {
            if !w::compare(m, revision, 1u64)?.is_eq() {
                return Err(shape::fail("initial cost lineage revision"));
            }
            if !meter::cmp(m, &transition.high_water, &transition.eligible)?.is_eq()
                || !meter::cmp(m, &transition.delta, &transition.eligible)?.is_eq()
            {
                return Err(shape::fail("new claim cannot inherit consumption credit"));
            }
            owning_reference(m, &arena, &decision.reference)?
        }
        Some(old) => {
            for value in [old.revision, old.consumption] {
                eq(m, &value["cost_claim_ref"], &json!(claim_reference))?;
            }
            eq(
                m,
                &old.revision["consumption_ref"],
                &json!(old.consumption_ref),
            )?;
            eq(m, &old.revision["owner_ref"], &json!(old.owner_ref))?;
            eq(
                m,
                &old.revision["current_evidence_ref"],
                &old.owner["current_evidence_ref"],
            )?;
            eq(m, &old.owner["line_key"], &claim_value["line_key"])?;
            eq(
                m,
                &old.owner["original_invocation_ref"],
                &claim_value["original_invocation_ref"],
            )?;
            eq(
                m,
                &old.consumption["original_invocation_ref"],
                &claim_value["original_invocation_ref"],
            )?;
            let old_revision = super::super::super::facts::counter_with_work(
                shape::text(m, old.revision, "revision")?,
                m,
            )?;
            let next = meter::add(m, &old_revision.into(), &1.into())?;
            if !meter::cmp(m, &revision.into(), &next)?.is_eq() {
                return Err(shape::fail("cost original lineage increment"));
            }
            eq(
                m,
                &proof.assertion["prior_final_evidence_ref"],
                &old.revision["current_evidence_ref"],
            )?;
            let previous = operands::amount(m, &old.consumption["high_water"])?;
            let expected = meter::add(m, &previous, &transition.delta)?;
            if !meter::cmp(m, &expected, &transition.high_water)?.is_eq() {
                return Err(shape::fail("cost high-water lineage"));
            }
            arena.clone_value(&old.owner["supplier_obligation_ref"], m)?
        }
    };
    let claim_ref = owning_reference(m, &arena, &claim_reference)?;
    let evidence_ref = owning_reference(m, &arena, &proof.assertion_ref)?;
    let invocation_ref = owning_reference(m, &arena, &selected.original_ref)?;
    let key = owned_key(m, &arena, &c.key)?;
    let eligible = amount(m, &arena, &transition.eligible, constants.value())?;
    let zero = BigInt::from(0);
    let remainder = amount(m, &arena, &zero, constants.value())?;
    let mut retail = Vec::new();
    let mut supplier = None;
    let slots = w::usize_add(m, actions.len(), 1)?;
    crate::validation_work::ValidationWork::arithmetic(
        m,
        crate::validation_work::ArithmeticOperation::Multiply,
        &[&slots.into(), &std::mem::size_of::<ExactRef>().into()],
    )?;
    let per_ref = w::usize_add(m, std::mem::size_of::<ExactRef>(), 263)?;
    crate::validation_work::ValidationWork::arithmetic(
        m,
        crate::validation_work::ArithmeticOperation::Multiply,
        &[&slots.into(), &per_ref.into()],
    )?;
    let bytes = slots
        .checked_mul(per_ref)
        .ok_or_else(|| shape::fail("cost action reference metadata"))?;
    let _action_metadata = arena.reserve_external_payload(bytes, m)?;
    retail
        .try_reserve_exact(actions.len())
        .map_err(|_| shape::fail("cost action references"))?;
    for action in actions {
        m.visit("cost.allocation_action")?;
        let value = action.decoded(m, 4194304)?;
        eq(m, &value["decision_id"], &d["decision_id"])?;
        eq(m, &value["revision"], &d["revision"])?;
        let posting = shape::text(m, &value, "posting_role")?;
        if shape::same_text(m, posting, "inverse")? {
            continue;
        }
        let book = shape::text(m, &value, "book")?;
        if shape::same_text(m, book, "supplier")? {
            if supplier.is_some() {
                return Err(shape::fail(
                    "one supplier action per original cost obligation",
                ));
            }
            let atoms = operands::amount(m, &value["amount"])?;
            if !meter::cmp(m, &atoms, &transition.eligible)?.is_eq() {
                return Err(shape::fail(
                    "supplier payable differs from eligible final line",
                ));
            }
            supplier = Some(owning_reference(m, &arena, &action.reference)?);
        } else {
            eq(m, &value["book"], &json!("retail"))?;
            retail.push(action.reference.clone());
        }
    }
    let nonzero = !meter::cmp(m, &transition.eligible, &zero)?.is_eq();
    if m.check("cost.supplier_action_presence", || {
        nonzero != supplier.is_some()
    })? {
        return Err(shape::fail("zero supplier action disposition"));
    }
    let retail = body::ref_set(m, retail)?;
    let retail = reference_array(m, &arena, &retail)?;
    let mut allocation = arena.object_from_fields(
        &[
            ("schema", &constants.value()["allocation"]),
            ("cost_claim_ref", claim_ref.value()),
            ("revision", cost_revision.value()),
            ("evidence_ref", evidence_ref.value()),
            ("retail_action_refs", retail.value()),
            ("eligible_supplier_amount", eligible.value()),
            ("retained_owner_remainder", remainder.value()),
        ],
        m,
    )?;
    if let Some(supplier) = supplier {
        allocation = with(m, allocation, "supplier_action_ref", supplier.value())?;
    }
    let id = body::semantic_id(
        m,
        "wcostallocation_",
        "workflow-cost-allocation",
        &json!([claim_reference.id, revision.to_string()]),
    )?;
    let allocation = record(m, c, id, &allocation)?;
    let allocation_ref = owning_reference(m, &arena, &allocation.reference)?;
    let terms_ref = owning_reference(m, &arena, &selected.terms_ref)?;
    let terms_refs = arena.array_from_values(&[terms_ref.value()], m)?;
    let owner = arena.object_from_fields(
        &[
            ("schema", &constants.value()["owner"]),
            ("line_key", &claim_value["line_key"]),
            ("owner_ref", &selected.original["cost_authority_ref"]),
            (
                "original_evidence_ref",
                &claim_value["original_evidence_ref"],
            ),
            ("supplier_terms_refs", terms_refs.value()),
            ("original_invocation_ref", invocation_ref.value()),
            ("original_ceiling", &selected.original["authorized"]),
            ("supplier_obligation_ref", original_decision.value()),
            ("revision", cost_revision.value()),
            ("current_evidence_ref", evidence_ref.value()),
            ("allocation_root", allocation_ref.value()),
        ],
        m,
    )?;
    let line = &claim_value["line_key"];
    let tuple = arena.array_from_values(
        &[
            &line["cost_authority_id"],
            &line["provider_account_identity"],
            &line["original_document_id"],
            &line["original_line_id"],
        ],
        m,
    )?;
    let id = body::semantic_id(m, "wcostowner_", "workflow-cost-owner", tuple.value())?;
    let owner = record(m, c, id, &owner)?;
    let high = amount(m, &arena, &transition.high_water, constants.value())?;
    let delta = amount(m, &arena, &transition.delta, constants.value())?;
    let mut consumption = arena.object_from_fields(
        &[
            ("schema", &constants.value()["consumption"]),
            ("cost_claim_ref", claim_ref.value()),
            ("original_invocation_ref", invocation_ref.value()),
            ("revision", cost_revision.value()),
            ("eligible_amount", eligible.value()),
            ("high_water", high.value()),
            ("consumption_delta", delta.value()),
            ("operation_key", key.value()),
        ],
        m,
    )?;
    if let Some(old) = &prior {
        let old_ref = owning_reference(m, &arena, old.consumption_ref)?;
        consumption = with(m, consumption, "prior_ref", old_ref.value())?;
    }
    let id = body::semantic_id(
        m,
        "wcostconsumption_",
        "workflow-cost-consumption",
        &json!([claim_reference.id, revision.to_string()]),
    )?;
    let consumption = record(m, c, id, &consumption)?;
    let consumed_ref = owning_reference(m, &arena, &consumption.reference)?;
    let mut invocation = arena.clone_value(&selected.current, m)?;
    invocation.replace_existing_owned("revision", next_invocation_revision, m)?;
    invocation.replace_existing_owned(
        "consumed",
        amount(
            m,
            &arena,
            &transition.invocation.consumed,
            constants.value(),
        )?,
        m,
    )?;
    invocation.replace_existing_owned(
        "held",
        amount(m, &arena, &transition.invocation.held, constants.value())?,
        m,
    )?;
    invocation.replace_existing_owned(
        "released",
        amount(
            m,
            &arena,
            &transition.invocation.released,
            constants.value(),
        )?,
        m,
    )?;
    invocation.replace_existing_owned(
        "state",
        arena.clone_value(&constants.value()["completed"], m)?,
        m,
    )?;
    invocation.replace_existing_owned("operation_key", arena.clone_value(key.value(), m)?, m)?;
    let current_ref = owning_reference(m, &arena, &selected.current_ref)?;
    if selected.current.get("prior_ref").is_some() {
        invocation.replace_existing_owned("prior_ref", current_ref, m)?;
    } else {
        invocation = with(m, invocation, "prior_ref", current_ref.value())?;
    }
    // Exact latest per-claim set. Replace only this immutable claim's previous
    // consumption; every other original obligation remains represented.
    let old_values = shape::array(m, &selected.current, "cost_consumption_refs")?;
    let count = w::usize_add(m, old_values.len(), 1)?;
    // One controlled exact-capacity Ref vector. The independently owning
    // reference_array below accounts its Values/key nodes before every copy;
    // no json! graph is allocated and retroactively counted.
    let ref_payload = w::usize_add(m, std::mem::size_of::<ExactRef>(), 263)?;
    let per = ref_payload;
    crate::validation_work::ValidationWork::arithmetic(
        m,
        crate::validation_work::ArithmeticOperation::Multiply,
        &[&count.into(), &per.into()],
    )?;
    let bytes = count
        .checked_mul(per)
        .ok_or_else(|| shape::fail("cost consumption reference metadata"))?;
    let bytes = w::usize_add(m, bytes, 3 * std::mem::size_of::<Vec<ExactRef>>())?;
    let _consumption_metadata = arena.reserve_external_payload(bytes, m)?;
    let mut old_refs = Vec::new();
    old_refs
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("cost current consumption references"))?;
    for value in old_values {
        m.visit("cost.advance_consumption_set")?;
        let reference = shape::reference(m, value)?;
        if let Some(old) = &prior {
            if shape::same_ref(m, &reference, old.consumption_ref)? {
                continue;
            }
        }
        old_refs.push(reference);
    }
    old_refs.push(consumption.reference.clone());
    let old_refs = body::ref_set(m, old_refs)?;
    let refs = reference_array(m, &arena, &old_refs)?;
    invocation.replace_existing_owned("cost_consumption_refs", refs, m)?;
    let invocation = body::record_borrowed(
        m,
        &c.key,
        "workflow-invocation",
        selected.current_ref.id.clone(),
        invocation.value(),
    )?;
    let invocation_ref = owning_reference(m, &arena, &invocation.reference)?;
    let decision_ref = owning_reference(m, &arena, &decision.reference)?;
    let owner_ref = owning_reference(m, &arena, &owner.reference)?;
    let mut revision_value = arena.object_from_fields(
        &[
            ("schema", &constants.value()["revision"]),
            ("cost_claim_ref", claim_ref.value()),
            ("revision", cost_revision.value()),
            ("owner_ref", owner_ref.value()),
            ("current_decision_ref", decision_ref.value()),
            ("current_evidence_ref", evidence_ref.value()),
            ("invocation_state_ref", invocation_ref.value()),
            ("operation_key", key.value()),
            ("consumption_ref", consumed_ref.value()),
        ],
        m,
    )?;
    if let Some(old) = &prior {
        let old_ref = owning_reference(m, &arena, old.revision_ref)?;
        revision_value = with(m, revision_value, "prior_ref", old_ref.value())?;
    }
    let id = body::semantic_id(
        m,
        "wcostrevision_",
        "workflow-cost-revision",
        &json!([claim_reference.id, revision.to_string()]),
    )?;
    let revision = record(m, c, id, &revision_value)?;
    Ok(CostLineage {
        allocation,
        owner,
        consumption,
        invocation,
        revision,
        _ownership: ownership,
    })
}
