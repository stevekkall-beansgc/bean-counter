//! Actual app observation/final-cost retention producer. Fresh and original
//! replay call the same validator and record construction. Returned DATA does
//! not replace held source/party/original-prefix authentication or atomic keys.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    authority::EconomicMode,
    body,
    candidate::{ClaimEffect, PermanentClaimKey},
    data::*,
    emitted_record::ConstructedEnvelope,
    records::Records,
    retention::{RetainedAssertionData, RetainedAssertionEnvelope},
    shape,
    source_authority::SourceSelection,
    working_value::owning_reference,
    CoreMeter,
};
use super::{evidence, original};
use crate::{
    canonical,
    canonical::audit_arena::{AuditArena, AuditedValue},
    domain::Timestamp,
    validation_work::{self as w, RepresentationKind, ValidationWork},
    Error, Result,
};
use serde_json::Value;
macro_rules! call {
    ($f:path,$m:expr,$a:expr) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn mul(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, w::ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("app retention metadata product"))
}
pub(in crate::policy::workflow::transition_constructor) fn refs(
    m: &mut CoreMeter,
    arena: &AuditArena,
    items: &[&ExactRef],
) -> Result<AuditedValue> {
    let slot = w::usize_add(
        m,
        std::mem::size_of::<AuditedValue>(),
        std::mem::size_of::<&Value>(),
    )?;
    let size = mul(m, items.len(), slot)?;
    let size = w::usize_add(m, size, 2 * std::mem::size_of::<Vec<AuditedValue>>())?;
    let _metadata = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(items.len())
        .map_err(|_| shape::fail("app reference set allocation"))?;
    for item in items {
        m.visit("app.retention_reference_member")?;
        values.push(owning_reference(m, arena, item)?);
    }
    let mut borrowed = Vec::new();
    borrowed
        .try_reserve_exact(items.len())
        .map_err(|_| shape::fail("app reference borrow allocation"))?;
    for item in &values {
        m.visit("app.retention_reference_borrow")?;
        borrowed.push(item.value());
    }
    let array = arena.array_from_values(&borrowed, m)?;
    let mut object = arena.object_from_fields(&[("refs", array.value())], m)?;
    super::super::intent::sort_owned_set(&mut object, "refs", false, m, arena)?;
    arena.clone_value(&object.value()["refs"], m)
}
fn pending(
    m: &mut CoreMeter,
    r: &Records,
    c: &DraftContextData,
    source: &SourceSelection,
    output: &mut Vec<RetainedAssertionEnvelope>,
) -> Result<()> {
    for row in &c.source_observation_inputs {
        m.visit("app.pending_source_exact_set")?;
        if !shape::same_ref(m, &row.reference, source.required_verifier_ref()?)?
            && !shape::same_ref(
                m,
                &row.reference,
                c.authorities[0].required_observation_ref()?,
            )?
        {
            return Err(shape::fail("extra pending app source observation"));
        }
    }
    if source.observation.is_none() {
        if !c.source_observation_inputs.is_empty() {
            return Err(shape::fail("local retention pending audit input"));
        }
        return Ok(());
    }
    for expected in [
        source.required_verifier_ref()?,
        c.authorities[0].required_observation_ref()?,
    ] {
        let mut found = false;
        for input in &c.source_observation_inputs {
            m.visit("app.pending_source_representation")?;
            if shape::same_ref(m, &input.reference, expected)? {
                if found {
                    return Err(shape::fail("duplicate pending app source representation"));
                }
                found = true;
            }
        }
        if found {
            let raw = r.canonical(m, expected)?;
            m.new_body(raw.len())?;
            let arena = m
                .representation_arena()
                .ok_or_else(|| shape::fail("owning pending app observation"))?;
            let bytes =
                m.copy_owned_representation(&arena, raw, RepresentationKind::SourceClosure)?;
            let envelope = ConstructedEnvelope::from_owned(
                reference(m, expected)?,
                bytes,
                &arena,
                m,
                RepresentationKind::SourceClosure,
            )?;
            output.push(RetainedAssertionEnvelope { inner: envelope });
        }
    }
    Ok(())
}
struct AppRetention<'raw> {
    normalized: crate::canonical::audit_arena::AuditedBytes,
    introduced: Vec<RetainedAssertionEnvelope>,
    authority_ref: ExactRef,
    assertion_ref: ExactRef,
    assertion_index: usize,
    verification_ref: ExactRef,
    reused_assertion: Option<RetainedAssertionEnvelope>,
    records: Records<'raw, 'raw>,
    claims: Vec<ClaimEffect>,
    terms: super::super::authority::Terms,
    work: super::super::record_body::RecordBody,
    ownership: crate::canonical::audit_arena::ExternalBufferLease,
    original_ownership: crate::canonical::audit_arena::ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) fn execute<'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    parsed: AuditedValue,
    inputs: &'raw [EconomicRecordView<'raw>],
    state: &EconomicStateData<'_>,
    arena: &AuditArena,
    m: &mut CoreMeter,
) -> Result<RetainedAssertionData> {
    let output = produce(program, c, parsed, inputs, state, arena, m)?;
    let AppRetention {
        normalized,
        introduced,
        authority_ref,
        assertion_ref,
        assertion_index,
        verification_ref,
        reused_assertion,
        records,
        claims,
        ownership,
        ..
    } = output;
    Ok(RetainedAssertionData {
        normalized,
        introduced,
        authority_ref,
        assertion_ref,
        assertion_index,
        reused_assertion,
        verification_ref,
        _consumed: records.into_consumed_owned(),
        claims,
        _output_ownership: Some(ownership),
    })
}
/// Same producer under the complete factory's ALREADY OPEN economic child.
/// Owning records are moved, never recreated from consumed Ref metadata.
pub(in crate::policy::workflow::transition_constructor) fn execute_complete<'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    parsed: AuditedValue,
    inputs: &'raw [EconomicRecordView<'raw>],
    state: &EconomicStateData<'_>,
    arena: &AuditArena,
    m: &mut CoreMeter,
) -> Result<Construction<'raw, 'raw>> {
    let output = produce(program, c, parsed, inputs, state, arena, m)?;
    let AppRetention {
        normalized,
        mut introduced,
        records,
        claims,
        terms,
        work,
        reused_assertion,
        ownership,
        original_ownership,
        ..
    } = output;
    let n = mul(
        m,
        introduced.len(),
        std::mem::size_of::<ConstructedEnvelope>(),
    )?;
    let n = w::usize_add(
        m,
        n,
        std::mem::size_of::<Construction>() + std::mem::size_of::<AppEffects>(),
    )?;
    let transfer = arena.reserve_external_payload(n, m)?;
    m.write_bytes(n)?;
    let mut envelopes = Vec::new();
    envelopes
        .try_reserve_exact(introduced.len())
        .map_err(|_| shape::fail("complete app retention output transfer"))?;
    for row in introduced.drain(..) {
        m.visit("app.retention_move_same_envelope")?;
        m.scan_bytes(std::mem::size_of::<ConstructedEnvelope>())?;
        envelopes.push(row.inner);
    }
    let normalized_payload =
        super::super::emitted_record::EncodedBody::owned(arena, normalized, m)?;
    Ok(Construction {
        economic: EconomicConstruction {
            terms,
            original_work: OriginalWorkPayload::Retained(work),
        },
        original_reservations: None,
        financial_reservations: None,
        normalized_payload,
        new_envelopes: envelopes,
        reused_result: reused_assertion.map(|x| x.inner.reference),
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
        app_effects: Some(AppEffects {
            claims,
            heads: Vec::new(),
        }),
        _app_causal: None,
        current_capacity: None,
        candidate_heads: None,
        candidate_introductions: None,
        financial_observation: None,
        funding_effects: None,
        control_owners: [
            Some(ownership),
            Some(original_ownership),
            Some(transfer),
            None,
            None,
        ],
    })
}
fn produce<'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    mut parsed: AuditedValue,
    inputs: &'raw [EconomicRecordView<'raw>],
    state: &EconomicStateData<'_>,
    arena: &AuditArena,
    m: &mut CoreMeter,
) -> Result<AppRetention<'raw>> {
    if !program.uses_app_owned_cost_recipe() {
        return Err(Error::new(
            "ORIGINAL_VERIFIER_UNAVAILABLE",
            "app retention original recipe",
        ));
    }
    let schema = shape::text(m, parsed.value(), "schema")?;
    let observe = shape::same_text(m, schema, "ledger-workflow-app-observe/1")?;
    if !observe && !shape::same_text(m, schema, "ledger-workflow-app-cost-attest/1")? {
        return Err(shape::fail("app retention payload tuple"));
    }
    shape::closed(m, parsed.value(), &["schema", "attestation"], &[])?;
    let normalized = evidence::normalize(m, &parsed.value()["attestation"])?;
    parsed.replace_existing_owned("attestation", normalized, m)?;
    let att = &parsed.value()["attestation"];
    // Output vectors, key copies and temporary exact references are admitted
    // BEFORE producers. JSON bodies/encoded bytes retain independent owners.
    let envelopes = mul(m, 5, std::mem::size_of::<RetainedAssertionEnvelope>())?;
    let header = w::usize_add(
        m,
        std::mem::size_of::<RetainedAssertionData>() + std::mem::size_of::<AppRetention>(),
        envelopes,
    )?;
    let claims = mul(m, 4, std::mem::size_of::<ClaimEffect>())?;
    let header = w::usize_add(m, header, claims)?;
    let header = w::usize_add(m, header, 13 * (64 + 128 + 71))?;
    let stores = mul(m, 4, c.key.logical_store_id.len())?;
    let mut bytes = w::usize_add(m, header, stores)?;
    for value in [&c.key.scope[0], &c.key.scope[1], &c.key.source] {
        bytes = w::usize_add(m, bytes, value.len())?;
    }
    let assertion_id = shape::text(m, att, "attestation_id")?;
    bytes = w::usize_add(m, bytes, assertion_id.len())?;
    let subject = &att["subject"];
    for value in [
        &subject["app_call"]["source_namespace"],
        &subject["app_call"]["call_id"],
        &subject["component_slot"],
    ] {
        if let Some(value) = value.as_str() {
            bytes = w::usize_add(m, bytes, value.len())?;
        }
    }
    if let Some(provider) = att.get("provider_key") {
        for field in ["provider", "account", "request_id"] {
            if let Some(value) = provider[field].as_str() {
                bytes = w::usize_add(m, bytes, value.len())?;
            }
        }
    }
    let ownership = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(header)?;
    let work_ref = shape::reference(m, &subject["original_work_ref"])?;
    let slot = shape::text(m, subject, "component_slot")?;
    let mut r = Records::borrowed_app(m, arena, &c.key, inputs, &c.source_observation_inputs)?;
    r.install_initial_source_outputs(m, state.initial_source_correspondences)?;
    let original = original::select(m, &mut r, c, program, &work_ref, slot, state)?;
    let mut observation = None;
    let source_slot = if observe {
        evidence::observation(m, c, program, &original, att)?;
        shape::text(m, original.declaration, "work_source_slot")?
    } else {
        let observation_ref = shape::reference(m, &att["observation_ref"])?;
        let observed = r.get(
            m,
            &c.key,
            &observation_ref,
            "workflow-fact-attestation",
            "ledger-workflow-app-observation/1",
        )?;
        evidence::observation(m, c, program, &original, &observed)?;
        evidence::final_amount(m, c, program, &original, &observed, att)?;
        let revision = crate::policy::workflow::facts::counter_with_work(
            shape::text(m, att, "cost_revision")?,
            m,
        )?;
        if w::compare(m, revision, 1u64)?.is_gt() {
            let previous = shape::reference(m, &att["prior_final_evidence_ref"])?;
            let old = r.get(
                m,
                &c.key,
                &previous,
                "workflow-fact-attestation",
                "ledger-workflow-app-final-cost/1",
            )?;
            eq(m, &old["subject"], &att["subject"])?;
            eq(m, &old["observation_ref"], &att["observation_ref"])?;
            let prior = crate::policy::workflow::facts::counter_with_work(
                shape::text(m, &old, "cost_revision")?,
                m,
            )?;
            let next = w::usize_add(
                m,
                usize::try_from(prior).map_err(|_| shape::fail("app evidence revision width"))?,
                1,
            )?;
            if !w::compare(m, revision, next as u64)?.is_eq() {
                return Err(shape::fail("app final evidence exact next lineage"));
            }
        }
        observation = Some(observation_ref);
        let first = w::compare(m, revision, 1u64)?.is_eq();
        shape::text(
            m,
            original.declaration,
            if first {
                "cost_source_slot"
            } else {
                "correction_source_slot"
            },
        )?
    };
    let source = original.source(
        m,
        &mut r,
        c,
        program,
        source_slot,
        EconomicMode::EvidenceRetain,
    )?;
    let declared_source = shape::text(m, att, "source")?;
    if !shape::same_text(m, declared_source, &c.key.source)? {
        return Err(shape::fail("app retention authenticated source"));
    }
    let nominations = shape::array(m, &original.terms.binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", source_slot)?;
    let kinds = shape::array(m, nomination, "evidence_kinds")?;
    if !shape::member(m, kinds, &att["evidence_kind"])? {
        return Err(shape::fail("app original evidence kind nomination"));
    }
    let received = Timestamp::parse_with_work(&c.received_at, m)?.micros();
    let accepted = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    let issued = Timestamp::parse_with_work(
        shape::text(m, att, if observe { "observed_at" } else { "issued_at" })?,
        m,
    )?
    .micros();
    let starts =
        Timestamp::parse_with_work(shape::text(m, &source.current, "starts_at")?, m)?.micros();
    let ends = Timestamp::parse_with_work(shape::text(m, &source.current, "ends_at")?, m)?.micros();
    if w::compare(m, issued, received)?.is_gt()
        || w::compare(m, received, accepted)?.is_gt()
        || w::compare(m, received, starts)?.is_lt()
        || !w::compare(m, received, ends)?.is_lt()
    {
        return Err(shape::fail("app retention trusted source time"));
    }
    let assertion = body::document_borrowed(m, "workflow-fact-attestation", att)?;
    let reuse = super::super::retention::existing_assertion(m, &mut r, c, state, &assertion)?;
    let assertion_value = owning_reference(m, arena, &assertion.reference)?;
    let observation_refs = if observe {
        refs(m, arena, &[&assertion.reference])?
    } else {
        refs(m, arena, &[observation.as_ref().unwrap()])?
    };
    let final_refs = if observe {
        refs(m, arena, &[])?
    } else {
        refs(m, arena, &[&assertion.reference])?
    };
    let constants=m.parse_owned_representation(arena,br#"{"schema":"ledger-workflow-economic-authority/6","new_schema":"ledger-workflow-economic-authority/7","operation":"evidence.retain","rights":["read","submit"],"empty":[],"verification_schema":"ledger-workflow-evidence-verification/1","verification_primitive":"authenticated-nominated-source-retention/1"}"#,RepresentationKind::SourceDefinition)?;
    let actor = arena.string_value(&c.actor, m)?;
    let source_name = arena.string_value(&c.key.source, m)?;
    let namespace = arena.string_value(&c.canonical_economic_namespace, m)?;
    let mode = arena.string_value(&c.installation_mode, m)?;
    let observed_at = arena.string_value(&c.accepted_at, m)?;
    let slot = arena.string_value(source_slot, m)?;
    let frame = &c.authorities[0];
    let enrollment = owning_reference(m, arena, &frame.enrollment_ref)?;
    let current_state = owning_reference(m, arena, &frame.current_state_ref)?;
    let current_grant = owning_reference(m, arena, &frame.current_grant_ref)?;
    let binding = owning_reference(m, arena, &original.binding_ref)?;
    let accepted_agreement = owning_reference(m, arena, &frame.accepted_agreement_ref)?;
    let per = mul(
        m,
        frame.original_grant_refs.len(),
        std::mem::size_of::<&ExactRef>(),
    )?;
    let _originals = arena.reserve_external_payload(per, m)?;
    let mut original_grants = Vec::new();
    original_grants
        .try_reserve_exact(frame.original_grant_refs.len())
        .map_err(|_| shape::fail("app original grant borrow allocation"))?;
    for reference in &frame.original_grant_refs {
        m.visit("app.original_grant_borrow")?;
        original_grants.push(reference);
    }
    let original_grants = refs(m, arena, &original_grants)?;
    let authority = arena.object_from_fields(
        &[
            (
                "schema",
                if program.uses_app_cost_causal_recipe() {
                    &constants.value()["new_schema"]
                } else {
                    &constants.value()["schema"]
                },
            ),
            ("operation", &constants.value()["operation"]),
            ("actor", actor.value()),
            ("source", source_name.value()),
            ("scope", &original.work["key"]["scope"]),
            ("customer", &original.terms.binding["customer"]),
            ("canonical_economic_namespace", namespace.value()),
            ("installation_mode", mode.value()),
            ("enrollment_ref", enrollment.value()),
            ("current_state_ref", current_state.value()),
            ("current_grant_ref", current_grant.value()),
            ("original_binding_ref", binding.value()),
            (
                "original_accepted_agreement_ref",
                accepted_agreement.value(),
            ),
            ("original_grant_refs", original_grants.value()),
            ("source_slot", slot.value()),
            ("rights", &constants.value()["rights"]),
            ("evidence_refs", &constants.value()["empty"]),
            ("verified_fact_refs", &constants.value()["empty"]),
            ("observed_at", observed_at.value()),
            ("causal_closure_checks", &constants.value()["empty"]),
            ("app_observation_refs", observation_refs.value()),
            ("app_final_evidence_refs", final_refs.value()),
            ("app_cost_revision_refs", &constants.value()["empty"]),
            ("app_cost_closure_checks", &constants.value()["empty"]),
        ],
        m,
    )?;
    let authority = if program.uses_app_cost_causal_recipe() {
        super::super::working_value::WorkingValue::Owned(authority)
            .with_fields(
                m,
                &[(
                    "app_pending_cost_closure_checks",
                    &constants.value()["empty"],
                )],
            )?
            .into_owned()?
    } else {
        authority
    };
    let authority = super::authority::attach_source_observation(m, program, frame, authority)?;
    let authority = body::document_borrowed(m, "workflow-economic-authority", authority.value())?;
    let subject_refs = if let Some(observation) = &observation {
        refs(
            m,
            arena,
            &[&original.work_ref, &original.binding_ref, observation],
        )?
    } else {
        refs(m, arena, &[&original.work_ref, &original.binding_ref])?
    };
    let canonical = m.canonical_owned_representation(arena, att, RepresentationKind::Record)?;
    let raw_sha =
        m.hash_owned_representation(arena, canonical.bytes(), RepresentationKind::Record)?;
    let _sha = arena.reserve_external_payload(64, m)?;
    let raw_sha = canonical::hex_with_work(&raw_sha, m)?;
    let raw_sha = arena.string_value(&raw_sha, m)?;
    let digest = arena.string_value(&assertion.reference.digest, m)?;
    let trusted = arena.string_value(&c.received_at, m)?;
    let verification = arena.object_from_fields(
        &[
            ("schema", &constants.value()["verification_schema"]),
            ("attestation_ref", assertion_value.value()),
            ("attestation_raw_sha256", raw_sha.value()),
            ("attestation_digest", digest.value()),
            ("verified_source", source_name.value()),
            (
                "verification_primitive",
                &constants.value()["verification_primitive"],
            ),
            ("source_grant_ref", current_grant.value()),
            ("source_grant_revision", &source.current["revision"]),
            ("authenticated_principal_ref", enrollment.value()),
            ("trusted_received_at", trusted.value()),
            ("subject_binding_refs", subject_refs.value()),
            ("verification_evidence_refs", &constants.value()["empty"]),
        ],
        m,
    )?;
    let verification =
        body::document_borrowed(m, "workflow-evidence-verification", verification.value())?;
    let mut claims = Vec::new();
    claims
        .try_reserve_exact(4)
        .map_err(|_| shape::fail("app attribution claim allocation"))?;
    // Shared fact-attestation identity is independent of the additional app
    // call/component/provider keys. It also applies to final evidence.
    claims.push(ClaimEffect::derived(
        PermanentClaimKey::Assertion {
            logical_store_id: copy(m, &c.key.logical_store_id)?,
            scope: [copy(m, &c.key.scope[0])?, copy(m, &c.key.scope[1])?],
            verified_source: copy(m, &c.key.source)?,
            attestation_id: copy(m, assertion_id)?,
        },
        reference(m, &assertion.reference)?,
    ));
    if observe {
        if let Some(app) = state.app {
            for prior in app.observation_refs {
                let old = r.get(
                    m,
                    &c.key,
                    prior,
                    "workflow-fact-attestation",
                    "ledger-workflow-app-observation/1",
                )?;
                let same_call =
                    shape::same(m, &old["subject"]["app_call"], &att["subject"]["app_call"])?;
                let same_component = shape::same(
                    m,
                    &old["subject"]["original_work_ref"],
                    &att["subject"]["original_work_ref"],
                )? && shape::same(
                    m,
                    &old["subject"]["component_slot"],
                    &att["subject"]["component_slot"],
                )?;
                let same_provider = match (old.get("provider_key"), att.get("provider_key")) {
                    (Some(a), Some(b)) => shape::same(m, a, b)?,
                    _ => false,
                };
                if (same_call || same_component || same_provider)
                    && !shape::same_ref(m, prior, &assertion.reference)?
                {
                    return Err(Error::new(
                        "IDENTITY_CONFLICT",
                        "app call/component/provider attribution already retained",
                    ));
                }
            }
        }
        let call = &att["subject"]["app_call"];
        claims.push(ClaimEffect::derived(
            PermanentClaimKey::AppCall {
                logical_store_id: copy(m, &c.key.logical_store_id)?,
                source_namespace: call!(copy, m, shape::text(m, call, "source_namespace")?)?,
                call_id: call!(copy, m, shape::text(m, call, "call_id")?)?,
            },
            reference(m, &assertion.reference)?,
        ));
        claims.push(ClaimEffect::derived(
            PermanentClaimKey::AppComponentObservation {
                logical_store_id: copy(m, &c.key.logical_store_id)?,
                original_work_ref: reference(m, &original.work_ref)?,
                component_slot: copy(m, &original.component_slot)?,
            },
            reference(m, &assertion.reference)?,
        ));
        if let Some(provider) = att.get("provider_key") {
            claims.push(ClaimEffect::derived(
                PermanentClaimKey::AppProviderCall {
                    logical_store_id: copy(m, &c.key.logical_store_id)?,
                    provider: call!(copy, m, shape::text(m, provider, "provider")?)?,
                    account: call!(copy, m, shape::text(m, provider, "account")?)?,
                    request_id: call!(copy, m, shape::text(m, provider, "request_id")?)?,
                },
                reference(m, &assertion.reference)?,
            ));
        }
    }
    let mut introduced = Vec::new();
    introduced
        .try_reserve_exact(5)
        .map_err(|_| shape::fail("app retention output allocation"))?;
    pending(m, &r, c, &source, &mut introduced)?;
    let authority_ref = reference(m, &authority.reference)?;
    let assertion_ref = reference(m, &assertion.reference)?;
    let verification_ref = reference(m, &verification.reference)?;
    introduced.push(RetainedAssertionEnvelope { inner: authority });
    let assertion_index = introduced.len();
    let reused_assertion = if reuse {
        Some(RetainedAssertionEnvelope { inner: assertion })
    } else {
        introduced.push(RetainedAssertionEnvelope { inner: assertion });
        None
    };
    introduced.push(RetainedAssertionEnvelope {
        inner: verification,
    });
    let normalized = m.canonical_owned_representation(
        arena,
        parsed.value(),
        RepresentationKind::OriginalRequest,
    )?;
    Ok(AppRetention {
        normalized,
        introduced,
        authority_ref,
        assertion_ref,
        assertion_index,
        verification_ref,
        reused_assertion,
        records: r,
        claims,
        terms: original.terms,
        work: original.work,
        ownership,
        original_ownership: original._ownership,
    })
}
