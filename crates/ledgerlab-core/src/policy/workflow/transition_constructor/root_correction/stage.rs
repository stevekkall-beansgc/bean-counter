//! Stage causal closure is an exact dependent of its original fixed Work.
//! Each original component resolves its own correction role; no grant union.
use super::super::{
    cost_postings,
    working_value::{owning_reference, parse_owned_reference, WorkingKey, WorkingValue},
};
use super::*;
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, RepresentationKind},
};

pub(super) struct Batch {
    pub records: Vec<ConstructedEnvelope>,
    pub heads: Vec<(String, String, ExactRef)>,
    pub slots: Vec<String>,
    pub used: Vec<usize>,
    pub owner: ExternalBufferLease,
}
fn push_authority(m: &mut CoreMeter, batch: &mut Batch, output: ConstructedEnvelope) -> Result<()> {
    for old in &batch.records {
        m.visit("Stage.unique_component_authority_output")?;
        if shape::same_ref(m, &old.reference, &output.reference)? {
            if !shape::same_bytes(m, &old.canonical_body, &output.canonical_body)? {
                return Err(shape::fail("Stage duplicate authority body mismatch"));
            }
            return Ok(());
        }
    }
    batch.records.push(output);
    Ok(())
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn literal(m: &mut CoreMeter, v: &Value, k: &str, want: &str) -> Result<()> {
    let actual = text(m, v, k)?;
    if !shape::same_text(m, actual, want)? {
        return Err(shape::fail("Stage causal literal"));
    }
    Ok(())
}
fn revision_id(m: &mut CoreMeter, id: &Value, rev: &Value) -> Result<String> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("Stage revision owner"))?;
    let seed = a.array_from_values(&[id, rev], m)?;
    body::semantic_id(m, "wdrev_", "workflow-decision-revision", seed.value())
}

