//! Consuming declared same-Binding Work references. A Ref's syntax/hash and a
//! source namespace never substitute for its immutable birth/authority joins.
use super::super::selected_program::SelectedProgram;
use super::{
    authority, body,
    data::*,
    fact_references,
    records::Records,
    shape,
    working_value::{owning_definition, owning_reference, parse_owned_reference, WorkingKey},
    CoreMeter,
};
use crate::{
    domain::Timestamp,
    validation_work::{self as w, OriginalAuditSession},
    Result,
};
use serde_json::Value;

fn eq_field(m: &mut CoreMeter, value: &Value, field: &str, expected: &Value) -> Result<()> {
    let actual = shape::field(m, value, field)?;
    shape::equal(m, actual, expected)
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    value: &Value,
    kind: &str,
    schema: &str,
) -> Result<super::record_body::RecordBody> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning reference relation"))?;
    let reference = parse_owned_reference(m, &a, value)?;
    r.get(m, key, reference.reference(), kind, schema)
}
fn endpoint(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    reference: &ExactRef,
    subject: (&Value, &Value, &str),
) -> Result<()> {
    let (binding_ref, binding, at) = subject;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work reference"))?;
    // Fixed temporary Ref/semantic-ID/hash controls coexist with independently
    // owned parsed bodies; this is requested storage, not a new allowance.
    let _scratch = a.reserve_external_payload(4096, m)?;
    let work = r.get(
        m,
        key,
        reference,
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
    let original_key = WorkingKey::parse(m, &work["key"])?;
    if !shape::same_text(m, &original_key.logical_store_id, &key.logical_store_id)?
        || !shape::same_text(m, &original_key.scope[0], &key.scope[0])?
        || !shape::same_text(m, &original_key.scope[1], &key.scope[1])?
    {
        return Err(shape::fail("reference Work store/scope"));
    }
    let p = shape::field(m, &work, "payload")?;
    eq_field(m, p, "binding_ref", binding_ref)?;
    eq_field(m, p, "customer", &binding["customer"])?;
    eq_field(m, &work, "source_namespace", &binding["source_namespace"])?;
    let original_product = owning_definition(m, &a, program.product_ref())?;
    let original_policy = owning_definition(m, &a, program.root_policy_ref())?;
    eq_field(m, p, "product_ref", original_product.value())?;
    eq_field(m, p, "policy_ref", original_policy.value())?;
    let (product, activity) = super::input_selection::work_activity(program, p, m)?;
    let status = shape::text(m, p, "status")?;
    if !shape::same_text(m, status, "succeeded")? && !shape::same_text(m, status, "failed")? {
        return Err(shape::fail("reference original Work completion"));
    }
    let mut schema = None;
    for (reference, definition) in program.fact_schemas() {
        let value = owning_definition(m, &a, reference)?;
        if shape::same(m, value.value(), &p["fact_schema_ref"])? {
            schema = Some(definition);
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("reference original Work schema"))?;
    let normalized = super::working_value::NormalizedInput::normalize(m, schema, &p["facts"])?;
    shape::equal(m, normalized.canonical_value(), &p["facts"])?;
    let event = super::work_keys::event(m, &original_key, p)?;
    if !shape::same_text(m, &event, &reference.id)? {
        return Err(shape::fail("reference Work semantic identity"));
    }
    let namespace = shape::text(m, &work, "source_namespace")?;
    let claim_id = super::work_keys::claim(m, &original_key, namespace, p)?;
    let claimed = shape::text(m, &work, "claim_id")?;
    if !shape::same_text(m, &claim_id, claimed)? {
        return Err(shape::fail("reference Work permanent claim identity"));
    }
    let (claim_ref, claim) = r.by_identity(
        m,
        key,
        "workflow-work-claim",
        &claim_id,
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
    let scope = super::work_keys::scope(m, key)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let work_ref = owning_reference(m, &a, reference)?;
    let completion = a.string_value("completion", m)?;
    for (field, expected) in [
        ("logical_store_id", store.value()),
        ("scope", scope.value()),
        ("source_namespace", &work["source_namespace"]),
        ("completion", completion.value()),
        ("work_ref", work_ref.value()),
    ] {
        eq_field(m, &claim, field, expected)?;
    }
    super::outcome::original_claim_association(m, &claim, &work)?;
    let decision_id = super::work_keys::decision(m, &claim_ref.id)?;
    let id = a.string_value(&decision_id, m)?;
    let one = a.string_value("1", m)?;
    let tuple = a.array_from_values(&[id.value(), one.value()], m)?;
    let revision_id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
    let (_, decision) = r.by_identity(
        m,
        key,
        "workflow-decision",
        &revision_id,
        "ledger-workflow-decision-revision/1",
    )?;
    super::correction::dictionary_decision(m, &decision)?;
    if decision.get("prior_ref").is_some() {
        return Err(shape::fail("initial reference Work prior"));
    }
    let work_kind = a.string_value("work", m)?;
    let transition = a.string_value("initial", m)?;
    let active = a.string_value("active", m)?;
    for (field, expected) in [
        ("decision_id", id.value()),
        ("revision", one.value()),
        ("subject_ref", work_ref.value()),
        ("original_terms_ref", binding_ref),
        ("kind", work_kind.value()),
        ("transition", transition.value()),
        ("status", active.value()),
        ("operation_key", &work["key"]),
        ("accepted_at", &work["accepted_at"]),
    ] {
        eq_field(m, &decision, field, expected)?;
    }
    let original_at = shape::text(m, &work, "accepted_at")?;
    let received = shape::text(m, &work, "received_at")?;
    let original_time = Timestamp::parse_with_work(original_at, m)?.micros();
    let received_time = Timestamp::parse_with_work(received, m)?.micros();
    let use_time = Timestamp::parse_with_work(at, m)?.micros();
    if w::compare(m, received_time, original_time)?.is_gt()
        || w::compare(m, original_time, use_time)?.is_gt()
    {
        return Err(shape::fail("reference Work historical time"));
    }
    let evaluation = load(
        m,
        r,
        key,
        &decision["evaluation_ref"],
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
    let plan = owning_definition(m, &a, program.compiled_plan().reference())?;
    for (field, expected) in [
        ("subject_ref", work_ref.value()),
        ("policy_ref", &p["policy_ref"]),
        ("compiled_plan_ref", plan.value()),
        (
            "compiler_ref",
            &program.compiled_plan().canonical_value()["compiler_ref"],
        ),
        ("binding_ref", binding_ref),
        ("fact_schema_ref", &p["fact_schema_ref"]),
        ("facts", &p["facts"]),
        ("context", &binding["accepted_context"]),
    ] {
        eq_field(m, &evaluation, field, expected)?;
    }
    let auth = load(
        m,
        r,
        key,
        &decision["authority_ref"],
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    let operation = a.string_value("work.accept", m)?;
    for (field, expected) in [
        ("operation", operation.value()),
        ("scope", scope.value()),
        ("customer", &binding["customer"]),
        ("original_binding_ref", binding_ref),
        ("source", &work["key"]["source"]),
        ("canonical_economic_namespace", &work["source_namespace"]),
        ("observed_at", &work["accepted_at"]),
    ] {
        eq_field(m, &auth, field, expected)?;
    }
    let slot = shape::text(m, activity, "source_slot")?;
    let auth_slot = shape::text(m, &auth, "source_slot")?;
    if !shape::same_text(m, slot, auth_slot)? {
        return Err(shape::fail("reference original worker slot"));
    }
    let slots = shape::array(m, binding, "source_slots")?;
    let nomination = shape::named(m, slots, "name", slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    if !shape::member(m, sources, &auth["source"])? {
        return Err(shape::fail("reference worker original nomination"));
    }
    let enrollment = load(
        m,
        r,
        key,
        &auth["enrollment_ref"],
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    eq_field(m, &enrollment, "scope", scope.value())?;
    let ceiling = shape::field(m, &enrollment, "ceiling")?;
    for name in ["source", "canonical_economic_namespace"] {
        eq_field(m, ceiling, name, &auth[name])?;
    }
    eq_field(m, ceiling, "principal_id", &auth["actor"])?;
    let state_ref = parse_owned_reference(m, &a, &auth["current_state_ref"])?;
    let state = r.get(
        m,
        key,
        state_ref.reference(),
        "workflow-source-state",
        "ledger-workflow-source-state/1",
    )?;
    shape::closed(
        m,
        &state,
        &[
            "schema",
            "enrollment_ref",
            "revision",
            "status",
            "current_grant_ref",
            "operation_key",
            "actor",
            "accepted_at",
        ],
        &["prior_state_ref"],
    )?;
    let state_active = a.string_value("active", m)?;
    for (field, expected) in [
        ("enrollment_ref", &auth["enrollment_ref"]),
        ("current_grant_ref", &auth["current_grant_ref"]),
        ("status", state_active.value()),
        ("actor", &auth["actor"]),
    ] {
        eq_field(m, &state, field, expected)?;
    }
    let state_key = WorkingKey::parse(m, &state["operation_key"])?;
    if !shape::same_text(m, &state_key.logical_store_id, &key.logical_store_id)?
        || !shape::same_text(m, &state_key.scope[0], &key.scope[0])?
        || !shape::same_text(m, &state_key.scope[1], &key.scope[1])?
    {
        return Err(shape::fail("reference worker state scope"));
    }
    let state_at = shape::text(m, &state, "accepted_at")?;
    let state_time = Timestamp::parse_with_work(state_at, m)?.micros();
    if w::compare(m, state_time, original_time)?.is_gt() {
        return Err(shape::fail("reference worker state time"));
    }
    let revision = shape::text(m, &state, "revision")?;
    let revision_number = super::super::facts::counter_with_work(revision, m)?;
    if !w::compare(m, revision_number, 0u64)?.is_gt() {
        return Err(shape::fail("reference worker state revision"));
    }
    let state_tuple = a.array_from_values(
        &[
            store.value(),
            scope.value(),
            &enrollment["enrollment_id"],
            &state["revision"],
        ],
        m,
    )?;
    let state_id = body::semantic_id(m, "wsource_", "workflow-source-state", state_tuple.value())?;
    if !shape::same_text(m, &state_id, &state_ref.reference().id)? {
        return Err(shape::fail("reference worker state identity"));
    }
    let owner = super::representation::InvocationToken::new();
    let product_id = shape::text(m, product, "product_id")?;
    let activity_name = shape::text(m, p, "activity")?;
    let accepted = shape::array(m, binding, "authority_refs")?;
    let originals = shape::array(m, &auth, "original_grant_refs")?;
    if !w::compare(m, originals.len(), 0usize)?.is_gt() {
        return Err(shape::fail("reference original worker grants"));
    }
    for (grant_ref, is_original) in std::iter::once((&auth["current_grant_ref"], false))
        .chain(originals.iter().map(|v| (v, true)))
    {
        if is_original && !shape::member(m, accepted, grant_ref)? {
            return Err(shape::fail("reference grant absent accepted Binding"));
        }
        let grant = load(
            m,
            r,
            key,
            grant_ref,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        authority::grant_shape(m, &grant)?;
        if !is_original {
            eq_field(m, &grant, "revision", &state["revision"])?;
        }
        eq_field(m, &grant, "original_ceiling_ref", &auth["enrollment_ref"])?;
        eq_field(m, &grant, "grant_id", &enrollment["enrollment_id"])?;
        for name in ["source", "principal_id", "canonical_economic_namespace"] {
            eq_field(m, &grant, name, &ceiling[name])?;
        }
        authority::grant_intersection(m, &owner, &grant, ceiling, &original_time.into())?;
        authority::grant_coverage(
            m,
            &grant,
            &binding["customer"],
            activity_name,
            product_id,
            slot,
            ("work.accept", &["read", "submit"]),
        )?;
    }
    Ok(())
}

pub(super) fn fields(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    subject: (&Value, &Value, &str),
    actual: (&Value, &Value),
) -> Result<()> {
    if !program.uses_work_lifecycle_recipe() {
        return Ok(());
    }
    let inputs = fact_references::select_group(m, program, actual.0, actual.1, false)?;
    consume(m, r, key, program, subject, &inputs, None)
}
fn consume(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    subject: (&Value, &Value, &str),
    inputs: &fact_references::FactReferenceInputs,
    work: Option<(&Value, &str)>,
) -> Result<()> {
    for input in inputs.entries() {
        if program.uses_stage_lifecycle_recipe()
            && shape::same_text(m, &input.reference().kind, "workflow-invocation")?
            && shape::same_text(m, input.relation(), "same_invocation")?
        {
            let (payload, source) =
                work.ok_or_else(|| shape::fail("same Invocation requires an actual Work subject"))?;
            super::invocation::reference_for_work(
                m,
                r,
                key,
                program,
                payload,
                subject.1,
                input.reference(),
                source,
                subject.2,
            )?;
            continue;
        }
        if !shape::same_text(m, &input.reference().kind, "workflow-work")?
            || !shape::same_text(m, input.relation(), "same_binding")?
        {
            return Err(shape::fail("unsupported lifecycle Reference consumer"));
        }
        endpoint(m, r, key, program, input.reference(), subject)?;
    }
    Ok(())
}

/// Consuming facts retain the subject's original Binding, not the corrector's
/// source namespace. Endpoint births are checked without following endpoint
/// facts recursively or consulting any current endpoint head.
pub(super) fn facts(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    payload: &Value,
    values: &Value,
    at: &str,
) -> Result<()> {
    facts_original(m, r, key, program, payload, values, at, &key.source)
}
pub(super) fn facts_original(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    payload: &Value,
    values: &Value,
    at: &str,
    source: &str,
) -> Result<()> {
    if !program.uses_work_lifecycle_recipe() {
        return Ok(());
    }
    let inputs =
        fact_references::select_group(m, program, &payload["fact_schema_ref"], values, false)?;
    if w::compare(m, inputs.entries().len(), 0usize)?.is_eq() {
        return Ok(());
    }
    let binding_ref = shape::field(m, payload, "binding_ref")?;
    let binding = load(
        m,
        r,
        key,
        binding_ref,
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    consume(
        m,
        r,
        key,
        program,
        (binding_ref, &binding, at),
        &inputs,
        Some((payload, source)),
    )
}
pub(super) fn context(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    binding: (&Value, &Value),
    at: &str,
) -> Result<()> {
    if !program.uses_work_lifecycle_recipe() {
        return Ok(());
    }
    let context = shape::field(m, binding.1, "accepted_context")?;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning context relation"))?;
    let expected = owning_definition(m, &a, program.context_schema_ref())?;
    eq_field(m, context, "fact_schema_ref", expected.value())?;
    fields(
        m,
        r,
        key,
        program,
        (binding.0, binding.1, at),
        (&context["fact_schema_ref"], &context["facts"]),
    )
}
/// Acceptance and its original replay supply the actual candidate Binding and
/// retained original inputs. This proves reference association only; the caller
/// still proves the acceptance's authority, publication and member recipe.
pub fn validate_lifecycle_context_references(
    program: &SelectedProgram,
    key: &OperationKey,
    binding: EconomicRecordView<'_>,
    inputs: &[EconomicRecordView<'_>],
    trusted_accepted_at: &str,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let result = (|| {
        session.check_cancelled()?;
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_work_lifecycle_recipe() {
            return Err(shape::fail("exact lifecycle context recipe"));
        }
        let a = session.arena().shared_handle();
        let m = session.meter_mut();
        let one = [binding];
        let mut candidate = Records::borrowed_parent_selected(m, &a, key, &one, program)?;
        let value = candidate.get(
            m,
            key,
            binding.reference,
            "workflow-binding",
            "ledger-workflow-binding/2",
        )?;
        let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        let reference = owning_reference(m, &a, binding.reference)?;
        context(
            m,
            &mut r,
            key,
            program,
            (reference.value(), &value),
            trusted_accepted_at,
        )
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
