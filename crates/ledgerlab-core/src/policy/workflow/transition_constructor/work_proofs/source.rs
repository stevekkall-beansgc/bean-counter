//! The nominated attestation issuer is not the subject's worker/assessor.
//! These are immutable original/historical joins; current consuming authority
//! and the retained assertion's completed origin remain separate obligations.
use super::*;

/// A field issuer is not a promised monetary principal merely because its
/// nomination shares a Binding. Slots with ANY actual business use remain
/// subject to the independent current/causal promise checks.
pub(in crate::policy::workflow::transition_constructor) fn proof_only_slot(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    slot: &str,
) -> Result<bool> {
    let mut proof = false;
    for (_, schema) in program.fact_schemas() {
        for field in shape::array(m, schema.canonical_value(), "fields")? {
            let provenance = &field["provenance"];
            let kind = shape::text(m, provenance, "kind")?;
            if shape::same_text(m, kind, "attested")? {
                let issuer = shape::text(m, provenance, "source_slot")?;
                proof |= shape::same_text(m, issuer, slot)?;
            }
        }
    }
    if program.uses_component_lifecycle_recipe() {
        for rule in shape::array(m, program.original_policy_value(), "rules")? {
            let formula = &rule["formula"];
            let kind = shape::text(m, formula, "kind")?;
            if shape::same_text(m, kind, "observe_cost")? {
                let issuer = shape::text(m, formula, "evidence_slot")?;
                proof |= shape::same_text(m, issuer, slot)?;
            }
        }
    }
    if !proof {
        return Ok(false);
    }
    for document in program.source_documents() {
        let body = document.canonical_value();
        let schema = shape::text(m, body, "schema")?;
        if shape::same_text(m, schema, "ledger-workflow-product/1")? {
            for activity in shape::array(m, body, "activities")? {
                let source = shape::text(m, activity, "source_slot")?;
                if shape::same_text(m, source, slot)? {
                    return Ok(false);
                }
            }
        }
    }
    let policy = program.original_policy_value();
    for binding in shape::array(m, policy, "binding_slots")? {
        let source = shape::text(m, binding, "correction_source_slot")?;
        if shape::same_text(m, source, slot)? {
            return Ok(false);
        }
    }
    for family in shape::array(m, policy, "families")? {
        for name in ["source_slot", "correction_source_slot"] {
            let source = shape::text(m, family, name)?;
            if shape::same_text(m, source, slot)? {
                return Ok(false);
            }
        }
        if let Some(close) = family.get("no_success_close") {
            let source = shape::text(m, close, "close_source_slot")?;
            if shape::same_text(m, source, slot)? {
                return Ok(false);
            }
        }
    }
    if program.uses_stage_lifecycle_recipe() {
        for stage in shape::array(m, policy, "stages")? {
            for field in ["authorization_source_slot", "closure_source_slot"] {
                let source = shape::text(m, stage, field)?;
                if shape::same_text(m, source, slot)? {
                    return Ok(false);
                }
            }
            for expected in shape::array(m, stage, "expected_slots")? {
                let source = shape::text(m, expected, "source_slot")?;
                if shape::same_text(m, source, slot)? {
                    return Ok(false);
                }
            }
        }
    }
    for declaration in shape::array(m, policy, "app_delayed_costs")? {
        for name in [
            "work_source_slot",
            "cost_source_slot",
            "correction_source_slot",
        ] {
            let source = shape::text(m, declaration, name)?;
            if shape::same_text(m, source, slot)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
pub(in crate::policy::workflow::transition_constructor) fn proof_only_nominee(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    binding: &Value,
    source: &Value,
) -> Result<bool> {
    let mut found = false;
    for nomination in shape::array(m, binding, "source_slots")? {
        let sources = shape::array(m, nomination, "sources")?;
        if shape::member(m, sources, source)? {
            let slot = shape::text(m, nomination, "name")?;
            if !proof_only_slot(m, program, slot)? {
                return Ok(false);
            }
            found = true;
        }
    }
    Ok(found)
}

fn binding_terms(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    payload: &Value,
    program: &SelectedProgram,
) -> Result<(
    super::super::record_body::RecordBody,
    super::super::record_body::RecordBody,
)> {
    let (_, binding) = load(
        m,
        r,
        c,
        &payload["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    for name in ["product_ref", "policy_ref"] {
        shape::equal(m, &binding[name], &payload[name])?;
    }
    if let Some(customer) = payload.get("customer") {
        shape::equal(m, &binding["customer"], customer)?;
    }
    let bindings = shape::array(m, &binding, "bindings")?;
    let selected = if program.uses_component_lifecycle_recipe() {
        super::super::input_selection::component_retail_row(program, payload, &binding, m)?
    } else if program.uses_stage_lifecycle_recipe() {
        let mut selected = None;
        for rule in shape::array(m, program.original_policy_value(), "rules")? {
            if !shape::same(m, &rule["activity"], &payload["activity"])? {
                continue;
            }
            let name = shape::text(m, rule, "binding_slot")?;
            let definitions = shape::array(m, program.original_policy_value(), "binding_slots")?;
            let definition = shape::named(m, definitions, "name", name)?;
            let book = shape::text(m, definition, "book")?;
            if !shape::same_text(m, book, "retail")? {
                continue;
            }
            let actual = shape::named(m, bindings, "slot", name)?;
            if let Some(previous) = selected {
                shape::equal(m, previous, actual)?;
            }
            selected = Some(actual);
        }
        selected.ok_or_else(|| shape::fail("field issuer exact original retail slot"))?
    } else {
        if !w::compare(m, bindings.len(), 1usize)?.is_eq() {
            return Err(shape::fail("field issuer original retail binding"));
        }
        &bindings[0]
    };
    let schema = if program.uses_structural_binding_recipe() {
        "ledger-workflow-slot-binding-terms/3"
    } else {
        "ledger-workflow-slot-binding-terms/7"
    };
    let (_, terms) = load(
        m,
        r,
        c,
        &selected["binding_ref"],
        "workflow-slot-terms",
        schema,
    )?;
    for field in ["customer", "agreement_id"] {
        shape::equal(m, &terms[field], &binding[field])?;
    }
    shape::equal(m, &terms["slot"], &selected["slot"])?;
    Ok((binding, terms))
}
fn enrollment_join(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    grant: &Value,
) -> Result<(ExactRef, super::super::record_body::RecordBody)> {
    let (reference, enrollment) = load(
        m,
        r,
        c,
        &grant["original_ceiling_ref"],
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning field issuer identity"))?;
    let key = super::super::cost_postings::operation_key(m, &arena, c)?;
    shape::equal(m, &enrollment["scope"], &key.value()["scope"])?;
    let mode = arena.string_value(&c.installation_mode, m)?;
    shape::equal(m, &enrollment["installation_mode"], mode.value())?;
    shape::equal(m, &grant["grant_id"], &enrollment["enrollment_id"])?;
    for field in ["source", "principal_id", "canonical_economic_namespace"] {
        shape::equal(m, &grant[field], &enrollment["ceiling"][field])?;
    }
    Ok((reference, enrollment))
}
fn product<'a>(
    m: &mut CoreMeter,
    program: &'a SelectedProgram,
    payload: &Value,
) -> Result<&'a Value> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning field issuer product"))?;
    let mut selected = None;
    for document in program.source_documents() {
        let reference =
            super::super::working_value::owning_definition(m, &arena, document.reference())?;
        if shape::same(m, reference.value(), &payload["product_ref"])? {
            selected = Some(document.canonical_value());
        }
    }
    selected.ok_or_else(|| shape::fail("field issuer original product"))
}
/// Before Work exists, bind its explicit business namespace to the accepted
/// worker nomination/ceiling, rather than the independent issuer's namespace.
pub(in crate::policy::workflow::transition_constructor) fn work_namespace(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    payload: &Value,
    namespace: &str,
) -> Result<()> {
    let (binding, terms) = binding_terms(m, r, c, payload, program)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work subject namespace"))?;
    let expected = arena.string_value(namespace, m)?;
    shape::equal(m, &binding["source_namespace"], expected.value())?;
    let product = product(m, program, payload)?;
    let activity_name = shape::text(m, payload, "activity")?;
    let activities = shape::array(m, product, "activities")?;
    let activity = shape::named(m, activities, "name", activity_name)?;
    let slot = shape::text(m, activity, "source_slot")?;
    let nominations = shape::array(m, &binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    let accepted = shape::array(m, &binding, "authority_refs")?;
    let product_id = shape::text(m, product, "product_id")?;
    let mut found = false;
    for value in shape::array(m, &terms, "source_grant_refs")? {
        let (_, grant) = load(
            m,
            r,
            c,
            value,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        if !shape::member(m, accepted, value)? {
            return Err(shape::fail("Work namespace grant absent accepted Binding"));
        }
        if !shape::member(m, sources, &grant["source"])?
            || !shape::same(m, &grant["canonical_economic_namespace"], expected.value())?
        {
            continue;
        }
        authority::grant_shape(m, &grant)?;
        enrollment_join(m, r, c, &grant)?;
        authority::grant_coverage(
            m,
            &grant,
            &payload["customer"],
            activity_name,
            product_id,
            slot,
            ("work.accept", &["read", "submit"]),
        )?;
        found = true;
    }
    if !found {
        return Err(shape::fail(
            "Work subject namespace lacks original worker nomination",
        ));
    }
    Ok(())
}
/// Revalidate the ACTUAL retained issuer and its accepted original nomination.
/// No current issuer state/grant or business-operation permission is consumed.
pub(in crate::policy::workflow::transition_constructor) fn retained(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    payload: &Value,
    selected: (&Value, &Value, &Value, &str),
) -> Result<()> {
    let (provenance, att, verification, received_at) = selected;
    let kind = shape::text(m, provenance, "kind")?;
    if !shape::same_text(m, kind, "attested")? {
        return Err(shape::fail("retained field caller provenance"));
    }
    shape::equal(m, &att["evidence_kind"], &provenance["evidence_kind"])?;
    let (binding, terms) = binding_terms(m, r, c, payload, program)?;
    let slot = shape::text(m, provenance, "source_slot")?;
    retained_selected(
        m,
        r,
        c,
        program,
        payload,
        (&binding, &terms),
        (slot, att, verification, received_at),
    )
}
/// Same retained issuer intersection for the exact observation Terms selected
/// from its original formula. No present issuer head or payable permission.
pub(in crate::policy::workflow::transition_constructor) fn retained_observation(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    payload: &Value,
    original: (&Value, &Value),
    selected: (&str, &Value, &Value, &str),
) -> Result<()> {
    if !program.uses_component_lifecycle_recipe() {
        return Err(shape::fail("actual observation original recipe"));
    }
    retained_selected(m, r, c, program, payload, original, selected)
}
fn retained_selected(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    payload: &Value,
    original: (&Value, &Value),
    selected: (&str, &Value, &Value, &str),
) -> Result<()> {
    let (binding, terms) = original;
    let (slot, att, verification, received_at) = selected;
    let nominations = shape::array(m, binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    let kinds = shape::array(m, nomination, "evidence_kinds")?;
    if !shape::member(m, sources, &att["source"])?
        || !shape::member(m, kinds, &att["evidence_kind"])?
    {
        return Err(shape::fail("field issuer original nomination"));
    }
    let (_, grant) = load(
        m,
        r,
        c,
        &verification["source_grant_ref"],
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let (enrollment_ref, enrollment) = enrollment_join(m, r, c, &grant)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning retained field issuer"))?;
    let reference = owning_reference(m, &arena, &enrollment_ref)?;
    shape::equal(
        m,
        &verification["authenticated_principal_ref"],
        reference.value(),
    )?;
    shape::equal(
        m,
        &grant["revision"],
        &verification["source_grant_revision"],
    )?;
    shape::equal(m, &grant["source"], &att["source"])?;
    shape::equal(m, &verification["verified_source"], &att["source"])?;
    let issued = Timestamp::parse_with_work(shape::text(m, att, "issued_at")?, m)?.micros();
    let trusted =
        Timestamp::parse_with_work(shape::text(m, verification, "trusted_received_at")?, m)?
            .micros();
    let received = Timestamp::parse_with_work(received_at, m)?.micros();
    if w::compare(m, issued, trusted)?.is_gt() || w::compare(m, trusted, received)?.is_gt() {
        return Err(shape::fail("field issuer historical trusted time"));
    }
    authority::grant_shape(m, &grant)?;
    let token = InvocationToken::new();
    authority::grant_intersection(m, &token, &grant, &enrollment["ceiling"], &trusted.into())?;
    let product = product(m, program, payload)?;
    let product_id = shape::text(m, product, "product_id")?;
    let activity = shape::text(m, payload, "activity")?;
    authority::grant_coverage(
        m,
        &grant,
        &binding["customer"],
        activity,
        product_id,
        slot,
        ("evidence.retain", &["read", "submit"]),
    )?;
    let accepted = shape::array(m, binding, "authority_refs")?;
    let mut found = false;
    for value in shape::array(m, terms, "source_grant_refs")? {
        let (_, original) = load(
            m,
            r,
            c,
            value,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        if !shape::member(m, accepted, value)? {
            return Err(shape::fail("field issuer grant absent accepted Binding"));
        }
        if !shape::same(m, &original["original_ceiling_ref"], reference.value())? {
            continue;
        }
        for field in [
            "source",
            "principal_id",
            "canonical_economic_namespace",
            "grant_id",
        ] {
            shape::equal(m, &original[field], &grant[field])?;
        }
        authority::grant_shape(m, &original)?;
        authority::grant_intersection(
            m,
            &token,
            &original,
            &enrollment["ceiling"],
            &trusted.into(),
        )?;
        authority::grant_coverage(
            m,
            &original,
            &binding["customer"],
            activity,
            product_id,
            slot,
            ("evidence.retain", &["read", "submit"]),
        )?;
        found = true;
    }
    if !found {
        return Err(shape::fail(
            "field issuer lacks originally accepted enrollment",
        ));
    }
    Ok(())
}
