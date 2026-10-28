//! Actual original supplier authorization and immutable Invocation lifecycle.
//! Held registration, completed originals and absence remain host obligations.
use super::super::selected_program::SelectedProgram;
use super::{
    arithmetic,
    authority::{EconomicMode, Terms},
    body,
    data::*,
    emitted_record::ConstructedEnvelope,
    inventory, meter,
    records::Records,
    representation::InvocationToken,
    shape,
    working_value::{owning_reference, parse_owned_reference, WorkingValue},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    domain::Timestamp,
    validation_work::{self as w, RepresentationKind},
    Error, Result,
};
use num_bigint::BigInt;
use serde_json::Value;
use std::mem;
fn arena(m: &mut CoreMeter) -> Result<AuditArena> {
    m.representation_arena()
        .ok_or_else(|| shape::fail("owning Invocation producer"))
}
fn literal(m: &mut CoreMeter, v: &Value, k: &str, s: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, s)? {
        return Err(shape::fail("Invocation literal"));
    }
    Ok(())
}
fn same_field(m: &mut CoreMeter, v: &Value, k: &str, want: &str) -> Result<bool> {
    let text = shape::text(m, v, k)?;
    shape::same_text(m, text, want)
}
fn member(m: &mut CoreMeter, v: &Value, k: &str, want: &Value) -> Result<()> {
    let rows = shape::array(m, v, k)?;
    if !shape::member(m, rows, want)? {
        return Err(shape::fail("Invocation original membership"));
    }
    Ok(())
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<super::record_body::RecordBody> {
    let reference = shape::reference(m, v)?;
    r.get(m, &c.key, &reference, kind, schema)
}
fn time(m: &mut CoreMeter, v: &Value, k: &str) -> Result<i64> {
    let raw = shape::text(m, v, k)?;
    Ok(Timestamp::parse_with_work(raw, m)?.micros())
}
fn ratio_bound(m: &mut CoreMeter, value: &arithmetic::Ratio, ceiling: &Value) -> Result<()> {
    let raw = ceiling
        .as_str()
        .ok_or_else(|| shape::fail("Invocation quantity ceiling"))?;
    let maximum = arithmetic::decimal(m, raw)?;
    let lhs = meter::mul(m, &value.numerator, &maximum.denominator)?;
    let rhs = meter::mul(m, &maximum.numerator, &value.denominator)?;
    if meter::cmp(m, &value.numerator, &0.into())?.is_lt() || meter::cmp(m, &lhs, &rhs)?.is_gt() {
        return Err(shape::fail("Invocation quantity exceeds original ceiling"));
    }
    Ok(())
}
fn amount(m: &mut CoreMeter, v: &Value) -> Result<BigInt> {
    shape::closed(m, v, &["currency", "scale", "atoms"], &[])?;
    literal(m, v, "currency", "USD")?;
    if v["scale"].as_u64() != Some(18) {
        return Err(shape::fail("Invocation amount scale"));
    }
    let raw = shape::text(m, v, "atoms")?;
    let n = arithmetic::integer(m, raw, false)?;
    let max = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, &n, &max)?.is_gt() {
        return Err(shape::fail("Invocation Amount bound"));
    }
    Ok(n)
}
fn amount_bound(m: &mut CoreMeter, value: &Value, ceiling: &Value) -> Result<()> {
    let n = amount(m, value)?;
    let max = amount(m, ceiling)?;
    if meter::cmp(m, &n, &max)?.is_gt() {
        return Err(shape::fail("Invocation original exposure ceiling"));
    }
    Ok(())
}
/// A quantity is an independently accepted usage operand, even for Fixed/Share
/// pricing. Exact Offer/Terms fields must uniquely agree; no default or scan of
/// arbitrary numeric facts supplies it.
pub(super) fn quantity_field<'a>(
    m: &mut CoreMeter,
    terms: &'a Value,
    offer: &Value,
    input: &Value,
) -> Result<&'a str> {
    let activity = shape::text(m, input, "activity")?;
    let unit = shape::text(m, input, "unit")?;
    let raw = shape::text(m, input, "maximum_quantity")?;
    let maximum = arithmetic::decimal(m, raw)?;
    let mut selected = None;
    for row in shape::array(m, terms, "quantity_ceilings")? {
        m.visit("invocation.original_quantity_field")?;
        shape::closed(m, row, &["activity", "field", "unit", "maximum"], &[])?;
        if !same_field(m, row, "activity", activity)? || !same_field(m, row, "unit", unit)? {
            continue;
        }
        let field = shape::text(m, row, "field")?;
        if let Some(old) = selected {
            if !shape::same_text(m, old, field)? {
                return Err(shape::fail("ambiguous original Invocation quantity field"));
            }
        }
        ratio_bound(m, &maximum, &row["maximum"])?;
        selected = Some(field);
    }
    let field =
        selected.ok_or_else(|| shape::fail("missing original Invocation quantity field"))?;
    let mut covered = false;
    for row in shape::array(m, offer, "unit_ceilings")? {
        m.visit("invocation.offer_quantity_field")?;
        shape::closed(m, row, &["activity", "field", "unit", "maximum"], &[])?;
        if same_field(m, row, "activity", activity)? && same_field(m, row, "unit", unit)? {
            if !same_field(m, row, "field", field)? {
                return Err(shape::fail("ambiguous Offer Invocation quantity field"));
            }
            ratio_bound(m, &maximum, &row["maximum"])?;
            covered = true;
        }
    }
    if !covered {
        return Err(shape::fail("Offer missing Invocation quantity field"));
    }
    Ok(field)
}
fn supplier(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    retail: &Terms,
) -> Result<(
    super::record_body::RecordBody,
    super::record_body::RecordBody,
)> {
    let binding = &retail.binding;
    shape::equal(m, &p["agreement_id"], &binding["agreement_id"])?;
    let slot = shape::text(m, p, "supplier_slot")?;
    let slots = shape::array(m, binding, "bindings")?;
    let selected = shape::named(m, slots, "slot", slot)?;
    let terms = load(
        m,
        r,
        c,
        &selected["binding_ref"],
        "workflow-slot-terms",
        "ledger-workflow-slot-binding-terms/8",
    )?;
    shape::closed(
        m,
        &terms,
        &[
            "schema",
            "scope",
            "agreement_id",
            "customer",
            "slot",
            "book",
            "role_slot",
            "role_ref",
            "funding_leg",
            "allowed_activities",
            "quantity_ceilings",
            "exposure_ceiling",
            "source_grant_refs",
            "correction_source_slot",
            "offer_ref",
            "cost_authority_ref",
            "provider_account_identity",
            "consumption_basis",
            "commitment_ref",
        ],
        &[],
    )?;
    literal(m, &terms, "book", "supplier")?;
    literal(m, &terms, "slot", slot)?;
    literal(
        m,
        &terms,
        "consumption_basis",
        "owned_obligation_high_water",
    )?;
    let scope = super::work_keys::scope(m, &c.key)?;
    shape::equal(m, &terms["scope"], scope.value())?;
    for k in ["agreement_id", "customer"] {
        shape::equal(m, &terms[k], &binding[k])?;
    }
    member(m, &terms, "allowed_activities", &p["activity"])?;
    let role = load(
        m,
        r,
        c,
        &terms["role_ref"],
        "workflow-roles",
        "ledger-workflow-role-assignment/1",
    )?;
    for k in ["scope", "agreement_id", "customer", "role_slot"] {
        shape::equal(m, &role[k], &terms[k])?;
    }
    member(m, binding, "roles_refs", &terms["role_ref"])?;
    let offer = load(
        m,
        r,
        c,
        &terms["offer_ref"],
        "workflow-offer",
        "ledger-workflow-offer/4",
    )?;
    shape::closed(
        m,
        &offer,
        &[
            "schema",
            "agreement_id",
            "version",
            "provider",
            "valuation_mode",
            "role_refs",
            "activity_names",
            "unit_ceilings",
            "exposure_ceiling",
            "allowed_component_slots",
            "source_slots",
            "starts_at",
            "ends_at",
            "evidence_refs",
            "commitment_ref",
        ],
        &[],
    )?;
    shape::equal(m, &offer["agreement_id"], &binding["agreement_id"])?;
    shape::equal(m, &offer["version"], &binding["version"])?;
    shape::equal(m, &offer["provider"], &role["provider"])?;
    shape::equal(m, &offer["commitment_ref"], &terms["commitment_ref"])?;
    member(m, &offer, "role_refs", &terms["role_ref"])?;
    member(m, &offer, "activity_names", &p["activity"])?;
    member(m, &offer, "source_slots", &p["source_slot"])?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    let before = time(m, p, "start_before")?;
    for v in [&**binding, &*terms, &*offer] {
        if v.get("starts_at").is_some() {
            let start = time(m, v, "starts_at")?;
            if w::compare(m, now, start)?.is_lt() {
                return Err(shape::fail("Invocation before original start"));
            }
        }
        if v.get("ends_at").is_some() {
            let end = time(m, v, "ends_at")?;
            if !w::compare(m, now, end)?.is_lt() || w::compare(m, before, end)?.is_gt() {
                return Err(shape::fail("Invocation outside original end"));
            }
        }
    }
    if !w::compare(m, now, before)?.is_lt() {
        return Err(shape::fail("Invocation must precede start deadline"));
    }
    let field = quantity_field(m, &terms, &offer, p)?;
    let mut selected_schema = None;
    for document in program.source_documents() {
        m.visit("invocation.original_activity_quantity")?;
        if !shape::same_text(m, document.reference().kind(), "workflow-product")? {
            continue;
        }
        let activity_name = shape::text(m, p, "activity")?;
        let activities = shape::array(m, document.canonical_value(), "activities")?;
        let activity = shape::named(m, activities, "name", activity_name)?;
        shape::equal(m, &activity["source_slot"], &p["source_slot"])?;
        literal(m, activity, "primitive", "work")?;
        for (reference, schema) in program.fact_schemas() {
            let owner = arena(m)?;
            let value = super::working_value::owning_definition(m, &owner, reference)?;
            if shape::same(m, value.value(), &activity["fact_schema"])? {
                selected_schema = Some(schema);
            }
        }
    }
    let schema = selected_schema.ok_or_else(|| shape::fail("Invocation original Work schema"))?;
    let unit = schema.quantity_unit_with_work(field, m)?;
    literal(m, p, "unit", unit)?;
    let raw = shape::text(m, p, "maximum_quantity")?;
    let quantity = arithmetic::decimal(m, raw)?;
    let mut ceiling = false;
    for row in shape::array(m, binding, "quantity_ceilings")? {
        m.visit("invocation.binding_quantity_ceiling")?;
        if shape::same(m, &row["activity"], &p["activity"])? && same_field(m, row, "field", field)?
        {
            shape::equal(m, &row["unit"], &p["unit"])?;
            ratio_bound(m, &quantity, &row["maximum"])?;
            ceiling = true;
        }
    }
    if !ceiling {
        return Err(shape::fail("Invocation accepted Binding quantity ceiling"));
    }
    for v in [&terms["exposure_ceiling"], &offer["exposure_ceiling"]] {
        amount_bound(m, &p["maximum_exposure"], v)?;
    }
    let funding_legs = shape::array(m, binding, "funding_legs")?;
    let funding_name = shape::text(m, &terms, "funding_leg")?;
    let funding = shape::named(m, funding_legs, "name", funding_name)?;
    literal(m, funding, "mode", "platform")?;
    shape::equal(m, &funding["supplier_binding_slot"], &terms["slot"])?;
    shape::equal(m, &funding["payer"], &role["payer"])?;
    shape::equal(m, &funding["resource_owner"], &role["bearer"])?;
    let owner = load(
        m,
        r,
        c,
        &terms["cost_authority_ref"],
        "workflow-authority",
        "ledger-cost-authority/1",
    )?;
    literal(m, &owner, "owner_logical_store", &c.key.logical_store_id)?;
    literal(m, &owner, "revision", "1")?;
    member(
        m,
        &owner,
        "provider_account_identities",
        &terms["provider_account_identity"],
    )?;
    let activation = load(
        m,
        r,
        c,
        &owner["owner_activation_ref"],
        "workflow-authority",
        "ledger-cost-owner-activation/1",
    )?;
    for k in ["authority_id", "owner_logical_store"] {
        shape::equal(m, &activation[k], &owner[k])?;
    }
    literal(m, &activation, "installation_mode", &c.installation_mode)?;
    shape::equal(m, &activation["activated_at"], &owner["starts_at"])?;
    let principal = arena(m)?.string_value(&c.actor, m)?;
    let root = inventory::evidence::root(
        m,
        r,
        c,
        &InvocationToken::new(),
        &activation["local_authority_ref"],
        &activation,
        (
            "activated_at",
            principal.value(),
            Some(&activation["operation_key"]["source"]),
        ),
    )?;
    let _ = root;
    member(
        m,
        &owner,
        "administrator_refs",
        &activation["local_authority_ref"],
    )?;
    let _ = program;
    Ok((terms, offer))
}
/// Pure request normalization shared by forward and original intent paths.
pub(super) fn normalize_authorization(m: &mut CoreMeter, p: &mut WorkingValue) -> Result<()> {
    shape::closed(
        m,
        p,
        &[
            "schema",
            "agreement_id",
            "binding_ref",
            "business_operation_id",
            "chain_id",
            "activity",
            "supplier_slot",
            "source_slot",
            "unit",
            "maximum_quantity",
            "maximum_exposure",
            "start_before",
            "evidence_refs",
        ],
        &[],
    )?;
    literal(m, p, "schema", "ledger-workflow-invocation-authorize/1")?;
    let a = arena(m)?;
    let scalar = super::super::facts::SignedScalar::parse_with_work(
        shape::text(m, p, "maximum_quantity")?,
        18,
        m,
    )?;
    let normalized = a.string_value(scalar.as_str(), m)?;
    p.replace(m, "maximum_quantity", normalized.value())?;
    p.normalize_set(m, "evidence_refs", false)?;
    Ok(())
}

