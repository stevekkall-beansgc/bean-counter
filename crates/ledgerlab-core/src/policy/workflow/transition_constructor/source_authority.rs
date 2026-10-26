//! Actual original/current nominated source intersection shared by app and AP
//! consumers. The operands are original Binding/slot terms/work DATA, never a
//! synthetic invocation or caller authentication flag. Completed origin and
//! current snapshot custody remain the held host's independent obligations.
use super::super::selected_program::SelectedProgram;
use super::{
    authority::EconomicMode, body, data::*, inventory, meter, record_body::RecordBody,
    records::Records, representation::InvocationToken, shape, CoreMeter,
};
use crate::{canonical::audit_arena::ExternalBufferLease, validation_work as w, Error, Result};
use serde_json::{json, Value};
#[derive(Clone, Copy)]
pub(super) struct OriginalSourceSubject<'a> {
    pub binding_ref: &'a ExactRef,
    pub binding: &'a Value,
    pub terms: &'a Value,
    pub payload: &'a Value,
}
pub(super) struct SourceSelection {
    pub frame_index: usize,
    pub _enrollment: RecordBody,
    pub registered: RecordBody,
    pub source_slot: String,
    pub original_grant_refs: Vec<ExactRef>,
    pub current: RecordBody,
    pub _state: RecordBody,
    pub observation: Option<RecordBody>,
    pub verifier_ref: Option<ExactRef>,
    pub _root: Option<RecordBody>,
    _ownership: ExternalBufferLease,
}
impl SourceSelection {
    pub(super) fn required_verifier_ref(&self) -> Result<&ExactRef> {
        self.verifier_ref
            .as_ref()
            .ok_or_else(|| shape::fail("experimental verifier is required"))
    }
    pub(super) fn required_observation(&self) -> Result<&RecordBody> {
        self.observation
            .as_ref()
            .ok_or_else(|| shape::fail("experimental observation body is required"))
    }
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
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
    let body = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, body))
}
/// Exact current source grants are independent of historical worker namespace.
/// The original accepted supplier grants are selected by actual source/principal
/// association and compared in full with the supplied observation nominations.
pub(super) fn source(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: OriginalSourceSubject<'_>,
    source_slot: &str,
    mode: EconomicMode,
) -> Result<SourceSelection> {
    source_data(
        m,
        r,
        c,
        program,
        selected,
        source_slot,
        (mode, !program.uses_local_execution_recipe(), None),
    )
}

