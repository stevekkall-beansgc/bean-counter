//! One original supplier component/line selection for finalize and correction.
//! Original monetary inputs come from the frozen plan and explicit retained
//! evidence. Saved decision/actions are comparison and inverse operands only.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    CoreMeter, arithmetic, body, data::*, meter, record_body::RecordBody, records::Records, shape,
    working_value::WorkingValue,
};
use super::operands::{self, FinalEvidence, InvocationSelection};
use crate::{
    Error, Result,
    canonical::audit_arena::ExternalBufferLease,
    domain::Timestamp,
    validation_work::{self as w},
};
use num_bigint::BigInt;
use serde_json::{Value, json};

pub(super) struct CurrentCost {
    pub claim_ref: ExactRef,
    pub claim: RecordBody,
    pub revision_ref: ExactRef,
    pub revision: RecordBody,
    pub decision_ref: ExactRef,
    pub decision: RecordBody,
    pub owner_ref: ExactRef,
    pub owner: RecordBody,
    pub consumption_ref: ExactRef,
    pub consumption: RecordBody,
    pub evidence: RecordBody,
}
pub(super) struct CostPlan<'p> {
    pub request: WorkingValue,
    pub selected: InvocationSelection,
    pub proof: FinalEvidence,
    pub original_work_claim_ref: ExactRef,
    pub original_work_claim: RecordBody,
    pub instruction: &'p Value,
    pub delayed: &'p Value,
    pub supplier_component_slot: String,
    pub decision_id: String,
    pub next_revision: BigInt,
    pub current: Option<CurrentCost>,
    _ownership: ExternalBufferLease,
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
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
pub(in crate::policy::workflow::transition_constructor) fn normalize_request(
    m: &mut CoreMeter,
    p: &mut WorkingValue,
) -> Result<bool> {
    let schema = shape::text(m, p, "schema")?;
    let correct = shape::same_text(m, schema, "ledger-workflow-cost-correct/1")?;
    if correct {
        shape::closed(
            m,
            p,
            &[
                "schema",
                "original_cost_claim_ref",
                "expected_revision",
                "replacement_final_cost_proof",
                "dependent_reversals",
                "evidence_refs",
            ],
            &[],
        )?;
        let raw = shape::text(m, p, "expected_revision")?;
        let revision = super::super::super::facts::counter_with_work(raw, m)?;
        if !w::compare(m, revision, 0u64)?.is_gt() {
            return Err(shape::fail("positive cost expected revision"));
        }
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning supplier normalization"))?;
        let value = super::super::close::counter_value(m, &arena, revision)?;
        p.replace(m, "expected_revision", value.value())?;
        shape::reference(m, &p["original_cost_claim_ref"])?;
        let dependents = shape::array(m, p, "dependent_reversals")?;
        if w::compare(m, dependents.len(), 31usize)?.is_gt() {
            return Err(shape::fail("cost dependent count"));
        }
        // Each complete closure object is a named semantic set. Normalize the
        // nested Ref set before the containing object set and semantic matching.
        for index in 0..dependents.len() {
            let item = &p["dependent_reversals"][index];
            shape::closed(
                m,
                item,
                &[
                    "decision_ref",
                    "expected_revision",
                    "authority_evidence_refs",
                ],
                &[],
            )?;
            shape::reference(m, &item["decision_ref"])?;
            let raw = shape::text(m, item, "expected_revision")?;
            let value = super::super::super::facts::counter_with_work(raw, m)?;
            if !w::compare(m, value, 0u64)?.is_gt() {
                return Err(shape::fail("positive cost dependent revision"));
            }
            let values = shape::array(m, item, "authority_evidence_refs")?;
            if w::compare(m, values.len(), 16usize)?.is_gt() {
                return Err(shape::fail("cost dependent authority evidence bound"));
            }
            let mut refs = Vec::new();
            for reference in values {
                m.visit("cost.normalize_dependent_evidence")?;
                refs.push(shape::reference(m, reference)?);
            }
            let refs = body::ref_set(m, refs)?;
            p.replace_reference_array_field(
                m,
                "dependent_reversals",
                index,
                "authority_evidence_refs",
                &refs,
            )?;
            let value = super::super::close::counter_value(m, &arena, value)?;
            match p {
                WorkingValue::Owned(p) => p.replace_existing_array_field_owned(
                    "dependent_reversals",
                    index,
                    "expected_revision",
                    value,
                    m,
                )?,
                WorkingValue::Legacy(_) => {
                    return Err(shape::fail("cost normalization requires owning profile"));
                }
            }
        }
        p.normalize_set(m, "dependent_reversals", false)?;
    } else {
        if !shape::same_text(m, schema, "ledger-workflow-cost-finalize/1")? {
            return Err(shape::fail("cost request tuple"));
        }
        shape::closed(
            m,
            p,
            &[
                "schema",
                "original_work_ref",
                "original_invocation_ref",
                "supplier_component_slot",
                "final_cost_proof",
                "evidence_refs",
            ],
            &[],
        )?;
        shape::reference(m, &p["original_work_ref"])?;
        shape::reference(m, &p["original_invocation_ref"])?;
        super::super::inventory::binding::id(m, &p["supplier_component_slot"], 128)?;
    }
    let proof = if correct {
        &p["replacement_final_cost_proof"]
    } else {
        &p["final_cost_proof"]
    };
    shape::closed(m, proof, &["attestation_ref", "verification_ref"], &[])?;
    shape::reference(m, &proof["attestation_ref"])?;
    shape::reference(m, &proof["verification_ref"])?;
    let evidence = shape::array(m, p, "evidence_refs")?;
    if w::compare(m, evidence.len(), 16usize)?.is_gt() {
        return Err(shape::fail("cost evidence bound"));
    }
    for reference in evidence {
        shape::reference(m, reference)?;
    }
    p.normalize_set(m, "evidence_refs", false)?;
    Ok(correct)
}
fn current(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &Value,
    state: &EconomicStateData<'_>,
) -> Result<CurrentCost> {
    let (claim_ref, claim) = load(
        m,
        r,
        c,
        &p["original_cost_claim_ref"],
        "workflow-cost",
        "ledger-workflow-cost-claim/1",
    )?;
    shape::closed(
        m,
        &claim,
        &[
            "schema",
            "line_key",
            "owner_ref",
            "original_binding_ref",
            "original_work_ref",
            "original_invocation_ref",
            "supplier_component_slot",
            "original_evidence_ref",
            "operation_key",
        ],
        &[],
    )?;
    let supplier = state
        .supplier
        .ok_or_else(|| shape::fail("missing complete supplier heads"))?;
    let mut head = None;
    for candidate in supplier.cost_heads {
        m.visit("cost.current_revision_head")?;
        if shape::same_ref(m, &candidate.cost_claim_ref, &claim_ref)? {
            if head.is_some() {
                return Err(shape::fail("duplicate cost head"));
            }
            head = Some(candidate);
        }
    }
    let head = head.ok_or_else(|| shape::fail("missing cost revision head"))?;
    let revision = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-cost",
        "ledger-workflow-cost-revision/1",
    )?;
    shape::closed(
        m,
        &revision,
        &[
            "schema",
            "cost_claim_ref",
            "revision",
            "owner_ref",
            "current_decision_ref",
            "current_evidence_ref",
            "invocation_state_ref",
            "operation_key",
            "consumption_ref",
        ],
        &["prior_ref"],
    )?;
    eq(
        m,
        &revision["cost_claim_ref"],
        &p["original_cost_claim_ref"],
    )?;
    eq(m, &revision["revision"], &json!(head.revision))?;
    eq(m, &revision["revision"], &p["expected_revision"])?;
    let (decision_ref, decision) = load(
        m,
        r,
        c,
        &revision["current_decision_ref"],
        "workflow-decision",
        "ledger-workflow-decision-revision/2",
    )?;
    for (key, want) in [
        ("schema", json!("ledger-workflow-decision-revision/2")),
        ("kind", json!("cost")),
        ("status", json!("active")),
        ("subject_ref", json!(claim_ref)),
        ("revision", revision["revision"].clone()),
        ("original_terms_ref", claim["original_binding_ref"].clone()),
    ] {
        eq(m, &decision[key], &want)?;
    }
    let mut decision_head = None;
    for candidate in &state.financial.decision_heads {
        m.visit("cost.current_decision_head")?;
        if shape::same(m, &json!(candidate.decision_id), &decision["decision_id"])? {
            if decision_head.is_some() {
                return Err(shape::fail("duplicate cost decision head"));
            }
            decision_head = Some(candidate);
        }
    }
    let decision_head =
        decision_head.ok_or_else(|| shape::fail("missing current cost decision"))?;
    if !shape::same_ref(m, &decision_head.reference, &decision_ref)? {
        return Err(shape::fail("stale cost decision"));
    }
    eq(m, &decision["revision"], &json!(decision_head.revision))?;
    let (owner_ref, owner) = load(
        m,
        r,
        c,
        &revision["owner_ref"],
        "workflow-cost",
        "ledger-cost-line-owner/1",
    )?;
    let (consumption_ref, consumption) = load(
        m,
        r,
        c,
        &revision["consumption_ref"],
        "workflow-cost",
        "ledger-cost-obligation-consumption/1",
    )?;
    eq(m, &consumption["cost_claim_ref"], &json!(claim_ref))?;
    eq(m, &consumption["revision"], &revision["revision"])?;
    eq(m, &owner["revision"], &revision["revision"])?;
    eq(m, &owner["line_key"], &claim["line_key"])?;
    eq(
        m,
        &owner["original_invocation_ref"],
        &claim["original_invocation_ref"],
    )?;
    eq(
        m,
        &owner["current_evidence_ref"],
        &revision["current_evidence_ref"],
    )?;
    let (_, evidence) = load(
        m,
        r,
        c,
        &revision["current_evidence_ref"],
        "workflow-fact-attestation",
        "ledger-workflow-supplier-final-cost/1",
    )?;
    Ok(CurrentCost {
        claim_ref,
        claim,
        revision_ref: head.reference.clone(),
        revision,
        decision_ref,
        decision,
        owner_ref,
        owner,
        consumption_ref,
        consumption,
        evidence,
    })
}
pub(super) fn prepare<'p>(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &'p SelectedProgram,
    mut request: WorkingValue,
    state: &EconomicStateData<'_>,
) -> Result<CostPlan<'p>> {
    if !program.uses_owner_local_cost_recipe() {
        return Err(Error::new(
            "ORIGINAL_VERIFIER_UNAVAILABLE",
            "original supplier recipe",
        ));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier selection"))?;
    let metadata = w::usize_add(m, std::mem::size_of::<CostPlan<'_>>(), 8192)?;
    let ownership = arena.reserve_external_payload(metadata, m)?;
    let correct = normalize_request(m, &mut request)?;
    let current = if correct {
        Some(current(m, r, c, &request, state)?)
    } else {
        None
    };
    let (work_ref, invocation_ref) = match &current {
        None => (
            shape::reference(m, &request["original_work_ref"])?,
            shape::reference(m, &request["original_invocation_ref"])?,
        ),
        Some(old) => (
            shape::reference(m, &old.claim["original_work_ref"])?,
            shape::reference(m, &old.claim["original_invocation_ref"])?,
        ),
    };
    let selected = operands::invocation(m, r, c, &work_ref, &invocation_ref, state)?;
    eq(
        m,
        &selected.binding["compiled_plan_ref"],
        &json!(program.compiled_plan().reference()),
    )?;
    eq(
        m,
        &selected.binding["policy_ref"],
        &json!(program.root_policy_ref()),
    )?;
    eq(
        m,
        &selected.binding["product_ref"],
        &json!(program.product_ref()),
    )?;
    eq(
        m,
        &selected.work["payload"]["policy_ref"],
        &selected.binding["policy_ref"],
    )?;
    eq(
        m,
        &selected.work["payload"]["product_ref"],
        &selected.binding["product_ref"],
    )?;
    let component_slot = if let Some(old) = &current {
        shape::text(m, &old.claim, "supplier_component_slot")?
    } else {
        shape::text(m, &request, "supplier_component_slot")?
    };
    let (_, instantiation) = load(
        m,
        r,
        c,
        &selected.binding["slot_instantiation_ref"],
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let component_map = shape::array(m, &instantiation, "component_map")?;
    let map = shape::named(m, component_map, "slot_id", component_slot)?;
    let component_key = shape::text(m, map, "template_key")?;
    let mut instruction = None;
    for candidate in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        m.visit("cost.original_selected_instruction")?;
        let opcode = shape::text(m, candidate, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_rule")? {
            continue;
        }
        if shape::same(
            m,
            &candidate["rule"]["component_key"],
            &json!(component_key),
        )? {
            if instruction.is_some() {
                return Err(shape::fail("duplicate original supplier instruction"));
            }
            instruction = Some(candidate);
        }
    }
    let instruction =
        instruction.ok_or_else(|| shape::fail("missing original supplier instruction"))?;
    eq(
        m,
        &instruction["rule"]["formula"]["kind"],
        &json!("supplier_final_cost"),
    )?;
    eq(
        m,
        &instruction["rule"]["activity"],
        &selected.work["payload"]["activity"],
    )?;
    eq(
        m,
        &instruction["rule"]["binding_slot"],
        &selected.original["supplier_slot"],
    )?;
    let delayed_rows = shape::array(m, program.original_policy_value(), "delayed_costs")?;
    let delayed = shape::named(m, delayed_rows, "component_key", component_key)?;
    eq(m, &delayed["on_missing"], &json!("wait"))?;
    eq(
        m,
        &delayed["evidence_source_slot"],
        &instruction["rule"]["formula"]["evidence_slot"],
    )?;
    let horizon = shape::text(m, delayed, "finalize_within_us")?;
    let horizon = arithmetic::integer(m, horizon, false)?;
    let original_at =
        Timestamp::parse_with_work(shape::text(m, &selected.work, "accepted_at")?, m)?.micros();
    let at = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    if !correct {
        let deadline = meter::add(m, &original_at.into(), &horizon)?;
        if meter::cmp(m, &at.into(), &deadline)?.is_gt() {
            return Err(Error::new(
                "FINAL_COST_DEADLINE",
                "original supplier finalization horizon",
            ));
        }
    }
    let proof = if correct {
        &request["replacement_final_cost_proof"]
    } else {
        &request["final_cost_proof"]
    };
    let proof = operands::final_evidence(
        m,
        r,
        c,
        &selected,
        proof,
        current.as_ref().map(|x| &x.evidence),
    )?;
    if let Some(old) = &current {
        eq(
            m,
            &old.claim["original_binding_ref"],
            &json!(selected.binding_ref),
        )?;
        eq(
            m,
            &old.claim["owner_ref"],
            &selected.original["cost_authority_ref"],
        )?;
        eq(m, &old.owner["owner_ref"], &old.claim["owner_ref"])?;
        eq(
            m,
            &old.owner["original_evidence_ref"],
            &old.claim["original_evidence_ref"],
        )?;
        let latest = shape::array(m, &selected.current, "cost_consumption_refs")?;
        if !shape::member(m, latest, &json!(old.consumption_ref))? {
            return Err(shape::fail(
                "current cost consumption omitted from invocation",
            ));
        }
        let supplier = state
            .supplier
            .ok_or_else(|| shape::fail("missing current cost owner set"))?;
        let mut owner_head = None;
        for head in supplier.cost_owner_heads {
            m.visit("cost.current_line_owner_head")?;
            let value = r.get(
                m,
                &c.key,
                &head.reference,
                "workflow-cost",
                "ledger-cost-line-owner/1",
            )?;
            if shape::same(m, &value["line_key"], &old.claim["line_key"])? {
                if owner_head.is_some() {
                    return Err(shape::fail("duplicate current owner line"));
                }
                if !shape::same_ref(m, &head.reference, &old.owner_ref)? {
                    return Err(shape::fail("stale provider line owner"));
                }
                eq(m, &value["revision"], &json!(head.revision))?;
                owner_head = Some(head);
            }
        }
        if owner_head.is_none() {
            return Err(shape::fail("missing actual provider line owner head"));
        }
        let (_, first) = load(
            m,
            r,
            c,
            &old.owner["supplier_obligation_ref"],
            "workflow-decision",
            "ledger-workflow-decision-revision/2",
        )?;
        for (key, want) in [
            ("revision", json!("1")),
            ("kind", json!("cost")),
            ("transition", json!("cost_finalize")),
            ("subject_ref", json!(old.claim_ref)),
            ("original_terms_ref", json!(selected.binding_ref)),
        ] {
            eq(m, &first[key], &want)?;
        }
        let (_, original_evidence) = load(
            m,
            r,
            c,
            &old.claim["original_evidence_ref"],
            "workflow-fact-attestation",
            "ledger-workflow-supplier-final-cost/1",
        )?;
        eq(
            m,
            &original_evidence["subject"],
            &proof.assertion["subject"],
        )?;
        let line = &old.claim["line_key"];
        let expected = body::semantic_id(
            m,
            "wcostclaim_",
            "workflow-cost-claim",
            &json!([
                line["cost_authority_id"],
                line["provider_account_identity"],
                line["original_document_id"],
                line["original_line_id"]
            ]),
        )?;
        if !shape::same_text(m, &expected, &old.claim_ref.id)? {
            return Err(shape::fail(
                "permanent original provider line claim identity",
            ));
        }
    }
    let claim_id = shape::text(m, &selected.work, "claim_id")?;
    let (work_claim_ref, work_claim) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    super::super::inventory::entry::historical_work_claim(
        m,
        c,
        &work_claim_ref,
        &work_claim,
        &selected.work,
    )?;
    super::super::outcome::original_claim_association(m, &work_claim, &selected.work)?;
    let decision_id = body::semantic_id(
        m,
        "wdecision_",
        "workflow-decision",
        &json!([c.key.scope, work_claim_ref.id, component_slot, "cost"]),
    )?;
    let next_revision = match &current {
        None => 1.into(),
        Some(old) => {
            eq(m, &old.decision["decision_id"], &json!(decision_id))?;
            let raw = shape::text(m, &old.decision, "revision")?;
            let revision = arithmetic::integer(m, raw, false)?;
            meter::add(m, &revision, &1.into())?
        }
    };
    let supplier_component_slot = copy(m, component_slot)?;
    for value in shape::array(m, &request, "evidence_refs")? {
        let reference = shape::reference(m, value)?;
        r.provenance(m, &c.key, &reference)?;
    }
    Ok(CostPlan {
        request,
        selected,
        proof,
        original_work_claim_ref: work_claim_ref,
        original_work_claim: work_claim,
        instruction,
        delayed,
        supplier_component_slot,
        decision_id,
        next_revision,
        current,
        _ownership: ownership,
    })
}