pub(super) fn normalize_close(m: &mut CoreMeter, p: &mut WorkingValue) -> Result<()> {
    shape::closed(
        m,
        p,
        &[
            "schema",
            "invocation_ref",
            "expected_revision",
            "reason",
            "evidence_refs",
        ],
        &[],
    )?;
    literal(m, p, "schema", "ledger-workflow-invocation-close/1")?;
    p.normalize_set(m, "evidence_refs", false)
}

pub(super) fn authorize<'a>(
    program: &SelectedProgram,
    c: &'a DraftContextData,
    mut p: WorkingValue,
    inputs: &'a [EconomicRecordView<'a>],
    state: &EconomicStateData<'a>,
    m: &mut CoreMeter,
) -> Result<Construction<'a, 'a>> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("exact Invocation recipe"));
    }
    inventory::entry::context_selected(m, c, program)?;
    normalize_authorization(m, &mut p)?;
    let a = arena(m)?;
    let evidence = shape::array(m, &p, "evidence_refs")?;
    if w::compare(m, evidence.len(), 1usize)?.is_lt()
        || w::compare(m, evidence.len(), 16usize)?.is_gt()
    {
        return Err(shape::fail("Invocation evidence bound"));
    }
    let binding_ref = parse_owned_reference(m, &a, &p["binding_ref"])?;
    let mut records =
        Records::borrowed_selected(m, &a, &c.key, inputs, &c.source_observation_inputs, program)?;
    records.install_initial_source_outputs(m, state.initial_source_correspondences)?;
    let binding = records.get(
        m,
        &c.key,
        binding_ref.reference(),
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let purpose = a.object_from_fields(
        &[
            ("binding_ref", &p["binding_ref"]),
            ("customer", &binding["customer"]),
            ("product_ref", &binding["product_ref"]),
            ("policy_ref", &binding["policy_ref"]),
            ("activity", &p["activity"]),
            ("evidence_refs", &p["evidence_refs"]),
        ],
        m,
    )?;
    let namespace = shape::text(m, &binding, "source_namespace")?;
    // Standalone authorization is the actual initiating Work source; Stage
    // embedded inputs select their separately authenticated worker frame.
    let slot = shape::text(m, &p, "source_slot")?;
    let resolved = super::app_cost::binding_resolved(
        m,
        &mut records,
        c,
        purpose.value(),
        program,
        EconomicMode::InvocationAuthorize,
        (slot, &[], namespace),
    )?;
    let owner = a.reserve_external_payload(
        mem::size_of::<Construction>() + 3 * mem::size_of::<ConstructedEnvelope>() + 4096,
        m,
    )?;
    let (terms, authority_value, terms_owner) = resolved.into_local_parts()?;
    super::app_cost::check_current_consent(
        program,
        m,
        &mut records,
        c,
        &terms.binding,
        &terms.accepted,
        state,
    )?;
    super::supplier_consent::current(
        program,
        m,
        &mut records,
        c,
        &terms.binding,
        &terms.accepted,
        state.current_party_authorizations,
    )?;
    let authority =
        body::document_borrowed(m, "workflow-economic-authority", authority_value.value())?;
    let (supplier, _offer) = supplier(m, &mut records, c, program, &p, &terms)?;
    let instance = authorization_record(
        m,
        c,
        &p,
        &terms,
        &supplier,
        (&authority.reference, authority_value.value()),
        state,
    )?;
    let mut envelopes = Vec::new();
    envelopes
        .try_reserve_exact(2)
        .map_err(|_| shape::fail("Invocation result allocation"))?;
    envelopes.push(authority);
    envelopes.push(instance);
    construction(
        m,
        p,
        terms,
        records,
        envelopes,
        [
            Some(owner),
            terms_owner,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
    )
}
pub(super) fn embedded_authorize(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    input: &Value,
    state: &EconomicStateData,
) -> Result<([ConstructedEnvelope; 2], WorkingValue)> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("embedded Invocation recipe"));
    }
    let a = arena(m)?;
    let mut p = WorkingValue::copy(m, input)?;
    normalize_authorization(m, &mut p)?;
    let evidence = shape::array(m, &p, "evidence_refs")?;
    if evidence.is_empty() || w::compare(m, evidence.len(), 16usize)?.is_gt() {
        return Err(shape::fail("embedded Invocation evidence bound"));
    }
    let binding = load(
        m,
        r,
        c,
        &p["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let purpose = a.object_from_fields(
        &[
            ("binding_ref", &p["binding_ref"]),
            ("customer", &binding["customer"]),
            ("product_ref", &binding["product_ref"]),
            ("policy_ref", &binding["policy_ref"]),
            ("activity", &p["activity"]),
            ("evidence_refs", &p["evidence_refs"]),
        ],
        m,
    )?;
    let slot = shape::text(m, &p, "source_slot")?;
    let namespace = shape::text(m, &binding, "source_namespace")?;
    let resolved = super::app_cost::binding_resolved_dependent(
        m,
        r,
        c,
        purpose.value(),
        program,
        EconomicMode::InvocationAuthorize,
        (slot, &[], namespace),
    )?;
    let (terms, authority_value, _terms_owner) = resolved.into_local_parts()?;
    let authority =
        body::document_borrowed(m, "workflow-economic-authority", authority_value.value())?;
    let (supplier, _offer) = supplier(m, r, c, program, &p, &terms)?;
    let instance = authorization_record(
        m,
        c,
        &p,
        &terms,
        &supplier,
        (&authority.reference, authority_value.value()),
        state,
    )?;
    Ok(([authority, instance], p))
}
fn authorization_record(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    retail: &Terms,
    supplier: &Value,
    authority: (&ExactRef, &Value),
    state: &EconomicStateData,
) -> Result<ConstructedEnvelope> {
    let a = arena(m)?;
    let scope = super::work_keys::scope(m, &c.key)?;
    let tuple = a.array_from_values(
        &[
            scope.value(),
            &p["agreement_id"],
            &p["business_operation_id"],
        ],
        m,
    )?;
    let id = body::semantic_id(m, "winv_", "workflow-invocation", tuple.value())?;
    let heads = state
        .supplier
        .ok_or_else(|| shape::fail("held Invocation head selection required"))?;
    for h in heads.invocation_heads {
        m.visit("invocation.permanent_head")?;
        if shape::same_text(m, &h.invocation_id, &id)? {
            return Err(Error::new(
                "IDENTITY_CONFLICT",
                "Invocation identity already authorized",
            ));
        }
    }
    let constants=m.parse_owned_representation(&a,br#"{"schema":"ledger-workflow-invocation/1","one":"1","state":"authorized","zero":{"currency":"USD","scale":18,"atoms":"0"},"empty":[]}"#,RepresentationKind::SourceDefinition)?;
    let identifier = a.string_value(&id, m)?;
    let source = a.clone_value(&authority.1["source"], m)?;
    let namespace = a.clone_value(&retail.binding["source_namespace"], m)?;
    let at = a.string_value(&c.accepted_at, m)?;
    shape::equal(m, namespace.value(), &retail.binding["source_namespace"])?;
    let authority = owning_reference(m, &a, authority.0)?;
    let operation_key = super::cost_postings::operation_key(m, &a, c)?;
    let bindings = shape::array(m, &retail.binding, "bindings")?;
    let slot = shape::text(m, p, "supplier_slot")?;
    let selected = shape::named(m, bindings, "slot", slot)?;
    let fields = [
        ("schema", &constants.value()["schema"]),
        ("invocation_id", identifier.value()),
        ("revision", &constants.value()["one"]),
        ("original_binding_ref", &p["binding_ref"]),
        ("agreement_id", &p["agreement_id"]),
        ("customer", &retail.binding["customer"]),
        ("business_operation_id", &p["business_operation_id"]),
        ("chain_id", &p["chain_id"]),
        ("activity", &p["activity"]),
        ("source_namespace", namespace.value()),
        ("source_slot", &p["source_slot"]),
        ("source", source.value()),
        ("supplier_slot", &p["supplier_slot"]),
        ("supplier_terms_ref", &selected["binding_ref"]),
        ("offer_ref", &supplier["offer_ref"]),
        ("cost_authority_ref", &supplier["cost_authority_ref"]),
        (
            "provider_account_identity",
            &supplier["provider_account_identity"],
        ),
        ("role_ref", &supplier["role_ref"]),
        ("funding_leg", &supplier["funding_leg"]),
        ("unit", &p["unit"]),
        ("maximum_quantity", &p["maximum_quantity"]),
        ("authorized", &p["maximum_exposure"]),
        ("consumed", &constants.value()["zero"]),
        ("held", &p["maximum_exposure"]),
        ("released", &constants.value()["zero"]),
        ("state", &constants.value()["state"]),
        ("original_authority_ref", authority.value()),
        ("authorized_at", at.value()),
        ("start_before", &p["start_before"]),
        ("operation_key", operation_key.value()),
        ("cost_consumption_refs", &constants.value()["empty"]),
        ("other_consumed", &constants.value()["zero"]),
    ];
    let value = a.object_from_fields(&fields, m)?;
    body::record_borrowed(m, &c.key, "workflow-invocation", id, value.value())
}
fn construction<'a>(
    m: &mut CoreMeter,
    p: WorkingValue,
    terms: Terms,
    records: Records<'a, 'a>,
    envelopes: Vec<ConstructedEnvelope>,
    owners: [Option<ExternalBufferLease>; 9],
) -> Result<Construction<'a, 'a>> {
    Ok(Construction {
        economic: EconomicConstruction {
            terms,
            original_work: OriginalWorkPayload::Unattached,
        },
        original_reservations: None,
        financial_reservations: None,
        normalized_payload: p.bytes(m)?,
        new_envelopes: envelopes,
        reused_result: None,
        evaluation_steps: Vec::new(),
        proposed_heads: ProposedHeads {
            decision: Vec::new(),
            family: Vec::new(),
            chain: None,
        },
        claim_introductions: Vec::new(),
        link_introductions: Vec::new(),
        prospective_obligations: Vec::new(),
        records,
        history_edges: Vec::new(),
        app_effects: None,
        _app_causal: None,
        current_capacity: None,
        candidate_heads: None,
        candidate_introductions: None,
        financial_observation: None,
        funding_effects: None,
        control_owners: owners,
    })
}

