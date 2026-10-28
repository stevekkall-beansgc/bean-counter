//! Stage-close's fixed original member/clock/basis consumer. Every operand is
//! retained under the same Records owner as the final monetary construction.
use super::super as tc;
use super::*;
use crate::canonical::audit_arena::AuditedValue;
use num_bigint::BigInt;
use tc::{
    authority::EconomicMode,
    record_body::RecordBody,
    working_value::{NormalizedInput, WorkingValue},
};

struct Member {
    work_ref: OwnedExactRef,
    claim_ref: OwnedExactRef,
    work: RecordBody,
    original: RecordBody,
    current: RecordBody,
}
fn revision_id(m: &mut CoreMeter, decision: &Value, revision: &Value) -> Result<String> {
    let a = arena(m)?;
    let tuple = a.array_from_values(&[decision, revision], m)?;
    body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())
}
fn member(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (&Value, &Value, &Value, &Value),
    held: (&DraftStateData, bool),
) -> Result<Member> {
    let (binding, instance, instance_ref, expected) = selected;
    let (state, current_required) = held;
    let a = arena(m)?;
    let namespace = shape::text(m, binding, "source_namespace")?;
    let claim = tc::work_keys::claim(m, &c.key, namespace, expected)?;
    let (claim_ref, claim) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        &claim,
        "ledger-workflow-work-claim/1",
    )?;
    let claim_ref_value = owning_reference(m, &a, &claim_ref)?;
    let claim_ref = parse_owned_reference(m, &a, claim_ref_value.value())?;
    let work_ref = parse_owned_reference(m, &a, &claim["work_ref"])?;
    let work = r.get(
        m,
        &c.key,
        work_ref.reference(),
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    tc::inventory::entry::historical_work_claim(m, c, claim_ref.reference(), &claim, &work)?;
    shape::equal(m, &work["payload"]["stage_instance_ref"], instance_ref)?;
    for field in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &work["payload"][field], &instance[field])?;
    }
    for field in ["business_operation_id", "activity"] {
        shape::equal(m, &work["payload"][field], &expected[field])?;
    }
    shape::equal(m, &work["key"]["source"], &expected["source"])?;
    shape::equal(m, &work["source_namespace"], &binding["source_namespace"])?;
    let permanent = tc::work_keys::decision(m, &claim_ref.reference().id)?;
    let permanent = a.string_value(&permanent, m)?;
    let one = a.string_value("1", m)?;
    let identity = revision_id(m, permanent.value(), one.value())?;
    let (_, original) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &identity,
        "ledger-workflow-decision-revision/1",
    )?;
    same(m, &original, "kind", "work")?;
    same(m, &original, "revision", "1")?;
    same(m, &original, "transition", "initial")?;
    same(m, &original, "status", "active")?;
    let work_value = owning_reference(m, &a, work_ref.reference())?;
    for (field, value) in [
        ("decision_id", permanent.value()),
        ("subject_ref", work_value.value()),
        ("original_terms_ref", &instance["binding_ref"]),
        ("operation_key", &work["key"]),
        ("accepted_at", &work["accepted_at"]),
    ] {
        shape::equal(m, &original[field], value)?;
    }
    tc::authority::historical_namespace(
        m,
        r,
        c,
        &original["authority_ref"],
        &work["source_namespace"],
        Some(&work["key"]["source"]),
        (
            "work.accept",
            &instance["binding_ref"],
            &work["accepted_at"],
        ),
    )?;
    let at = shape::text(m, &work, "accepted_at")?;
    let at = Timestamp::parse_with_work(at, m)?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?;
    if w::compare(m, at.micros(), now.micros())?.is_gt() {
        return Err(shape::fail("Stage member accepted after closing operation"));
    }
    let namespace = shape::text(m, &work, "source_namespace")?;
    let received = shape::text(m, &work, "received_at")?;
    let proofs = tc::work_proofs::retained(
        m,
        r,
        c,
        program,
        &tc::work_proofs::WorkProofRequest {
            payload: &work["payload"],
            facts: &work["payload"]["facts"],
            namespace,
            received_at: received,
            only_field: None,
            worker_authority: None,
        },
        &original["authority_ref"],
        true,
    )?;
    drop(proofs);
    if !current_required {
        m.write_bytes(mem::size_of::<RecordBody>())?;
        return Ok(Member {
            work_ref,
            claim_ref,
            work,
            current: original.clone(),
            original,
        });
    }
    tc::declared_links::current_work(m, r, c, work_ref.reference(), &work, state)?;
    let mut selected = None;
    for head in &state.decision_heads {
        if shape::same_text(
            m,
            &head.decision_id,
            permanent
                .value()
                .as_str()
                .ok_or_else(|| shape::fail("Stage member permanent Decision"))?,
        )? && selected.replace(head).is_some()
        {
            return Err(shape::fail("duplicate current Stage member Decision"));
        }
    }
    let head = selected.ok_or_else(|| shape::fail("missing current Stage member Decision"))?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    let expected = revision_id(m, &current["decision_id"], &current["revision"])?;
    if !shape::same_text(m, &head.reference.id, &expected)? {
        return Err(shape::fail("current Stage member revision identity"));
    }
    if !shape::same_text(m, &head.revision, "1")? {
        return Err(crate::Error::new(
            "TARGET_CLOSED",
            "Stage member root changed after its original promise",
        ));
    }
    shape::equal(m, &current, &original)?;
    Ok(Member {
        work_ref,
        claim_ref,
        work,
        original,
        current,
    })
}

