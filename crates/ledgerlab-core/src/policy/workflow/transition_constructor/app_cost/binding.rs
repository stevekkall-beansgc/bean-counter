//! Original app agreement/retail/resource associations. Completed custody and
//! original management semantics remain host obligations; current party heads
//! are independently selected, never copied from an old verification record.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    authority::{self, EconomicMode},
    body, cost_postings,
    data::*,
    inventory,
    record_body::RecordBody,
    records::Records,
    representation::InvocationToken,
    shape,
    source_authority::{self, OriginalSourceSubject, SourceSelection},
    working_value::owning_reference,
    CoreMeter,
};
use crate::{
    domain::Timestamp,
    validation_work::{self as w, RepresentationKind},
    Error, Result,
};
use serde_json::Value;
macro_rules! call {
    ($f:path,$m:expr,$a:expr,$b:expr) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn literal(m: &mut CoreMeter, v: &Value, k: &str, s: &str) -> Result<()> {
    let a = shape::text(m, v, k)?;
    if !shape::same_text(m, a, s)? {
        return Err(shape::fail("app original binding literal"));
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
) -> Result<(ExactRef, RecordBody)> {
    let reference = shape::reference(m, v)?;
    let record = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, record))
}
fn has(m: &mut CoreMeter, v: &Value, field: &str, want: &Value) -> Result<()> {
    let array = shape::array(m, v, field)?;
    if !shape::member(m, array, want)? {
        return Err(shape::fail("app original binding member"));
    }
    Ok(())
}
fn at(m: &mut CoreMeter, v: &Value, k: &str) -> Result<i64> {
    let raw = shape::text(m, v, k)?;
    Ok(Timestamp::parse_with_work(raw, m)?.micros())
}
/// The same original source/plan/body checks are shared by work and delayed
/// cost consumers. This returns DATA, never an accepted-origin certificate.
pub(super) fn resolve(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    payload: &Value,
    program: &SelectedProgram,
    mode: EconomicMode,
    source_slot: &str,
) -> Result<(ExactRef, authority::Terms, SourceSelection)> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app binding"))?;
    let key = cost_postings::operation_key(m, &arena, c)?;
    let (reference, binding) = load(
        m,
        r,
        c,
        &payload["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let frame = &c.authorities[0];
    if !shape::same_ref(m, &reference, &frame.binding_ref)? {
        return Err(shape::fail("app binding frame association"));
    }
    let accepted = r.get(
        m,
        &c.key,
        &frame.accepted_agreement_ref,
        "workflow-accepted-agreement",
        "ledger-workflow-accepted-agreement/1",
    )?;
    eq(m, &accepted["binding_ref"], &payload["binding_ref"])?;
    eq(m, &accepted["scope"], &key.value()["scope"])?;
    for field in ["agreement_id", "customer", "version"] {
        eq(m, &accepted[field], &binding[field])?;
    }
    for field in ["customer", "policy_ref", "product_ref"] {
        eq(m, &binding[field], &payload[field])?;
    }
    let (acceptance_ref, acceptance) = load(
        m,
        r,
        c,
        &accepted["terms_ref"],
        "workflow-acceptance-terms",
        "ledger-workflow-acceptance-terms/2",
    )?;
    eq(m, &acceptance["scope"], &key.value()["scope"])?;
    for field in ["agreement_id", "customer", "version"] {
        eq(m, &acceptance[field], &binding[field])?;
    }
    inventory::binding::validate_app(m, &binding, &acceptance)?;
    let bytes = r.canonical(m, &acceptance_ref)?;
    let digest =
        super::super::records::hash_parts(m, "workflow-acceptance-terms", &[bytes], false)?;
    literal(m, &accepted, "terms_digest", &digest)?;
    let owner = InvocationToken::new();
    inventory::publication::validate(m, r, c, &owner, program, &binding, &accepted, &acceptance)?;
    let activation = r.get(
        m,
        &c.key,
        &frame.activation_ref,
        "workflow-activation",
        "ledger-workflow-activation-transition/1",
    )?;
    shape::closed(
        m,
        &activation,
        &[
            "schema",
            "action",
            "head_transition_ref",
            "binding_ref",
            "effective_at",
            "commit_sequence",
            "accepted_at",
        ],
        &[],
    )?;
    eq(m, &activation["binding_ref"], &payload["binding_ref"])?;
    literal(m, &activation, "action", "activate")?;
    let (_, head) = load(
        m,
        r,
        c,
        &activation["head_transition_ref"],
        "workflow-head-transition",
        "ledger-workflow-head-transition/1",
    )?;
    eq(m, &head["scope"], &key.value()["scope"])?;
    for field in ["customer", "agreement_id"] {
        eq(m, &head[field], &binding[field])?;
    }
    eq(m, &head["accepted_binding_ref"], &payload["binding_ref"])?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    let effective = at(m, &activation, "effective_at")?;
    if w::compare(m, now, effective)?.is_lt() {
        return Err(shape::fail("app activation not effective"));
    }
    if mode == EconomicMode::Work {
        let start = at(m, &binding, "starts_at")?;
        if w::compare(m, now, start)?.is_lt() {
            return Err(shape::fail("app work before original binding start"));
        }
        if binding.get("ends_at").is_some() {
            let end = at(m, &binding, "ends_at")?;
            if !w::compare(m, now, end)?.is_lt() {
                return Err(shape::fail("app work outside original binding end"));
            }
        }
    }
    let (_, catalog) = load(
        m,
        r,
        c,
        &binding["slot_catalog_ref"],
        "workflow-slot-catalog",
        "ledger-workflow-slot-catalog/1",
    )?;
    let (_, instantiation) = load(
        m,
        r,
        c,
        &binding["slot_instantiation_ref"],
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let slots = shape::array(m, &binding, "bindings")?;
    if !w::compare(m, slots.len(), 1usize)?.is_eq() {
        return Err(Error::new(
            "UNSUPPORTED_COMPOSITION",
            "first app capability requires one retail binding",
        ));
    }
    let schema = if program.uses_app_cost_causal_recipe() {
        "ledger-workflow-slot-binding-terms/7"
    } else {
        "ledger-workflow-slot-binding-terms/6"
    };
    let (slot_terms_ref, slot_terms) = load(
        m,
        r,
        c,
        &slots[0]["binding_ref"],
        "workflow-slot-terms",
        schema,
    )?;
    eq(m, &slot_terms["scope"], &key.value()["scope"])?;
    for field in ["agreement_id", "customer"] {
        eq(m, &slot_terms[field], &binding[field])?;
    }
    eq(m, &slot_terms["slot"], &slots[0]["slot"])?;
    has(m, &slot_terms, "allowed_activities", &payload["activity"])?;
    let (role_ref, role) = load(
        m,
        r,
        c,
        &slot_terms["role_ref"],
        "workflow-roles",
        "ledger-workflow-role-assignment/1",
    )?;
    shape::closed(
        m,
        &role,
        &[
            "schema",
            "scope",
            "agreement_id",
            "customer",
            "role_slot",
            "provider",
            "cost_originator",
            "bearer",
            "payer",
            "beneficiary",
            "recipient",
        ],
        &[],
    )?;
    eq(m, &role["scope"], &key.value()["scope"])?;
    for field in ["agreement_id", "customer", "role_slot"] {
        eq(m, &role[field], &slot_terms[field])?;
    }
    has(m, &binding, "roles_refs", &slot_terms["role_ref"])?;
    let source = source_authority::source(
        m,
        r,
        c,
        program,
        OriginalSourceSubject {
            binding_ref: &reference,
            binding: &binding,
            terms: &slot_terms,
            payload,
        },
        source_slot,
        mode,
    )?;
    inventory::slots::validate(
        m,
        program,
        &binding,
        &catalog,
        &instantiation,
        &slot_terms,
        &role,
        &source.current,
    )?;
    if mode == EconomicMode::Work {
        future_source_shape(m, r, c, &binding, &slot_terms)?;
    }
    inventory::slots::validate_app_funding_projection(m, program, &binding, &slot_terms, &role)?;
    let funding = shape::array(m, &binding, "funding_legs")?;
    // All original funding evidence retains its exact purpose/subject/metadata;
    // its actual original authentication is completed by the host origin driver.
    for leg in funding {
        let metadata = arena.object_from_fields(
            &[
                ("mode", &leg["mode"]),
                ("payer", &leg["payer"]),
                ("resource_owner", &leg["resource_owner"]),
            ],
            m,
        )?;
        let subject = arena.object_from_fields(
            &[
                ("customer", &binding["customer"]),
                ("agreement_id", &binding["agreement_id"]),
            ],
            m,
        )?;
        for evidence in shape::array(m, leg, "evidence_refs")? {
            let (_, evidence) = load(
                m,
                r,
                c,
                evidence,
                "workflow-evidence",
                "ledger-workflow-retained-evidence/1",
            )?;
            inventory::evidence::retained(
                m,
                r,
                c,
                &owner,
                &evidence,
                "funding",
                subject.value(),
                metadata.value(),
                &binding,
                "accepted_at",
            )?;
        }
    }
    super::consent::original(
        program,
        m,
        r,
        c,
        &binding,
        &accepted,
        &acceptance_ref,
        &source.registered,
    )?;
    Ok((
        reference,
        authority::Terms {
            binding,
            accepted,
            activation,
            catalog,
            instantiation,
            slot_terms,
            slot_terms_ref,
            role_ref,
        },
        source,
    ))
}
/// Called by the SAME work constructor, replacing only its original binding
/// resolver for the code-owned app recipe. Pricing/actions remain unchanged.
pub(in crate::policy::workflow::transition_constructor) fn work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    payload: &Value,
    program: &SelectedProgram,
    source_slot: &str,
) -> Result<authority::Operands> {
    let (binding_ref, terms, source) =
        resolve(m, r, c, payload, program, EconomicMode::Work, source_slot)?;
    super::timing::prework(m, c, program, payload)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app work authority"))?;
    let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-economic-authority/7","old_schema":"ledger-workflow-economic-authority/6","operation":"work.accept","rights":["read","submit"],"empty":[]}"#,RepresentationKind::SourceDefinition)?;
    let key = cost_postings::operation_key(m, &arena, c)?;
    let actor = arena.string_value(&c.actor, m)?;
    let source_name = arena.string_value(&c.key.source, m)?;
    let namespace = arena.string_value(&c.canonical_economic_namespace, m)?;
    let mode = arena.string_value(&c.installation_mode, m)?;
    let at = arena.string_value(&c.accepted_at, m)?;
    let slot = arena.string_value(source_slot, m)?;
    let frame = &c.authorities[0];
    let enrollment = owning_reference(m, &arena, &frame.enrollment_ref)?;
    let current = owning_reference(m, &arena, &frame.current_state_ref)?;
    let grant = owning_reference(m, &arena, &frame.current_grant_ref)?;
    let binding = owning_reference(m, &arena, &binding_ref)?;
    let accepted = owning_reference(m, &arena, &frame.accepted_agreement_ref)?;
    let original_refs = super::authority::grant_refs(m, &source.original_grant_refs)?;
    let value = arena.object_from_fields(
        &[
            (
                "schema",
                if program.uses_app_cost_causal_recipe() {
                    &constants.value()["schema"]
                } else {
                    &constants.value()["old_schema"]
                },
            ),
            ("operation", &constants.value()["operation"]),
            ("actor", actor.value()),
            ("source", source_name.value()),
            ("scope", &key.value()["scope"]),
            ("customer", &payload["customer"]),
            ("canonical_economic_namespace", namespace.value()),
            ("installation_mode", mode.value()),
            ("enrollment_ref", enrollment.value()),
            ("current_state_ref", current.value()),
            ("current_grant_ref", grant.value()),
            ("original_binding_ref", binding.value()),
            ("original_accepted_agreement_ref", accepted.value()),
            ("original_grant_refs", original_refs.value()),
            ("source_slot", slot.value()),
            ("rights", &constants.value()["rights"]),
            ("evidence_refs", &payload["evidence_refs"]),
            ("verified_fact_refs", &constants.value()["empty"]),
            ("observed_at", at.value()),
            ("causal_closure_checks", &constants.value()["empty"]),
            ("app_observation_refs", &constants.value()["empty"]),
            ("app_final_evidence_refs", &constants.value()["empty"]),
            ("app_cost_revision_refs", &constants.value()["empty"]),
            ("app_cost_closure_checks", &constants.value()["empty"]),
        ],
        m,
    )?;
    let value = if program.uses_app_cost_causal_recipe() {
        super::super::working_value::WorkingValue::Owned(value)
            .with_fields(
                m,
                &[(
                    "app_pending_cost_closure_checks",
                    &constants.value()["empty"],
                )],
            )?
            .into_owned()?
    } else {
        value
    };
    let value = super::authority::attach_source_observation(m, program, frame, value)?;
    let authority = body::document_borrowed(m, "workflow-economic-authority", value.value())?;
    let mut introduced = Vec::new();
    introduced
        .try_reserve_exact(2)
        .map_err(|_| shape::fail("app pending source vector"))?;
    for item in &c.source_observation_inputs {
        m.visit("app.work_pending_source_exact")?;
        if !shape::same_ref(m, &item.reference, frame.required_observation_ref()?)?
            && !shape::same_ref(m, &item.reference, source.required_verifier_ref()?)?
        {
            return Err(shape::fail("extra app work source observation"));
        }
        let raw = r.canonical(m, &item.reference)?;
        m.new_body(raw.len())?;
        let bytes = m.copy_owned_representation(&arena, raw, RepresentationKind::SourceClosure)?;
        introduced.push(
            super::super::emitted_record::ConstructedEnvelope::from_owned(
                item.reference.clone(),
                bytes,
                &arena,
                m,
                RepresentationKind::SourceClosure,
            )?,
        );
    }
    Ok(authority::Operands {
        terms,
        authority,
        introduced,
    })
}

/// The same owning app resolver serves original work/outcome/root operations.
/// Historical subject identity is bound to the original payload, independently
/// of today's actual nominated source and economic namespace.
pub(in crate::policy::workflow::transition_constructor) fn resolved(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    payload: &Value,
    program: &SelectedProgram,
    mode: EconomicMode,
    source_slot: &str,
    verified_facts: &[ExactRef],
    original_namespace: &str,
) -> Result<authority::ResolvedOperands> {
    let (binding_ref, terms, source) = resolve(m, r, c, payload, program, mode, source_slot)?;
    let original = shape::text(m, &terms.binding, "source_namespace")?;
    if !shape::same_text(m, original, original_namespace)? {
        return Err(shape::fail("app historical subject namespace"));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app operation authority"))?;
    let (operation, rights) = match mode {
        EconomicMode::QuantityCorrect => ("quantity.correct", br#"["correct","read"]"#.as_slice()),
        EconomicMode::WorkReverse => ("work.reverse", br#"["correct","read"]"#.as_slice()),
        EconomicMode::OutcomeSubmit => ("outcome.submit", br#"["read","submit"]"#.as_slice()),
        EconomicMode::EvidenceRetain => ("evidence.retain", br#"["read","submit"]"#.as_slice()),
        EconomicMode::OutcomeCorrect => ("outcome.correct", br#"["correct","read"]"#.as_slice()),
        EconomicMode::OutcomeClose => ("outcome.close", br#"["read","submit"]"#.as_slice()),
        EconomicMode::DeadlineClose => ("outcome.close", br#"["close","read"]"#.as_slice()),
        _ => return Err(shape::fail("app generic economic operation")),
    };
    let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-economic-authority/7","old_schema":"ledger-workflow-economic-authority/6","empty":[]}"#,RepresentationKind::SourceDefinition)?;
    let rights =
        m.parse_owned_representation(&arena, rights, RepresentationKind::SourceDefinition)?;
    let operation = arena.string_value(operation, m)?;
    let actor = arena.string_value(&c.actor, m)?;
    let source_name = arena.string_value(&c.key.source, m)?;
    let namespace = arena.string_value(&c.canonical_economic_namespace, m)?;
    let mode = arena.string_value(&c.installation_mode, m)?;
    let at = arena.string_value(&c.accepted_at, m)?;
    let slot = arena.string_value(source_slot, m)?;
    let key = cost_postings::operation_key(m, &arena, c)?;
    let frame = &c.authorities[0];
    let enrollment = owning_reference(m, &arena, &frame.enrollment_ref)?;
    let current = owning_reference(m, &arena, &frame.current_state_ref)?;
    let grant = owning_reference(m, &arena, &frame.current_grant_ref)?;
    let binding = owning_reference(m, &arena, &binding_ref)?;
    let accepted = owning_reference(m, &arena, &frame.accepted_agreement_ref)?;
    let original_refs = super::authority::grant_refs(m, &source.original_grant_refs)?;
    crate::validation_work::ValidationWork::arithmetic(
        m,
        crate::validation_work::ArithmeticOperation::Multiply,
        &[
            &verified_facts.len().into(),
            &std::mem::size_of::<&ExactRef>().into(),
        ],
    )?;
    let fact_metadata = verified_facts
        .len()
        .checked_mul(std::mem::size_of::<&ExactRef>())
        .ok_or_else(|| shape::fail("app authority verified fact metadata product"))?;
    let _fact_lease = arena.reserve_external_payload(fact_metadata, m)?;
    let mut facts = Vec::new();
    facts
        .try_reserve_exact(verified_facts.len())
        .map_err(|_| shape::fail("app authority facts"))?;
    for fact in verified_facts {
        m.visit("app.authority_verified_fact")?;
        facts.push(fact);
    }
    let facts = super::retention::refs(m, &arena, &facts)?;
    let value = arena.object_from_fields(
        &[
            (
                "schema",
                if program.uses_app_cost_causal_recipe() {
                    &constants.value()["schema"]
                } else {
                    &constants.value()["old_schema"]
                },
            ),
            ("operation", operation.value()),
            ("actor", actor.value()),
            ("source", source_name.value()),
            ("scope", &key.value()["scope"]),
            ("customer", &payload["customer"]),
            ("canonical_economic_namespace", namespace.value()),
            ("installation_mode", mode.value()),
            ("enrollment_ref", enrollment.value()),
            ("current_state_ref", current.value()),
            ("current_grant_ref", grant.value()),
            ("original_binding_ref", binding.value()),
            ("original_accepted_agreement_ref", accepted.value()),
            ("original_grant_refs", original_refs.value()),
            ("source_slot", slot.value()),
            ("rights", rights.value()),
            ("evidence_refs", &payload["evidence_refs"]),
            ("verified_fact_refs", facts.value()),
            ("observed_at", at.value()),
            ("causal_closure_checks", &constants.value()["empty"]),
            ("app_observation_refs", &constants.value()["empty"]),
            ("app_final_evidence_refs", &constants.value()["empty"]),
            ("app_cost_revision_refs", &constants.value()["empty"]),
            ("app_cost_closure_checks", &constants.value()["empty"]),
        ],
        m,
    )?;
    let value = if program.uses_app_cost_causal_recipe() {
        super::super::working_value::WorkingValue::Owned(value)
            .with_fields(
                m,
                &[(
                    "app_pending_cost_closure_checks",
                    &constants.value()["empty"],
                )],
            )?
            .into_owned()?
    } else {
        value
    };
    let value = super::authority::attach_source_observation(m, program, frame, value)?;
    let headers = crate::validation_work::usize_add(
        m,
        2 * std::mem::size_of::<super::super::emitted_record::ConstructedEnvelope>(),
        2 * (64 + 128 + 71),
    )?;
    let _introduced_lease = arena.reserve_external_payload(headers, m)?;
    let mut introduced = Vec::new();
    introduced
        .try_reserve_exact(2)
        .map_err(|_| shape::fail("app operation pending source vector"))?;
    for item in &c.source_observation_inputs {
        m.visit("app.operation_pending_source_exact")?;
        if !shape::same_ref(m, &item.reference, frame.required_observation_ref()?)?
            && !shape::same_ref(m, &item.reference, source.required_verifier_ref()?)?
        {
            return Err(shape::fail("extra app operation source observation"));
        }
        let raw = r.canonical(m, &item.reference)?;
        m.new_body(raw.len())?;
        let bytes = m.copy_owned_representation(&arena, raw, RepresentationKind::SourceClosure)?;
        introduced.push(
            super::super::emitted_record::ConstructedEnvelope::from_owned(
                item.reference.clone(),
                bytes,
                &arena,
                m,
                RepresentationKind::SourceClosure,
            )?,
        );
    }
    Ok(authority::ResolvedOperands::owning(
        terms,
        introduced,
        value,
        _introduced_lease,
    ))
}

/// Temporary fail-closed guard while the independent multi-source/principal
/// caller is unfinished. This is NOT full source-composition support or a new
/// policy restriction. Refuse before creating any work/dependency promise;
/// later consumers still recheck current grants. The full implementation must
/// supply each independently authenticated dependent frame, never substitute
/// the worker namespace or root actor for another principal.
fn future_source_shape(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    binding: &Value,
    terms: &Value,
) -> Result<()> {
    for nomination in shape::array(m, binding, "source_slots")? {
        m.visit("app.promise_source_nomination")?;
        for source in shape::array(m, nomination, "sources")? {
            let actual = source
                .as_str()
                .ok_or_else(|| shape::fail("app nominated source scalar"))?;
            if !shape::same_text(m, actual, &c.key.source)? {
                return Err(Error::new("UNSUPPORTED_COMPOSITION","this app capability requires each promised slot to nominate the same authenticated source"));
            }
        }
    }
    let frame = &c.authorities[0];
    let enrollment = owning_reference(
        m,
        &m.representation_arena()
            .ok_or_else(|| shape::fail("app original source shape owner"))?,
        &frame.enrollment_ref,
    )?;
    for value in shape::array(m, terms, "source_grant_refs")? {
        m.visit("app.promise_original_principal")?;
        let reference = shape::reference(m, value)?;
        let grant = r.get(
            m,
            &c.key,
            &reference,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        let source = shape::text(m, &grant, "source")?;
        let principal = shape::text(m, &grant, "principal_id")?;
        if !shape::same_text(m, source, &c.key.source)?
            || !shape::same_text(m, principal, &c.actor)?
            || !shape::same(m, &grant["original_ceiling_ref"], enrollment.value())?
        {
            return Err(Error::new("UNSUPPORTED_COMPOSITION","this app capability cannot promise a dependent requiring another original source/principal frame"));
        }
    }
    Ok(())
}
