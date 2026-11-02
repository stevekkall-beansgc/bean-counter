//! Fixed selected observe_cost consumer. A retained source assertion is an
//! observation, never supplier authorization or a payable pricing input.
use super::{
    authority, cost_postings,
    data::*,
    emitted_record::ConstructedEnvelope,
    inventory,
    records::Records,
    shape,
    work_proofs::{self, WorkProofRequest},
    working_value::{owning_reference, parse_owned_reference},
    CoreMeter,
};
use crate::{
    canonical::{self, audit_arena::AuditedValue},
    policy::workflow::selected_program::SelectedProgram,
    validation_work::{self as w},
    Error, Result,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) fn evaluation_schema(program: &SelectedProgram) -> &'static str {
    if program.uses_component_lifecycle_recipe() {
        "ledger-workflow-component-evaluation-proof/1"
    } else {
        "ledger-workflow-evaluation-proof/1"
    }
}
pub(super) struct OriginalPostings {
    pub actions: Vec<ConstructedEnvelope>,
    pub steps: Vec<AuditedValue>,
    _owner: crate::canonical::audit_arena::ExternalBufferLease,
}
pub(super) fn reconstruct(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: OriginalObservationWork<'_>,
) -> Result<Option<OriginalPostings>> {
    if !program.uses_component_lifecycle_recipe() {
        return Ok(None);
    }
    let activity = shape::text(m, original.payload, "activity")?;
    let n = count(m, program, activity)?;
    let per = std::mem::size_of::<ConstructedEnvelope>()
        + std::mem::size_of::<AuditedValue>()
        + 64
        + 128
        + 71;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&n.into(), &per.into()],
    )?;
    let size = n
        .checked_mul(per)
        .ok_or_else(|| shape::fail("observation original controls"))?;
    let size = w::usize_add(m, size, std::mem::size_of::<OriginalPostings>())?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("observation original owner"))?;
    let owner = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    let mut actions = Vec::new();
    let mut steps = Vec::new();
    actions
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("observation original action vector"))?;
    steps
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("observation original step vector"))?;
    append_original(m, r, c, program, original, (&mut actions, &mut steps))?;
    Ok(Some(OriginalPostings {
        actions,
        steps,
        _owner: owner,
    }))
}
pub(super) struct OriginalObservationWork<'a> {
    pub terms: &'a authority::Terms,
    pub payload: &'a Value,
    pub claim_ref: &'a ExactRef,
    pub decision_id: &'a str,
    pub key: &'a OperationKey,
    pub received_at: &'a str,
}
pub(super) fn count(m: &mut CoreMeter, program: &SelectedProgram, activity: &str) -> Result<usize> {
    if !program.uses_component_lifecycle_recipe() {
        return Ok(0);
    }
    let mut count = 0;
    for instruction in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_rule")? {
            continue;
        }
        let rule = &instruction["rule"];
        let name = shape::text(m, rule, "activity")?;
        let kind = shape::text(m, &rule["formula"], "kind")?;
        if shape::same_text(m, name, activity)? && shape::same_text(m, kind, "observe_cost")? {
            count = w::usize_add(m, count, 1)?;
        }
    }
    Ok(count)
}
/// Caller reserves output-vector controls for count() before this shared fresh
/// and original posting interpreter. Every emitted body retains its paid arena.
pub(super) fn append_original(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: OriginalObservationWork<'_>,
    outputs: (&mut Vec<ConstructedEnvelope>, &mut Vec<AuditedValue>),
) -> Result<()> {
    if !program.uses_component_lifecycle_recipe() {
        return Ok(());
    }
    let (actions, steps) = outputs;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning observation consumer"))?;
    let p = original.payload;
    let terms = original.terms;
    let activity = shape::text(m, p, "activity")?;
    let status = shape::text(m, p, "status")?;
    let failed = shape::same_text(m, status, "failed")?;
    let namespace = shape::text(m, &terms.binding, "source_namespace")?;
    let query = WorkProofRequest {
        payload: p,
        facts: &p["facts"],
        namespace,
        received_at: original.received_at,
        only_field: None,
        worker_authority: None,
    };
    let expected_subject = work_proofs::subject(m, c, &query)?;
    let present = shape::array(m, p, "evidence_refs")?;
    if w::compare(m, present.len(), 32usize)?.is_gt() {
        return Err(shape::fail("observation Work evidence bound"));
    }
    // Exactly the existing Work evidence bound; no second evidence inventory.
    let mut used = [false; 32];
    for instruction in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        m.visit("component.observation_instruction")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_rule")? {
            continue;
        }
        let rule = &instruction["rule"];
        let rule_activity = shape::text(m, rule, "activity")?;
        let formula = &rule["formula"];
        let kind = shape::text(m, formula, "kind")?;
        if !shape::same_text(m, rule_activity, activity)?
            || !shape::same_text(m, kind, "observe_cost")?
        {
            continue;
        }
        let ordinal = shape::text(m, instruction, "ordinal")?;
        let binding_slot = shape::text(m, rule, "binding_slot")?;
        let original_slots = shape::array(m, program.original_policy_value(), "binding_slots")?;
        let declared = shape::named(m, original_slots, "name", binding_slot)?;
        let book = shape::text(m, declared, "book")?;
        if !shape::same_text(m, book, "cost_observation")? {
            return Err(shape::fail("observation declared book"));
        }
        let bindings = shape::array(m, &terms.binding, "bindings")?;
        let slot_row = shape::named(m, bindings, "slot", binding_slot)?;
        let terms_ref = parse_owned_reference(m, &arena, &slot_row["binding_ref"])?;
        let observation_terms = r.get(
            m,
            &c.key,
            terms_ref.reference(),
            "workflow-slot-terms",
            "ledger-workflow-observation-terms/1",
        )?;
        let key = cost_postings::operation_key(m, &arena, c)?;
        shape::equal(m, &observation_terms["scope"], &key.value()["scope"])?;
        for field in ["agreement_id", "customer"] {
            shape::equal(m, &observation_terms[field], &terms.binding[field])?;
        }
        shape::equal(m, &observation_terms["slot"], &slot_row["slot"])?;
        let role_ref = parse_owned_reference(m, &arena, &observation_terms["role_ref"])?;
        let role = r.get(
            m,
            &c.key,
            role_ref.reference(),
            "workflow-roles",
            "ledger-workflow-role-assignment/1",
        )?;
        shape::equal(m, &role["scope"], &key.value()["scope"])?;
        for field in ["agreement_id", "customer", "role_slot"] {
            shape::equal(m, &role[field], &observation_terms[field])?;
        }
        let roles = shape::array(m, &terms.binding, "roles_refs")?;
        if !shape::member(m, roles, &observation_terms["role_ref"])? {
            return Err(shape::fail("observation original role membership"));
        }
        inventory::slots::validate_original_projection(
            m,
            program,
            &terms.binding,
            &terms.catalog,
            &terms.instantiation,
            &observation_terms,
            &role,
        )?;
        inventory::slots::validate_app_funding_projection(
            m,
            program,
            &terms.binding,
            &observation_terms,
            &role,
        )?;
        super::app_cost::original_future_sources(
            m,
            r,
            c,
            &terms.binding,
            &observation_terms,
            program,
        )?;
        let source_slot = shape::text(m, formula, "evidence_slot")?;
        let mut selected = None;
        for (index, reference) in present.iter().enumerate() {
            let reference = parse_owned_reference(m, &arena, reference)?;
            if !shape::same_text(m, &reference.reference().kind, "workflow-fact-attestation")? {
                continue;
            }
            let body = r.provenance(m, &c.key, reference.reference())?;
            let schema = shape::text(m, &body, "schema")?;
            if !shape::same_text(m, schema, "ledger-workflow-cost-observation-evidence/1")? {
                continue;
            }
            super::retention::closed_observation(m, &body)?;
            let slot = shape::text(m, &body, "evidence_slot")?;
            if !shape::same_text(m, slot, source_slot)? {
                continue;
            }
            // Explicitly named observation assertions must concern this exact
            // immutable Work subject; another Work cannot fill a missing input.
            shape::equal(m, &body["subject"], expected_subject.value())?;
            if selected.is_some() {
                return Err(shape::fail("ambiguous applicable observation assertion"));
            }
            selected = Some((index, reference, body));
        }
        let predicate = program.evaluate_work_rule_condition_operational(
            m,
            &rule["when"],
            activity,
            &p["facts"],
            (
                &terms.binding["accepted_context"],
                Some((&terms.binding, &original.key.source, binding_slot)),
            ),
            status,
        )?;
        let mut verification = None;
        if let Some((att_index, reference, att)) = &selected {
            let att_value = owning_reference(m, &arena, reference.reference())?;
            for (index, candidate) in present.iter().enumerate() {
                let candidate = parse_owned_reference(m, &arena, candidate)?;
                if !shape::same_text(
                    m,
                    &candidate.reference().kind,
                    "workflow-evidence-verification",
                )? {
                    continue;
                }
                let body = r.get(
                    m,
                    &c.key,
                    candidate.reference(),
                    "workflow-evidence-verification",
                    "ledger-workflow-evidence-verification/1",
                )?;
                if !shape::same(m, &body["attestation_ref"], att_value.value())? {
                    continue;
                }
                if verification.is_some() {
                    return Err(shape::fail("ambiguous observation verification"));
                }
                verification = Some((index, candidate, body));
            }
            let (verified_index, _, verified) = verification
                .as_ref()
                .ok_or_else(|| shape::fail("retained observation verification missing"))?;
            validate_verification(
                m,
                r,
                c,
                reference.reference(),
                att,
                verified,
                &p["binding_ref"],
            )?;
            work_proofs::source::retained_observation(
                m,
                r,
                c,
                program,
                p,
                (&terms.binding, &observation_terms),
                (source_slot, att, verified, original.received_at),
            )?;
            m.visit("observation.consume_assertion_and_verifier")?;
            used[*att_index] = true;
            used[*verified_index] = true;
        }
        if !failed && predicate == "true" && selected.is_none() {
            let missing = shape::text(m, rule, "dependency")?;
            if !shape::same_text(m, missing, "on_missing_wait")? {
                return Err(Error::new(
                    "MISSING_DEPENDENCY",
                    "observe_cost original nominated assertion",
                ));
            }
        }
        let basis = match (&selected, &verification) {
            (Some((_, att, _)), Some((_, verified, _))) => {
                super::app_cost::reference_set(m, &arena, &[att.reference(), verified.reference()])?
            }
            _ => arena.array_from_values(&[], m)?,
        };
        let known = selected
            .as_ref()
            .map(|(_, _, att)| inventory::check::amount(m, &att["amount"]))
            .transpose()?;
        let component = shape::text(m, rule, "component_key")?;
        let mappings = shape::array(m, &terms.instantiation, "component_map")?;
        let mapping = shape::named(m, mappings, "template_key", component)?;
        let components = shape::array(m, &terms.catalog, "components")?;
        let slot = shape::text(m, mapping, "slot_id")?;
        let catalog = shape::named(m, components, "slot_id", slot)?;
        shape::equal(m, &catalog["book"], &observation_terms["book"])?;
        shape::equal(m, &catalog["role_slot"], &observation_terms["role_slot"])?;
        // observe_cost is a direct nonpayable root, not an attached monetary basis.
        shape::equal(m, &catalog["root_slot"], &mapping["slot_id"])?;
        let posting = cost_postings::original_observation_posting(
            m,
            original.key,
            cost_postings::OriginalCostPosting {
                binding: &terms.binding,
                original_work_claim: original.claim_ref,
                component_slot: &mapping["slot_id"],
                root_slot: &catalog["root_slot"],
                book: &catalog["book"],
                terms_ref: terms_ref.reference(),
                terms: &observation_terms,
                role_ref: role_ref.reference(),
                decision_id: original.decision_id,
                revision: &1.into(),
                replacement: false,
                basis_refs: Some(basis.value()),
            },
            (ordinal, predicate, failed, known.as_ref()),
        )?;
        if let Some(action) = posting.action {
            actions.push(action);
        }
        steps.push(posting.step);
    }
    for (index, _) in present.iter().enumerate() {
        m.visit("observation.all_present_Work_evidence_used")?;
        if !used[index] {
            return Err(shape::fail("unused or off-slot observation Work evidence"));
        }
    }
    Ok(())
}
fn validate_verification(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    reference: &ExactRef,
    att: &Value,
    verification: &Value,
    binding_ref: &Value,
) -> Result<()> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning observation verification"))?;
    shape::closed(
        m,
        verification,
        &[
            "schema",
            "attestation_ref",
            "attestation_raw_sha256",
            "attestation_digest",
            "verified_source",
            "verification_primitive",
            "source_grant_ref",
            "source_grant_revision",
            "authenticated_principal_ref",
            "trusted_received_at",
            "subject_binding_refs",
            "verification_evidence_refs",
        ],
        &[],
    )?;
    let att_ref = owning_reference(m, &arena, reference)?;
    shape::equal(m, &verification["attestation_ref"], att_ref.value())?;
    let digest = arena.string_value(&reference.digest, m)?;
    shape::equal(m, &verification["attestation_digest"], digest.value())?;
    shape::equal(m, &verification["verified_source"], &att["source"])?;
    let primitive = shape::text(m, verification, "verification_primitive")?;
    if !shape::same_text(m, primitive, "authenticated-nominated-source-retention/1")? {
        return Err(shape::fail("observation verification primitive"));
    }
    let subjects = arena.array_from_values(&[binding_ref], m)?;
    shape::equal(m, &verification["subject_binding_refs"], subjects.value())?;
    if !shape::array(m, verification, "verification_evidence_refs")?.is_empty() {
        return Err(shape::fail("observation supplemental verifier evidence"));
    }
    let raw = r.canonical(m, reference)?;
    m.hash_bytes(raw.len())?;
    let hash = canonical::hex_with_work(&Sha256::digest(raw), m)?;
    let actual = shape::text(m, verification, "attestation_raw_sha256")?;
    if !shape::same_text(m, &hash, actual)? {
        return Err(shape::fail("observation raw assertion hash"));
    }
    let supporting = shape::array(m, att, "supporting_evidence_refs")?;
    if w::compare(m, supporting.len(), 16usize)?.is_gt() {
        return Err(shape::fail("observation supporting reference bound"));
    }
    for value in supporting {
        let reference = parse_owned_reference(m, &arena, value)?;
        r.provenance(m, &c.key, reference.reference())?;
    }
    Ok(())
}