pub(super) fn prepare(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &RootPlan,
    request: &WorkingValue,
    state: &EconomicStateData,
) -> Result<Option<Batch>> {
    if !program.uses_stage_lifecycle_recipe() {
        return Ok(None);
    }
    let Some(instance_value) = plan.work["payload"].get("stage_instance_ref") else {
        return Ok(None);
    };
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("Stage causal owner"))?;
    let owner = a.reserve_external_payload(
        128 * (std::mem::size_of::<ConstructedEnvelope>()
            + std::mem::size_of::<(String, String, ExactRef)>()
            + 1024),
        m,
    )?;
    let mut batch = Batch {
        records: Vec::new(),
        heads: Vec::new(),
        slots: Vec::new(),
        used: Vec::new(),
        owner,
    };
    batch
        .records
        .try_reserve_exact(128)
        .map_err(|_| shape::fail("Stage causal output controls"))?;
    batch
        .heads
        .try_reserve_exact(1)
        .map_err(|_| shape::fail("Stage causal head controls"))?;
    batch
        .slots
        .try_reserve_exact(32)
        .map_err(|_| shape::fail("Stage causal slot controls"))?;
    batch
        .used
        .try_reserve_exact(1)
        .map_err(|_| shape::fail("Stage causal nomination controls"))?;
    let instance_ref = parse_owned_reference(m, &a, instance_value)?;
    let instance = r.get(
        m,
        &c.key,
        instance_ref.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    super::super::stage::closed_instance(m, &c.key, instance_ref.reference(), &instance)?;
    for field in ["binding_ref", "customer", "chain"] {
        shape::equal(m, &instance[field], &plan.payload[field])?;
    }
    let name = text(m, &instance, "stage_name")?;
    let definition = super::super::stage::definition(m, program, name)?;
    let agreement = text(m, &instance, "agreement_id")?;
    let chain = text(m, &instance, "chain")?;
    let mut selected = None;
    for head in &state.financial.stage_heads {
        m.visit("Stage.causal_actual_head")?;
        if shape::same_text(m, &head.agreement, agreement)?
            && shape::same_text(m, &head.chain, chain)?
            && shape::same_text(m, &head.stage, name)?
            && selected.replace(head).is_some()
        {
            return Err(shape::fail("duplicate affected Stage head"));
        }
    }
    let head = selected.ok_or_else(|| shape::fail("affected Stage head missing"))?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-stage-state",
        "ledger-workflow-stage-state/1",
    )?;
    shape::equal(m, &current["stage_instance_ref"], instance_value)?;
    let revision = text(m, &current, "revision")?;
    if !shape::same_text(m, revision, &head.revision)? {
        return Err(shape::fail("affected Stage head revision"));
    }
    let identity =
        super::super::stage::state_id(m, &c.key, instance_ref.reference(), &current["revision"])?;
    if !shape::same_text(m, &head.reference.id, &identity)? {
        return Err(shape::fail("affected Stage state identity"));
    }
    let status = text(m, &current, "state")?;
    if shape::same_text(m, status, "closed_by_root")? {
        return Ok(Some(batch));
    }
    let closed = shape::same_text(m, status, "closed")?;
    let permanent = super::super::stage::closure_decision_id(m, &c.key, &instance)?;
    let mut selected = None;
    if closed {
        for head in &state.financial.decision_heads {
            if shape::same_text(m, &head.decision_id, &permanent)?
                && selected.replace(head).is_some()
            {
                return Err(shape::fail("duplicate Stage dependent head"));
            }
        }
    }
    let dependent = if closed {
        let head =
            selected.ok_or_else(|| shape::fail("closed Stage dependent Decision missing"))?;
        let decision = r.get(
            m,
            &c.key,
            &head.reference,
            "workflow-decision",
            "ledger-workflow-decision-revision/1",
        )?;
        super::super::correction::dictionary_decision(m, &decision)?;
        literal(m, &decision, "kind", "outcome")?;
        let id = revision_id(m, &decision["decision_id"], &decision["revision"])?;
        if !shape::same_text(m, &id, &head.reference.id)? {
            return Err(shape::fail("Stage dependent revision identity"));
        }
        shape::equal(m, &decision["subject_ref"], &current["closure_claim_ref"])?;
        let status = text(m, &decision, "status")?;
        if shape::same_text(m, status, "reversed")? {
            literal(m, &decision, "transition", "dependent_reverse")?;
            let auth = shape::reference(m, &decision["authority_ref"])?;
            let auth = r.get(
                m,
                &c.key,
                &auth,
                "workflow-economic-authority",
                "ledger-workflow-stage-causal-authority/1",
            )?;
            shape::equal(m, &auth["stage_instance_ref"], instance_value)?;
            shape::equal(m, &auth["operation_key"], &decision["operation_key"])?;
            shape::equal(m, &auth["accepted_at"], &decision["accepted_at"])?;
            validate_aggregate(m, r, c, program, plan, (&instance, &auth, &decision))?;
            let prior_ref = shape::reference(m, &decision["prior_ref"])?;
            let prior = r.get(
                m,
                &c.key,
                &prior_ref,
                "workflow-decision",
                "ledger-workflow-decision-revision/1",
            )?;
            literal(m, &prior, "revision", "1")?;
            literal(m, &prior, "transition", "initial")?;
            literal(m, &prior, "status", "active")?;
            let claim_ref = shape::reference(m, &prior["subject_ref"])?;
            let claim = r.get(
                m,
                &c.key,
                &claim_ref,
                "workflow-stage-closure-claim",
                "ledger-workflow-stage-closure-claim/1",
            )?;
            let emitted_key = WorkingKey::parse(m, &decision["operation_key"])?;
            let emitted_time = text(m, &decision, "accepted_at")?;
            let auth_ref = shape::reference(m, &decision["authority_ref"])?;
            let raw = r.canonical(m, &auth_ref)?;
            let bytes = m.copy_owned_representation(&a, raw, RepresentationKind::SourceClosure)?;
            let aggregate = ConstructedEnvelope::from_owned(
                auth_ref,
                bytes,
                &a,
                m,
                RepresentationKind::SourceClosure,
            )?;
            reverse(
                m,
                r,
                c,
                program,
                (plan, &instance, &claim, &prior, &prior_ref),
                &aggregate,
                (&mut batch, state, (&emitted_key, emitted_time)),
            )?;
            for expected in &batch.records {
                let body = expected.decoded(m, 4194304)?;
                let schema = text(m, &body, "schema")?;
                let actual = r.get(
                    m,
                    &c.key,
                    &expected.reference,
                    &expected.reference.kind,
                    schema,
                )?;
                shape::equal(m, &actual, &body)?;
            }
            batch.records.clear();
            batch.heads.clear();
            return Ok(Some(batch));
        }
        literal(m, &decision, "revision", "1")?;
        literal(m, &decision, "transition", "initial")?;
        literal(m, &decision, "status", "active")?;
        let reversals = shape::array(m, request, "dependent_reversals")?;
        let mut nomination = None;
        let reference = owning_reference(m, &a, &head.reference)?;
        for (index, row) in reversals.iter().enumerate() {
            if shape::same(m, &row["decision_ref"], reference.value())?
                && nomination.replace(index).is_some()
            {
                return Err(shape::fail("duplicate Stage dependent reversal"));
            }
        }
        let index = nomination.ok_or_else(|| {
            Error::new(
                "REVERSAL_DEPENDENTS_REQUIRED",
                "actual Stage dependent reversal missing",
            )
        })?;
        let row = &reversals[index];
        shape::closed(
            m,
            row,
            &[
                "decision_ref",
                "expected_revision",
                "authority_evidence_refs",
            ],
            &[],
        )?;
        shape::equal(m, &row["expected_revision"], &decision["revision"])?;
        batch.used.push(index);
        Some((
            copy_ref(m, &head.reference)?,
            decision,
            WorkingValue::copy(m, &row["authority_evidence_refs"])?,
        ))
    } else {
        if !shape::same_text(m, status, "declared")? && !shape::same_text(m, status, "started")? {
            return Err(shape::fail("affected Stage terminal state"));
        }
        None
    };
    let mode = if plan.reverse {
        EconomicMode::WorkReverse
    } else {
        EconomicMode::QuantityCorrect
    };
    let operation = if plan.reverse {
        "work.reverse"
    } else {
        "quantity.correct"
    };
    let evidence = match &dependent {
        Some((_, _, evidence)) => &**evidence,
        None => &request["evidence_refs"],
    };
    let purpose = WorkingValue::copy(m, &plan.payload)?.with_fields(
        m,
        &[
            ("activity", &definition["closure_activity"]),
            ("evidence_refs", evidence),
        ],
    )?;
    let mut checks =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let maps = shape::array(m, &plan.operands.instantiation, "component_map")?;
    let mut count = 0usize;
    for rule in shape::array(m, program.original_policy_value(), "rules")? {
        if !shape::same(m, &rule["activity"], &definition["closure_activity"])? {
            continue;
        }
        let binding_slot = text(m, rule, "binding_slot")?;
        let resolved = super::super::app_cost::stage_component_authority(
            m,
            r,
            c,
            program,
            &plan.operands.terms,
            (&purpose, mode, binding_slot),
        )?;
        let slot = text(m, &resolved.slot_terms, "correction_source_slot")?;
        let mut seen = false;
        for old in &batch.slots {
            if shape::same_text(m, old, slot)? {
                seen = true;
            }
        }
        if !seen {
            batch.slots.push(copy(m, slot)?);
        }
        let component = text(m, rule, "component_key")?;
        let map = shape::named(m, maps, "template_key", component)?;
        let catalog = shape::array(m, &plan.operands.catalog, "components")?;
        let component_id = text(m, map, "slot_id")?;
        let root = shape::named(m, catalog, "slot_id", component_id)?;
        let terms = owning_reference(m, &a, &resolved.slot_terms_ref)?;
        let authority = resolved.authority_document(m, None)?;
        let authority_ref = owning_reference(m, &a, &authority.reference)?;
        let slot = a.string_value(binding_slot, m)?;
        let check = a.object_from_fields(
            &[
                ("component_slot", &map["slot_id"]),
                ("root_slot", &root["root_slot"]),
                ("binding_slot", slot.value()),
                ("terms_ref", terms.value()),
                ("authority_ref", authority_ref.value()),
                ("authority_evidence_refs", evidence),
            ],
            m,
        )?;
        checks.unique(m, &a, check.value())?;
        push_authority(m, &mut batch, authority)?;
        count = w::usize_add(m, count, 1)?;
    }
    if count == 0 {
        return Err(shape::fail("Stage causal component closure empty"));
    }
    // Evidence selects the full affected operation, not arbitrary extra inputs.
    for value in evidence
        .as_array()
        .ok_or_else(|| shape::fail("Stage authority evidence array"))?
    {
        let reference = shape::reference(m, value)?;
        let one = std::slice::from_ref(&reference);
        let mut applicable = false;
        for output in &batch.records {
            let body = output.decoded(m, 4194304)?;
            match super::dependent::evidence_values(m, &plan.operands.binding, &body, one) {
                Ok(()) => applicable = true,
                Err(e) if e.code == "AUTHORITY_REQUIRED" => {}
                Err(e) => return Err(e),
            }
        }
        if !applicable {
            return Err(Error::new(
                "AUTHORITY_REQUIRED",
                "Stage dependent evidence is unrelated to its actual component roles",
            ));
        }
    }
    let checks = checks.finish(m, &a, true)?;
    let key = cost_postings::operation_key(m, &a, c)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let prior_root = owning_reference(m, &a, &plan.current_ref)?;
    if let Some((prior_ref, decision, _)) = dependent {
        let claim_ref = parse_owned_reference(m, &a, &decision["subject_ref"])?;
        let claim = r.get(
            m,
            &c.key,
            claim_ref.reference(),
            "workflow-stage-closure-claim",
            "ledger-workflow-stage-closure-claim/1",
        )?;
        shape::equal(m, &claim["stage_instance_ref"], instance_value)?;
        shape::equal(m, &claim["operation_key"], &decision["operation_key"])?;
        shape::equal(m, &claim["authority_ref"], &decision["authority_ref"])?;
        for value in shape::array(m, &claim, "anchor_link_refs")? {
            let reference = shape::reference(m, value)?;
            let link = r.get(
                m,
                &c.key,
                &reference,
                "workflow-link",
                "ledger-workflow-link/1",
            )?;
            if shape::same(m, &link["child"], instance_value)? {
                super::super::stage::retained_link(
                    m,
                    r,
                    c,
                    program,
                    (&reference, &link, instance_ref.reference(), &plan.payload),
                )?;
            }
        }
        let schema = a.string_value("ledger-workflow-stage-causal-authority/1", m)?;
        let op = a.string_value("dependent_reverse", m)?;
        let root_operation = a.string_value(operation, m)?;
        let prior = owning_reference(m, &a, &prior_ref)?;
        let aggregate = a.object_from_fields(
            &[
                ("schema", schema.value()),
                ("operation", op.value()),
                ("stage_instance_ref", instance_value),
                ("prior_stage_decision_ref", prior.value()),
                ("causal_root_ref", prior_root.value()),
                ("causal_root_operation", root_operation.value()),
                ("operation_key", key.value()),
                ("accepted_at", at.value()),
                ("component_checks", checks.value()),
            ],
            m,
        )?;
        let aggregate =
            body::document_borrowed(m, "workflow-economic-authority", aggregate.value())?;
        reverse(
            m,
            r,
            c,
            program,
            (plan, &instance, &claim, &decision, &prior_ref),
            &aggregate,
            (&mut batch, state, (&c.key, &c.accepted_at)),
        )?;
        batch.records.push(aggregate);
    } else {
        let prior = owning_reference(m, &a, &head.reference)?;
        let rev = arithmetic::integer(m, revision, false)?;
        let next = meter::add(m, &rev, &1.into())?;
        let next = cost_postings::scalar(m, &a, &next)?;
        let status = a.string_value("closed_by_root", m)?;
        // The root authority already authenticates the initiating C role. Each
        // separately checked original component role is retained above.
        let authority = plan.operands.authority_document(m, None)?;
        let authority_ref = owning_reference(m, &a, &authority.reference)?;
        let value = WorkingValue::copy(m, &current)?.with_fields(
            m,
            &[
                ("revision", next.value()),
                ("state", status.value()),
                ("prior_ref", prior.value()),
                ("operation_key", key.value()),
                ("authority_ref", authority_ref.value()),
                ("accepted_at", at.value()),
                ("closed_by_root_ref", prior_root.value()),
            ],
        )?;
        let id = super::super::stage::state_id(m, &c.key, instance_ref.reference(), next.value())?;
        batch.records.push(body::record_borrowed(
            m,
            &c.key,
            "workflow-stage-state",
            id,
            &value,
        )?);
        push_authority(m, &mut batch, authority)?;
    }
    Ok(Some(batch))
}

