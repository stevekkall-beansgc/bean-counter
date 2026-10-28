//! Component corrections retain one primary caller and separately validate each
//! affected original component's Terms/correction nomination. No rights union.
use super::super::working_value::{owning_reference, WorkingValue};
use super::*;
use crate::{
    canonical::audit_arena::{AuditedValue, ExternalBufferLease},
    validation_work as w,
};

pub(super) struct Batch {
    pub records: Vec<ConstructedEnvelope>,
    pub checks: AuditedValue,
    pub slots: Vec<String>,
    _owner: ExternalBufferLease,
}
pub(super) fn prepare(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &RootPlan,
    request: &Value,
) -> Result<Option<Batch>> {
    if !program.uses_component_lifecycle_recipe() {
        return Ok(None);
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning component correction authority"))?;
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let n = instructions.len();
    if w::compare(m, n, 128usize)?.is_gt() {
        return Err(shape::fail("component authority check bound"));
    }
    let per =
        std::mem::size_of::<ConstructedEnvelope>() + 2 * (std::mem::size_of::<String>() + 128);
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&n.into(), &per.into()],
    )?;
    let bytes = n
        .checked_mul(per)
        .ok_or_else(|| shape::fail("component authority controls"))?;
    let size = w::usize_add(m, bytes, std::mem::size_of::<Batch>())?;
    let owner = arena.reserve_external_payload(size, m)?;
    m.write_bytes(bytes)?;
    let mut records = Vec::<ConstructedEnvelope>::new();
    let mut slots = Vec::<String>::new();
    records
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("component authority result controls"))?;
    slots
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("component authority slot controls"))?;
    let mut checks =
        crate::policy::workflow::selected_program::supplier_projection::Values::new(m, &arena, n)?;
    let mode = if plan.reverse {
        EconomicMode::WorkReverse
    } else {
        EconomicMode::QuantityCorrect
    };
    let payload = WorkingValue::copy(m, &plan.payload)?
        .with_fields(m, &[("evidence_refs", &request["evidence_refs"])])?;
    let mappings = shape::array(m, &plan.operands.instantiation, "component_map")?;
    let components = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "component_slots",
    )?;
    for instruction in instructions {
        m.visit("component.affected_original_authority")?;
        let rule = &instruction["rule"];
        if !shape::same(m, &rule["activity"], &plan.payload["activity"])? {
            continue;
        }
        let component = text(m, rule, "component_key")?;
        let original = shape::named(m, components, "key", component)?;
        let root = text(m, original, "root_key")?;
        let kind = text(m, &rule["formula"], "kind")?;
        let observation = shape::same_text(m, kind, "observe_cost")?;
        if observation && !plan.reverse || !observation && !is_affected(m, plan, root)? {
            continue;
        }
        let binding_slot = text(m, rule, "binding_slot")?;
        let resolved = super::super::app_cost::component_correction_authority(
            m,
            r,
            c,
            program,
            &plan.operands.terms,
            (&payload, mode, binding_slot),
        )?;
        let slot = text(m, &resolved.slot_terms, "correction_source_slot")?;
        let mut used = false;
        for prior in &slots {
            if shape::same_text(m, prior, slot)? {
                used = true;
            }
        }
        if !used {
            m.scan_bytes(slot.len())?;
            m.write_bytes(slot.len())?;
            slots.push(slot.to_owned());
        }
        let map = shape::named(m, mappings, "template_key", component)?;
        let root_map = shape::named(m, mappings, "template_key", root)?;
        let terms_ref = owning_reference(m, &arena, &resolved.slot_terms_ref)?;
        let snapshot = resolved.app_authority_value(m, &[])?;
        let evidence = shape::array(m, request, "evidence_refs")?;
        let mut selected_evidence =
            crate::policy::workflow::selected_program::supplier_projection::Values::new(
                m,
                &arena,
                evidence.len(),
            )?;
        for value in evidence {
            let reference = super::super::working_value::parse_owned_reference(m, &arena, value)?;
            match dependent::evidence_values(
                m,
                &plan.operands.binding,
                snapshot.value(),
                std::slice::from_ref(reference.reference()),
            ) {
                Ok(()) => selected_evidence.unique(m, &arena, value)?,
                Err(e) if e.code == "AUTHORITY_REQUIRED" => {}
                Err(e) => return Err(e),
            }
        }
        let evidence = selected_evidence.finish(m, &arena, true)?;
        if w::compare(
            m,
            evidence
                .value()
                .as_array()
                .ok_or_else(|| shape::fail("component evidence subset"))?
                .len(),
            16usize,
        )?
        .is_gt()
        {
            return Err(shape::fail("component authority evidence bound"));
        }
        let snapshot =
            WorkingValue::Owned(snapshot).with_fields(m, &[("evidence_refs", evidence.value())])?;
        let output = body::document_borrowed(m, "workflow-economic-authority", &snapshot)?;
        let authority_ref = owning_reference(m, &arena, &output.reference)?;
        let binding_slot = arena.string_value(binding_slot, m)?;
        let check = arena.object_from_fields(
            &[
                ("component_slot", &map["slot_id"]),
                ("root_slot", &root_map["slot_id"]),
                ("binding_slot", binding_slot.value()),
                ("terms_ref", terms_ref.value()),
                ("authority_ref", authority_ref.value()),
                ("authority_evidence_refs", evidence.value()),
            ],
            m,
        )?;
        checks.unique(m, &arena, check.value())?;
        let mut duplicate = false;
        for prior in &records {
            let prior: &ConstructedEnvelope = prior;
            if shape::same_ref(m, &prior.reference, &output.reference)? {
                if !shape::same_bytes(m, &prior.canonical_body, &output.canonical_body)? {
                    return Err(shape::fail("component duplicate authority bytes"));
                }
                duplicate = true;
            }
        }
        if !duplicate {
            records.push(output);
        }
    }
    let checks = checks.finish(m, &arena, true)?;
    // Empty is meaningful only for an original activity with no affected
    // instructions. The loop above derives every actual instruction, including
    // zero/false/unknown rows, rather than inspecting Action existence.
    Ok(Some(Batch {
        records,
        checks,
        slots,
        _owner: owner,
    }))
}
pub(super) fn aggregate(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    plan: &RootPlan,
    base: ConstructedEnvelope,
    batch: &Batch,
) -> Result<ConstructedEnvelope> {
    if !program.uses_component_lifecycle_recipe() {
        return Err(shape::fail("component Authority9 recipe"));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("component aggregate owner"))?;
    let body = base.decoded(m, 4194304)?;
    let schema = text(m, &body, "schema")?;
    if !shape::same_text(m, schema, "ledger-workflow-economic-authority/8")? {
        return Err(shape::fail("component aggregate primary Authority8"));
    }
    let schema = arena.string_value("ledger-workflow-economic-authority/9", m)?;
    let work = owning_reference(m, &arena, &plan.work_ref)?;
    let prior = owning_reference(m, &arena, &plan.current_ref)?;
    let value = WorkingValue::copy(m, &body)?.with_fields(
        m,
        &[
            ("schema", schema.value()),
            ("original_work_ref", work.value()),
            ("prior_decision_ref", prior.value()),
            ("component_authority_checks", batch.checks.value()),
        ],
    )?;
    body::document_borrowed(m, "workflow-economic-authority", &value)
}