#[cfg(test)]
mod mixed_plan_tests {
    use super::*;
    use crate::{
        canonical::audit_arena::AuditArena,
        policy::workflow::selected_program::{
            compile_selected_publication, SelectedCompilerSupport,
        },
        validation_work::{OriginalAuditSession, ResourceLimits},
    };

    #[test]
    fn observation_count_ignores_family_instructions_in_actual_component_plan() {
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut author = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support =
            SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut author)
                .unwrap();
        let input = super::super::pricing::component_tests::component_input(&support, &mut author);
        let mut compilation = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let program = compile_selected_publication(input, &support, &mut compilation).unwrap();
        let instructions = program.compiled_plan().canonical_value()["typed_instructions"]
            .as_array()
            .unwrap();
        assert!(instructions
            .iter()
            .any(|i| i["opcode"] == "evaluate_family" && i.get("rule").is_none()));
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = session.meter_mut().local_economic_meter().unwrap();
        assert_eq!(count(&mut m, &program, "support:complete").unwrap(), 1);
        assert_eq!(count(&mut m, &program, "support:tool").unwrap(), 0);
        let (_, context, _, inputs) = super::super::first_work_tests::test_inputs();
        let views: Vec<_> = inputs
            .iter()
            .map(|row| EconomicRecordView {
                reference: &row.reference,
                canonical_body: &row.canonical_body,
            })
            .collect();
        let mut records = Records::borrowed_app(&mut m, &arena, &context.key, &views, &[]).unwrap();
        let binding = super::super::record_body::RecordBody::Legacy(
            serde_json::json!({"source_namespace":"fixture-source"}),
        );
        let blank = super::super::record_body::RecordBody::Legacy(serde_json::json!({}));
        let reference = inputs[0].reference.clone();
        let terms = authority::Terms {
            binding,
            accepted: blank.clone(),
            _activation: blank.clone(),
            catalog: blank.clone(),
            instantiation: blank.clone(),
            slot_terms: blank,
            slot_terms_ref: reference.clone(),
            role_ref: reference.clone(),
        };
        let payload = serde_json::json!({"schema":"ledger-workflow-work/1","activity":"support:tool","status":"succeeded","facts":{},"evidence_refs":[],"business_operation_id":"tool","customer":"customer","product_ref":{},"binding_ref":{}});
        let original = OriginalObservationWork {
            terms: &terms,
            payload: &payload,
            claim_ref: &reference,
            decision_id: "decision",
            key: &context.key,
            received_at: &context.received_at,
        };
        let (mut actions, mut steps) = (Vec::new(), Vec::new());
        append_original(
            &mut m,
            &mut records,
            &context,
            &program,
            original,
            (&mut actions, &mut steps),
        )
        .unwrap();
        assert!(actions.is_empty() && steps.is_empty());
    }
}
