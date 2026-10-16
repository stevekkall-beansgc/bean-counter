//! Complete selected family selection before root mutation. Original targets
//! and assented clauses, not caller-provided reversals, define the closure.
use super::super::targets;
use super::*;
pub(super) struct FamilyPlan {
    pub target_ref: ExactRef,
    pub target: Value,
    pub declaration: Value,
    pub family: Value,
    pub prior_ref: ExactRef,
    pub prior: Value,
    pub evidence: Vec<ExactRef>,
    pub dependent: Option<(ExactRef, Value, Value)>,
}
pub(super) fn references(
    m: &mut CoreMeter,
    v: &Value,
    min: usize,
    max: usize,
) -> Result<Vec<ExactRef>> {
    let values = v
        .as_array()
        .ok_or_else(|| shape::fail("causal evidence Ref array"))?;
    if meter::cmp(m, &values.len().into(), &min.into())?.is_lt()
        || meter::cmp(m, &values.len().into(), &max.into())?.is_gt()
    {
        return Err(shape::fail("causal evidence count"));
    }
    let mut refs = vec![];
    for value in values {
        m.visit("causal.evidence")?;
        refs.push(shape::reference(m, value)?);
    }
    let ordered = body::ref_set(m, refs)?;
    if !meter::cmp(m, &ordered.len().into(), &values.len().into())?.is_eq() {
        return Err(shape::fail("duplicate causal evidence"));
    }
    Ok(ordered)
}
fn clause(m: &mut CoreMeter, terms: &Value, root: &str, slot: &str, source: &str) -> Result<Value> {
    let clauses = shape::array(m, terms, "causal_reversal_clauses")?;
    let mut found = None;
    for c in clauses {
        m.visit("causal.original_clause")?;
        shape::closed(
            m,
            c,
            &[
                "root_component_slot",
                "family_slot",
                "correction_source_slot",
                "mode",
                "standalone_deadline_effect",
            ],
            &[],
        )?;
        if text_is(m, c, "root_component_slot", root)? && text_is(m, c, "family_slot", slot)? {
            if found.is_some() {
                return Err(shape::fail("duplicate original causal clause"));
            }
            equal(m, &c["correction_source_slot"], &json!(source))?;
            equal(m, &c["mode"], &json!("exact_inverse_and_permanent_close"))?;
            equal(
                m,
                &c["standalone_deadline_effect"],
                &json!("does_not_expire_preaccepted_causal_reversal"),
            )?;
            found = Some(c.clone());
        }
    }
    found.ok_or_else(|| Error::new("AUTHORITY_REQUIRED", "original causal clause missing"))
}
fn matching_input(
    m: &mut CoreMeter,
    values: &[Value],
    field_name: &str,
    wanted: &ExactRef,
    used: &mut Vec<usize>,
) -> Result<usize> {
    let mut found = None;
    for (i, value) in values.iter().enumerate() {
        m.visit("causal.explicit_input")?;
        shape::closed(
            m,
            value,
            &[field_name, "expected_revision", "authority_evidence_refs"],
            &[],
        )?;
        let reference = shape::reference(m, &value[field_name])?;
        if shape::same_ref(m, &reference, wanted)? {
            if found.is_some() {
                return Err(shape::fail("duplicate semantic causal closure"));
            }
            found = Some(i);
        }
    }
    let index = found.ok_or_else(|| {
        Error::new(
            "REVERSAL_DEPENDENTS_REQUIRED",
            "explicit original family/decision closure missing",
        )
    })?;
    for prior in used.iter() {
        if meter::cmp(m, &(*prior).into(), &index.into())?.is_eq() {
            return Err(shape::fail("causal input reused"));
        }
    }
    used.push(index);
    Ok(index)
}
fn closed_lineage(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    plan: &RootPlan,
    family: &Value,
    head: &ExactRef,
) -> Result<()> {
    let cause = field(m, family, "causal_closure")?;
    shape::closed(
        m,
        cause,
        &[
            "root_decision_ref",
            "root_expected_revision",
            "authority_ref",
            "original_slot_terms_ref",
            "root_component_slot",
        ],
        &[],
    )?;
    equal(
        m,
        &cause["original_slot_terms_ref"],
        &json!(plan.operands.slot_terms_ref),
    )?;
    let (_, root) = load(
        m,
        r,
        c,
        &cause["root_decision_ref"],
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    equal(m, &root["decision_id"], &plan.current["decision_id"])?;
    equal(m, &root["revision"], &cause["root_expected_revision"])?;
    let (_, authority) = load(
        m,
        r,
        c,
        &cause["authority_ref"],
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/3",
    )?;
    equal(m, &authority["scope"], &json!(c.key.scope))?;
    equal(
        m,
        &authority["original_binding_ref"],
        &plan.payload["binding_ref"],
    )?;
    if !text_is(m, &authority, "operation", "quantity.correct")?
        && !text_is(m, &authority, "operation", "work.reverse")?
    {
        return Err(shape::fail("first-closing authority operation"));
    }
    let closing_revision = counter(m, &root, "revision")?;
    let current_revision = counter(m, &plan.current, "revision")?;
    if !meter::cmp(m, &closing_revision, &current_revision)?.is_lt() {
        return Err(shape::fail(
            "first closing cause is not prior root revision",
        ));
    }
    let checks = shape::array(m, &authority, "causal_closure_checks")?;
    let mut found = false;
    for check in checks {
        m.visit("causal.first_closing_authority")?;
        if shape::same(m, &check["family_state_ref"], &family["prior_ref"])? {
            if found {
                return Err(shape::fail("duplicate first-closing family check"));
            }
            equal(
                m,
                &check["original_slot_terms_ref"],
                &cause["original_slot_terms_ref"],
            )?;
            equal(
                m,
                &check["root_component_slot"],
                &cause["root_component_slot"],
            )?;
            equal(m, &check["family_slot"], &family["family_slot"])?;
            let (_, before) = load(
                m,
                r,
                c,
                &check["family_state_ref"],
                "workflow-family-state",
                "ledger-workflow-family-state/1",
            )?;
            equal(m, &before["target_ref"], &family["target_ref"])?;
            equal(m, &before["family_slot"], &family["family_slot"])?;
            equal(m, &before["revision"], &check["expected_revision"])?;
            match (before.get("decision_ref"), family.get("decision_ref")) {
                (Some(old_ref), Some(new_ref)) => {
                    let (_, old) = load(
                        m,
                        r,
                        c,
                        old_ref,
                        "workflow-decision",
                        "ledger-workflow-decision-revision/1",
                    )?;
                    let (_, reversed) = load(
                        m,
                        r,
                        c,
                        new_ref,
                        "workflow-decision",
                        "ledger-workflow-decision-revision/1",
                    )?;
                    super::super::correction::dictionary_decision(m, &reversed)?;
                    for (k, v) in [
                        ("decision_id", old["decision_id"].clone()),
                        ("subject_ref", before["claim_ref"].clone()),
                        ("kind", json!("outcome")),
                        ("transition", json!("dependent_reverse")),
                        ("status", json!("reversed")),
                        ("prior_ref", old_ref.clone()),
                        ("authority_ref", check["dependent_authority_ref"].clone()),
                        ("active_action_refs", json!([])),
                    ] {
                        equal(m, &reversed[k], &v)?;
                    }
                    let old_revision = counter(m, &old, "revision")?;
                    let next = meter::add(m, &old_revision, &1.into())?;
                    equal(m, &reversed["revision"], &json!(next.to_string()))?;
                }
                (None, None) => {}
                _ => {
                    return Err(shape::fail(
                        "terminal family must retain same-operation reversal decision",
                    ));
                }
            }
            let prior_revision = counter(m, &before, "revision")?;
            let expected = meter::add(m, &prior_revision, &1.into())?;
            equal(m, &family["revision"], &json!(expected.to_string()))?;
            for optional in ["window", "anchor_work_ref", "claim_ref"] {
                m.visit("causal.closed_optional_presence")?;
                match (before.get(optional), family.get(optional)) {
                    (Some(a), Some(b)) => equal(m, a, b)?,
                    (None, None) => {}
                    _ => return Err(shape::fail("closed family changed original footprint")),
                }
            }
            found = true;
        }
    }
    if !found {
        return Err(shape::fail(
            "terminal family lacks its exact first-closing authority",
        ));
    }
    let revision = counter(m, family, "revision")?;
    let target = shape::reference(m, &family["target_ref"])?;
    let id = body::semantic_id(
        m,
        "wfamily_",
        "workflow-family-state",
        &json!([target.id, family["family_slot"], revision.to_string()]),
    )?;
    if !shape::same_text(m, &id, &head.id)? {
        return Err(shape::fail("terminal family identity"));
    }
    Ok(())
}
pub(super) fn prepare(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &RootPlan,
    request: &mut Value,
    state: &DraftStateData,
) -> Result<Vec<FamilyPlan>> {
    let closures = shape::array(m, request, "family_closures")?.to_vec();
    let reversals = shape::array(m, request, "dependent_reversals")?.to_vec();
    if meter::cmp(m, &closures.len().into(), &64.into())?.is_gt()
        || meter::cmp(m, &reversals.len().into(), &31.into())?.is_gt()
    {
        return Err(shape::fail("causal closure count"));
    }
    let mut used_closures = vec![];
    let mut used_reversals = vec![];
    let mut out = vec![];
    let instructions = shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )?;
    for instruction in instructions {
        if !text_is(m, instruction, "opcode", "evaluate_family")? {
            continue;
        }
        let family = field(m, instruction, "family")?;
        if !shape::same(
            m,
            &family["selector"]["target_activity"],
            &plan.payload["activity"],
        )? {
            continue;
        }
        let root_key = text(m, &family["selector"], "component_key")?;
        if !is_affected(m, plan, root_key)? {
            continue;
        }
        let components = shape::array(m, &plan.operands.instantiation, "component_map")?;
        let component = shape::named(m, components, "template_key", root_key)?;
        let root_slot = text(m, component, "slot_id")?;
        let family_key = text(m, family, "family_key")?;
        let family_maps = shape::array(m, &plan.operands.instantiation, "family_map")?;
        let mapping = shape::named(m, family_maps, "template_key", family_key)?;
        let slot = text(m, mapping, "slot_id")?;
        let target_id = body::semantic_id(
            m,
            "wtarget_",
            "workflow-target",
            &json!([
                c.key.scope,
                plan.operands.binding["agreement_id"],
                plan.claim_ref.id,
                root_slot
            ]),
        )?;
        let (target_ref, target) = r.by_identity(
            m,
            &c.key,
            "workflow-target",
            &target_id,
            "ledger-workflow-component-target/1",
        )?;
        shape::closed(
            m,
            &target,
            &[
                "schema",
                "scope",
                "agreement_id",
                "work_claim_ref",
                "work_ref",
                "root_component_slot",
                "original_decision_ref",
                "original_action_refs",
                "original_net",
                "policy_ref",
                "product_ref",
                "binding_ref",
                "compiled_plan_ref",
                "families",
                "original_work_accepted_at",
            ],
            &[],
        )?;
        for (key, wanted) in [
            ("scope", json!(c.key.scope)),
            (
                "agreement_id",
                plan.operands.binding["agreement_id"].clone(),
            ),
            ("work_claim_ref", json!(plan.claim_ref)),
            ("work_ref", json!(plan.work_ref)),
            ("root_component_slot", json!(root_slot)),
            ("original_decision_ref", json!(plan.original_ref)),
            ("binding_ref", plan.payload["binding_ref"].clone()),
            ("policy_ref", plan.payload["policy_ref"].clone()),
            ("product_ref", plan.payload["product_ref"].clone()),
            (
                "compiled_plan_ref",
                json!(program.compiled_plan().reference()),
            ),
            (
                "original_work_accepted_at",
                plan.work["accepted_at"].clone(),
            ),
        ] {
            equal(m, &target[key], &wanted)?;
        }
        let mut original_basis = vec![];
        for original_action in shape::array(m, &plan.original, "active_action_refs")? {
            m.visit("causal.original_named_root_basis")?;
            let (reference, action) = load(
                m,
                r,
                c,
                original_action,
                "workflow-action",
                "ledger-workflow-action/1",
            )?;
            if text_is(m, &action, "root_component_slot", root_slot)? {
                original_basis.push(reference);
            }
        }
        let original_basis = body::ref_set(m, original_basis)?;
        equal(m, &target["original_action_refs"], &json!(original_basis))?;
        let declarations = shape::array(m, &target, "families")?;
        let mut expected_families = BigInt::from(0);
        for candidate in instructions {
            m.visit("causal.original_family_declaration")?;
            if !text_is(m, candidate, "opcode", "evaluate_family")? {
                continue;
            }
            let declared = field(m, candidate, "family")?;
            if shape::same(
                m,
                &declared["selector"]["target_activity"],
                &plan.payload["activity"],
            )? && text_is(m, &declared["selector"], "component_key", root_key)?
            {
                expected_families = meter::add(m, &expected_families, &1.into())?;
            }
        }
        if !meter::cmp(m, &declarations.len().into(), &expected_families)?.is_eq() {
            return Err(shape::fail("original target family cardinality"));
        }
        for declaration in declarations {
            shape::closed(
                m,
                declaration,
                &[
                    "family_key",
                    "family_slot",
                    "prospective_decision_id",
                    "window_terms",
                    "maximum_premium",
                    "maximum_discount",
                ],
                &[],
            )?;
        }
        let declaration = shape::named(m, declarations, "family_slot", slot)?;
        outcome::original_target_association(
            m,
            &plan.operands.terms,
            &target,
            declaration,
            family,
        )?;
        let activity = text(m, &plan.payload, "activity")?;
        let original_prices = pricing::work(program, activity, &plan.payload["facts"], m)?;
        let mut net = BigInt::from(0);
        for price in &original_prices {
            if shape::same_text(m, &price.root_key, root_key)? {
                net = meter::add(m, &net, &price.atoms.into())?;
            }
        }
        equal(
            m,
            &target["original_net"],
            &json!({"currency":"USD","scale":18,"atoms":net.to_string()}),
        )?;
        let (premium, discount) = targets::maxima(m, family, &net)?;
        equal(
            m,
            &declaration["maximum_premium"],
            &json!({"currency":"USD","scale":18,"atoms":premium.to_string()}),
        )?;
        equal(
            m,
            &declaration["maximum_discount"],
            &json!({"currency":"USD","scale":18,"atoms":discount.to_string()}),
        )?;
        let prospective = targets::prospective_decision(m, &target_id, slot)?;
        equal(
            m,
            &declaration["prospective_decision_id"],
            &json!(prospective),
        )?;
        equal(m, &declaration["window_terms"], &family["window"])?;
        let mut head = None;
        for candidate in &state.family_heads {
            m.visit("causal.current_family")?;
            if shape::same_ref(m, &candidate.target_ref, &target_ref)?
                && shape::same_text(m, &candidate.family_slot, slot)?
            {
                if head.is_some() {
                    return Err(shape::fail("duplicate current family head"));
                }
                head = Some(candidate);
            }
        }
        let head = head.ok_or_else(|| {
            Error::new(
                "REVERSAL_DEPENDENTS_REQUIRED",
                "original prospective family head missing",
            )
        })?;
        let prior = r.get_schemas(
            m,
            &c.key,
            &head.reference,
            "workflow-family-state",
            &[
                "ledger-workflow-family-state/1",
                "ledger-workflow-family-state/2",
            ],
        )?;
        shape::closed(
            m,
            &prior,
            &[
                "schema",
                "target_ref",
                "family_slot",
                "revision",
                "state",
                "closure_evidence_refs",
                "operation_key",
                "accepted_at",
            ],
            &[
                "window",
                "anchor_work_ref",
                "claim_ref",
                "decision_ref",
                "prior_ref",
                "causal_closure",
            ],
        )?;
        equal(m, &prior["target_ref"], &json!(target_ref))?;
        equal(m, &prior["family_slot"], &json!(slot))?;
        equal(m, &prior["revision"], &json!(head.revision))?;
        let now =
            BigInt::from(crate::domain::Timestamp::parse_with_work(&c.accepted_at, m)?.micros());
        let prior_raw = text(m, &prior, "accepted_at")?;
        let previous =
            BigInt::from(crate::domain::Timestamp::parse_with_work(prior_raw, m)?.micros());
        if meter::cmp(m, &now, &previous)?.is_lt() {
            return Err(Error::new("CLOCK_REGRESSION", "causal family history"));
        }
        let family_revision = counter(m, &prior, "revision")?;
        let expected_id = body::semantic_id(
            m,
            "wfamily_",
            "workflow-family-state",
            &json!([target_ref.id, slot, family_revision.to_string()]),
        )?;
        if !shape::same_text(m, &head.reference.id, &expected_id)? {
            return Err(shape::fail("causal current family identity"));
        }
        references(m, &prior["closure_evidence_refs"], 0, 16)?;
        let work_time = text(m, &target, "original_work_accepted_at")?;
        if let Some(window) =
            targets::frozen_window(m, &family["window"], &plan.work_ref, work_time)?
        {
            equal(m, &prior["window"], &window)?;
            equal(m, &prior["anchor_work_ref"], &json!(plan.work_ref))?;
        } else {
            return Err(unsupported(
                "historical predecessor anchor validation not yet connected",
            ));
        }
        if text_is(m, &prior, "state", "closed_by_root")? {
            equal(
                m,
                &prior["schema"],
                &json!("ledger-workflow-family-state/2"),
            )?;
            closed_lineage(m, r, c, plan, &prior, &head.reference)?;
            continue;
        }
        equal(
            m,
            &prior["schema"],
            &json!("ledger-workflow-family-state/1"),
        )?;
        let fi = matching_input(
            m,
            &closures,
            "family_state_ref",
            &head.reference,
            &mut used_closures,
        )?;
        equal(m, &closures[fi]["expected_revision"], &prior["revision"])?;
        let evidence = references(m, &closures[fi]["authority_evidence_refs"], 1, 16)?;
        request["family_closures"][fi]["authority_evidence_refs"] = json!(evidence);
        let source = text(m, family, "correction_source_slot")?;
        clause(m, &plan.operands.slot_terms, root_slot, slot, source)?;
        let dependent = match (prior.get("claim_ref"), prior.get("decision_ref")) {
            (Some(claim), Some(decision)) => {
                if !text_is(m, &prior, "state", "claimed")? {
                    return Err(shape::fail("claimed family lifecycle"));
                }
                let decision_ref = shape::reference(m, decision)?;
                let di = matching_input(
                    m,
                    &reversals,
                    "decision_ref",
                    &decision_ref,
                    &mut used_reversals,
                )?;
                let decision = r.get(
                    m,
                    &c.key,
                    &decision_ref,
                    "workflow-decision",
                    "ledger-workflow-decision-revision/1",
                )?;
                equal(m, &decision["subject_ref"], claim)?;
                equal(
                    m,
                    &decision["revision"],
                    &reversals[di]["expected_revision"],
                )?;
                equal(m, &decision["decision_id"], &json!(prospective))?;
                let current = original_head(m, state, &prospective)?;
                if !shape::same_ref(m, &current.reference, &decision_ref)? {
                    return Err(Error::new(
                        "REVISION_CONFLICT",
                        "current dependent decision",
                    ));
                }
                equal(m, &decision["revision"], &json!(current.revision))?;
                let dep_evidence = references(m, &reversals[di]["authority_evidence_refs"], 1, 16)?;
                request["dependent_reversals"][di]["authority_evidence_refs"] = json!(dep_evidence);
                // Each explicit nomination is independently checked later against
                // current source and exact original terms; equality of two user
                // lists is not a substitute for either authority resolution.
                Some((decision_ref, decision, json!(dep_evidence)))
            }
            (None, None) => {
                if !text_is(m, &prior, "state", "open")?
                    && !text_is(m, &prior, "state", "awaiting_anchor")?
                    && !text_is(m, &prior, "state", "closed_no_success")?
                    && !text_is(m, &prior, "state", "expired")?
                {
                    return Err(shape::fail("unclaimed family lifecycle"));
                }
                None
            }
            _ => return Err(shape::fail("family claim/decision footprint")),
        };
        let declaration = declaration.clone();
        out.push(FamilyPlan {
            target_ref,
            target,
            declaration,
            family: family.clone(),
            prior_ref: head.reference.clone(),
            prior,
            evidence,
            dependent,
        });
    }
    if !meter::cmp(m, &used_closures.len().into(), &closures.len().into())?.is_eq()
        || !meter::cmp(m, &used_reversals.len().into(), &reversals.len().into())?.is_eq()
    {
        return Err(shape::fail("extra or terminal causal closure nomination"));
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn causal_nomination_requires_exactly_one_current_identity_not_distinct_rows() {
        let mut m = CoreMeter::new(65_536).unwrap();
        let reference = ExactRef {
            kind: "workflow-family-state".into(),
            id: format!("wfamily_{}", "1".repeat(64)),
            digest: format!("sha256:{}", "2".repeat(64)),
        };
        let first = json!({"family_state_ref":reference,"expected_revision":"1","authority_evidence_refs":[]});
        let mut second = first.clone();
        second["expected_revision"] = json!("2");
        let mut used = vec![];
        let error = matching_input(
            &mut m,
            &[first.clone(), second],
            "family_state_ref",
            &reference,
            &mut used,
        )
        .unwrap_err();
        assert_eq!(error.detail, "duplicate semantic causal closure");
        assert!(used.is_empty());
        let index = matching_input(
            &mut m,
            &[first.clone()],
            "family_state_ref",
            &reference,
            &mut used,
        )
        .unwrap();
        assert_eq!(index, 0);
        let error = matching_input(&mut m, &[first], "family_state_ref", &reference, &mut used)
            .unwrap_err();
        assert_eq!(error.detail, "causal input reused");
        let error =
            matching_input(&mut m, &[], "family_state_ref", &reference, &mut vec![]).unwrap_err();
        assert_eq!(error.code, "REVERSAL_DEPENDENTS_REQUIRED");
    }
}