fn booked_basis(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (&Value, &Value, &Value, &Member),
    basis: &mut crate::policy::workflow::selected_program::supplier_projection::Values,
) -> Result<BigInt> {
    let (binding, instance, expected, member) = selected;
    let a = arena(m)?;
    let evaluation_ref = parse_owned_reference(m, &a, &member.current["evaluation_ref"])?;
    let evaluation = r.get(
        m,
        &c.key,
        evaluation_ref.reference(),
        "workflow-evaluation",
        "ledger-workflow-evaluation-proof/1",
    )?;
    let work = owning_reference(m, &a, member.work_ref.reference())?;
    shape::equal(m, &evaluation["subject_ref"], work.value())?;
    for field in [
        "policy_ref",
        "product_ref",
        "binding_ref",
        "fact_schema_ref",
    ] {
        if field != "product_ref" {
            shape::equal(m, &evaluation[field], &member.work["payload"][field])?;
        }
    }
    shape::equal(m, &evaluation["context"], &binding["accepted_context"])?;
    let activity = shape::text(m, &member.work["payload"], "activity")?;
    let source = shape::text(m, &member.work["key"], "source")?;
    let prices = tc::pricing::work_subject(
        program,
        (activity, &member.work["payload"]),
        (&evaluation["facts"], &binding["accepted_context"]),
        Some((binding, source)),
        m,
    )?;
    let active = shape::array(m, &member.current, "active_action_refs")?;
    let selected = shape::array(m, expected, "component_slots")?;
    let inst_ref = parse_owned_reference(m, &a, &binding["slot_instantiation_ref"])?;
    let inst = r.get(
        m,
        &c.key,
        inst_ref.reference(),
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let maps = shape::array(m, &inst, "component_map")?;
    let mut net = BigInt::from(0);
    for price in prices.iter() {
        let mapping = shape::named(m, maps, "template_key", &price.component_key)?;
        let mut matched = None;
        for value in active {
            let reference = parse_owned_reference(m, &a, value)?;
            let action = r.get(
                m,
                &c.key,
                reference.reference(),
                "workflow-action",
                "ledger-workflow-action/1",
            )?;
            if !shape::same(m, &action["component_slot"], &mapping["slot_id"])? {
                continue;
            }
            if matched.is_some() {
                return Err(shape::fail("duplicate active Stage member component"));
            }
            tc::correction::dictionary_decision(m, &member.current)?;
            shape::closed(
                m,
                &action,
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
            shape::equal(m, &action["decision_id"], &member.current["decision_id"])?;
            same(m, &action, "revision", "1")?;
            same(m, &action, "posting_role", "original")?;
            shape::equal(m, &action["operation_key"], &member.work["key"])?;
            let binding_rows = shape::array(m, binding, "bindings")?;
            let selected_terms = shape::named(m, binding_rows, "slot", &price.binding_slot)?;
            shape::equal(m, &action["slot_terms_ref"], &selected_terms["binding_ref"])?;
            let selected_ref = parse_owned_reference(m, &a, &selected_terms["binding_ref"])?;
            let original_terms = r.get(
                m,
                &c.key,
                selected_ref.reference(),
                "workflow-slot-terms",
                "ledger-workflow-slot-binding-terms/3",
            )?;
            shape::equal(m, &action["role_ref"], &original_terms["role_ref"])?;
            shape::equal(m, &action["funding_leg"], &original_terms["funding_leg"])?;
            let root = shape::named(m, maps, "template_key", &price.root_key)?;
            shape::equal(m, &action["root_component_slot"], &root["slot_id"])?;
            let effect = tc::cost_postings::original_effect(
                m,
                &a,
                c,
                binding,
                member.claim_ref.reference(),
                &mapping["slot_id"],
            )?;
            same(m, &action, "effect_id", &effect)?;
            let expected_id = tc::cost_postings::action_id(
                m,
                &a,
                &action["effect_id"],
                &action["revision"],
                &action["posting_role"],
            )?;
            if !shape::same_text(m, &reference.reference().id, &expected_id)? {
                return Err(shape::fail("Stage member stored Action identity"));
            }
            let rounded = tc::cost_postings::amount(m, &a, &price.atoms.into())?;
            shape::equal(m, &action["amount"], rounded.value())?;
            let amount = shape::text(m, &action["amount"], "atoms")?;
            let amount = tc::arithmetic::integer(m, amount, true)?;
            if !tc::meter::cmp(m, &amount, &price.atoms.into())?.is_eq() {
                return Err(shape::fail(
                    "Stage basis differs from actual current member pricing",
                ));
            }
            if shape::member(m, selected, &mapping["slot_id"])? {
                same(m, &action, "book", "retail")?;
                net = tc::meter::add(m, &net, &amount)?;
                basis.unique(m, &a, value)?;
            }
            matched = Some(reference);
        }
        if !tc::meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq() && matched.is_none() {
            return Err(shape::fail(
                "Stage member nonzero price missing active Action",
            ));
        }
        if tc::meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq() && matched.is_some() {
            return Err(shape::fail("Stage member zero price has active Action"));
        }
    }
    let mut expected_actions = 0usize;
    for price in prices.iter() {
        m.visit("Stage.member_nonzero_posting_count")?;
        if !tc::meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq() {
            expected_actions = w::usize_add(m, expected_actions, 1)?;
        }
    }
    if !w::compare(m, active.len(), expected_actions)?.is_eq() {
        return Err(shape::fail("Stage member current active Action closure"));
    }
    let _ = instance;
    Ok(net)
}

fn timestamp(m: &mut CoreMeter, value: &Value, field: &str) -> Result<i64> {
    let raw = shape::text(m, value, field)?;
    Ok(Timestamp::parse_with_work(raw, m)?.micros())
}
fn state_started(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    instance: &Value,
    instance_ref: &OwnedExactRef,
    state: &DraftStateData,
) -> Result<(RecordBody, OwnedExactRef)> {
    let a = arena(m)?;
    let mut selected = None;
    for head in &state.stage_heads {
        let agreement = shape::text(m, instance, "agreement_id")?;
        let chain = shape::text(m, instance, "chain")?;
        let name = shape::text(m, instance, "stage_name")?;
        if shape::same_text(m, &head.agreement, agreement)?
            && shape::same_text(m, &head.chain, chain)?
            && shape::same_text(m, &head.stage, name)?
            && selected.replace(head).is_some()
        {
            return Err(shape::fail("duplicate held closing Stage head"));
        }
    }
    let head = selected.ok_or_else(|| shape::fail("actual started Stage head missing"))?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-stage-state",
        "ledger-workflow-stage-state/1",
    )?;
    let status = shape::text(m, &current, "state")?;
    if !shape::same_text(m, status, "started")? {
        return Err(crate::Error::new(
            "TARGET_CLOSED",
            "Stage is not eligible for its permanent closure",
        ));
    }
    if !shape::same_text(m, &head.revision, "2")? {
        return Err(shape::fail("Stage current head revision"));
    }
    validate_started(
        m,
        r,
        c,
        program,
        (instance_ref.reference(), instance),
        (&head.reference, &current),
        &c.accepted_at,
    )?;
    let current_value = owning_reference(m, &a, &head.reference)?;
    let current_ref = parse_owned_reference(m, &a, current_value.value())?;
    Ok((current, current_ref))
}

/// Pure closed Stage-close request normalization under the selected original program.
pub(in crate::policy::workflow::transition_constructor) fn normalize_request(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    p: &mut WorkingValue,
) -> Result<()> {
    shape::closed(
        m,
        p,
        &[
            "schema",
            "external_event_id",
            "business_operation_id",
            "customer",
            "chain",
            "activity",
            "product_ref",
            "policy_ref",
            "fact_schema_ref",
            "binding_ref",
            "facts",
            "fact_proofs",
            "links",
            "evidence_refs",
            "occurred_at",
            "stage",
            "stage_instance_ref",
        ],
        &[],
    )?;
    same(m, p, "schema", "ledger-workflow-stage-close/1")?;
    let a = arena(m)?;
    for field in [
        "external_event_id",
        "business_operation_id",
        "customer",
        "chain",
    ] {
        tc::inventory::binding::id(m, &p[field], 128)?;
    }
    for field in ["links", "fact_proofs", "evidence_refs"] {
        p.normalize_set(m, field, false)?;
    }
    let occurrence = shape::text(m, p, "occurred_at")?;
    let occurrence = Timestamp::parse_with_work(occurrence, m)?;
    let occurrence_value = a.string_value(occurrence.as_str(), m)?;
    p.replace(m, "occurred_at", occurrence_value.value())?;
    let mut schema = None;
    for (reference, definition) in program.fact_schemas() {
        let value = tc::working_value::owning_definition(m, &a, reference)?;
        if shape::same(m, value.value(), &p["fact_schema_ref"])? {
            schema = Some(definition);
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("Stage closure original schema"))?;
    let normalized = NormalizedInput::normalize(m, schema, &p["facts"])?;
    p.replace(m, "facts", normalized.canonical_value())
}

pub(in crate::policy::workflow::transition_constructor) fn produce<'a>(
    program: &SelectedProgram,
    c: &'a DraftContextData,
    mut p: WorkingValue,
    inputs: &'a [EconomicRecordView<'a>],
    state: &EconomicStateData<'a>,
    m: &mut CoreMeter,
) -> Result<Construction<'a, 'a>> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("exact Stage close recipe"));
    }
    tc::inventory::entry::context_selected(m, c, program)?;
    normalize_request(program, m, &mut p)?;
    let a = arena(m)?;
    let occurrence = Timestamp::parse_with_work(shape::text(m, &p, "occurred_at")?, m)?;
    let mut r =
        Records::borrowed_selected(m, &a, &c.key, inputs, &c.source_observation_inputs, program)?;
    r.install_initial_source_outputs(m, state.initial_source_correspondences)?;
    let instance_ref = parse_owned_reference(m, &a, &p["stage_instance_ref"])?;
    let instance = r.get(
        m,
        &c.key,
        instance_ref.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    closed_instance(m, &c.key, instance_ref.reference(), &instance)?;
    for field in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &p[field], &instance[field])?;
    }
    shape::equal(m, &p["stage"], &instance["stage_name"])?;
    let name = shape::text(m, &instance, "stage_name")?;
    let def = definition(m, program, name)?;
    shape::equal(m, &p["activity"], &def["closure_activity"])?;
    let binding_ref = parse_owned_reference(m, &a, &p["binding_ref"])?;
    let binding = r.get(
        m,
        &c.key,
        binding_ref.reference(),
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    for field in ["product_ref", "policy_ref"] {
        shape::equal(m, &p[field], &binding[field])?;
    }
    let (started, started_ref) = state_started(
        m,
        &mut r,
        c,
        program,
        &instance,
        &instance_ref,
        state.financial,
    )?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?;
    let deadline = timestamp(m, &started, "activation_deadline")?;
    if w::compare(m, now.micros(), deadline)?.is_gt() {
        return Err(crate::Error::new(
            "WINDOW_CLOSED",
            "Stage close accepted after original activation deadline",
        ));
    }
    let activities = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "activity_table",
    )?;
    let activity_name = shape::text(m, &p, "activity")?;
    let activity = shape::named(m, activities, "activity", activity_name)?;
    shape::equal(m, &p["fact_schema_ref"], &activity["fact_schema_ref"])?;
    tc::fact_relations::facts(m, &mut r, &c.key, program, &p, &p["facts"], &c.accepted_at)?;
    let proof_request = tc::work_proofs::WorkProofRequest {
        payload: &p,
        facts: &p["facts"],
        namespace: shape::text(m, &binding, "source_namespace")?,
        received_at: &c.received_at,
        only_field: None,
        worker_authority: None,
    };
    let proof_rows = shape::array(m, &p, "fact_proofs")?;
    let mut proofs = tc::work_proofs::resolve(
        m,
        &mut r,
        c,
        program,
        &proof_request,
        proof_rows,
        &state.financial.evidence_verifications,
    )?;
    let source_slot = shape::text(m, def, "closure_source_slot")?;
    let namespace = shape::text(m, &binding, "source_namespace")?;
    let resolved = tc::app_cost::binding_resolved(
        m,
        &mut r,
        c,
        &p,
        program,
        EconomicMode::OutcomeSubmit,
        (
            source_slot,
            proofs.as_ref().map_or(&[], |p| p.references.as_slice()),
            namespace,
        ),
    )?;
    let (terms, authority_value, terms_owner) = resolved.into_local_parts()?;
    tc::supplier_consent::original(program, m, &mut r, c, &binding, &terms.accepted)?;
    let authority =
        body::document_borrowed(m, "workflow-economic-authority", authority_value.value())?;
    let authority_ref = owning_reference(m, &a, &authority.reference)?;
    let expected = shape::array(m, &instance, "expected")?;
    let bytes = expected
        .len()
        .checked_mul(mem::size_of::<Member>() + 5 * (64 + 128 + 71))
        .ok_or_else(|| shape::fail("Stage fixed member controls"))?;
    let members_owner = a.reserve_external_payload(bytes + mem::size_of::<Vec<Member>>(), m)?;
    let mut members = Vec::new();
    members
        .try_reserve_exact(expected.len())
        .map_err(|_| shape::fail("Stage fixed member allocation"))?;
    let instance_value = owning_reference(m, &a, instance_ref.reference())?;
    let mut prior_basis =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut prior_net = BigInt::from(0);
    let mut first = false;
    for row in expected {
        let member = member(
            m,
            &mut r,
            c,
            program,
            (&binding, &instance, instance_value.value(), row),
            (state.financial, true),
        )?;
        let work_value = owning_reference(m, &a, member.work_ref.reference())?;
        if shape::same(m, &started["first_work_ref"], work_value.value())? {
            first = true;
            shape::equal(
                m,
                &started["authority_ref"],
                &member.original["authority_ref"],
            )?;
        }
        let net = booked_basis(
            m,
            &mut r,
            c,
            program,
            (&binding, &instance, row, &member),
            &mut prior_basis,
        )?;
        prior_net = tc::meter::add(m, &prior_net, &net)?;
        members.push(member);
    }
    if !first {
        return Err(shape::fail(
            "Stage first Work is not an original fixed member",
        ));
    }
    // The explicit path is followed by exact declared slots. Its first edge is
    // produced now with original Instance as child, avoiding a Claim hash cycle.
    let path = shape::array(m, &def["window"], "anchor_path")?;
    if path.is_empty() || w::compare(m, path.len(), 4usize)?.is_gt() {
        return Err(shape::fail("Stage exact anchor path"));
    }
    tc::declared_links::validate_inputs(m, program, &p)?
        .ok_or_else(|| shape::fail("Stage closure declared predecessor"))?;
    let links = shape::array(m, &p, "links")?;
    let first_slot = path[0]
        .as_str()
        .ok_or_else(|| shape::fail("Stage first link slot"))?;
    let input = shape::named(m, links, "slot", first_slot)?;
    shape::closed(m, input, &["slot", "predecessor"], &[])?;
    let mut endpoint = parse_owned_reference(m, &a, &input["predecessor"])?;
    tc::declared_links::stage_endpoint(
        m,
        &mut r,
        c,
        program,
        &p,
        &input["predecessor"],
        &c.accepted_at,
    )?;
    let mut old_links =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 4)?;
    for slot in &path[1..] {
        let work = r.get(
            m,
            &c.key,
            endpoint.reference(),
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        let slot = slot
            .as_str()
            .ok_or_else(|| shape::fail("Stage predecessor path slot"))?;
        let links = shape::array(m, &work["payload"], "links")?;
        let next = shape::named(m, links, "slot", slot)?;
        let link =
            tc::declared_links::retained_work(m, &mut r, c, program, endpoint.reference(), &work)?;
        let link_value = owning_reference(m, &a, &link)?;
        old_links.unique(m, &a, link_value.value())?;
        endpoint = parse_owned_reference(m, &a, &next["predecessor"])?;
    }
    let mut anchor = None;
    for member in &members {
        m.visit("Stage.exact_original_anchor_member")?;
        if shape::same_ref(m, member.work_ref.reference(), endpoint.reference())?
            && anchor.replace(member).is_some()
        {
            return Err(shape::fail("duplicate Stage anchor member"));
        }
    }
    let anchor =
        anchor.ok_or_else(|| shape::fail("Stage anchor is outside fixed original members"))?;
    let occurred = shape::text(m, &anchor.work["payload"], "occurred_at")?;
    let anchor_at = Timestamp::parse_with_work(occurred, m)?;
    let anchor_received = timestamp(m, &anchor.work, "received_at")?;
    if w::compare(m, anchor_at.micros(), anchor_received)?.is_gt() {
        return Err(shape::fail(
            "Stage original predecessor occurrence after receipt",
        ));
    }
    let deadline = shape::text(m, &started, "activation_deadline")?;
    let window = tc::targets::stage_frozen_window(
        m,
        &def["window"],
        endpoint.reference(),
        occurred,
        deadline,
    )?;
    let received = Timestamp::parse_with_work(&c.received_at, m)?;
    for (at, field, inclusive) in [
        (occurrence.micros(), "occurs_before", false),
        (received.micros(), "received_by", true),
        (now.micros(), "accepted_by", true),
    ] {
        let bound = timestamp(m, &window, field)?;
        let ordering = w::compare(m, at, bound)?;
        if if inclusive {
            ordering.is_gt()
        } else {
            !ordering.is_lt()
        } {
            return Err(crate::Error::new(
                "WINDOW_CLOSED",
                "Stage original frozen window",
            ));
        }
    }
    let start = timestamp(m, &window, "starts_at")?;
    if w::compare(m, occurrence.micros(), start)?.is_lt() {
        return Err(crate::Error::new(
            "WINDOW_CLOSED",
            "Stage occurrence before original window",
        ));
    }
    let key = tc::cost_postings::operation_key(m, &a, c)?;
    let tuple = a.array_from_values(
        &[
            &key.value()["scope"],
            instance_value.value(),
            &p["product_ref"],
            &input["slot"],
            &input["predecessor"],
        ],
        m,
    )?;
    let link_id = body::semantic_id(m, "wlink_", "workflow-link", tuple.value())?;
    tc::declared_links::current_link_rights(m, &mut r, c, &input["slot"])?;
    let classification = a.string_value("monetary", m)?;
    let link_schema = a.string_value("ledger-workflow-link/1", m)?;
    let link_value = a.object_from_fields(
        &[
            ("schema", link_schema.value()),
            ("scope", &key.value()["scope"]),
            ("child", instance_value.value()),
            ("product_ref", &p["product_ref"]),
            ("binding_ref", &p["binding_ref"]),
            ("slot", &input["slot"]),
            ("predecessor", &input["predecessor"]),
            ("classification", classification.value()),
            ("authority_ref", authority_ref.value()),
            ("operation_key", key.value()),
        ],
        m,
    )?;
    let link = body::record_borrowed(m, &c.key, "workflow-link", link_id, link_value.value())?;
    let link_ref = owning_reference(m, &a, &link.reference)?;
    old_links.unique(m, &a, link_ref.value())?;
    let anchor_links = old_links.finish(m, &a, true)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let claim_id = closure_claim_id(m, &c.key, &instance)?;
    let schema = a.string_value("ledger-workflow-stage-closure-claim/1", m)?;
    let anchor_ref = owning_reference(m, &a, endpoint.reference())?;
    let claim_value = a.object_from_fields(
        &[
            ("schema", schema.value()),
            ("stage_instance_ref", instance_value.value()),
            ("operation_key", key.value()),
            ("original_payload", &p),
            ("first_work_ref", &started["first_work_ref"]),
            ("anchor_work_ref", anchor_ref.value()),
            ("anchor_link_refs", anchor_links.value()),
            ("window", &window),
            ("authority_ref", authority_ref.value()),
            ("accepted_at", at.value()),
        ],
        m,
    )?;
    let claim = body::record_borrowed(
        m,
        &c.key,
        "workflow-stage-closure-claim",
        claim_id,
        claim_value.value(),
    )?;
    // Posting, retained Invocation debit, terminal state and all ancestor
    // reservations are assembled below under these same owning inputs.
    finish(
        m,
        program,
        c,
        Closing {
            p,
            state,
            r,
            terms,
            terms_owner,
            authority,
            link,
            claim,
            started,
            started_ref,
            members,
            members_owner,
            proofs: proofs.take(),
            prior_net,
            prior_basis,
        },
    )
}

fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: text(m, &r.kind)?,
        id: text(m, &r.id)?,
        digest: text(m, &r.digest)?,
    })
}
struct Closing<'a, 'held> {
    p: WorkingValue,
    state: &'held EconomicStateData<'a>,
    r: Records<'a, 'a>,
    terms: tc::authority::Terms,
    terms_owner: Option<ExternalBufferLease>,
    authority: ConstructedEnvelope,
    link: ConstructedEnvelope,
    claim: ConstructedEnvelope,
    started: RecordBody,
    started_ref: OwnedExactRef,
    members: Vec<Member>,
    members_owner: ExternalBufferLease,
    proofs: Option<tc::work_proofs::WorkProofs>,
    prior_net: BigInt,
    prior_basis: crate::policy::workflow::selected_program::supplier_projection::Values,
}
fn finish<'a>(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    c: &'a DraftContextData,
    closing: Closing<'a, '_>,
) -> Result<Construction<'a, 'a>> {
    let Closing {
        p,
        state,
        mut r,
        terms,
        terms_owner,
        authority,
        link,
        claim,
        started,
        started_ref,
        members,
        members_owner,
        mut proofs,
        prior_net,
        prior_basis,
    } = closing;
    let a = arena(m)?;
    let activity = shape::text(m, &p, "activity")?;
    let prices = tc::pricing::stage(
        program,
        activity,
        &p["facts"],
        (
            &terms.binding["accepted_context"],
            &terms.binding,
            &c.key.source,
        ),
        &prior_net,
        m,
    )?;
    let count = w::usize_add(m, prices.len(), members.len())?;
    let count = w::usize_add(m, count, c.source_observation_inputs.len())?;
    let count = w::usize_add(m, count, 16)?;
    let per = mem::size_of::<ConstructedEnvelope>()
        + mem::size_of::<AuditedValue>()
        + mem::size_of::<(String, ExactRef)>()
        + 3 * (64 + 128 + 71)
        + 512;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&count.into(), &per.into()],
    )?;
    let bytes = count
        .checked_mul(per)
        .ok_or_else(|| shape::fail("Stage closing output controls"))?;
    let owner = a.reserve_external_payload(bytes, m)?;
    let mut outputs = Vec::new();
    outputs
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("Stage closing output allocation"))?;
    let mut steps = Vec::new();
    steps
        .try_reserve_exact(prices.len())
        .map_err(|_| shape::fail("Stage closing step allocation"))?;
    let mut action_slots = Vec::<(String, ExactRef)>::new();
    action_slots
        .try_reserve_exact(prices.len())
        .map_err(|_| shape::fail("Stage closing action basis"))?;
    let claim_ref = owning_reference(m, &a, &claim.reference)?;
    let instance_ref = parse_owned_reference(m, &a, &p["stage_instance_ref"])?;
    let instance = r.get(
        m,
        &c.key,
        instance_ref.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    let decision_id = closure_decision_id(m, &c.key, &instance)?;
    let decision_name = a.string_value(&decision_id, m)?;
    let key = tc::cost_postings::operation_key(m, &a, c)?;
    let one = a.string_value("1", m)?;
    let mut dependencies =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 32)?;
    for member in &members {
        let tuple = a.array_from_values(
            &[
                &member.original["decision_id"],
                decision_name.value(),
                &p["binding_ref"],
            ],
            m,
        )?;
        let identity = body::semantic_id(m, "wdep_", "workflow-dependency", tuple.value())?;
        let (reference, edge) = r.by_identity(
            m,
            &c.key,
            "workflow-dependency",
            &identity,
            "ledger-workflow-dependency/2",
        )?;
        same(m, &edge, "kind", "work_basis")?;
        for (field, value) in [
            ("ancestor_decision_id", &member.original["decision_id"]),
            ("dependent_decision_id", decision_name.value()),
            ("target_ref", &p["stage_instance_ref"]),
            ("original_terms_ref", &p["binding_ref"]),
            ("operation_key", &member.work["key"]),
        ] {
            shape::equal(m, &edge[field], value)?;
        }
        let claim_id = shape::text(m, &member.work, "claim_id")?;
        let permanent = tc::work_keys::decision(m, claim_id)?;
        let root_name = a.string_value(&permanent, m)?;
        let original_id = revision_id(m, root_name.value(), one.value())?;
        let (original_ref, _) = r.by_identity(
            m,
            &c.key,
            "workflow-decision",
            &original_id,
            "ledger-workflow-decision-revision/1",
        )?;
        let capacity = tc::capacity::current::retained_local_root_head(
            m,
            &mut r,
            c,
            &original_ref,
            &original_ref,
            state,
        )?;
        let mut held = false;
        for reserved in &capacity.reserved {
            if shape::same_ref(m, reserved, &reference)? {
                held = true;
            }
        }
        if !held {
            return Err(shape::fail(
                "Stage original ancestor reservation no longer held",
            ));
        }
        let value = owning_reference(m, &a, &reference)?;
        dependencies.unique(m, &a, value.value())?;
    }
    let dependencies = dependencies.finish(m, &a, true)?;
    let mut retail_actions =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut retail_net = BigInt::from(0);
    let mut share = None;
    for price in prices.iter() {
        let maps = shape::array(m, &terms.instantiation, "component_map")?;
        let map = shape::named(m, maps, "template_key", &price.component_key)?;
        let root = shape::named(m, maps, "template_key", &price.root_key)?;
        let catalog_rows = shape::array(m, &terms.catalog, "components")?;
        let mapped = shape::text(m, map, "slot_id")?;
        let catalog = shape::named(m, catalog_rows, "slot_id", mapped)?;
        shape::equal(m, &catalog["root_slot"], &root["slot_id"])?;
        same(m, catalog, "role_slot", &price.role_slot)?;
        let binding_rows = shape::array(m, &terms.binding, "bindings")?;
        let selected = shape::named(m, binding_rows, "slot", &price.binding_slot)?;
        let book = shape::text(m, catalog, "book")?;
        let supplier = shape::same_text(m, book, "supplier")?;
        let schema = if supplier {
            "ledger-workflow-slot-binding-terms/8"
        } else {
            "ledger-workflow-slot-binding-terms/3"
        };
        let selected_ref = parse_owned_reference(m, &a, &selected["binding_ref"])?;
        let selected_terms = r.get(
            m,
            &c.key,
            selected_ref.reference(),
            "workflow-slot-terms",
            schema,
        )?;
        shape::equal(m, &selected_terms["book"], &catalog["book"])?;
        let role_ref = parse_owned_reference(m, &a, &selected_terms["role_ref"])?;
        let role = r.get(
            m,
            &c.key,
            role_ref.reference(),
            "workflow-roles",
            "ledger-workflow-role-assignment/1",
        )?;
        same(m, &role, "role_slot", &price.role_slot)?;
        for field in ["customer", "agreement_id"] {
            shape::equal(m, &role[field], &terms.binding[field])?;
        }
        let mut basis =
            crate::policy::workflow::selected_program::supplier_projection::Values::new(
                m, &a, 128,
            )?;
        for component in &price.basis_components {
            for (name, reference) in &action_slots {
                if shape::same_text(m, name, component)? {
                    let value = owning_reference(m, &a, reference)?;
                    basis.unique(m, &a, value.value())?;
                }
            }
        }
        let instructions = shape::array(
            m,
            program.compiled_plan().canonical_value(),
            "typed_instructions",
        )?;
        let instruction = shape::named(m, instructions, "ordinal", &price.ordinal)?;
        if instruction["rule"]["formula"]["kind"].as_str() == Some("closure_cap") {
            for value in prior_basis.values() {
                basis.unique(m, &a, value.value())?;
            }
        }
        let basis = basis.finish(m, &a, true)?;
        let posting = tc::cost_postings::original_cost_posting(
            m,
            c,
            tc::cost_postings::OriginalCostPosting {
                binding: &terms.binding,
                original_work_claim: &claim.reference,
                component_slot: &map["slot_id"],
                root_slot: &root["slot_id"],
                book: &catalog["book"],
                terms_ref: selected_ref.reference(),
                terms: &selected_terms,
                role_ref: role_ref.reference(),
                decision_id: &decision_id,
                revision: &1.into(),
                replacement: false,
                basis_refs: Some(basis.value()),
            },
            price,
        )?;
        if supplier {
            if share.is_some() {
                return Err(shape::fail("Stage has more than one governing share"));
            }
            let mut selected_member = None;
            for member in &members {
                let mut applicable = false;
                for value in shape::array(m, &member.work["payload"], "invocation_refs")? {
                    let reference = parse_owned_reference(m, &a, value)?;
                    let invocation = r.get(
                        m,
                        &c.key,
                        reference.reference(),
                        "workflow-invocation",
                        "ledger-workflow-invocation/1",
                    )?;
                    if shape::same(
                        m,
                        &invocation["supplier_terms_ref"],
                        &selected["binding_ref"],
                    )? {
                        applicable = true;
                    }
                }
                if applicable && selected_member.replace(member).is_some() {
                    return Err(shape::fail(
                        "ambiguous fixed member governing Stage share Invocation",
                    ));
                }
            }
            let member = selected_member
                .ok_or_else(|| shape::fail("Stage share original Invocation member missing"))?;
            if let Some(invocation) = tc::invocation::consume_stage_share(
                m,
                &mut r,
                c,
                program,
                (
                    &member.work,
                    &terms.binding,
                    &price.binding_slot,
                    &price.atoms.into(),
                ),
                state,
            )? {
                outputs.push(invocation);
            }
            share = Some((
                posting
                    .action
                    .as_ref()
                    .map(|action| copy_ref(m, &action.reference))
                    .transpose()?,
                price.atoms,
                role,
            ));
        } else {
            retail_net = tc::meter::add(m, &retail_net, &price.atoms.into())?;
            if let Some(action) = &posting.action {
                let value = owning_reference(m, &a, &action.reference)?;
                retail_actions.unique(m, &a, value.value())?;
            }
        }
        if let Some(action) = posting.action {
            action_slots.push((
                text(m, &price.component_key)?,
                copy_ref(m, &action.reference)?,
            ));
            outputs.push(action);
        }
        steps.push(posting.step);
    }
    let mut refs = Vec::new();
    refs.try_reserve_exact(action_slots.len())
        .map_err(|_| shape::fail("Stage Action reference views"))?;
    for (_, reference) in &action_slots {
        m.visit("Stage.Action_reference_view")?;
        m.write_bytes(mem::size_of::<&ExactRef>())?;
        refs.push(reference);
    }
    let action_refs = tc::app_cost::reference_set(m, &a, &refs)?;
    let mut step_refs = Vec::new();
    step_refs
        .try_reserve_exact(steps.len())
        .map_err(|_| shape::fail("Stage evaluation step views"))?;
    for step in &steps {
        m.visit("Stage.Evaluation_step_view")?;
        m.write_bytes(mem::size_of::<&Value>())?;
        step_refs.push(step.value());
    }
    let all_steps = a.array_from_values(&step_refs, m)?;
    let plan = tc::working_value::owning_definition(m, &a, program.compiled_plan().reference())?;
    let eval_schema = a.string_value("ledger-workflow-evaluation-proof/1", m)?;
    let value = a.object_from_fields(
        &[
            ("schema", eval_schema.value()),
            ("subject_ref", claim_ref.value()),
            ("policy_ref", &p["policy_ref"]),
            ("compiled_plan_ref", plan.value()),
            (
                "compiler_ref",
                &program.compiled_plan().canonical_value()["compiler_ref"],
            ),
            ("binding_ref", &p["binding_ref"]),
            ("fact_schema_ref", &p["fact_schema_ref"]),
            ("facts", &p["facts"]),
            ("context", &terms.binding["accepted_context"]),
            ("steps", all_steps.value()),
        ],
        m,
    )?;
    let evaluation = body::document_borrowed(m, "workflow-evaluation", value.value())?;
    let evaluation_ref = owning_reference(m, &a, &evaluation.reference)?;
    let authority_ref = owning_reference(m, &a, &authority.reference)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let constants=m.parse_owned_representation(&a,br#"{"schema":"ledger-workflow-decision-revision/1","kind":"outcome","transition":"initial","status":"active"}"#,RepresentationKind::SourceDefinition)?;
    let decision_body = a.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            ("decision_id", decision_name.value()),
            ("revision", one.value()),
            ("subject_ref", claim_ref.value()),
            ("kind", &constants.value()["kind"]),
            ("transition", &constants.value()["transition"]),
            ("status", &constants.value()["status"]),
            ("original_terms_ref", &p["binding_ref"]),
            ("authority_ref", authority_ref.value()),
            ("evaluation_ref", evaluation_ref.value()),
            ("actions_appended", action_refs.value()),
            ("active_action_refs", action_refs.value()),
            ("dependency_refs", dependencies.value()),
            ("operation_key", key.value()),
            ("accepted_at", at.value()),
        ],
        m,
    )?;
    let decision_ref_id = revision_id(m, decision_name.value(), one.value())?;
    let decision = body::record_borrowed(
        m,
        &c.key,
        "workflow-decision",
        decision_ref_id,
        decision_body.value(),
    )?;
    let instance_ref = parse_owned_reference(m, &a, &p["stage_instance_ref"])?;
    let state_constants = m.parse_owned_representation(
        &a,
        br#"{"schema":"ledger-workflow-stage-state/1","revision":"3","state":"closed"}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let started_value = owning_reference(m, &a, started_ref.reference())?;
    let state_value = a.object_from_fields(
        &[
            ("schema", &state_constants.value()["schema"]),
            ("stage_instance_ref", &p["stage_instance_ref"]),
            ("revision", &state_constants.value()["revision"]),
            ("state", &state_constants.value()["state"]),
            ("operation_key", key.value()),
            ("authority_ref", authority_ref.value()),
            ("accepted_at", at.value()),
            ("prior_ref", started_value.value()),
            ("first_work_ref", &started["first_work_ref"]),
            ("first_work_accepted_at", &started["first_work_accepted_at"]),
            ("activation_deadline", &started["activation_deadline"]),
            ("closure_claim_ref", claim_ref.value()),
        ],
        m,
    )?;
    let state_id = state_id(
        m,
        &c.key,
        instance_ref.reference(),
        &state_constants.value()["revision"],
    )?;
    let closed = body::record_borrowed(
        m,
        &c.key,
        "workflow-stage-state",
        state_id,
        state_value.value(),
    )?;
    if let Some((Some(parent), atoms, role)) = share {
        let parent = owning_reference(m, &a, &parent)?;
        let inputs = retail_actions.finish(m, &a, true)?;
        let remainder = tc::meter::sub(m, &retail_net, &atoms.into())?;
        if tc::meter::cmp(m, &remainder, &0.into())?.is_lt() {
            return Err(shape::fail("negative Stage host allocation remainder"));
        }
        let retail_role = r.get(
            m,
            &c.key,
            &terms.role_ref,
            "workflow-roles",
            "ledger-workflow-role-assignment/1",
        )?;
        shape::equal(m, &retail_role["recipient"], &role["bearer"])?;
        let schema = a.string_value("ledger-workflow-allocation/1", m)?;
        for (partition, amount, recipient) in [
            ("supplier_share", BigInt::from(atoms), &role["recipient"]),
            ("host_remainder", remainder, &retail_role["recipient"]),
        ] {
            let partition = a.string_value(partition, m)?;
            let amount = tc::cost_postings::amount(m, &a, &amount)?;
            let value = a.object_from_fields(
                &[
                    ("schema", schema.value()),
                    ("stage_instance_ref", &p["stage_instance_ref"]),
                    ("allocation_parent", parent.value()),
                    ("inputs", inputs.value()),
                    ("partition", partition.value()),
                    ("amount", amount.value()),
                    ("recipient", recipient),
                ],
                m,
            )?;
            outputs.push(body::document_borrowed(
                m,
                "workflow-allocation",
                value.value(),
            )?);
        }
    }
    let instance = r.get(
        m,
        &c.key,
        instance_ref.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    let chain = transition_chain(
        m,
        &mut r,
        c,
        program,
        &instance,
        state.financial,
        Some(&link.reference),
    )?;
    let chain_ref = copy_ref(m, &chain.reference)?;
    let link_ref = copy_ref(m, &link.reference)?;
    let decision_ref = copy_ref(m, &decision.reference)?;
    outputs.extend([authority, link, claim, evaluation, decision, closed, chain]);
    if let Some(proofs) = &mut proofs {
        proofs.attach(m, &mut outputs)?;
    }
    let proof_owner = proofs.map(tc::work_proofs::WorkProofs::into_owner);
    for input in &c.source_observation_inputs {
        let raw = r.canonical(m, &input.reference)?;
        m.new_body(raw.len())?;
        let bytes = m.copy_owned_representation(&a, raw, RepresentationKind::SourceClosure)?;
        let reference = copy_ref(m, &input.reference)?;
        outputs.push(ConstructedEnvelope::from_owned(
            reference,
            bytes,
            &a,
            m,
            RepresentationKind::SourceClosure,
        )?);
    }
    let normalized = p.bytes(m)?;
    let first_ref = parse_owned_reference(m, &a, &started["first_work_ref"])?;
    let mut anchor = None;
    for member in &members {
        if shape::same_ref(m, member.work_ref.reference(), first_ref.reference())? {
            anchor = Some(member);
        }
    }
    let original_work = anchor
        .ok_or_else(|| shape::fail("Stage original first member absent"))?
        .work
        .clone();
    Ok(Construction {
        economic: EconomicConstruction {
            terms,
            original_work: OriginalWorkPayload::Retained(original_work),
        },
        original_reservations: None,
        financial_reservations: None,
        normalized_payload: normalized,
        new_envelopes: outputs,
        reused_result: None,
        evaluation_steps: Vec::new(),
        proposed_heads: ProposedHeads {
            decision: vec![(decision_id, "1".into(), decision_ref)],
            family: Vec::new(),
            chain: Some(chain_ref),
        },
        claim_introductions: Vec::new(),
        link_introductions: vec![link_ref],
        prospective_obligations: Vec::new(),
        records: r,
        history_edges: Vec::new(),
        app_effects: None,
        _app_causal: None,
        current_capacity: None,
        candidate_heads: None,
        candidate_introductions: None,
        financial_observation: None,
        funding_effects: None,
        control_owners: [
            Some(owner),
            terms_owner,
            proof_owner,
            None,
            Some(members_owner),
            None,
            None,
            None,
            None,
        ],
    })
}

/// Reconstruct original Stage basis through the same fixed Work consumer used
/// at closure; this is never a current-price or whole-chain sum.
pub(in crate::policy::workflow::transition_constructor) fn prior_net(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    binding: &Value,
    instance: &Value,
    state: &DraftStateData,
) -> Result<(BigInt, AuditedValue)> {
    let a = arena(m)?;
    let reference = instance_id(m, &c.key, instance)?;
    let (reference, _) = r.by_identity(
        m,
        &c.key,
        "workflow-stage-instance",
        &reference,
        "ledger-workflow-stage-instance/1",
    )?;
    let value = owning_reference(m, &a, &reference)?;
    let mut basis =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut net = BigInt::from(0);
    for row in shape::array(m, instance, "expected")? {
        let member = member(
            m,
            r,
            c,
            program,
            (binding, instance, value.value(), row),
            (state, false),
        )?;
        let amount = booked_basis(
            m,
            r,
            c,
            program,
            (binding, instance, row, &member),
            &mut basis,
        )?;
        net = tc::meter::add(m, &net, &amount)?;
    }
    let refs = basis.finish(m, &a, true)?;
    Ok((net, refs))
}