pub(super) fn reference_for_work(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    payload: &Value,
    binding: &Value,
    original: (&ExactRef, &str, &str),
) -> Result<()> {
    let (reference, source, at) = original;
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("same Invocation exact Stage recipe"));
    }
    let a = arena(m)?;
    let original = r.get(
        m,
        key,
        reference,
        "workflow-invocation",
        "ledger-workflow-invocation/1",
    )?;
    closed(m, &original)?;
    literal(m, &original, "revision", "1")?;
    literal(m, &original, "state", "authorized")?;
    literal(m, &original, "invocation_id", &reference.id)?;
    literal(m, &original, "source", source)?;
    if original.get("prior_ref").is_some() {
        return Err(shape::fail(
            "same Invocation is not its original authorization",
        ));
    }
    let value = owning_reference(m, &a, reference)?;
    member(m, payload, "invocation_refs", value.value())?;
    for (field, expected) in [
        ("original_binding_ref", &payload["binding_ref"]),
        ("customer", &payload["customer"]),
        ("business_operation_id", &payload["business_operation_id"]),
        ("chain_id", &payload["chain"]),
        ("activity", &payload["activity"]),
        ("source_namespace", &binding["source_namespace"]),
        ("agreement_id", &binding["agreement_id"]),
    ] {
        shape::equal(m, &original[field], expected)?;
    }
    let auth_ref = parse_owned_reference(m, &a, &original["original_authority_ref"])?;
    let auth = r.get(
        m,
        key,
        auth_ref.reference(),
        "workflow-economic-authority",
        "ledger-workflow-stage-authority/1",
    )?;
    literal(m, &auth, "operation", "invocation.authorize")?;
    literal(m, &auth, "source", source)?;
    let scope = super::work_keys::scope(m, key)?;
    shape::equal(m, &auth["scope"], scope.value())?;
    for (field, expected) in [
        ("original_binding_ref", &payload["binding_ref"]),
        ("source_slot", &original["source_slot"]),
        ("observed_at", &original["authorized_at"]),
        ("customer", &payload["customer"]),
        ("canonical_economic_namespace", &binding["source_namespace"]),
    ] {
        shape::equal(m, &auth[field], expected)?;
    }
    let enrollment_ref = parse_owned_reference(m, &a, &auth["enrollment_ref"])?;
    let enrollment = r.get(
        m,
        key,
        enrollment_ref.reference(),
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    let grant_ref = parse_owned_reference(m, &a, &auth["current_grant_ref"])?;
    let grant = r.get(
        m,
        key,
        grant_ref.reference(),
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let enrollment_value = owning_reference(m, &a, enrollment_ref.reference())?;
    shape::equal(m, &grant["original_ceiling_ref"], enrollment_value.value())?;
    shape::equal(m, &grant["grant_id"], &enrollment["enrollment_id"])?;
    for field in ["source", "principal_id", "canonical_economic_namespace"] {
        shape::equal(m, &grant[field], &enrollment["ceiling"][field])?;
    }
    shape::equal(m, &grant["source"], &auth["source"])?;
    shape::equal(m, &grant["principal_id"], &auth["actor"])?;
    let authorized = time(m, &original, "authorized_at")?;
    let accepted = Timestamp::parse_with_work(at, m)?.micros();
    if w::compare(m, authorized, accepted)?.is_gt() {
        return Err(shape::fail("same Invocation authorized after Work"));
    }
    let activity_name = shape::text(m, payload, "activity")?;
    let source_slot = shape::text(m, &original, "source_slot")?;
    let mut product_id = None;
    for document in program.source_documents() {
        if !shape::same_text(m, document.reference().kind(), "workflow-product")? {
            continue;
        }
        let activities = shape::array(m, document.canonical_value(), "activities")?;
        let activity = shape::named(m, activities, "name", activity_name)?;
        shape::equal(m, &activity["source_slot"], &original["source_slot"])?;
        shape::equal(m, &activity["fact_schema"], &payload["fact_schema_ref"])?;
        product_id = Some(shape::text(m, document.canonical_value(), "product_id")?);
    }
    let product_id = product_id.ok_or_else(|| shape::fail("same Invocation original Product"))?;
    let token = InvocationToken::new();
    super::authority::grant_shape(m, &grant)?;
    super::authority::grant_intersection(
        m,
        &token,
        &grant,
        &enrollment["ceiling"],
        &authorized.into(),
    )?;
    for original in shape::array(m, &auth, "original_grant_refs")? {
        member(m, binding, "authority_refs", original)?;
        let reference = parse_owned_reference(m, &a, original)?;
        let grant = r.get(
            m,
            key,
            reference.reference(),
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        super::authority::grant_coverage(
            m,
            &grant,
            &binding["customer"],
            activity_name,
            product_id,
            source_slot,
            ("invocation.authorize", &["invocation", "read"]),
        )?;
    }
    super::authority::grant_coverage(
        m,
        &grant,
        &binding["customer"],
        activity_name,
        product_id,
        source_slot,
        ("invocation.authorize", &["invocation", "read"]),
    )?;
    Ok(())
}
pub(super) struct WorkInvocations {
    pub records: Vec<ConstructedEnvelope>,
    pub ownership: ExternalBufferLease,
}
/// A final Work consumes its one start eligibility, not the promised financial
/// hold. Later close/Stage/correction operate on this immutable authorization.
pub(super) fn consume_work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    binding: &Value,
    state: Option<&EconomicStateData>,
) -> Result<Option<WorkInvocations>> {
    let refs = shape::array(m, p, "invocation_refs")?;
    if refs.is_empty() {
        return Ok(None);
    }
    if !program.uses_stage_lifecycle_recipe() || w::compare(m, refs.len(), 16usize)?.is_gt() {
        return Err(shape::fail("Work Invocation recipe/count"));
    }
    let supplier = state
        .and_then(|v| v.supplier)
        .ok_or_else(|| shape::fail("actual held Work Invocation heads"))?;
    let a = arena(m)?;
    let per = mem::size_of::<ConstructedEnvelope>() + 64 + 128 + 71;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&refs.len().into(), &per.into()],
    )?;
    let size = refs
        .len()
        .checked_mul(per)
        .ok_or_else(|| shape::fail("Work Invocation storage"))?;
    let size = w::usize_add(m, size, mem::size_of::<WorkInvocations>())?;
    let ownership = a.reserve_external_payload(size, m)?;
    let mut outputs = Vec::new();
    outputs
        .try_reserve_exact(refs.len())
        .map_err(|_| shape::fail("Work Invocation allocation"))?;
    let work_at = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    for reference in refs {
        m.visit("Work.actual_original_Invocation")?;
        let reference = parse_owned_reference(m, &a, reference)?;
        let original = r.get(
            m,
            &c.key,
            reference.reference(),
            "workflow-invocation",
            "ledger-workflow-invocation/1",
        )?;
        closed(m, &original)?;
        literal(m, &original, "revision", "1")?;
        literal(m, &original, "state", "authorized")?;
        literal(m, &original, "invocation_id", &reference.reference().id)?;
        if original.get("prior_ref").is_some() {
            return Err(shape::fail("Work Invocation original predecessor"));
        }
        for (field, value) in [
            ("original_binding_ref", &p["binding_ref"]),
            ("customer", &p["customer"]),
            ("business_operation_id", &p["business_operation_id"]),
            ("activity", &p["activity"]),
            ("chain_id", &p["chain"]),
            ("agreement_id", &binding["agreement_id"]),
        ] {
            shape::equal(m, &original[field], value)?;
        }
        literal(m, &original, "source", &c.key.source)?;
        literal(
            m,
            &original,
            "source_namespace",
            &c.canonical_economic_namespace,
        )?;
        let at = time(m, &original, "authorized_at")?;
        let end = time(m, &original, "start_before")?;
        if w::compare(m, work_at, at)?.is_lt() || !w::compare(m, work_at, end)?.is_lt() {
            return Err(shape::fail(
                "Work outside original Invocation start interval",
            ));
        }
        super::authority::historical_namespace(
            m,
            r,
            c,
            &original["original_authority_ref"],
            &original["source_namespace"],
            Some(&original["source"]),
            (
                "invocation.authorize",
                &original["original_binding_ref"],
                &original["authorized_at"],
            ),
        )?;
        let mut head = None;
        for candidate in supplier.invocation_heads {
            if shape::same_text(m, &candidate.invocation_id, &reference.reference().id)?
                && head.replace(candidate).is_some()
            {
                return Err(shape::fail("duplicate held Work Invocation"));
            }
        }
        let head = head.ok_or_else(|| shape::fail("missing held Work Invocation"))?;
        let current = r.get(
            m,
            &c.key,
            &head.reference,
            "workflow-invocation",
            "ledger-workflow-invocation/1",
        )?;
        closed(m, &current)?;
        immutable(m, &original, &current)?;
        literal(m, &current, "revision", &head.revision)?;
        literal(m, &current, "state", "authorized")?;
        // Authorization is once-only. An existing terminal/completed head can
        // retain money, but cannot start another Work under the same identity.
        let supplied = super::super::facts::counter_with_work(&head.revision, m)?;
        if !w::compare(m, supplied, 1u64)?.is_eq()
            || !shape::same_ref(m, &head.reference, reference.reference())?
        {
            return Err(shape::fail("Work Invocation start already consumed"));
        }
        let terms = load(
            m,
            r,
            c,
            &original["supplier_terms_ref"],
            "workflow-slot-terms",
            "ledger-workflow-slot-binding-terms/8",
        )?;
        let offer = load(
            m,
            r,
            c,
            &original["offer_ref"],
            "workflow-offer",
            "ledger-workflow-offer/4",
        )?;
        let field = quantity_field(m, &terms, &offer, &original)?;
        let raw_quantity = shape::text(m, &p["facts"], field)?;
        let quantity = arithmetic::decimal(m, raw_quantity)?;
        ratio_bound(m, &quantity, &original["maximum_quantity"])?;
        let revision = a.string_value("2", m)?;
        let completed = a.string_value("completed", m)?;
        let prior = owning_reference(m, &a, &head.reference)?;
        let key = super::cost_postings::operation_key(m, &a, c)?;
        let value = WorkingValue::copy(m, &current)?.with_fields(
            m,
            &[
                ("revision", revision.value()),
                ("state", completed.value()),
                ("prior_ref", prior.value()),
                ("operation_key", key.value()),
            ],
        )?;
        closed(m, &value)?;
        let id = text(m, &head.invocation_id)?;
        outputs.push(body::record_borrowed(
            m,
            &c.key,
            "workflow-invocation",
            id,
            &value,
        )?);
    }
    Ok(Some(WorkInvocations {
        records: outputs,
        ownership,
    }))
}
fn text(m: &mut CoreMeter, value: &str) -> Result<String> {
    m.scan_bytes(value.len())?;
    m.write_bytes(value.len())?;
    Ok(value.to_owned())
}
fn closed(m: &mut CoreMeter, v: &Value) -> Result<()> {
    shape::closed(
        m,
        v,
        &[
            "schema",
            "invocation_id",
            "revision",
            "original_binding_ref",
            "agreement_id",
            "customer",
            "business_operation_id",
            "chain_id",
            "activity",
            "source_namespace",
            "source_slot",
            "source",
            "supplier_slot",
            "supplier_terms_ref",
            "offer_ref",
            "cost_authority_ref",
            "provider_account_identity",
            "role_ref",
            "funding_leg",
            "unit",
            "maximum_quantity",
            "authorized",
            "consumed",
            "held",
            "released",
            "state",
            "original_authority_ref",
            "authorized_at",
            "start_before",
            "operation_key",
            "cost_consumption_refs",
            "other_consumed",
        ],
        &["prior_ref"],
    )?;
    literal(m, v, "schema", "ledger-workflow-invocation/1")?;
    let authorized = amount(m, &v["authorized"])?;
    let consumed = amount(m, &v["consumed"])?;
    let held = amount(m, &v["held"])?;
    let released = amount(m, &v["released"])?;
    let used = meter::add(m, &consumed, &held)?;
    let total = meter::add(m, &used, &released)?;
    if !meter::cmp(m, &total, &authorized)?.is_eq() {
        return Err(shape::fail("Invocation authorized=consumed+held+released"));
    }
    Ok(())
}
fn immutable(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    for k in [
        "invocation_id",
        "original_binding_ref",
        "agreement_id",
        "customer",
        "business_operation_id",
        "chain_id",
        "activity",
        "source_namespace",
        "source_slot",
        "source",
        "supplier_slot",
        "supplier_terms_ref",
        "offer_ref",
        "cost_authority_ref",
        "provider_account_identity",
        "role_ref",
        "funding_leg",
        "unit",
        "maximum_quantity",
        "authorized",
        "original_authority_ref",
        "authorized_at",
        "start_before",
    ] {
        shape::equal(m, &a[k], &b[k])?;
    }
    Ok(())
}
pub(super) fn close<'a>(
    program: &SelectedProgram,
    c: &'a DraftContextData,
    mut p: WorkingValue,
    inputs: &'a [EconomicRecordView<'a>],
    state: &EconomicStateData<'a>,
    m: &mut CoreMeter,
) -> Result<Construction<'a, 'a>> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("exact Invocation close recipe"));
    }
    inventory::entry::context_selected(m, c, program)?;
    normalize_close(m, &mut p)?;
    let a = arena(m)?;
    let selected = parse_owned_reference(m, &a, &p["invocation_ref"])?;
    let mut r =
        Records::borrowed_selected(m, &a, &c.key, inputs, &c.source_observation_inputs, program)?;
    r.install_initial_source_outputs(m, state.initial_source_correspondences)?;
    let named = r.get(
        m,
        &c.key,
        selected.reference(),
        "workflow-invocation",
        "ledger-workflow-invocation/1",
    )?;
    closed(m, &named)?;
    let id = shape::text(m, &named, "invocation_id")?;
    if !shape::same_text(m, id, &selected.reference().id)? {
        return Err(shape::fail("Invocation close identity"));
    }
    let (original_ref, original) = r.original_invocation(m, &c.key, id)?;
    closed(m, &original)?;
    literal(m, &original, "revision", "1")?;
    literal(m, &original, "state", "authorized")?;
    if original.get("prior_ref").is_some() {
        return Err(shape::fail("initial Invocation predecessor"));
    }
    immutable(m, &original, &named)?;
    let supplier = state
        .supplier
        .ok_or_else(|| shape::fail("held Invocation close heads"))?;
    let mut head = None;
    for h in supplier.invocation_heads {
        m.visit("invocation.close_current_head")?;
        if shape::same_text(m, &h.invocation_id, id)? && head.replace(h).is_some() {
            return Err(shape::fail("duplicate Invocation head"));
        }
    }
    let head = head.ok_or_else(|| shape::fail("missing current Invocation head"))?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-invocation",
        "ledger-workflow-invocation/1",
    )?;
    closed(m, &current)?;
    immutable(m, &original, &current)?;
    literal(m, &current, "revision", &head.revision)?;
    let expected = shape::text(m, &p, "expected_revision")?;
    let expected = super::super::facts::counter_with_work(expected, m)?;
    let actual = super::super::facts::counter_with_work(&head.revision, m)?;
    if !w::compare(m, expected, actual)?.is_eq() {
        return Err(Error::new("STALE_REVISION", "Invocation expected revision"));
    }
    let current_state = shape::text(m, &current, "state")?;
    if !shape::same_text(m, current_state, "authorized")?
        && !shape::same_text(m, current_state, "completed")?
    {
        return Err(Error::new("TARGET_CLOSED", "Invocation is terminal"));
    }
    super::authority::historical_namespace(
        m,
        &mut r,
        c,
        &original["original_authority_ref"],
        &original["source_namespace"],
        Some(&original["source"]),
        (
            "invocation.authorize",
            &original["original_binding_ref"],
            &original["authorized_at"],
        ),
    )?;
    let binding = load(
        m,
        &mut r,
        c,
        &original["original_binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let purpose = a.object_from_fields(
        &[
            ("binding_ref", &original["original_binding_ref"]),
            ("customer", &original["customer"]),
            ("product_ref", &binding["product_ref"]),
            ("policy_ref", &binding["policy_ref"]),
            ("activity", &original["activity"]),
            ("evidence_refs", &p["evidence_refs"]),
        ],
        m,
    )?;
    let slot = shape::text(m, &original, "source_slot")?;
    let namespace = shape::text(m, &original, "source_namespace")?;
    let resolved = super::app_cost::binding_resolved(
        m,
        &mut r,
        c,
        purpose.value(),
        program,
        EconomicMode::InvocationClose,
        (slot, &[], namespace),
    )?;
    let owner = a.reserve_external_payload(
        mem::size_of::<Construction>() + 3 * mem::size_of::<ConstructedEnvelope>() + 8192,
        m,
    )?;
    let (terms, authority_value, terms_owner) = resolved.into_local_parts()?;
    // Completion fulfils an original promise; provider revocation is not a
    // right to erase its retained held exposure or preaccepted liabilities.
    super::supplier_consent::original(program, m, &mut r, c, &terms.binding, &terms.accepted)?;
    let authority =
        body::document_borrowed(m, "workflow-economic-authority", authority_value.value())?;
    let mut consumed = amount(m, &current["other_consumed"])?;
    for v in shape::array(m, &current, "cost_consumption_refs")? {
        m.visit("invocation.close_complete_consumption")?;
        let reference = parse_owned_reference(m, &a, v)?;
        let consumption = r.get(
            m,
            &c.key,
            reference.reference(),
            "workflow-cost",
            "ledger-cost-obligation-consumption/1",
        )?;
        let original_value = owning_reference(m, &a, original_ref.reference())?;
        shape::equal(
            m,
            &consumption["original_invocation_ref"],
            original_value.value(),
        )?;
        let claim = parse_owned_reference(m, &a, &consumption["cost_claim_ref"])?;
        let mut hit = None;
        for h in supplier.cost_consumption_heads {
            m.visit("invocation.close_consumption_head")?;
            if shape::same_ref(m, &h.cost_claim_ref, claim.reference())? && hit.replace(h).is_some()
            {
                return Err(shape::fail("duplicate cost consumption head"));
            }
        }
        let hit = hit.ok_or_else(|| shape::fail("missing actual cost consumption head"))?;
        if !shape::same_ref(m, &hit.reference, reference.reference())? {
            return Err(shape::fail("stale cost consumption"));
        }
        literal(m, &consumption, "revision", &hit.revision)?;
        let high = amount(m, &consumption["high_water"])?;
        consumed = meter::add(m, &consumed, &high)?;
    }
    let expected_consumed = amount(m, &current["consumed"])?;
    if !meter::cmp(m, &consumed, &expected_consumed)?.is_eq() {
        return Err(shape::fail("Invocation complete consumed sum"));
    }
    let business = a.object_from_fields(
        &[("business_operation_id", &original["business_operation_id"])],
        m,
    )?;
    let claim_id = super::work_keys::claim(m, &c.key, namespace, business.value())?;
    let mut claim = None;
    for reference in &state.financial.work_claims {
        m.visit("invocation.close_permanent_work")?;
        if shape::same_text(m, &reference.id, &claim_id)? && claim.replace(reference).is_some() {
            return Err(shape::fail("duplicate completion claim"));
        }
    }
    let reason = shape::text(m, &p, "reason")?;
    let status = if shape::same_text(m, reason, "unused_expired")? {
        if claim.is_some()
            || !shape::array(m, &current, "cost_consumption_refs")?.is_empty()
            || !meter::cmp(m, &consumed, &0.into())?.is_eq()
        {
            return Err(shape::fail("used Invocation cannot close unused"));
        }
        let now = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
        let before = time(m, &original, "start_before")?;
        if w::compare(m, now, before)?.is_lt() {
            return Err(shape::fail("unused Invocation not expired"));
        }
        "closed_unused"
    } else {
        if !shape::same_text(m, reason, "completed")? {
            return Err(shape::fail("Invocation close reason"));
        }
        let claim =
            claim.ok_or_else(|| shape::fail("completed Invocation needs actual final Work"))?;
        let claim = r.get(
            m,
            &c.key,
            claim,
            "workflow-work-claim",
            "ledger-workflow-work-claim/1",
        )?;
        let work = load(
            m,
            &mut r,
            c,
            &claim["work_ref"],
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        let original_value = owning_reference(m, &a, original_ref.reference())?;
        member(
            m,
            &work["payload"],
            "invocation_refs",
            original_value.value(),
        )?;
        for (k, v) in [
            ("customer", "customer"),
            ("chain_id", "chain"),
            ("activity", "activity"),
            ("business_operation_id", "business_operation_id"),
            ("original_binding_ref", "binding_ref"),
        ] {
            shape::equal(m, &original[k], &work["payload"][v])?;
        }
        shape::equal(m, &original["source_namespace"], &work["source_namespace"])?;
        shape::equal(m, &original["source"], &work["key"]["source"])?;
        "closed_completed"
    };
    let old_held = amount(m, &current["held"])?;
    let old_released = amount(m, &current["released"])?;
    let release = if status == "closed_unused" {
        old_held.clone()
    } else {
        0.into()
    };
    let held = meter::sub(m, &old_held, &release)?;
    let released = meter::add(m, &old_released, &release)?;
    let held = super::cost_postings::amount(m, &a, &held)?;
    let released = super::cost_postings::amount(m, &a, &released)?;
    let revision = w::usize_add(
        m,
        usize::try_from(actual).map_err(|_| shape::fail("Invocation revision"))?,
        1,
    )?;
    let revision = super::close::counter_value(m, &a, revision as u64)?;
    let prior = owning_reference(m, &a, &head.reference)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let status = a.string_value(status, m)?;
    let next = WorkingValue::copy(m, &current)?.with_fields(
        m,
        &[
            ("revision", revision.value()),
            ("prior_ref", prior.value()),
            ("state", status.value()),
            ("operation_key", key.value()),
            ("held", held.value()),
            ("released", released.value()),
        ],
    )?;
    closed(m, &next)?;
    let id = shape::text(m, &current, "invocation_id")?;
    m.scan_bytes(id.len())?;
    m.write_bytes(id.len())?;
    let next = body::record_borrowed(m, &c.key, "workflow-invocation", id.to_owned(), &next)?;
    let mut envelopes = Vec::new();
    envelopes
        .try_reserve_exact(2)
        .map_err(|_| shape::fail("Invocation close output"))?;
    envelopes.push(authority);
    envelopes.push(next);
    let raw = super::close::counter_value(m, &a, expected)?;
    p.replace(m, "expected_revision", raw.value())?;
    construction(
        m,
        p,
        terms,
        r,
        envelopes,
        [
            Some(owner),
            terms_owner,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
    )
}
/// Owning lookup DATA for the actual invocation purpose. These fields select
/// held records only; the child repeats all source, supplier and origin joins.
pub struct InvocationOperationLocators {
    id: String,
    binding: super::working_value::OwnedExactRef,
    named: Option<super::working_value::OwnedExactRef>,
    source_slot: String,
    activity: String,
    chain: String,
    agreement: String,
    customer: String,
    namespace: String,
    business: String,
    claim: Option<String>,
    _ownership: ExternalBufferLease,
}
impl InvocationOperationLocators {
    pub fn invocation_id(&self) -> &str {
        &self.id
    }
    pub fn original_binding_ref(&self) -> &ExactRef {
        self.binding.reference()
    }
    pub fn named_invocation_ref(&self) -> Option<&ExactRef> {
        self.named.as_ref().map(|v| v.reference())
    }
    pub fn source_slot(&self) -> &str {
        &self.source_slot
    }
    pub fn activity(&self) -> &str {
        &self.activity
    }
    pub fn chain(&self) -> &str {
        &self.chain
    }
    pub fn agreement(&self) -> &str {
        &self.agreement
    }
    pub fn customer(&self) -> &str {
        &self.customer
    }
    pub fn source_namespace(&self) -> &str {
        &self.namespace
    }
    pub fn business_operation_id(&self) -> &str {
        &self.business
    }
    pub fn work_claim_id(&self) -> Option<&str> {
        self.claim.as_deref()
    }
}
pub fn select_invocation_operation_locators(
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut crate::validation_work::OriginalAuditSession,
) -> Result<Option<InvocationOperationLocators>> {
    let result = (|| {
        program.require_attempt_owner(s.meter_mut())?;
        if !program.uses_stage_lifecycle_recipe() {
            return Ok(None);
        }
        let a = s.arena().shared_handle();
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        let schema = shape::text(m, p.value(), "schema")?;
        let authorize = shape::same_text(m, schema, "ledger-workflow-invocation-authorize/1")?;
        if !authorize && !shape::same_text(m, schema, "ledger-workflow-invocation-close/1")? {
            return Ok(None);
        }
        let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        let named = if authorize {
            None
        } else {
            Some(parse_owned_reference(m, &a, &p.value()["invocation_ref"])?)
        };
        let invocation = if let Some(reference) = &named {
            Some(r.get(
                m,
                key,
                reference.reference(),
                "workflow-invocation",
                "ledger-workflow-invocation/1",
            )?)
        } else {
            None
        };
        let subject = invocation.as_deref().unwrap_or_else(|| p.value());
        let binding_value = if authorize {
            &subject["binding_ref"]
        } else {
            &subject["original_binding_ref"]
        };
        let binding = parse_owned_reference(m, &a, binding_value)?;
        let body = r.get(
            m,
            key,
            binding.reference(),
            "workflow-binding",
            "ledger-workflow-binding/2",
        )?;
        let scope = super::work_keys::scope(m, key)?;
        let tuple = a.array_from_values(
            &[
                scope.value(),
                &subject["agreement_id"],
                &subject["business_operation_id"],
            ],
            m,
        )?;
        let mut n = mem::size_of::<InvocationOperationLocators>() + 4096;
        for (v, k) in [
            (subject, "source_slot"),
            (subject, "activity"),
            (subject, "chain_id"),
            (subject, "agreement_id"),
            (&*body, "customer"),
            (&*body, "source_namespace"),
            (subject, "business_operation_id"),
        ] {
            inventory::binding::id(m, &v[k], if k == "source_namespace" { 256 } else { 128 })?;
            let text = shape::text(m, v, k)?;
            n = w::usize_add(m, n, text.len())?;
        }
        let owner = a.reserve_external_payload(n, m)?;
        let id = body::semantic_id(m, "winv_", "workflow-invocation", tuple.value())?;
        if let Some(v) = &invocation {
            literal(m, v, "invocation_id", &id)?;
            shape::equal(m, &v["source_namespace"], &body["source_namespace"])?;
        }
        let copy = |m: &mut CoreMeter, v: &Value, k: &str| -> Result<String> {
            let text = shape::text(m, v, k)?;
            m.scan_bytes(text.len())?;
            m.write_bytes(text.len())?;
            Ok(text.to_owned())
        };
        let namespace = copy(m, &body, "source_namespace")?;
        let claim = if authorize {
            None
        } else {
            Some(super::work_keys::claim(m, key, &namespace, subject)?)
        };
        Ok(Some(InvocationOperationLocators {
            id,
            binding,
            named,
            source_slot: copy(m, subject, "source_slot")?,
            activity: copy(m, subject, "activity")?,
            chain: copy(m, subject, "chain_id")?,
            agreement: copy(m, subject, "agreement_id")?,
            customer: copy(m, &body, "customer")?,
            namespace,
            business: copy(m, subject, "business_operation_id")?,
            claim,
            _ownership: owner,
        }))
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

/// Fulfil the already-authorized Stage share from its actual member's original
/// Invocation. A completed close preserves this hold; it grants no new Work,
/// release, replenishment, supplier choice or authorization.
pub(super) fn consume_stage_share(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (&Value, &Value, &str, &num_bigint::BigInt),
    state: &EconomicStateData<'_>,
) -> Result<Option<ConstructedEnvelope>> {
    let (work, binding, supplier_slot, atoms) = selected;
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("Stage held consumption recipe"));
    }
    if meter::cmp(m, atoms, &0.into())?.is_lt() {
        return Err(shape::fail("negative Stage supplier consumption"));
    }
    let a = arena(m)?;
    let payload = shape::field(m, work, "payload")?;
    let mut selected = None;
    for value in shape::array(m, payload, "invocation_refs")? {
        let reference = parse_owned_reference(m, &a, value)?;
        let original = r.get(
            m,
            &c.key,
            reference.reference(),
            "workflow-invocation",
            "ledger-workflow-invocation/1",
        )?;
        closed(m, &original)?;
        literal(m, &original, "revision", "1")?;
        literal(m, &original, "state", "authorized")?;
        if original.get("prior_ref").is_some() {
            return Err(shape::fail("Stage original Invocation predecessor"));
        }
        let slot = shape::text(m, &original, "supplier_slot")?;
        if !shape::same_text(m, slot, supplier_slot)? {
            continue;
        }
        if selected.replace((reference, original)).is_some() {
            return Err(shape::fail("ambiguous original Stage supplier Invocation"));
        }
    }
    let (reference, original) =
        selected.ok_or_else(|| shape::fail("Stage share has no original member Invocation"))?;
    for (inv, member) in [
        ("original_binding_ref", "binding_ref"),
        ("customer", "customer"),
        ("chain_id", "chain"),
        ("business_operation_id", "business_operation_id"),
        ("activity", "activity"),
    ] {
        shape::equal(m, &original[inv], &payload[member])?;
    }
    shape::equal(m, &original["source"], &work["key"]["source"])?;
    shape::equal(m, &original["source_namespace"], &work["source_namespace"])?;
    shape::equal(
        m,
        &original["source_namespace"],
        &binding["source_namespace"],
    )?;
    super::authority::historical_namespace(
        m,
        r,
        c,
        &original["original_authority_ref"],
        &original["source_namespace"],
        Some(&original["source"]),
        (
            "invocation.authorize",
            &original["original_binding_ref"],
            &original["authorized_at"],
        ),
    )?;
    let terms = load(
        m,
        r,
        c,
        &original["supplier_terms_ref"],
        "workflow-slot-terms",
        "ledger-workflow-slot-binding-terms/8",
    )?;
    let offer = load(
        m,
        r,
        c,
        &original["offer_ref"],
        "workflow-offer",
        "ledger-workflow-offer/4",
    )?;
    let rows = shape::array(m, binding, "bindings")?;
    let row = shape::named(m, rows, "slot", supplier_slot)?;
    shape::equal(m, &row["binding_ref"], &original["supplier_terms_ref"])?;
    shape::equal(m, &terms["offer_ref"], &original["offer_ref"])?;
    for field in [
        "cost_authority_ref",
        "provider_account_identity",
        "role_ref",
        "funding_leg",
    ] {
        shape::equal(m, &original[field], &terms[field])?;
    }
    let field = quantity_field(m, &terms, &offer, &original)?;
    let raw = shape::text(m, &payload["facts"], field)?;
    let quantity = arithmetic::decimal(m, raw)?;
    ratio_bound(m, &quantity, &original["maximum_quantity"])?;
    let supplier = state
        .supplier
        .ok_or_else(|| shape::fail("actual Stage Invocation head absent"))?;
    let mut selected_head = None;
    for head in supplier.invocation_heads {
        if shape::same_text(m, &head.invocation_id, &reference.reference().id)?
            && selected_head.replace(head).is_some()
        {
            return Err(shape::fail("duplicate Stage Invocation current head"));
        }
    }
    let head = selected_head.ok_or_else(|| shape::fail("Stage Invocation held head absent"))?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-invocation",
        "ledger-workflow-invocation/1",
    )?;
    closed(m, &current)?;
    immutable(m, &original, &current)?;
    literal(m, &current, "revision", &head.revision)?;
    let status = shape::text(m, &current, "state")?;
    if !shape::same_text(m, status, "completed")?
        && !shape::same_text(m, status, "closed_completed")?
    {
        return Err(shape::fail(
            "Stage fulfilment needs actual completed member Invocation",
        ));
    }
    let held = amount(m, &current["held"])?;
    if meter::cmp(m, atoms, &held)?.is_gt() {
        return Err(Error::new(
            "EXPOSURE_EXCEEDED",
            "Stage supplier hold is insufficient",
        ));
    }
    if meter::cmp(m, atoms, &0.into())?.is_eq() {
        return Ok(None);
    }
    let held = meter::sub(m, &held, atoms)?;
    let consumed = amount(m, &current["consumed"])?;
    let consumed = meter::add(m, &consumed, atoms)?;
    let other = amount(m, &current["other_consumed"])?;
    let other = meter::add(m, &other, atoms)?;
    let revision = arithmetic::integer(m, &head.revision, false)?;
    let revision = meter::add(m, &revision, &1.into())?;
    let revision = super::cost_postings::scalar(m, &a, &revision)?;
    if w::compare(
        m,
        revision
            .value()
            .as_str()
            .ok_or_else(|| shape::fail("Stage Invocation revision"))?
            .len(),
        19usize,
    )?
    .is_gt()
    {
        return Err(shape::fail("Stage Invocation revision Counter bound"));
    }
    let held = super::cost_postings::amount(m, &a, &held)?;
    let consumed = super::cost_postings::amount(m, &a, &consumed)?;
    let other = super::cost_postings::amount(m, &a, &other)?;
    let prior = owning_reference(m, &a, &head.reference)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let value = WorkingValue::copy(m, &current)?.with_fields(
        m,
        &[
            ("revision", revision.value()),
            ("prior_ref", prior.value()),
            ("held", held.value()),
            ("consumed", consumed.value()),
            ("other_consumed", other.value()),
            ("operation_key", key.value()),
        ],
    )?;
    closed(m, &value)?;
    immutable(m, &original, &value)?;
    let id = text(m, &head.invocation_id)?;
    Ok(Some(body::record_borrowed(
        m,
        &c.key,
        "workflow-invocation",
        id,
        &value,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn accepted_usage_quantity_is_unique_without_a_unit_price_formula() {
        let a = AuditArena::new().unwrap();
        let mut s =
            crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let m = s.meter_mut();
        let terms = json!({"quantity_ceilings":[{"activity":"test:prepare","field":"test:quantity","unit":"test:item","maximum":"3"}]});
        let mut offer = json!({"unit_ceilings":[{"activity":"test:prepare","field":"test:quantity","unit":"test:item","maximum":"2"}]});
        let input = json!({"activity":"test:prepare","unit":"test:item","maximum_quantity":"1"});
        assert_eq!(
            quantity_field(m, &terms, &offer, &input).unwrap(),
            "test:quantity"
        );
        offer["unit_ceilings"][0]["field"] = json!("test:other");
        assert!(quantity_field(m, &terms, &offer, &input).is_err());
        let empty = json!({"quantity_ceilings":[]});
        assert!(quantity_field(m, &empty, &offer, &input).is_err());
    }
}