pub(super) struct RetainedSelection {
    pub roots: Vec<String>,
    pub quantity: Option<String>,
    _owner: ExternalBufferLease,
}
/// Historical checks consume exact retained source snapshots. The host still
/// owns each producer/member/as-of certificate; today's grants never replace it.
pub(super) fn retained(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &RootPlan,
    decision: &Value,
) -> Result<RetainedSelection> {
    if !program.uses_component_lifecycle_recipe() {
        return Err(shape::fail("retained Component correction recipe"));
    }
    let (_, aggregate) = load(
        m,
        r,
        c,
        &decision["authority_ref"],
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/9",
    )?;
    shape::closed(
        m,
        &aggregate,
        &[
            "schema",
            "operation",
            "actor",
            "source",
            "scope",
            "customer",
            "canonical_economic_namespace",
            "installation_mode",
            "enrollment_ref",
            "current_state_ref",
            "current_grant_ref",
            "original_binding_ref",
            "original_accepted_agreement_ref",
            "original_grant_refs",
            "source_slot",
            "rights",
            "evidence_refs",
            "verified_fact_refs",
            "observed_at",
            "causal_closure_checks",
            "app_observation_refs",
            "app_final_evidence_refs",
            "app_cost_revision_refs",
            "app_cost_closure_checks",
            "app_pending_cost_closure_checks",
            "original_work_ref",
            "prior_decision_ref",
            "component_authority_checks",
        ],
        &[],
    )?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("retained Component authority owner"))?;
    let mut canonical = arena.clone_value(&aggregate, m)?;
    super::super::intent::sort_owned_set(
        &mut canonical,
        "component_authority_checks",
        false,
        m,
        &arena,
    )?;
    shape::equal(m, canonical.value(), &aggregate)?;
    let work = owning_reference(m, &arena, &plan.work_ref)?;
    for (field, expected) in [
        ("original_work_ref", work.value()),
        ("prior_decision_ref", &decision["prior_ref"]),
        ("original_binding_ref", &plan.payload["binding_ref"]),
        ("customer", &plan.payload["customer"]),
        ("observed_at", &decision["accepted_at"]),
        ("source", &decision["operation_key"]["source"]),
    ] {
        shape::equal(m, &aggregate[field], expected)?;
    }
    let operation = text(m, &aggregate, "operation")?;
    let reverse = shape::same_text(m, operation, "work.reverse")?;
    if !reverse && !shape::same_text(m, operation, "quantity.correct")? {
        return Err(shape::fail("Component retained correction operation"));
    }
    let transition = if reverse {
        "work_reverse"
    } else {
        "quantity_correct"
    };
    let actual = text(m, decision, "transition")?;
    if !shape::same_text(m, actual, transition)? {
        return Err(shape::fail("Component retained correction transition"));
    }
    super::super::authority::historical_namespace_body(
        m,
        r,
        c,
        &aggregate,
        &aggregate["canonical_economic_namespace"],
        Some(&aggregate["source"]),
        (
            operation,
            &plan.payload["binding_ref"],
            &decision["accepted_at"],
        ),
    )?;
    let (_, worker) = load(
        m,
        r,
        c,
        &plan.original["authority_ref"],
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    shape::equal(m, &aggregate["actor"], &worker["actor"])?;
    shape::equal(
        m,
        &aggregate["installation_mode"],
        &worker["installation_mode"],
    )?;
    let checks = shape::array(m, &aggregate, "component_authority_checks")?;
    if w::compare(m, checks.len(), 128usize)?.is_gt() {
        return Err(shape::fail("Component retained authority bound"));
    }
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    let maps = shape::array(m, &plan.operands.instantiation, "component_map")?;
    let defs = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "component_slots",
    )?;
    let size = w::usize_add(
        m,
        128 * (std::mem::size_of::<String>() + 128),
        std::mem::size_of::<RetainedSelection>() + 128,
    )?;
    let owner = arena.reserve_external_payload(size, m)?;
    m.write_bytes(size)?;
    let mut roots = Vec::<String>::new();
    roots
        .try_reserve_exact(128)
        .map_err(|_| shape::fail("retained Component root controls"))?;
    let mut quantity: Option<String> = None;
    if !reverse {
        for instruction in instructions {
            let rule = &instruction["rule"];
            if !shape::same(m, &rule["activity"], &plan.payload["activity"])? {
                continue;
            }
            let kind = text(m, &rule["formula"], "kind")?;
            if !shape::same_text(m, kind, "unit")? {
                continue;
            }
            let name = text(m, rule, "component_key")?;
            let map = shape::named(m, maps, "template_key", name)?;
            let mut selected = false;
            for check in checks {
                if shape::same(m, &check["component_slot"], &map["slot_id"])? {
                    selected = true;
                }
            }
            if !selected {
                continue;
            }
            let field = text(m, &rule["formula"], "quantity_field")?;
            match &quantity {
                Some(prior) if !shape::same_text(m, prior, field)? => {
                    return Err(shape::fail(
                        "retained correction has distinct original quantity fields",
                    ))
                }
                Some(_) => {}
                None => {
                    m.scan_bytes(field.len())?;
                    m.write_bytes(field.len())?;
                    quantity = Some(field.to_owned());
                }
            }
            let component = shape::named(m, defs, "key", name)?;
            let root = text(m, component, "root_key")?;
            push_root(m, &mut roots, root)?;
        }
        if quantity.is_none() {
            return Err(shape::fail(
                "retained correction lacks original Unit authority check",
            ));
        }
        // One quantity field may feed multiple independently named Unit roots.
        // The original instruction set determines all of them, not the supplied
        // check subset or a change in the scalar's current numerical value.
        for instruction in instructions {
            let rule = &instruction["rule"];
            if !shape::same(m, &rule["activity"], &plan.payload["activity"])? {
                continue;
            }
            let formula = &rule["formula"];
            let kind = text(m, formula, "kind")?;
            if !shape::same_text(m, kind, "unit")? {
                continue;
            }
            let field = text(m, formula, "quantity_field")?;
            if !shape::same_text(
                m,
                field,
                quantity
                    .as_deref()
                    .ok_or_else(|| shape::fail("retained quantity field"))?,
            )? {
                continue;
            }
            let name = text(m, rule, "component_key")?;
            let component = shape::named(m, defs, "key", name)?;
            let root = text(m, component, "root_key")?;
            push_root(m, &mut roots, root)?;
        }
    }
    let mut expected = 0usize;
    for instruction in instructions {
        let rule = &instruction["rule"];
        if !shape::same(m, &rule["activity"], &plan.payload["activity"])? {
            continue;
        }
        let name = text(m, rule, "component_key")?;
        let component = shape::named(m, defs, "key", name)?;
        let root = text(m, component, "root_key")?;
        if !reverse && !affected_root(m, &roots, root)? {
            continue;
        }
        expected = w::usize_add(m, expected, 1)?;
        let map = shape::named(m, maps, "template_key", name)?;
        let slot = text(m, map, "slot_id")?;
        let check = shape::named(m, checks, "component_slot", slot)?;
        shape::closed(
            m,
            check,
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
        let root_map = shape::named(m, maps, "template_key", root)?;
        shape::equal(m, &check["root_slot"], &root_map["slot_id"])?;
        shape::equal(m, &check["binding_slot"], &rule["binding_slot"])?;
        let binding_slot = text(m, rule, "binding_slot")?;
        let rows = shape::array(m, &plan.operands.binding, "bindings")?;
        let bound = shape::named(m, rows, "slot", binding_slot)?;
        shape::equal(m, &check["terms_ref"], &bound["binding_ref"])?;
        let definitions = shape::array(m, program.original_policy_value(), "binding_slots")?;
        let definition = shape::named(m, definitions, "name", binding_slot)?;
        let book = text(m, definition, "book")?;
        let schema = if shape::same_text(m, book, "cost_observation")? {
            "ledger-workflow-observation-terms/1"
        } else if shape::same_text(m, book, "retail")? {
            "ledger-workflow-slot-binding-terms/3"
        } else {
            return Err(shape::fail("retained Component original book"));
        };
        let (_, terms) = load(m, r, c, &check["terms_ref"], "workflow-slot-terms", schema)?;
        shape::equal(
            m,
            &terms["correction_source_slot"],
            &definition["correction_source_slot"],
        )?;
        for field in ["agreement_id", "customer"] {
            shape::equal(m, &terms[field], &plan.operands.binding[field])?;
        }
        let key_scope = arena.clone_value(&decision["operation_key"]["scope"], m)?;
        shape::equal(m, &terms["scope"], key_scope.value())?;
        let (_, authority) = load(
            m,
            r,
            c,
            &check["authority_ref"],
            "workflow-economic-authority",
            "ledger-workflow-economic-authority/8",
        )?;
        shape::closed(
            m,
            &authority,
            &[
                "schema",
                "operation",
                "actor",
                "source",
                "scope",
                "customer",
                "canonical_economic_namespace",
                "installation_mode",
                "enrollment_ref",
                "current_state_ref",
                "current_grant_ref",
                "original_binding_ref",
                "original_accepted_agreement_ref",
                "original_grant_refs",
                "source_slot",
                "rights",
                "evidence_refs",
                "verified_fact_refs",
                "observed_at",
                "causal_closure_checks",
                "app_observation_refs",
                "app_final_evidence_refs",
                "app_cost_revision_refs",
                "app_cost_closure_checks",
                "app_pending_cost_closure_checks",
            ],
            &[],
        )?;
        validate_role(
            m,
            r,
            c,
            plan,
            (program, &terms, &authority),
            (operation, &decision["accepted_at"]),
        )?;
        shape::equal(m, &authority["actor"], &aggregate["actor"])?;
        shape::equal(
            m,
            &check["authority_evidence_refs"],
            &authority["evidence_refs"],
        )?;
        // Recompute the exact applicable subset of the primary full request
        // union, rather than accepting arbitrary membership-only subsets.
        let mut subset =
            crate::policy::workflow::selected_program::supplier_projection::Values::new(
                m, &arena, 32,
            )?;
        for value in shape::array(m, &aggregate, "evidence_refs")? {
            let reference = super::super::working_value::parse_owned_reference(m, &arena, value)?;
            match dependent::evidence_values(
                m,
                &plan.operands.binding,
                &authority,
                std::slice::from_ref(reference.reference()),
            ) {
                Ok(()) => subset.unique(m, &arena, value)?,
                Err(e) if e.code == "AUTHORITY_REQUIRED" => {}
                Err(e) => return Err(e),
            }
        }
        let subset = subset.finish(m, &arena, true)?;
        shape::equal(m, &check["authority_evidence_refs"], subset.value())?;
        if reverse {
            push_root(m, &mut roots, root)?;
        }
    }
    if !w::compare(m, expected, checks.len())?.is_eq() {
        return Err(shape::fail(
            "retained Component complete affected authority set",
        ));
    }
    // Primary correction authority is independently mandatory even when this
    // genuinely unpriced activity has no affected component checks.
    validate_role(
        m,
        r,
        c,
        plan,
        (program, &plan.operands.slot_terms, &aggregate),
        (operation, &decision["accepted_at"]),
    )?;
    Ok(RetainedSelection {
        roots,
        quantity,
        _owner: owner,
    })
}
fn push_root(m: &mut CoreMeter, roots: &mut Vec<String>, root: &str) -> Result<()> {
    for prior in roots.iter() {
        if shape::same_text(m, prior, root)? {
            return Ok(());
        }
    }
    m.scan_bytes(root.len())?;
    m.write_bytes(root.len())?;
    roots.push(root.to_owned());
    Ok(())
}
fn validate_role(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    plan: &RootPlan,
    selected: (&SelectedProgram, &Value, &Value),
    purpose: (&str, &Value),
) -> Result<()> {
    let (program, terms, authority) = selected;
    let (operation, at) = purpose;
    let (_, enrollment) = load(
        m,
        r,
        c,
        &authority["enrollment_ref"],
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    shape::equal(m, &enrollment["scope"], &authority["scope"])?;
    shape::equal(
        m,
        &enrollment["installation_mode"],
        &authority["installation_mode"],
    )?;

    let rights = shape::array(m, authority, "rights")?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("retained Component role owner"))?;
    for right in ["correct", "read"] {
        let required = arena.string_value(right, m)?;
        if !shape::member(m, rights, required.value())? {
            return Err(Error::new(
                "AUTHORITY_REQUIRED",
                "retained Component correction rights",
            ));
        }
    }

    shape::equal(
        m,
        &authority["source_slot"],
        &terms["correction_source_slot"],
    )?;
    shape::equal(
        m,
        &authority["original_binding_ref"],
        &plan.payload["binding_ref"],
    )?;
    shape::equal(m, &authority["customer"], &plan.payload["customer"])?;
    super::super::authority::historical_namespace_body(
        m,
        r,
        c,
        authority,
        &authority["canonical_economic_namespace"],
        Some(&authority["source"]),
        (operation, &plan.payload["binding_ref"], at),
    )?;
    let (_, state) = load(
        m,
        r,
        c,
        &authority["current_state_ref"],
        "workflow-source-state",
        "ledger-workflow-source-state/1",
    )?;
    let active = text(m, &state, "status")?;
    if !shape::same_text(m, active, "active")? {
        return Err(shape::fail(
            "retained Component active original source snapshot",
        ));
    }
    shape::equal(m, &state["enrollment_ref"], &authority["enrollment_ref"])?;
    shape::equal(
        m,
        &state["current_grant_ref"],
        &authority["current_grant_ref"],
    )?;
    let slot = text(m, terms, "correction_source_slot")?;
    let nominations = shape::array(m, &plan.operands.binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    if !shape::member(m, sources, &authority["source"])? {
        return Err(shape::fail("retained Component corrector nomination"));
    }
    let activity = text(m, &plan.payload, "activity")?;
    let product = original_product(m, program, &plan.payload["product_ref"])?;
    let product_id = text(m, product, "product_id")?;
    let originals = shape::array(m, authority, "original_grant_refs")?;
    if originals.is_empty() {
        return Err(shape::fail(
            "retained Component original grant intersection",
        ));
    }
    let binding_grants = shape::array(m, &plan.operands.binding, "authority_refs")?;
    let terms_grants = shape::array(m, terms, "source_grant_refs")?;
    let accepted = crate::domain::Timestamp::parse_with_work(
        text(m, &plan.operands.binding, "accepted_at")?,
        m,
    )?
    .micros();
    let actual_at = at
        .as_str()
        .ok_or_else(|| shape::fail("retained Component operation time"))?;
    let actual_at = crate::domain::Timestamp::parse_with_work(actual_at, m)?.micros();
    let state_at =
        crate::domain::Timestamp::parse_with_work(text(m, &state, "accepted_at")?, m)?.micros();
    if w::compare(m, state_at, actual_at)?.is_gt() {
        return Err(shape::fail("retained Component future source snapshot"));
    }
    for value in std::iter::once(&authority["current_grant_ref"]).chain(originals.iter()) {
        let original = shape::member(m, originals, value)?;
        if original
            && (!shape::member(m, terms_grants, value)?
                || !shape::member(m, binding_grants, value)?)
        {
            return Err(shape::fail("retained Component original grant membership"));
        }
        let (_, grant) = load(
            m,
            r,
            c,
            value,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        for (field, want) in [
            ("source", &authority["source"]),
            ("principal_id", &authority["actor"]),
            (
                "canonical_economic_namespace",
                &authority["canonical_economic_namespace"],
            ),
        ] {
            shape::equal(m, &grant[field], want)?;
        }
        if !super::super::authority::grant_covers(
            m,
            &grant,
            (
                &plan.payload["customer"],
                activity,
                product_id,
                slot,
                (operation, &["correct", "read"]),
                if original { accepted } else { actual_at },
            ),
        )? {
            return Err(Error::new(
                "AUTHORITY_REQUIRED",
                "retained Component original/current correction purpose",
            ));
        }
    }
    Ok(())
}