pub(super) fn source_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: OriginalSourceSubject<'_>,
    source_slot: &str,
    selected_operands: (EconomicMode, bool, Option<&Value>),
) -> Result<SourceSelection> {
    let (mode, audit_observation, dependent) = selected_operands;
    if audit_observation {
        inventory::entry::context(m, c)?;
    } else {
        inventory::entry::local_context(m, c)?;
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier authority"))?;
    let size = w::usize_add(m, std::mem::size_of::<SourceSelection>(), 16384)?;
    let ownership = arena.reserve_external_payload(size, m)?;
    let frame_index = match dependent {
        Some(evidence) if program.uses_work_lifecycle_recipe() => {
            select_dependent(m, r, c, program, (selected, source_slot, mode, evidence))?
        }
        _ => 0,
    };
    let a = &c.authorities[frame_index];
    if dependent.is_some() && program.uses_work_lifecycle_recipe() {
        let primary = &c.authorities[0];
        if !shape::same_ref(
            m,
            &a.accepted_agreement_ref,
            &primary.accepted_agreement_ref,
        )? || !shape::same_ref(m, &a.activation_ref, &primary.activation_ref)?
        {
            return Err(shape::fail(
                "dependent original acceptance/activation association",
            ));
        }
    }
    if !shape::same_ref(m, &a.binding_ref, selected.binding_ref)?
        || (!program.uses_app_owned_cost_recipe()
            && !shape::same_text(m, &a.source_slot, source_slot)?)
    {
        return Err(shape::fail("supplier original authority selection"));
    }
    let enrollment = r.get(
        m,
        &c.key,
        &a.enrollment_ref,
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    eq(m, &enrollment["scope"], &json!(c.key.scope))?;
    eq(
        m,
        &enrollment["installation_mode"],
        &json!(c.installation_mode),
    )?;
    let state = r.get(
        m,
        &c.key,
        &a.current_state_ref,
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
    eq(m, &state["enrollment_ref"], &json!(a.enrollment_ref))?;
    eq(m, &state["status"], &json!("active"))?;
    eq(m, &state["current_grant_ref"], &json!(a.current_grant_ref))?;
    inventory::source::current_header(m, c, &state)?;
    let enrollment_id = shape::text(m, &enrollment, "enrollment_id")?;
    let revision = shape::text(m, &state, "revision")?;
    let revision_value = super::super::facts::counter_with_work(revision, m)?;
    if !w::compare(m, revision_value, 0u64)?.is_gt() {
        return Err(shape::fail("positive current supplier source revision"));
    }
    let id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([c.key.logical_store_id, c.key.scope, enrollment_id, revision]),
    )?;
    if !shape::same_text(m, &a.current_state_ref.id, &id)? {
        return Err(shape::fail("supplier current source state identity"));
    }
    let birth_id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([c.key.logical_store_id, c.key.scope, enrollment_id, "1"]),
    )?;
    let (birth_ref, birth) = r.by_identity(
        m,
        &c.key,
        "workflow-source-state",
        &birth_id,
        "ledger-workflow-source-state/1",
    )?;
    let (birth_grant_ref, birth_grant) = load(
        m,
        r,
        c,
        &birth["current_grant_ref"],
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let token = InvocationToken::new();
    let registered = inventory::source::birth(
        m,
        r,
        c,
        &token,
        &enrollment,
        &birth,
        (
            &birth_grant,
            &selected.binding["customer"],
            &a.enrollment_ref,
            &birth_ref,
            &birth_grant_ref,
        ),
    )?;
    if w::compare(m, revision_value, 1u64)?.is_eq() {
        if state.get("prior_state_ref").is_some() {
            return Err(shape::fail("initial supplier source predecessor"));
        }
        eq(m, &state, &birth)?;
    } else {
        let prior = shape::reference(m, &state["prior_state_ref"])?;
        let previous = meter::sub(m, &revision_value.into(), &1.into())?;
        let expected = body::semantic_id(
            m,
            "wsource_",
            "workflow-source-state",
            &json!([
                c.key.logical_store_id,
                c.key.scope,
                enrollment_id,
                previous.to_string()
            ]),
        )?;
        if !shape::same_text(m, &prior.id, &expected)?
            || !shape::same_text(m, &prior.kind, "workflow-source-state")?
        {
            return Err(shape::fail("supplier current source predecessor"));
        }
    }
    let current = r.get(
        m,
        &c.key,
        &a.current_grant_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    eq(m, &current["grant_id"], &enrollment["enrollment_id"])?;
    eq(m, &current["revision"], &state["revision"])?;
    eq(
        m,
        &current["original_ceiling_ref"],
        &json!(a.enrollment_ref),
    )?;
    let at = token
        .timestamp(m, &c.accepted_at, "Timestamp::parse_with_work.accepted_at")?
        .micros();
    let state_raw = shape::text(m, &state, "accepted_at")?;
    let state_at = token
        .timestamp(m, state_raw, "Timestamp::parse_with_work.accepted_at")?
        .micros();
    if meter::cmp(m, &state_at.into(), &at.into())?.is_gt() {
        return Err(shape::fail("supplier source originates in future"));
    }
    let mut product = None;
    for document in program.source_documents() {
        m.visit("cost.authority_original_product")?;
        if shape::same(
            m,
            &json!(document.reference()),
            &selected.payload["product_ref"],
        )? {
            product = Some(document.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("supplier original product"))?;
    let product_id = shape::text(m, product, "product_id")?;
    let activity = shape::text(m, selected.payload, "activity")?;
    r.source_grant_identity(
        m,
        &token,
        &a.current_grant_ref,
        &current,
        (
            &a.enrollment_ref,
            &enrollment,
            c,
            &selected.binding["customer"],
            activity,
            product_id,
            source_slot,
            &at.into(),
        ),
        (
            mode,
            dependent.is_some() && program.uses_work_lifecycle_recipe(),
        ),
    )?;
    let nominations = shape::array(m, selected.binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", source_slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    let nominated = if dependent.is_some() && program.uses_work_lifecycle_recipe() {
        shape::member(m, sources, &current["source"])?
    } else {
        shape::member(m, sources, &json!(c.key.source))?
    };
    if !nominated {
        return Err(shape::fail("supplier source absent original nomination"));
    }
    let terms_grants = shape::array(m, selected.terms, "source_grant_refs")?;
    let binding_grants = shape::array(m, selected.binding, "authority_refs")?;
    let mut original_refs = Vec::new();
    original_refs
        .try_reserve_exact(terms_grants.len())
        .map_err(|_| shape::fail("supplier original grant vector"))?;
    for value in terms_grants {
        m.visit("cost.original_supplier_grant")?;
        let (reference, grant) = load(
            m,
            r,
            c,
            value,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        if !shape::member(m, binding_grants, value)? {
            return Err(shape::fail("supplier grant absent original Binding"));
        }
        let selected_source = if dependent.is_some() && program.uses_work_lifecycle_recipe() {
            shape::text(m, &current, "source")?
        } else {
            &c.key.source
        };
        let matches = if m.representation_arena().is_some() {
            super::input_selection::source_grant_matches(
                m,
                &grant,
                selected_source,
                &c.actor,
                &a.enrollment_ref,
            )?
        } else {
            // The legacy DATA recipe retains its original comparisons/trace.
            shape::same(m, &grant["source"], &json!(c.key.source))?
                && shape::same(m, &grant["principal_id"], &json!(c.actor))?
                && shape::same(m, &grant["original_ceiling_ref"], &json!(a.enrollment_ref))?
        };
        if !matches {
            continue;
        }
        r.source_grant_identity(
            m,
            &token,
            &reference,
            &grant,
            (
                &a.enrollment_ref,
                &enrollment,
                c,
                &selected.binding["customer"],
                activity,
                product_id,
                source_slot,
                &at.into(),
            ),
            (
                mode,
                dependent.is_some() && program.uses_work_lifecycle_recipe(),
            ),
        )?;
        original_refs.push(reference);
    }
    if !w::compare(m, original_refs.len(), 0usize)?.is_gt() {
        return Err(Error::new(
            "SOURCE_AUTHORITY_DENIED",
            "missing originally accepted supplier source grant",
        ));
    }
    let originals = body::ref_set(m, original_refs)?;
    eq(m, &json!(originals), &json!(a.original_grant_refs))?;
    if !audit_observation {
        m.scan_bytes(source_slot.len())?;
        m.write_bytes(source_slot.len())?;
        return Ok(SourceSelection {
            frame_index,
            _enrollment: enrollment,
            registered,
            source_slot: source_slot.to_owned(),
            original_grant_refs: originals,
            current,
            _state: state,
            observation: None,
            verifier_ref: None,
            _root: None,
            _ownership: ownership,
        });
    }
    let observation = r.get(
        m,
        &c.key,
        a.required_observation_ref()?,
        "workflow-authority-prefix-observation",
        "ledger-workflow-authority-prefix-observation/1",
    )?;
    for (key, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("installation_mode", json!(c.installation_mode)),
        ("enrollment_ref", json!(a.enrollment_ref)),
        ("current_state_ref", json!(a.current_state_ref)),
        ("current_grant_ref", json!(a.current_grant_ref)),
        ("original_grant_refs", json!(a.original_grant_refs)),
        ("observed_at", json!(c.accepted_at)),
    ] {
        eq(m, &observation[key], &want)?;
    }
    let (verifier_ref, verifier) = load(
        m,
        r,
        c,
        &observation["verifier_ref"],
        "workflow-authority-audit-verifier",
        "ledger-authority-audit-verifier/2",
    )?;
    let root = inventory::evidence::root(
        m,
        r,
        c,
        &token,
        &observation["root_ref"],
        &json!({"at":c.accepted_at}),
        ("at", &json!(c.actor), Some(&json!(c.key.source))),
    )?;
    for (key, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("principal_id", json!(c.actor)),
        ("source", json!(c.key.source)),
        (
            "canonical_economic_namespace",
            json!(c.canonical_economic_namespace),
        ),
    ] {
        eq(m, &root["basis"][key], &want)?;
    }
    let app_catalog = super::audit_metadata::selected_catalog(m, program)?;
    r.source_observation_after_joins(
        m,
        c,
        a.required_observation_ref()?,
        &state,
        &observation,
        (&verifier, &root, app_catalog),
    )?;
    m.scan_bytes(source_slot.len())?;
    m.write_bytes(source_slot.len())?;
    let source_slot = source_slot.to_owned();
    Ok(SourceSelection {
        frame_index,
        _enrollment: enrollment,
        registered,
        source_slot,
        original_grant_refs: originals,
        current,
        _state: state,
        observation: Some(observation),
        verifier_ref: Some(verifier_ref),
        _root: Some(root),
        _ownership: ownership,
    })
}

/// A causal dependent keeps the actual initiating key/actor. Only the selected
/// nominated role's grant/enrollment/namespace changes, through the same full
/// source interpreter after bounded DATA selection.
pub(super) fn dependent(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: OriginalSourceSubject<'_>,
    purpose: (&str, EconomicMode, &Value),
) -> Result<SourceSelection> {
    let (slot, mode, evidence) = purpose;
    source_data(
        m,
        r,
        c,
        program,
        selected,
        slot,
        (mode, !program.uses_local_execution_recipe(), Some(evidence)),
    )
}
fn select_dependent(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (OriginalSourceSubject<'_>, &str, EconomicMode, &Value),
) -> Result<usize> {
    let (subject, slot, mode, evidence) = selected;
    let at = crate::domain::Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    let hints = evidence
        .as_array()
        .ok_or_else(|| shape::fail("dependent evidence array"))?;
    let mut selected = None;
    let mut hinted = None;
    let mut count = 0usize;
    let nominations = shape::array(m, subject.binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    let mut product = None;
    for doc in program.source_documents() {
        let reference = super::working_value::owning_definition(
            m,
            &m.representation_arena()
                .ok_or_else(|| shape::fail("dependent source product owner"))?,
            doc.reference(),
        )?;
        if shape::same(m, reference.value(), &subject.payload["product_ref"])? {
            product = Some(doc.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("dependent original Product"))?;
    let product_name = shape::text(m, product, "product_id")?;
    let activity = shape::text(m, subject.payload, "activity")?;
    for (i, frame) in c.authorities.iter().enumerate() {
        m.visit("source.dependent_candidate")?;
        if !shape::same_text(m, &frame.source_slot, slot)?
            || !shape::same_ref(m, &frame.binding_ref, subject.binding_ref)?
        {
            continue;
        }
        let current = r.get(
            m,
            &c.key,
            &frame.current_grant_ref,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        let enrollment = r.get(
            m,
            &c.key,
            &frame.enrollment_ref,
            "workflow-source-enrollment",
            "ledger-workflow-source-enrollment/1",
        )?;
        let state = r.get(
            m,
            &c.key,
            &frame.current_state_ref,
            "workflow-source-state",
            "ledger-workflow-source-state/1",
        )?;
        let status = shape::text(m, &state, "status")?;
        if !shape::same_text(m, status, "active")? {
            continue;
        }
        let principal = shape::text(m, &current, "principal_id")?;
        if !shape::same_text(m, principal, &c.actor)?
            || !shape::member(m, sources, &current["source"])?
        {
            continue;
        }
        let ceiling = shape::field(m, &enrollment, "ceiling")?;
        super::authority::grant_selected_identity(m, &current, ceiling, c)?;
        if !super::authority::grant_covers(
            m,
            &current,
            (
                &subject.binding["customer"],
                activity,
                product_name,
                slot,
                (mode.operation(), mode.rights()),
                at,
            ),
        )? || !super::authority::grant_covers(
            m,
            ceiling,
            (
                &subject.binding["customer"],
                activity,
                product_name,
                slot,
                (mode.operation(), mode.rights()),
                at,
            ),
        )? {
            continue;
        }
        let mut originals = 0usize;
        let mut original_eligible = true;
        let mut hint = false;
        let accepted = shape::array(m, subject.binding, "authority_refs")?;
        for reference in &frame.original_grant_refs {
            let value = super::working_value::owning_reference(
                m,
                &m.representation_arena()
                    .ok_or_else(|| shape::fail("dependent original Ref owner"))?,
                reference,
            )?;
            if !shape::member(m, accepted, value.value())? {
                return Err(shape::fail("dependent original grant not accepted"));
            }
            let grant = r.get(
                m,
                &c.key,
                reference,
                "workflow-source-grant",
                "ledger-workflow-source-grant/1",
            )?;
            eq(m, &grant["source"], &current["source"])?;
            super::authority::grant_selected_identity(m, &grant, ceiling, c)?;
            original_eligible &= super::authority::grant_covers(
                m,
                &grant,
                (
                    &subject.binding["customer"],
                    activity,
                    product_name,
                    slot,
                    (mode.operation(), mode.rights()),
                    at,
                ),
            )?;
            originals = w::usize_add(m, originals, 1)?;
            if shape::member(m, hints, value.value())? {
                hint = true;
            }
        }
        if !original_eligible || !w::compare(m, originals, 0usize)?.is_gt() {
            continue;
        }
        let current_ref = super::working_value::owning_reference(
            m,
            &m.representation_arena()
                .ok_or_else(|| shape::fail("dependent current Ref owner"))?,
            &frame.current_grant_ref,
        )?;
        if shape::member(m, hints, current_ref.value())? {
            hint = true;
        }
        count = w::usize_add(m, count, 1)?;
        selected = Some(i);
        if hint && hinted.replace(i).is_some() {
            return Err(Error::new(
                "AUTHORITY_REQUIRED",
                "ambiguous explicit dependent source grants",
            ));
        }
    }
    if let Some(i) = hinted {
        return Ok(i);
    }
    if w::compare(m, count, 1usize)?.is_eq() {
        return selected.ok_or_else(|| shape::fail("dependent source selection"));
    }
    Err(Error::new(
        "AUTHORITY_REQUIRED",
        "unique current/original dependent source required",
    ))
}

#[cfg(test)]
mod role_tests {
    use super::*;
    use crate::{
        canonical::audit_arena::AuditArena,
        validation_work::{OriginalAuditSession, ResourceLimits},
    };
    #[test]
    fn lifecycle_dependent_selection_keeps_actual_root_and_uses_exact_original_current_role() {
        let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let program = super::super::lifecycle_consumer_tests::program(&a);
        let business: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-work-lifecycle/business.json"
        ))
        .unwrap();
        let mut producer = CoreMeter::for_compile(1_048_576).unwrap();
        let mut rows = Vec::new();
        let mut frames = Vec::new();
        let binding_ref = ExactRef {
            kind: "workflow-binding".into(),
            id: format!("wdoc_{}", "a".repeat(64)),
            digest: format!("sha256:{}", "a".repeat(64)),
        };
        for (name, slot) in [
            ("corrector", "support:corrector"),
            ("dependent", "support:cost-source"),
            ("source", "support:admin"),
        ] {
            let ceiling = &business[format!("{name}_ceiling")];
            let enrollment = body::document(
                &mut producer,
                "workflow-source-enrollment",
                json!({"schema":"ledger-workflow-source-enrollment/1","ceiling":ceiling}),
            )
            .unwrap();
            let mut grant = ceiling.clone();
            grant["schema"] = json!("ledger-workflow-source-grant/1");
            grant["original_ceiling_ref"] = json!(enrollment.reference);
            grant["grant_id"] = json!(format!("{name}-enrollment"));
            grant["revision"] = json!("1");
            let grant = body::document(&mut producer, "workflow-source-grant", grant).unwrap();
            let state_key = OperationKey {
                logical_store_id: "store".into(),
                scope: ["example-company".into(), "local".into()],
                source: "urn:example:work".into(),
                operation_id: format!("enroll-{name}"),
            };
            let state_id = body::semantic_id(
                &mut producer,
                "wsource_",
                "workflow-source-state",
                &json!([
                    state_key.logical_store_id,
                    state_key.scope,
                    format!("{name}-enrollment"),
                    "1"
                ]),
            )
            .unwrap();
            let state=body::record(&mut producer,&state_key,"workflow-source-state",state_id,json!({"schema":"ledger-workflow-source-state/1","status":"active","enrollment_ref":enrollment.reference,"revision":"1","current_grant_ref":grant.reference,"operation_key":state_key,"actor":"example-operator","accepted_at":"2026-10-26T00:00:00.000000Z"})).unwrap();
            frames.push(AuthorityContextData {
                source_slot: slot.into(),
                enrollment_ref: enrollment.reference.clone(),
                current_state_ref: state.reference.clone(),
                current_grant_ref: grant.reference.clone(),
                original_grant_refs: vec![grant.reference.clone()],
                binding_ref: binding_ref.clone(),
                accepted_agreement_ref: binding_ref.clone(),
                activation_ref: binding_ref.clone(),
                authority_observation_ref: None,
            });
            if name == "source" {
                frames.pop();
            }
            rows.extend([enrollment, grant, state]);
        }
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["example-company".into(), "local".into()],
            source: business["corrector_source"].as_str().unwrap().into(),
            operation_id: "root-reverse".into(),
        };
        let c = DraftContextData {
            key: key.clone(),
            actor: "example-operator".into(),
            canonical_economic_namespace: key.source.clone(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-26T00:00:00.000000Z".into(),
            accepted_at: "2026-10-26T00:00:00.000000Z".into(),
            authorities: frames,
            source_observation_inputs: vec![],
        };
        let binding = json!({"customer":"customer-B","authority_refs":[rows[1].reference,rows[4].reference,rows[7].reference],"source_slots":[{"name":"support:corrector","sources":[business["corrector_source"]]},{"name":"support:cost-source","sources":[business["dependent_source"]]}]});
        let payload = json!({"product_ref":program.product_ref(),"activity":"support:complete"});
        let terms = json!({});
        let evidence = json!([rows[1].reference, rows[4].reference]);
        let views: Vec<_> = rows
            .iter()
            .map(|r| EconomicRecordView {
                reference: &r.reference,
                canonical_body: &r.canonical_body,
            })
            .collect();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let arena = s.arena().shared_handle();
        let m = s.meter_mut();
        let mut r = Records::borrowed_parent_app(m, &arena, &key, &views).unwrap();
        let subject = OriginalSourceSubject {
            binding_ref: &binding_ref,
            binding: &binding,
            terms: &terms,
            payload: &payload,
        };
        let selected = select_dependent(
            m,
            &mut r,
            &c,
            &program,
            (
                subject,
                "support:cost-source",
                EconomicMode::CostCorrect,
                &evidence,
            ),
        )
        .unwrap();
        assert_eq!(selected, 1);
        assert_eq!(c.key, key);
        assert_eq!(c.canonical_economic_namespace, key.source);
        let g = r
            .get(
                m,
                &key,
                &rows[4].reference,
                "workflow-source-grant",
                "ledger-workflow-source-grant/1",
            )
            .unwrap();
        let enrollment = r
            .get(
                m,
                &key,
                &rows[3].reference,
                "workflow-source-enrollment",
                "ledger-workflow-source-enrollment/1",
            )
            .unwrap();
        // This SAME per-use check admits explicit dependent identity and refuses
        // treating its grant as the initiating C grant. It grants no origin.
        super::super::authority::grant_selected_identity(m, &g, &enrollment["ceiling"], &c)
            .unwrap();
        assert!(
            super::super::authority::grant_identity(m, &g, &enrollment["ceiling"], &c).is_err()
        );
        assert!(select_dependent(
            m,
            &mut r,
            &c,
            &program,
            (
                subject,
                "support:cost-source",
                EconomicMode::Work,
                &evidence
            )
        )
        .is_err());
        drop(r);
        let values: Vec<Value> = rows
            .iter()
            .map(|row| serde_json::from_slice(&row.canonical_body).unwrap())
            .collect();
        let originals: Vec<_> = [1usize, 4, 7]
            .iter()
            .map(|i| {
                vec![super::super::source_projection::NominatedSourceGrant {
                    reference: &rows[*i].reference,
                    body: &values[*i],
                }]
            })
            .collect();
        let accepted_frames: Vec<_> = [0usize, 3, 6]
            .iter()
            .enumerate()
            .map(
                |(i, n)| super::super::source_projection::NominatedSourceFrame {
                    enrollment_ref: &rows[*n].reference,
                    enrollment: &values[*n],
                    current_state_ref: &rows[*n + 2].reference,
                    current_state: &values[*n + 2],
                    current_grant_ref: &rows[*n + 1].reference,
                    current_grant: &values[*n + 1],
                    original_grants: &originals[i],
                },
            )
            .collect();
        let mut shape_session = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        super::super::source_projection::validate_app_local_source_shape(
            &program,
            &binding,
            &accepted_frames,
            &mut shape_session,
        )
        .unwrap();
        let mut r = Records::borrowed_parent_app(m, &arena, &key, &views).unwrap();
        let mut no_current = c.clone();
        no_current.authorities.pop();
        assert_eq!(
            select_dependent(
                m,
                &mut r,
                &no_current,
                &program,
                (
                    subject,
                    "support:cost-source",
                    EconomicMode::CostCorrect,
                    &evidence
                )
            )
            .unwrap_err()
            .code,
            "AUTHORITY_REQUIRED"
        );
        let mut no_original_binding = binding.clone();
        no_original_binding["authority_refs"] = json!([rows[1].reference]);
        assert!(select_dependent(
            m,
            &mut r,
            &c,
            &program,
            (
                OriginalSourceSubject {
                    binding: &no_original_binding,
                    ..subject
                },
                "support:cost-source",
                EconomicMode::CostCorrect,
                &evidence
            )
        )
        .is_err());
    }
}
