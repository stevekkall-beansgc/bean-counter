//! Original closing-operation association for a Stage-owned immutable Link.
//! Current permissions and mutable heads are deliberately separate inputs.
use super::super as tc;
use super::*;
use serde_json::Value;

pub(in crate::policy::workflow::transition_constructor) fn link(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (&ExactRef, &Value, &ExactRef, &Value),
) -> Result<()> {
    let (reference, link, instance_ref, current) = selected;
    let a = arena(m)?;
    let instance = r.get(
        m,
        &c.key,
        instance_ref,
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    closed_instance(m, &c.key, instance_ref, &instance)?;
    let instance_value = owning_reference(m, &a, instance_ref)?;
    let identity = closure_claim_id(m, &c.key, &instance)?;
    let (claim_ref, claim) = r.by_identity(
        m,
        &c.key,
        "workflow-stage-closure-claim",
        &identity,
        "ledger-workflow-stage-closure-claim/1",
    )?;
    shape::closed(
        m,
        &claim,
        &[
            "schema",
            "stage_instance_ref",
            "operation_key",
            "original_payload",
            "first_work_ref",
            "anchor_work_ref",
            "anchor_link_refs",
            "window",
            "authority_ref",
            "accepted_at",
        ],
        &[],
    )?;
    shape::equal(m, &claim["stage_instance_ref"], instance_value.value())?;
    let payload = shape::field(m, &claim, "original_payload")?;
    same(m, payload, "schema", "ledger-workflow-stage-close/1")?;
    for field in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &payload[field], &instance[field])?;
    }
    shape::equal(m, &payload["stage_instance_ref"], instance_value.value())?;
    shape::equal(m, &payload["stage"], &instance["stage_name"])?;
    for field in [
        "product_ref",
        "policy_ref",
        "binding_ref",
        "customer",
        "chain",
    ] {
        shape::equal(m, &payload[field], &current[field])?;
    }
    let input = tc::declared_links::validate_inputs(m, program, payload)?
        .ok_or_else(|| shape::fail("retained Stage inline predecessor"))?;
    for field in ["slot", "predecessor"] {
        shape::equal(m, &link[field], &input[field])?;
    }
    for field in ["operation_key", "authority_ref"] {
        shape::equal(m, &link[field], &claim[field])?;
    }
    let link_value = owning_reference(m, &a, reference)?;
    let links = shape::array(m, &claim, "anchor_link_refs")?;
    if !shape::member(m, links, link_value.value())? {
        return Err(shape::fail("Stage closing Claim exact Link membership"));
    }
    let scope = tc::work_keys::scope(m, &c.key)?;
    let tuple = a.array_from_values(
        &[
            scope.value(),
            instance_value.value(),
            &payload["product_ref"],
            &input["slot"],
            &input["predecessor"],
        ],
        m,
    )?;
    let expected = body::semantic_id(m, "wlink_", "workflow-link", tuple.value())?;
    if !shape::same_text(m, &reference.id, &expected)? {
        return Err(shape::fail("retained Stage Link identity"));
    }
    let accepted = shape::text(m, &claim, "accepted_at")?;
    tc::declared_links::stage_endpoint(m, r, c, program, payload, &input["predecessor"], accepted)?;
    let permanent = closure_decision_id(m, &c.key, &instance)?;
    let permanent = a.string_value(&permanent, m)?;
    let one = a.string_value("1", m)?;
    let tuple = a.array_from_values(&[permanent.value(), one.value()], m)?;
    let identity = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
    let (_, decision) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &identity,
        "ledger-workflow-decision-revision/1",
    )?;
    tc::correction::dictionary_decision(m, &decision)?;
    for (field, want) in [
        ("kind", "outcome"),
        ("transition", "initial"),
        ("status", "active"),
        ("revision", "1"),
    ] {
        same(m, &decision, field, want)?;
    }
    let claim_value = owning_reference(m, &a, &claim_ref)?;
    for (field, value) in [
        ("decision_id", permanent.value()),
        ("subject_ref", claim_value.value()),
        ("original_terms_ref", &payload["binding_ref"]),
        ("authority_ref", &claim["authority_ref"]),
        ("operation_key", &claim["operation_key"]),
        ("accepted_at", &claim["accepted_at"]),
    ] {
        shape::equal(m, &decision[field], value)?;
    }
    let auth_ref = parse_owned_reference(m, &a, &claim["authority_ref"])?;
    let authority = r.get(
        m,
        &c.key,
        auth_ref.reference(),
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    let name = shape::text(m, &instance, "stage_name")?;
    let def = definition(m, program, name)?;
    shape::equal(m, &payload["activity"], &def["closure_activity"])?;
    shape::equal(m, &authority["source_slot"], &def["closure_source_slot"])?;
    tc::authority::historical_namespace(
        m,
        r,
        c,
        &claim["authority_ref"],
        &authority["canonical_economic_namespace"],
        Some(&claim["operation_key"]["source"]),
        (
            "outcome.submit",
            &payload["binding_ref"],
            &claim["accepted_at"],
        ),
    )?;
    tc::declared_links::rights(m, r, c, &authority, &input["slot"])?;
    // Recompute the frozen clock from immutable started/declaration/first Work
    // identities. Today's closed head is not a replacement original clock.
    let two = a.string_value("2", m)?;
    let identity = state_id(m, &c.key, instance_ref, two.value())?;
    let (_, started) = r.by_identity(
        m,
        &c.key,
        "workflow-stage-state",
        &identity,
        "ledger-workflow-stage-state/1",
    )?;
    same(m, &started, "state", "started")?;
    same(m, &started, "revision", "2")?;
    shape::equal(m, &started["stage_instance_ref"], instance_value.value())?;
    shape::equal(m, &claim["first_work_ref"], &started["first_work_ref"])?;
    let prior = parse_owned_reference(m, &a, &started["prior_ref"])?;
    let declared = r.get(
        m,
        &c.key,
        prior.reference(),
        "workflow-stage-state",
        "ledger-workflow-stage-state/1",
    )?;
    same(m, &declared, "state", "declared")?;
    same(m, &declared, "revision", "1")?;
    shape::equal(m, &declared["stage_instance_ref"], instance_value.value())?;
    shape::equal(m, &declared["accepted_at"], &instance["declared_at"])?;
    let identity = state_id(m, &c.key, instance_ref, one.value())?;
    if !shape::same_text(m, &prior.reference().id, &identity)? {
        return Err(shape::fail("retained Stage declaration identity"));
    }
    let first_ref = parse_owned_reference(m, &a, &started["first_work_ref"])?;
    let first = r.get(
        m,
        &c.key,
        first_ref.reference(),
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::equal(
        m,
        &first["payload"]["stage_instance_ref"],
        instance_value.value(),
    )?;
    shape::equal(m, &first["accepted_at"], &started["first_work_accepted_at"])?;
    shape::equal(m, &first["accepted_at"], &started["accepted_at"])?;
    shape::equal(m, &first["key"], &started["operation_key"])?;
    let start = Timestamp::parse_with_work(shape::text(m, &first, "accepted_at")?, m)?;
    let duration = shape::text(m, &def["window"], "activation_deadline_us")?;
    let duration = tc::arithmetic::integer(m, duration, false)?;
    let expected = tc::meter::add(m, &start.micros().into(), &duration)?;
    let deadline = Timestamp::parse_with_work(shape::text(m, &started, "activation_deadline")?, m)?;
    if !tc::meter::cmp(m, &expected, &deadline.micros().into())?.is_eq() {
        return Err(shape::fail("retained Stage first Work deadline"));
    }
    let close = Timestamp::parse_with_work(accepted, m)?;
    if w::compare(m, close.micros(), deadline.micros())?.is_gt() {
        return Err(shape::fail("retained Stage close after deadline"));
    }
    let anchor_ref = parse_owned_reference(m, &a, &claim["anchor_work_ref"])?;
    let anchor = r.get(
        m,
        &c.key,
        anchor_ref.reference(),
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let mut endpoint = parse_owned_reference(m, &a, &input["predecessor"])?;
    let path = shape::array(m, &def["window"], "anchor_path")?;
    if path.is_empty() || w::compare(m, path.len(), 4usize)?.is_gt() {
        return Err(shape::fail("retained Stage anchor path"));
    }
    shape::equal(m, &path[0], &input["slot"])?;
    for slot in &path[1..] {
        let endpoint_work = r.get(
            m,
            &c.key,
            endpoint.reference(),
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        let name = slot
            .as_str()
            .ok_or_else(|| shape::fail("retained Stage path slot"))?;
        let rows = shape::array(m, &endpoint_work["payload"], "links")?;
        let next = shape::named(m, rows, "slot", name)?;
        tc::declared_links::retained_work(m, r, c, program, endpoint.reference(), &endpoint_work)?;
        endpoint = parse_owned_reference(m, &a, &next["predecessor"])?;
    }
    if !shape::same_ref(m, endpoint.reference(), anchor_ref.reference())? {
        return Err(shape::fail("retained Stage original path endpoint"));
    }
    let mut member = false;
    for row in shape::array(m, &instance, "expected")? {
        if shape::same(
            m,
            &row["business_operation_id"],
            &anchor["payload"]["business_operation_id"],
        )? && shape::same(m, &row["activity"], &anchor["payload"]["activity"])?
            && shape::same(m, &row["source"], &anchor["key"]["source"])?
        {
            member = true;
        }
    }
    if !member {
        return Err(shape::fail("retained Stage anchor fixed original member"));
    }
    shape::equal(
        m,
        &anchor["payload"]["stage_instance_ref"],
        instance_value.value(),
    )?;
    let occurred = shape::text(m, &anchor["payload"], "occurred_at")?;
    let deadline = shape::text(m, &started, "activation_deadline")?;
    let window = tc::targets::stage_frozen_window(
        m,
        &def["window"],
        anchor_ref.reference(),
        occurred,
        deadline,
    )?;
    shape::equal(m, &claim["window"], &window)?;
    Ok(())
}
