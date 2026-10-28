//! Consuming declared fixed Work reference relationships. A Ref's syntax/hash and a
//! source namespace never substitute for its immutable birth/authority joins.
use super::super::{selected_program::SelectedProgram, FactSchema};
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
// Fixed original semantic endpoint, not a recompiled Program or read capability.
struct ScopeDefinitions {
    binding: super::record_body::RecordBody,
    product: super::record_body::RecordBody,
    policy: super::record_body::RecordBody,
    plan: super::record_body::RecordBody,
    schema: FactSchema,
    _schema_owner: crate::canonical::audit_arena::ExternalBufferLease,
}
fn scope_definitions(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    payload: &Value,
) -> Result<ScopeDefinitions> {
    let binding = load(
        m,
        r,
        key,
        &payload["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    for field in ["customer", "product_ref", "policy_ref"] {
        eq_field(m, &binding, field, &payload[field])?;
    }
    let product = load(
        m,
        r,
        key,
        &binding["product_ref"],
        "workflow-product",
        "ledger-workflow-product/1",
    )?;
    let policy = load(
        m,
        r,
        key,
        &binding["policy_ref"],
        "workflow-policy",
        "ledger-workflow-policy/4",
    )?;
    eq_field(m, &policy, "product_ref", &binding["product_ref"])?;
    eq_field(m, &binding, "fact_schema_refs", &product["fact_schemas"])?;
    eq_field(
        m,
        &binding["accepted_context"],
        "fact_schema_ref",
        &policy["context_schema_ref"],
    )?;
    let plan = load(
        m,
        r,
        key,
        &binding["compiled_plan_ref"],
        "workflow-compiled-plan",
        "ledger-workflow-compiled-plan/8",
    )?;
    eq_field(m, &plan, "compiler_ref", &policy["compiler_ref"])?;
    eq_field(m, &binding, "evaluator", &plan["evaluator"])?;
    let sources = shape::array(m, &plan, "source_refs")?;
    for reference in [
        &binding["product_ref"],
        &binding["policy_ref"],
        &payload["fact_schema_ref"],
        &policy["context_schema_ref"],
        &policy["compiler_ref"],
    ] {
        if !shape::member(m, sources, reference)? {
            return Err(shape::fail("reference own original plan source membership"));
        }
    }
    let schemas = shape::array(m, &product, "fact_schemas")?;
    if !shape::member(m, schemas, &payload["fact_schema_ref"])? {
        return Err(shape::fail("reference own original Product schema"));
    }
    let raw_schema = load(
        m,
        r,
        key,
        &payload["fact_schema_ref"],
        "workflow-fact-schema",
        "ledger-workflow-fact-schema/1",
    )?;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning reference original schema"))?;
    // SAME typed source normalizer convention as selected_program::compiler:
    // prepaid raw Value copy plus its complete normalizer transient payload.
    let copy = crate::canonical::audit_arena::value_copy_payload_bound(&raw_schema, m)?;
    let normalized = FactSchema::normalization_payload_bound_with_work(&raw_schema, m)?;
    let bytes = w::usize_add(m, copy, normalized)?;
    let schema_owner = a.reserve_external_payload(bytes, m)?;
    let schema = FactSchema::from_value_complete_with_work((*raw_schema).clone(), m)?;
    shape::equal(m, schema.canonical_value(), &raw_schema)?;
    Ok(ScopeDefinitions {
        binding,
        product,
        policy,
        plan,
        schema,
        _schema_owner: schema_owner,
    })
}
fn scope_publication(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    payload: &Value,
    own: &ScopeDefinitions,
    authority: &Value,
) -> Result<()> {
    let binding = &own.binding;
    let accepted = load(
        m,
        r,
        key,
        &authority["original_accepted_agreement_ref"],
        "workflow-accepted-agreement",
        "ledger-workflow-accepted-agreement/1",
    )?;
    eq_field(m, &accepted, "binding_ref", &payload["binding_ref"])?;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning reference accepted publication"))?;
    let scope = super::work_keys::scope(m, key)?;
    eq_field(m, &accepted, "scope", scope.value())?;
    for field in ["customer", "agreement_id", "version", "accepted_at"] {
        eq_field(m, &accepted, field, &binding[field])?;
    }
    let accepted_at = Timestamp::parse_with_work(shape::text(m, &accepted, "accepted_at")?, m)?;
    let work_at = Timestamp::parse_with_work(shape::text(m, authority, "observed_at")?, m)?;
    if w::compare(m, accepted_at.micros(), work_at.micros())?.is_gt() {
        return Err(shape::fail("reference Work before its own acceptance"));
    }
    let accepted_key = WorkingKey::parse(m, &accepted["operation_key"])?;
    if !shape::same_text(m, &accepted_key.logical_store_id, &key.logical_store_id)?
        || !shape::same_text(m, &accepted_key.scope[0], &key.scope[0])?
        || !shape::same_text(m, &accepted_key.scope[1], &key.scope[1])?
    {
        return Err(shape::fail("reference own acceptance store/scope"));
    }
    let store = a.string_value(&key.logical_store_id, m)?;
    let tuple = a.array_from_values(
        &[
            store.value(),
            scope.value(),
            &accepted["customer"],
            &accepted["agreement_id"],
            &accepted["version"],
        ],
        m,
    )?;
    let id = body::semantic_id(
        m,
        "waccepted_",
        "workflow-agreement-acceptance",
        tuple.value(),
    )?;
    let id = a.string_value(&id, m)?;
    eq_field(
        m,
        &authority["original_accepted_agreement_ref"],
        "id",
        id.value(),
    )?;
    let terms_ref = parse_owned_reference(m, &a, &accepted["terms_ref"])?;
    let terms = r.get(
        m,
        key,
        terms_ref.reference(),
        "workflow-acceptance-terms",
        "ledger-workflow-acceptance-terms/2",
    )?;
    eq_field(m, &terms, "scope", scope.value())?;
    for field in ["customer", "agreement_id", "version", "publication_ref"] {
        eq_field(m, &terms, field, &accepted[field])?;
    }
    let raw = r.canonical(m, terms_ref.reference())?;
    let digest = super::records::hash_parts(m, "workflow-acceptance-terms", &[raw], false)?;
    let digest = a.string_value(&digest, m)?;
    eq_field(m, &accepted, "terms_digest", digest.value())?;
    let projected = shape::field(m, &terms, "binding_inputs")?;
    for field in [
        "agreement_id",
        "version",
        "customer",
        "source_namespace",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "roles_refs",
        "source_slots",
        "accepted_context",
        "bindings",
        "funding_legs",
        "starts_at",
        "quantity_ceilings",
        "exposure_ceilings",
    ] {
        eq_field(m, projected, field, &binding[field])?;
    }
    match (projected.get("ends_at"), binding.get("ends_at")) {
        (Some(left), Some(right)) => shape::equal(m, left, right)?,
        (None, None) => {}
        _ => return Err(shape::fail("reference own accepted end bound")),
    }
    let publication = load(
        m,
        r,
        key,
        &accepted["publication_ref"],
        "workflow-publication",
        "ledger-workflow-publication/1",
    )?;
    shape::closed(
        m,
        &publication,
        &[
            "schema",
            "root_policy_ref",
            "source_refs",
            "compiler_ref",
            "compiled_plan_ref",
            "provenance",
        ],
        &[],
    )?;
    eq_field(m, &publication, "root_policy_ref", &binding["policy_ref"])?;
    eq_field(
        m,
        &publication,
        "compiled_plan_ref",
        &binding["compiled_plan_ref"],
    )?;
    eq_field(m, &publication, "compiler_ref", &own.plan["compiler_ref"])?;
    eq_field(m, &own.policy, "compiler_ref", &publication["compiler_ref"])?;
    eq_field(m, &publication, "source_refs", &own.plan["source_refs"])?;
    let tuple = a.array_from_values(&[store.value(), scope.value(), &binding["policy_ref"]], m)?;
    let id = body::semantic_id(m, "wpub_", "workflow-publication", tuple.value())?;
    let id = a.string_value(&id, m)?;
    eq_field(m, &accepted["publication_ref"], "id", id.value())?;
    Ok(())
}

fn endpoint(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    reference: &ExactRef,
    subject: (&Value, &Value, &str),
    relation: &str,
) -> Result<()> {
    let (binding_ref, binding, at) = subject;
    let scope_customer = shape::same_text(m, relation, "same_scope_customer")?;
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
    // Cross-Binding references constrain the consumer only by store/scope/customer.
    // Every other association belongs to the endpoint's own original closure.
    eq_field(m, p, "customer", &binding["customer"])?;
    let own = if scope_customer {
        Some(scope_definitions(m, r, key, p)?)
    } else {
        None
    };
    let (binding_ref, binding) = match &own {
        Some(own) => (&p["binding_ref"], &*own.binding),
        None => (binding_ref, binding),
    };
    eq_field(m, p, "binding_ref", binding_ref)?;
    eq_field(m, p, "customer", &binding["customer"])?;
    eq_field(m, &work, "source_namespace", &binding["source_namespace"])?;
    let selected_product;
    let selected_policy;
    let (product, activity) = if let Some(own) = &own {
        let name = shape::text(m, p, "activity")?;
        let activities = shape::array(m, &own.product, "activities")?;
        let activity = shape::named(m, activities, "name", name)?;
        let work_primitive = a.string_value("work", m)?;
        eq_field(m, activity, "primitive", work_primitive.value())?;
        eq_field(m, activity, "fact_schema", &p["fact_schema_ref"])?;
        (&*own.product, activity)
    } else {
        selected_product = owning_definition(m, &a, program.product_ref())?;
        selected_policy = owning_definition(m, &a, program.root_policy_ref())?;
        eq_field(m, p, "product_ref", selected_product.value())?;
        eq_field(m, p, "policy_ref", selected_policy.value())?;
        super::input_selection::work_activity(program, p, m)?
    };
    let status = shape::text(m, p, "status")?;
    if !shape::same_text(m, status, "succeeded")? && !shape::same_text(m, status, "failed")? {
        return Err(shape::fail("reference original Work completion"));
    }
    let mut schema = None;
    if let Some(own) = &own {
        schema = Some(&own.schema);
    } else {
        for (reference, definition) in program.fact_schemas() {
            let value = owning_definition(m, &a, reference)?;
            if shape::same(m, value.value(), &p["fact_schema_ref"])? {
                schema = Some(definition);
            }
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
    let selected_plan;
    let (plan_ref, compiler_ref) = if let Some(own) = &own {
        (&own.binding["compiled_plan_ref"], &own.plan["compiler_ref"])
    } else {
        selected_plan = owning_definition(m, &a, program.compiled_plan().reference())?;
        (
            selected_plan.value(),
            &program.compiled_plan().canonical_value()["compiler_ref"],
        )
    };
    for (field, expected) in [
        ("subject_ref", work_ref.value()),
        ("policy_ref", &p["policy_ref"]),
        ("compiled_plan_ref", plan_ref),
        ("compiler_ref", compiler_ref),
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
    if let Some(own) = &own {
        scope_publication(m, r, key, p, own, &auth)?;
    }
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
                (input.reference(), source, subject.2),
            )?;
            continue;
        }
        if !shape::same_text(m, &input.reference().kind, "workflow-work")?
            || !(shape::same_text(m, input.relation(), "same_binding")?
                || shape::same_text(m, input.relation(), "same_scope_customer")?)
        {
            return Err(shape::fail("unsupported lifecycle Reference consumer"));
        }
        endpoint(
            m,
            r,
            key,
            program,
            input.reference(),
            subject,
            input.relation(),
        )?;
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
    facts_original(m, r, key, program, payload, values, (at, &key.source))
}
pub(super) fn facts_original(
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    payload: &Value,
    values: &Value,
    origin: (&str, &str),
) -> Result<()> {
    let (at, source) = origin;
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

#[cfg(test)]
mod scope_tests {
    use super::super::{body, records, work_keys};
    use super::*;
    use crate::{
        canonical::{self, audit_arena::AuditArena},
        policy::workflow::selected_program::{
            compile_selected_publication, load_local_selected_execution, SelectedCompilerSupport,
            SelectedSourceInput,
        },
    };
    use serde_json::json;
    fn source(
        support: &SelectedCompilerSupport,
        session: &mut OriginalAuditSession,
    ) -> SelectedSourceInput {
        fn reference(doc: &Value) -> Value {
            let schema = doc["schema"].as_str().unwrap();
            let kind = match schema {
                "ledger-workflow-fact-schema/1" => "workflow-fact-schema",
                "ledger-workflow-product/1" => "workflow-product",
                "ledger-workflow-policy/4" => "workflow-policy",
                _ => "workflow-slot-template",
            };
            let digest = crate::policy::workflow::document_digest(schema, doc).unwrap();
            json!({"kind":kind,"id":format!("wdoc_{}",&digest[7..]),"digest":digest})
        }
        fn rebind(value: &mut Value, old: &Value, new: &Value) {
            if value == old {
                *value = new.clone();
                return;
            }
            match value {
                Value::Array(rows) => {
                    for row in rows {
                        rebind(row, old, new);
                    }
                }
                Value::Object(rows) => {
                    for row in rows.values_mut() {
                        rebind(row, old, new);
                    }
                }
                _ => {}
            }
        }
        let mut data: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-work-lifecycle/source.json"
        ))
        .unwrap();
        // Author A2 and rebind the entire changed definition closure before the
        // SAME normalizers run. Frozen A1 input bytes are never mutated.
        let mut replacements = Vec::new();
        for doc in data["documents"].as_array_mut().unwrap() {
            let old = reference(doc);
            match doc["schema"].as_str().unwrap() {
                "ledger-workflow-product/1" | "ledger-workflow-policy/4" => {
                    doc["version"] = json!("2")
                }
                "ledger-workflow-fact-schema/1" if doc["schema_id"] == "support:context" => {
                    doc["version"] = json!("2");
                    doc["fields"].as_array_mut().unwrap().push(json!({"name":"support:prior-work","required":true,"type":{"kind":"reference","record_kind":"workflow-work","relation":"same_scope_customer"},"provenance":{"kind":"caller"}}));
                }
                _ => {}
            }
            replacements.push((old, reference(doc)));
        }
        for (old, new) in replacements {
            rebind(&mut data, &old, &new);
        }
        let value = crate::policy::workflow::selected_program::stage_fixture_normalized_source(
            &data, support, session,
        )
        .unwrap();
        SelectedSourceInput {
            documents: value["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .chain(std::iter::once(support.canonical_bytes().to_vec()))
                .collect(),
            root_policy_ref: serde_json::from_value(value["root_policy_ref"].clone()).unwrap(),
        }
    }
    #[test]
    fn scope_customer_context_uses_own_original_definition_and_refuses_foreign_relations() {
        let arena = AuditArena::new().unwrap();
        let p = super::super::lifecycle_consumer_tests::program(&arena);
        let mut producer_session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let producer = producer_session.meter_mut();
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["example-company".into(), "local".into()],
            source: "urn:example:worker".into(),
            operation_id: "fixed-birth".into(),
        };
        let at = "2026-10-26T00:00:00.000000Z";
        // Owned canonical relationship DATA, reusing the finite source fixture.
        // No host producer/member/custody or financial constructor proof claimed.
        let business: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-work-lifecycle/business.json"
        ))
        .unwrap();
        let ceiling = business["worker_ceiling"].clone();
        let enrollment=body::document(producer,"workflow-source-enrollment",json!({"schema":"ledger-workflow-source-enrollment/1","enrollment_id":"worker-enrollment","scope":key.scope,"ceiling":ceiling})).unwrap();
        let mut grant = ceiling.clone();
        grant["schema"] = json!("ledger-workflow-source-grant/1");
        grant["grant_id"] = json!("worker-enrollment");
        grant["revision"] = json!("1");
        grant["original_ceiling_ref"] = json!(enrollment.reference);
        let grant = body::document(producer, "workflow-source-grant", grant).unwrap();
        let state_id = body::semantic_id(
            producer,
            "wsource_",
            "workflow-source-state",
            &json!([key.logical_store_id, key.scope, "worker-enrollment", "1"]),
        )
        .unwrap();
        let state=body::record(producer,&key,"workflow-source-state",state_id,json!({"schema":"ledger-workflow-source-state/1","enrollment_ref":enrollment.reference,"revision":"1","status":"active","current_grant_ref":grant.reference,"operation_key":key,"actor":"example-operator","accepted_at":at})).unwrap();
        let role = body::document(producer, "workflow-roles", business["role"].clone()).unwrap();
        let catalog = body::document(
            producer,
            "workflow-slot-catalog",
            business["catalog"].clone(),
        )
        .unwrap();
        let inst = body::document(producer, "workflow-slot-instantiation", json!({"schema":"ledger-workflow-slot-instantiation/1","agreement_id":business["agreement"],"customer":"customer-B","slot_template_ref":p.original_policy_value()["slot_template_ref"],"slot_catalog_ref":catalog.reference,"component_map":business["component_map"],"family_map":[{"template_key":"support:quality","slot_id":"support:quality"}]})).unwrap();
        let mut retail = business["terms"].clone();
        retail["role_ref"] = json!(role.reference);
        retail["source_grant_refs"] = json!([grant.reference]);
        let retail = body::document(producer, "workflow-slot-terms", retail).unwrap();
        let mut binding_value = business["binding"].clone();
        binding_value["schema"] = json!("ledger-workflow-binding/2");
        binding_value["product_ref"] = json!(p.product_ref());
        binding_value["policy_ref"] = json!(p.root_policy_ref());
        binding_value["compiled_plan_ref"] = json!(p.compiled_plan().reference());
        binding_value["evaluator"] = json!("ledger-workflow-evaluator/1");
        let product = p
            .source_documents()
            .iter()
            .find(|d| d.reference() == p.product_ref())
            .unwrap();
        binding_value["fact_schema_refs"] = product.canonical_value()["fact_schemas"].clone();
        binding_value["slot_catalog_ref"] = json!(catalog.reference);
        binding_value["slot_instantiation_ref"] = json!(inst.reference);
        binding_value["roles_refs"] = json!([role.reference]);
        binding_value["authority_refs"] = json!([grant.reference]);
        binding_value["bindings"] =
            json!([{"slot":"support:retail","binding_ref":retail.reference}]);
        binding_value["accepted_context"] = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:eligibility":"support:eligible"}});
        binding_value["accepted_at"] = json!(at);
        binding_value["accepted_by"] = json!("example-operator");
        binding_value["assent_refs"] = json!([grant.reference]);
        let binding = body::document(producer, "workflow-binding", binding_value.clone()).unwrap();
        let pub_id = body::semantic_id(
            producer,
            "wpub_",
            "workflow-publication",
            &json!([key.logical_store_id, key.scope, p.root_policy_ref()]),
        )
        .unwrap();
        let publication = body::record(producer,&key,"workflow-publication",pub_id,json!({"schema":"ledger-workflow-publication/1","root_policy_ref":p.root_policy_ref(),"compiled_plan_ref":p.compiled_plan().reference(),"compiler_ref":p.compiled_plan().canonical_value()["compiler_ref"],"source_refs":p.compiled_plan().canonical_value()["source_refs"],"provenance":{"source_sha":"a".repeat(40),"binary_sha256":"b".repeat(64),"build_manifest_digest":format!("sha256:{}","c".repeat(64))}})).unwrap();
        let mut projected = binding_value.clone();
        for field in [
            "schema",
            "product_ref",
            "policy_ref",
            "compiled_plan_ref",
            "evaluator",
            "fact_schema_refs",
            "accepted_at",
            "accepted_by",
            "authority_refs",
            "assent_refs",
        ] {
            projected.as_object_mut().unwrap().remove(field);
        }
        let terms = body::document(producer,"workflow-acceptance-terms",json!({"schema":"ledger-workflow-acceptance-terms/2","scope":key.scope,"customer":"customer-B","agreement_id":binding_value["agreement_id"],"version":"1","publication_ref":publication.reference,"binding_inputs":projected})).unwrap();
        let terms_digest = records::hash_parts(
            producer,
            "workflow-acceptance-terms",
            &[&terms.canonical_body],
            false,
        )
        .unwrap();
        let accepted_id = body::semantic_id(
            producer,
            "waccepted_",
            "workflow-agreement-acceptance",
            &json!([
                key.logical_store_id,
                key.scope,
                "customer-B",
                binding_value["agreement_id"],
                "1"
            ]),
        )
        .unwrap();
        let accepted = body::record(producer,&key,"workflow-accepted-agreement",accepted_id,json!({"schema":"ledger-workflow-accepted-agreement/1","operation_key":key,"scope":key.scope,"customer":"customer-B","agreement_id":binding_value["agreement_id"],"version":"1","publication_ref":publication.reference,"binding_ref":binding.reference,"terms_ref":terms.reference,"terms_digest":terms_digest,"authority_observation_ref":grant.reference,"accepted_at":at,"commit_sequence":"1"})).unwrap();
        let activity = &p.compiled_plan().canonical_value()["activity_table"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["activity"] == "support:fixed")
            .unwrap();
        let payload = json!({"schema":"ledger-workflow-work/1","external_event_id":"fixed-event","business_operation_id":"fixed-operation","customer":"customer-B","chain":"one-chain","activity":"support:fixed","product_ref":p.product_ref(),"policy_ref":p.root_policy_ref(),"fact_schema_ref":activity["fact_schema_ref"],"binding_ref":binding.reference,"status":"succeeded","facts":{"support:quantity":"3"},"fact_proofs":[],"links":[],"evidence_refs":[],"invocation_refs":[]});
        let event = work_keys::event(producer, &key, &payload).unwrap();
        let claim_id = work_keys::claim(producer, &key, &key.source, &payload).unwrap();
        let work=body::record(producer,&key,"workflow-work",event,json!({"schema":"ledger-workflow-work-record/1","key":key,"payload":payload,"claim_id":claim_id,"received_at":at,"accepted_at":at,"source_namespace":key.source})).unwrap();
        let digest = records::hash(
            producer,
            "workflow-claim-facts",
            &json!([
                payload["customer"],
                payload["chain"],
                payload["status"],
                payload["activity"],
                payload["product_ref"],
                payload["policy_ref"],
                payload["fact_schema_ref"],
                payload["facts"],
                payload["links"],
                payload["binding_ref"],
                payload["invocation_refs"],
                payload["evidence_refs"]
            ]),
        )
        .unwrap();
        let claim=body::record(producer,&key,"workflow-work-claim",claim_id.clone(),json!({"schema":"ledger-workflow-work-claim/1","logical_store_id":key.logical_store_id,"scope":key.scope,"source_namespace":key.source,"business_operation_id":"fixed-operation","completion":"completion","work_ref":work.reference,"facts_digest":digest})).unwrap();
        let auth=body::document(producer,"workflow-economic-authority",json!({"schema":"ledger-workflow-economic-authority/8","operation":"work.accept","scope":key.scope,"customer":"customer-B","original_binding_ref":binding.reference,"source":key.source,"canonical_economic_namespace":key.source,"observed_at":at,"original_accepted_agreement_ref":accepted.reference,"source_slot":"support:worker","enrollment_ref":enrollment.reference,"current_state_ref":state.reference,"current_grant_ref":grant.reference,"original_grant_refs":[grant.reference],"actor":"example-operator"})).unwrap();
        let binding_value: Value = serde_json::from_slice(&binding.canonical_body).unwrap();
        let eval=body::document(producer,"workflow-evaluation",json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":work.reference,"policy_ref":p.root_policy_ref(),"compiled_plan_ref":p.compiled_plan().reference(),"compiler_ref":p.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":binding.reference,"fact_schema_ref":payload["fact_schema_ref"],"facts":payload["facts"],"context":binding_value["accepted_context"],"steps":[]})).unwrap();
        let permanent = work_keys::decision(producer, &claim_id).unwrap();
        let revision = body::semantic_id(
            producer,
            "wdrev_",
            "workflow-decision-revision",
            &json!([permanent, "1"]),
        )
        .unwrap();
        let decision=body::record(producer,&key,"workflow-decision",revision,json!({"schema":"ledger-workflow-decision-revision/1","decision_id":permanent,"revision":"1","subject_ref":work.reference,"kind":"work","transition":"initial","status":"active","original_terms_ref":binding.reference,"authority_ref":auth.reference,"evaluation_ref":eval.reference,"actions_appended":[],"active_action_refs":[],"dependency_refs":[],"operation_key":key,"accepted_at":at})).unwrap();

        let work_ref = work.reference.clone();
        let mut records = vec![
            enrollment,
            grant,
            state,
            role,
            catalog,
            inst,
            retail,
            binding,
            publication,
            terms,
            accepted,
            work,
            claim,
            auth,
            eval,
            decision,
        ];
        for document in p.source_documents() {
            records.push(
                super::super::emitted_record::ConstructedEnvelope::from_bytes(
                    serde_json::from_value(json!(document.reference())).unwrap(),
                    document.canonical_bytes().to_vec(),
                ),
            );
        }
        records.push(
            super::super::emitted_record::ConstructedEnvelope::from_bytes(
                serde_json::from_value(json!(p.compiled_plan().reference())).unwrap(),
                p.compiled_plan().canonical_bytes().to_vec(),
            ),
        );
        let views = records
            .iter()
            .map(|r| EconomicRecordView {
                reference: &r.reference,
                canonical_body: &r.canonical_body,
            })
            .collect::<Vec<_>>();
        let mut compile = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support =
            SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut compile)
                .unwrap();
        let next =
            compile_selected_publication(source(&support, &mut compile), &support, &mut compile)
                .unwrap();
        assert_ne!(next.root_policy_ref(), p.root_policy_ref());
        assert_ne!(next.product_ref(), p.product_ref());
        let mut next_binding = binding_value.clone();
        next_binding["agreement_id"] = json!("A2");
        next_binding["version"] = json!("2");
        next_binding["source_namespace"] = json!("urn:example:consumer");
        next_binding["policy_ref"] = json!(next.root_policy_ref());
        next_binding["product_ref"] = json!(next.product_ref());
        next_binding["compiled_plan_ref"] = json!(next.compiled_plan().reference());
        next_binding["accepted_context"] = json!({"fact_schema_ref":next.context_schema_ref(),"facts":{"support:eligibility":"support:eligible","support:prior-work":work_ref}});
        let next_binding_record =
            body::document(producer, "workflow-binding", next_binding.clone()).unwrap();
        let view = EconomicRecordView {
            reference: &next_binding_record.reference,
            canonical_body: &next_binding_record.canonical_body,
        };
        let mut check = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        validate_lifecycle_context_references(&next, &key, view, &views, at, &mut check).unwrap();
        // Saved reload repeats the same relation under A2's actual old plan.
        let plan = next.compiled_plan().canonical_bytes().to_vec();
        let plan_ref = next.compiled_plan().reference().clone();
        let mut reload = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let loaded = load_local_selected_execution(
            source(&support, &mut reload),
            &plan,
            &plan_ref,
            &mut reload,
        )
        .unwrap();
        validate_lifecycle_context_references(&loaded, &key, view, &views, at, &mut reload)
            .unwrap();
        let mut wrong_customer = next_binding.clone();
        wrong_customer["customer"] = json!("customer-C");
        let wrong_customer = body::document(producer, "workflow-binding", wrong_customer).unwrap();
        let mut check = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(validate_lifecycle_context_references(
            &next,
            &key,
            EconomicRecordView {
                reference: &wrong_customer.reference,
                canonical_body: &wrong_customer.canonical_body
            },
            &views,
            at,
            &mut check
        )
        .is_err());
        let mut foreign = key.clone();
        foreign.scope[0] = "other-company".into();
        let mut check = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(validate_lifecycle_context_references(
            &next, &foreign, view, &views, at, &mut check
        )
        .is_err());
        let no_publication = views
            .iter()
            .copied()
            .filter(|v| v.reference.kind != "workflow-publication")
            .collect::<Vec<_>>();
        let mut check = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(validate_lifecycle_context_references(
            &next,
            &key,
            view,
            &no_publication,
            at,
            &mut check
        )
        .is_err());
        // A same_binding field still refuses that exact cross-Binding endpoint.
        let mut check = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let a = check.arena().shared_handle();
        let m = check.meter_mut();
        let mut r = Records::borrowed_parent_selected(m, &a, &key, &views, &p).unwrap();
        let facts = json!({"support:quantity":"3","support:reference":work_ref});
        let schema = p.compiled_plan().canonical_value()["activity_table"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["activity"] == "support:fixed")
            .unwrap()["fact_schema_ref"]
            .clone();
        fields(
            m,
            &mut r,
            &key,
            &p,
            (&json!(next_binding_record.reference), &next_binding, at),
            (&schema, &facts),
        )
        .unwrap_err();
    }
}