fn reverse(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: (&RootPlan, &Value, &Value, &Value, &ExactRef),
    aggregate: &ConstructedEnvelope,
    closure: (&mut Batch, &EconomicStateData, (&OperationKey, &str)),
) -> Result<()> {
    let (plan, instance, claim, decision, prior) = original;
    let (batch, state, emitted) = closure;
    let (emission_key, emission_time) = emitted;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("Stage reverse owner"))?;
    let payload = shape::field(m, claim, "original_payload")?;
    let (net, prior_basis) = super::super::stage::retained_prior_net(
        m,
        r,
        c,
        program,
        &plan.operands.binding,
        instance,
        state.financial,
    )?;
    let activity = text(m, payload, "activity")?;
    let source = text(m, &claim["operation_key"], "source")?;
    let evaluation_ref = shape::reference(m, &decision["evaluation_ref"])?;
    let evaluation = r.get(
        m,
        &c.key,
        &evaluation_ref,
        "workflow-evaluation",
        "ledger-workflow-evaluation-proof/1",
    )?;
    shape::equal(m, &evaluation["facts"], &payload["facts"])?;
    shape::equal(
        m,
        &evaluation["context"],
        &plan.operands.binding["accepted_context"],
    )?;
    let prices = pricing::stage(
        program,
        activity,
        &payload["facts"],
        (
            &plan.operands.binding["accepted_context"],
            &plan.operands.binding,
            source,
        ),
        &net,
        m,
    )?;
    let mut active =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut steps =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut retail =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut retail_net = BigInt::from(0);
    let mut share = None;
    let mut inverse_refs =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let maps = shape::array(m, &plan.operands.instantiation, "component_map")?;
    let mut original_steps =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &a, 128)?;
    let mut known = Vec::<(String, ExactRef)>::new();
    known
        .try_reserve_exact(128)
        .map_err(|_| shape::fail("Stage original component basis controls"))?;
    let old_key = WorkingKey::parse(m, &claim["operation_key"])?;
    let claim_reference = parse_owned_reference(m, &a, &decision["subject_ref"])?;
    let permanent = text(m, decision, "decision_id")?;
    for price in prices.iter() {
        let mapping = shape::named(m, maps, "template_key", &price.component_key)?;
        let root = shape::named(m, maps, "template_key", &price.root_key)?;
        let rows = shape::array(m, &plan.operands.binding, "bindings")?;
        let row = shape::named(m, rows, "slot", &price.binding_slot)?;
        let terms_ref = parse_owned_reference(m, &a, &row["binding_ref"])?;
        let catalog = shape::array(m, &plan.operands.catalog, "components")?;
        let component = text(m, mapping, "slot_id")?;
        let component = shape::named(m, catalog, "slot_id", component)?;
        let book = text(m, component, "book")?;
        let schema = if shape::same_text(m, book, "supplier")? {
            "ledger-workflow-slot-binding-terms/8"
        } else {
            "ledger-workflow-slot-binding-terms/3"
        };
        let terms = r.get(
            m,
            &c.key,
            terms_ref.reference(),
            "workflow-slot-terms",
            schema,
        )?;
        let role = parse_owned_reference(m, &a, &terms["role_ref"])?;
        let mut basis =
            crate::policy::workflow::selected_program::supplier_projection::Values::new(
                m, &a, 128,
            )?;
        for component in &price.basis_components {
            for (name, reference) in &known {
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
        let formula = text(m, &instruction["rule"]["formula"], "kind")?;
        if shape::same_text(m, formula, "closure_cap")? {
            let values = prior_basis
                .value()
                .as_array()
                .ok_or_else(|| shape::fail("Stage original prior basis array"))?;
            for value in values {
                m.visit("Stage.original_cap_Work_basis")?;
                basis.unique(m, &a, value)?;
            }
        }
        let basis = basis.finish(m, &a, true)?;
        let original = cost_postings::original_cost_posting_key(
            m,
            &old_key,
            cost_postings::OriginalCostPosting {
                binding: &plan.operands.binding,
                original_work_claim: claim_reference.reference(),
                component_slot: &mapping["slot_id"],
                root_slot: &root["slot_id"],
                book: &component["book"],
                terms_ref: terms_ref.reference(),
                terms: &terms,
                role_ref: role.reference(),
                decision_id: permanent,
                revision: &1.into(),
                replacement: false,
                basis_refs: Some(basis.value()),
            },
            price,
        )?;
        original_steps.unique(m, &a, original.step.value())?;
        let Some(expected) = original.action else {
            continue;
        };
        let reference = copy_ref(m, &expected.reference)?;
        let action = r.get(
            m,
            &c.key,
            &reference,
            "workflow-action",
            "ledger-workflow-action/1",
        )?;
        let actual = expected.decoded(m, 4194304)?;
        shape::equal(m, &action, &actual)?;
        known.push((copy(m, &price.component_key)?, copy_ref(m, &reference)?));
        let old = owning_reference(m, &a, &reference)?;
        active.unique(m, &a, old.value())?;
        let inverse = cost_postings::exact_root_inverse_key(
            m,
            emission_key,
            &reference,
            &action,
            price,
            &2.into(),
        )?;
        steps.unique(m, &a, inverse.step.value())?;
        let book = text(m, &action, "book")?;
        if shape::same_text(m, book, "retail")? {
            retail.unique(m, &a, old.value())?;
            retail_net = meter::add(m, &retail_net, &price.atoms.into())?;
        } else {
            if share
                .replace((
                    copy_ref(m, &reference)?,
                    price.atoms,
                    shape::reference(m, &action["role_ref"])?,
                ))
                .is_some()
            {
                return Err(shape::fail("multiple Stage governing supplier share"));
            }
        }
        let inverse = inverse
            .action
            .ok_or_else(|| shape::fail("Stage inverse missing actual Action"))?;
        let inverse_ref = owning_reference(m, &a, &inverse.reference)?;
        inverse_refs.unique(m, &a, inverse_ref.value())?;
        batch.records.push(inverse);
    }
    let active = active.finish(m, &a, true)?;
    shape::equal(m, &decision["active_action_refs"], active.value())?;
    let original_steps = original_steps.finish(m, &a, false)?;
    shape::equal(m, &evaluation["steps"], original_steps.value())?;
    let prior = owning_reference(m, &a, prior)?;
    let refs = inverse_refs.finish(m, &a, true)?;
    let steps = steps.finish(m, &a, false)?;
    let eval = WorkingValue::copy(m, &evaluation)?.with_fields(m, &[("steps", steps.value())])?;
    let eval = body::document_borrowed(m, "workflow-evaluation", &eval)?;
    let eval_ref = owning_reference(m, &a, &eval.reference)?;
    let auth = owning_reference(m, &a, &aggregate.reference)?;
    let constants = m.parse_owned_representation(
        &a,
        br#"{"revision":"2","transition":"dependent_reverse","status":"reversed","empty":[]}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let key = super::super::cost_postings::operation_key_value(m, &a, emission_key)?;
    let at = a.string_value(emission_time, m)?;
    let result = WorkingValue::copy(m, decision)?.with_fields(
        m,
        &[
            ("revision", &constants.value()["revision"]),
            ("transition", &constants.value()["transition"]),
            ("status", &constants.value()["status"]),
            ("prior_ref", prior.value()),
            ("authority_ref", auth.value()),
            ("evaluation_ref", eval_ref.value()),
            ("actions_appended", refs.value()),
            ("active_action_refs", &constants.value()["empty"]),
            ("operation_key", key.value()),
            ("accepted_at", at.value()),
        ],
    )?;
    let identity = revision_id(m, &decision["decision_id"], &constants.value()["revision"])?;
    let result = body::record_borrowed(m, &c.key, "workflow-decision", identity, &result)?;
    let permanent = text(m, decision, "decision_id")?;
    batch.heads.push((
        copy(m, permanent)?,
        copy(m, "2")?,
        copy_ref(m, &result.reference)?,
    ));
    batch.records.extend([eval, result]);
    if let Some((parent, atoms, role_ref)) = share {
        let role = r.get(
            m,
            &c.key,
            &role_ref,
            "workflow-roles",
            "ledger-workflow-role-assignment/1",
        )?;
        let retail_role = r.get(
            m,
            &c.key,
            &plan.operands.role_ref,
            "workflow-roles",
            "ledger-workflow-role-assignment/1",
        )?;
        shape::equal(m, &retail_role["recipient"], &role["bearer"])?;
        let remainder = meter::sub(m, &retail_net, &atoms.into())?;
        let parent = owning_reference(m, &a, &parent)?;
        let basis = retail.finish(m, &a, true)?;
        let schema = a.string_value("ledger-workflow-allocation/1", m)?;
        for (partition, amount, recipient) in [
            ("supplier_share", BigInt::from(atoms), &role["recipient"]),
            ("host_remainder", remainder, &retail_role["recipient"]),
        ] {
            let partition = a.string_value(partition, m)?;
            let original = cost_postings::amount(m, &a, &amount)?;
            let original = a.object_from_fields(
                &[
                    ("schema", schema.value()),
                    ("stage_instance_ref", &claim["stage_instance_ref"]),
                    ("allocation_parent", parent.value()),
                    ("inputs", basis.value()),
                    ("partition", partition.value()),
                    ("amount", original.value()),
                    ("recipient", recipient),
                ],
                m,
            )?;
            let expected = body::document_borrowed(m, "workflow-allocation", original.value())?;
            let saved = r.get(
                m,
                &c.key,
                &expected.reference,
                "workflow-allocation",
                "ledger-workflow-allocation/1",
            )?;
            shape::equal(m, &saved, original.value())?;
            let old = owning_reference(m, &a, &expected.reference)?;
            let negative = meter::neg(m, &amount)?;
            let amount = cost_postings::amount(m, &a, &negative)?;
            let inverse = WorkingValue::copy(m, &saved)?
                .with_fields(m, &[("amount", amount.value()), ("reverses", old.value())])?;
            batch
                .records
                .push(body::document_borrowed(m, "workflow-allocation", &inverse)?);
        }
    }
    Ok(())
}

/// A terminal head proves the same complete component authority set and exact
/// stored inverse lineage at its own original operation time, never today's
/// member eligibility or today's provider/source mapping.
fn validate_aggregate(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &RootPlan,
    selected: (&Value, &Value, &Value),
) -> Result<()> {
    let (instance, aggregate, decision) = selected;
    shape::closed(
        m,
        aggregate,
        &[
            "schema",
            "operation",
            "stage_instance_ref",
            "prior_stage_decision_ref",
            "causal_root_ref",
            "causal_root_operation",
            "operation_key",
            "accepted_at",
            "component_checks",
        ],
        &[],
    )?;
    literal(m, aggregate, "operation", "dependent_reverse")?;
    shape::equal(
        m,
        &aggregate["prior_stage_decision_ref"],
        &decision["prior_ref"],
    )?;
    let root_ref = shape::reference(m, &aggregate["causal_root_ref"])?;
    let root = r.get(
        m,
        &c.key,
        &root_ref,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    literal(m, &root, "kind", "work")?;
    let work_ref = shape::reference(m, &root["subject_ref"])?;
    let work = r.get(
        m,
        &c.key,
        &work_ref,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::equal(
        m,
        &work["payload"]["stage_instance_ref"],
        &aggregate["stage_instance_ref"],
    )?;
    shape::equal(m, &work["payload"]["binding_ref"], &instance["binding_ref"])?;
    shape::equal(m, &work["payload"]["chain"], &instance["chain"])?;
    let operation = text(m, aggregate, "causal_root_operation")?;
    if !shape::same_text(m, operation, "work.reverse")?
        && !shape::same_text(m, operation, "quantity.correct")?
    {
        return Err(shape::fail("retained Stage causal operation"));
    }
    let at =
        crate::domain::Timestamp::parse_with_work(text(m, aggregate, "accepted_at")?, m)?.micros();
    let stage_name = text(m, instance, "stage_name")?;
    let definition = super::super::stage::definition(m, program, stage_name)?;
    let activity = text(m, definition, "closure_activity")?;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("retained Stage component owner"))?;
    let mut product = None;
    for source in program.source_documents() {
        let reference = super::super::working_value::owning_definition(m, &a, source.reference())?;
        if shape::same(m, reference.value(), &plan.payload["product_ref"])? {
            product = Some(source.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("retained Stage original Product"))?;
    let product = text(m, product, "product_id")?;
    let maps = shape::array(m, &plan.operands.instantiation, "component_map")?;
    let components = shape::array(m, &plan.operands.catalog, "components")?;
    let checks = shape::array(m, aggregate, "component_checks")?;
    let mut expected = 0usize;
    for rule in shape::array(m, program.original_policy_value(), "rules")? {
        if !shape::same(m, &rule["activity"], &definition["closure_activity"])? {
            continue;
        }
        expected = w::usize_add(m, expected, 1)?;
        let component = text(m, rule, "component_key")?;
        let map = shape::named(m, maps, "template_key", component)?;
        let component = text(m, map, "slot_id")?;
        let catalog = shape::named(m, components, "slot_id", component)?;
        let row = shape::named(m, checks, "component_slot", component)?;
        shape::closed(
            m,
            row,
            &[
                "component_slot",
                "root_slot",
                "binding_slot",
                "terms_ref",
                "authority_ref",
                "authority_evidence_refs",
            ],
            &[],
        )?;
        shape::equal(m, &row["root_slot"], &catalog["root_slot"])?;
        shape::equal(m, &row["binding_slot"], &rule["binding_slot"])?;
        let name = text(m, rule, "binding_slot")?;
        let rows = shape::array(m, &plan.operands.binding, "bindings")?;
        let bound = shape::named(m, rows, "slot", name)?;
        shape::equal(m, &row["terms_ref"], &bound["binding_ref"])?;
        let book = text(m, catalog, "book")?;
        let schema = if shape::same_text(m, book, "supplier")? {
            "ledger-workflow-slot-binding-terms/8"
        } else {
            "ledger-workflow-slot-binding-terms/3"
        };
        let terms_ref = shape::reference(m, &row["terms_ref"])?;
        let terms = r.get(m, &c.key, &terms_ref, "workflow-slot-terms", schema)?;
        let reference = shape::reference(m, &row["authority_ref"])?;
        let authority = r.get(
            m,
            &c.key,
            &reference,
            "workflow-economic-authority",
            "ledger-workflow-economic-authority/8",
        )?;
        shape::equal(
            m,
            &authority["source_slot"],
            &terms["correction_source_slot"],
        )?;
        shape::equal(
            m,
            &authority["evidence_refs"],
            &row["authority_evidence_refs"],
        )?;
        super::super::authority::historical_namespace(
            m,
            r,
            c,
            &row["authority_ref"],
            &authority["canonical_economic_namespace"],
            Some(&authority["source"]),
            (
                operation,
                &instance["binding_ref"],
                &aggregate["accepted_at"],
            ),
        )?;
        let slot = text(m, &terms, "correction_source_slot")?;
        let nominations = shape::array(m, &plan.operands.binding, "source_slots")?;
        let nomination = shape::named(m, nominations, "name", slot)?;
        let nominees = shape::array(m, nomination, "sources")?;
        if !shape::member(m, nominees, &authority["source"])? {
            return Err(shape::fail("retained Stage original corrector nomination"));
        }
        let original_grants = shape::array(m, &authority, "original_grant_refs")?;
        if original_grants.is_empty() {
            return Err(shape::fail(
                "retained Stage original grant intersection empty",
            ));
        }
        for value in std::iter::once(&authority["current_grant_ref"]).chain(original_grants.iter())
        {
            let reference = shape::reference(m, value)?;
            let grant = r.get(
                m,
                &c.key,
                &reference,
                "workflow-source-grant",
                "ledger-workflow-source-grant/1",
            )?;
            for field in ["source", "principal_id", "canonical_economic_namespace"] {
                let want = if field == "principal_id" {
                    &authority["actor"]
                } else {
                    &authority[field]
                };
                shape::equal(m, &grant[field], want)?;
            }
            let accepted = crate::domain::Timestamp::parse_with_work(
                text(m, &plan.operands.binding, "accepted_at")?,
                m,
            )?
            .micros();
            let original = shape::member(m, original_grants, value)?;
            if original {
                let terms_grants = shape::array(m, &terms, "source_grant_refs")?;
                let binding_grants = shape::array(m, &plan.operands.binding, "authority_refs")?;
                if !shape::member(m, terms_grants, value)?
                    || !shape::member(m, binding_grants, value)?
                {
                    return Err(shape::fail(
                        "retained Stage exact original grant membership",
                    ));
                }
            }
            if !super::super::authority::grant_covers(
                m,
                &grant,
                (
                    &instance["customer"],
                    activity,
                    product,
                    slot,
                    (operation, &["correct", "read"]),
                    if original { accepted } else { at },
                ),
            )? {
                return Err(Error::new(
                    "AUTHORITY_REQUIRED",
                    "retained Stage component original/current purpose coverage",
                ));
            }
        }
    }
    if !w::compare(m, expected, checks.len())?.is_eq() {
        return Err(shape::fail(
            "retained Stage complete component authority set",
        ));
    }
    Ok(())
}
