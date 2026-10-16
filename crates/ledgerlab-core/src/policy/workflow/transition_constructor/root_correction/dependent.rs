//! A root correction consumes the original family's current decision and uses
//! that family's nominated correction authority. No replacement outcome is
//! asserted: the only possible money here is exact inverses of active actions.
use super::super::correction;
use super::causal::FamilyPlan;
use super::*;
pub(super) struct ClosedFamily {
    pub family: FamilyPlan,
    pub introduced: Vec<DraftEnvelope>,
    pub authority: DraftEnvelope,
    pub decision: Option<DraftEnvelope>,
    pub records: Vec<DraftEnvelope>,
    pub steps: Vec<Value>,
    pub source_slot: String,
}
fn at(m: &mut CoreMeter, v: &Value, key: &str) -> Result<BigInt> {
    Ok(
        crate::domain::Timestamp::parse_with_work(text(m, v, key)?, m)?
            .micros()
            .into(),
    )
}
pub(super) fn build(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    root: &RootPlan,
    f: FamilyPlan,
    already: &[DraftEnvelope],
) -> Result<ClosedFamily> {
    let source_slot = text(m, &f.family, "correction_source_slot")?.to_owned();
    let local = slot_context(m, r, c, &source_slot, already)?;
    entry::context(m, &local)?;
    let product = original_product(m, program, &root.payload["product_ref"])?;
    let activities = shape::array(m, product, "activities")?;
    let trigger = text(m, &f.family, "trigger_activity")?;
    let activity = shape::named(m, activities, "name", trigger)?;
    equal(m, &activity["primitive"], &json!("outcome"))?;
    let mut current_activity = activity.clone();
    current_activity["source_slot"] = json!(source_slot);
    let evidence = match &f.dependent {
        Some((_, _, e)) => e.clone(),
        None => json!(f.evidence),
    };
    let mut p = json!({"customer":root.payload["customer"],"product_ref":root.payload["product_ref"],"policy_ref":root.payload["policy_ref"],"binding_ref":root.payload["binding_ref"],"fact_schema_ref":activity["fact_schema"],"activity":f.family["trigger_activity"],"evidence_refs":evidence});
    let claim = if let Some((_, old, _)) = &f.dependent {
        correction::dictionary_decision(m, old)?;
        equal(
            m,
            &old["schema"],
            &json!("ledger-workflow-decision-revision/1"),
        )?;
        equal(m, &old["kind"], &json!("outcome"))?;
        equal(m, &old["status"], &json!("active"))?;
        equal(m, &old["original_terms_ref"], &root.payload["binding_ref"])?;
        let (reference, claim) = load(
            m,
            r,
            c,
            &old["subject_ref"],
            "workflow-outcome-claim",
            "ledger-workflow-outcome-claim/1",
        )?;
        shape::closed(
            m,
            &claim,
            &[
                "schema",
                "scope",
                "agreement_id",
                "family_slot",
                "target_ref",
                "source_namespace",
                "business_outcome_token",
                "original_payload",
                "received_at",
                "accepted_at",
            ],
            &[],
        )?;
        for (k, v) in [
            ("scope", json!(c.key.scope)),
            ("agreement_id", f.target["agreement_id"].clone()),
            ("family_slot", f.prior["family_slot"].clone()),
            ("target_ref", json!(f.target_ref)),
        ] {
            equal(m, &claim[k], &v)?;
        }
        let id = body::semantic_id(
            m,
            "woclaim_",
            "workflow-outcome-claim",
            &json!([
                c.key.scope,
                claim["agreement_id"],
                claim["family_slot"],
                f.target_ref.id
            ]),
        )?;
        if !shape::same_text(m, &reference.id, &id)? {
            return Err(shape::fail("causal permanent outcome claim identity"));
        }
        let original = field(m, &claim, "original_payload")?;
        correction::dictionary_outcome(m, original)?;
        for k in [
            "activity",
            "product_ref",
            "policy_ref",
            "binding_ref",
            "fact_schema_ref",
        ] {
            equal(m, &original[k], &p[k])?;
        }
        for k in ["target_ref", "family_slot", "business_outcome_token"] {
            equal(m, &original[k], &claim[k])?;
        }
        let original_evidence = p["evidence_refs"].clone();
        p = original.clone();
        p["customer"] = root.payload["customer"].clone();
        p["evidence_refs"] = original_evidence;
        Some((reference, claim))
    } else {
        None
    };
    let token = InvocationToken::new();
    let times = ParsedDraftTimes::parse(m, &token, &c.received_at, &c.accepted_at)?;
    let operands = authority::economic(
        m,
        r,
        &local,
        &p,
        &current_activity,
        product,
        &token,
        &times,
        program,
        if root.reverse {
            EconomicMode::WorkReverse
        } else {
            EconomicMode::QuantityCorrect
        },
        &[],
    )?;
    equal(
        m,
        &json!(operands.slot_terms_ref),
        &json!(root.operands.slot_terms_ref),
    )?;
    outcome::original_target_association(m, &operands, &f.target, &f.declaration, &f.family)?;
    // Root permission does not replace dependent-slot permission. Both sets
    // must be applicable retained evidence under these exact original terms.
    evidence_membership(m, r, &local, &operands, &f.evidence)?;
    let dependent_refs = causal::references(m, &p["evidence_refs"], 1, 16)?;
    evidence_membership(m, r, &local, &operands, &dependent_refs)?;
    let mut records = vec![];
    let mut steps = vec![];
    let decision = if let (Some((old_ref, old, _)), Some((claim_ref, _claim))) =
        (&f.dependent, &claim)
    {
        let revision = counter(m, old, "revision")?;
        let did = body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!([old["decision_id"], revision.to_string()]),
        )?;
        if !shape::same_text(m, &did, &old_ref.id)? {
            return Err(shape::fail("causal dependent current revision identity"));
        }
        let accepted = at(m, &json!({"accepted_at":c.accepted_at}), "accepted_at")?;
        for historical in [old, &f.prior] {
            let prior_time = at(m, historical, "accepted_at")?;
            if meter::cmp(m, &accepted, &prior_time)?.is_lt() {
                return Err(Error::new("CLOCK_REGRESSION", "causal dependent history"));
            }
        }
        // The immutable link identity is derived, then its body is resolved;
        // no caller's current link index can choose a different effect basis.
        let lid = body::semantic_id(
            m,
            "wlink_",
            "workflow-link",
            &json!([
                c.key.scope,
                claim_ref,
                p["product_ref"],
                f.family["target_link_slot"],
                root.work_ref
            ]),
        )?;
        let (lr, link) =
            r.by_identity(m, &c.key, "workflow-link", &lid, "ledger-workflow-link/1")?;
        shape::closed(
            m,
            &link,
            &[
                "schema",
                "scope",
                "child",
                "product_ref",
                "binding_ref",
                "slot",
                "predecessor",
                "classification",
                "authority_ref",
                "operation_key",
            ],
            &[],
        )?;
        for (k, v) in [
            ("scope", json!(c.key.scope)),
            ("child", json!(claim_ref)),
            ("product_ref", p["product_ref"].clone()),
            ("binding_ref", p["binding_ref"].clone()),
            ("slot", f.family["target_link_slot"].clone()),
            ("predecessor", json!(root.work_ref)),
            ("classification", json!("monetary")),
        ] {
            equal(m, &link[k], &v)?;
        }
        let link_ref = lr;
        let root_slot = text(m, &f.target, "root_component_slot")?;
        let effect = outcome::original_family_effect(
            m,
            &c.key.scope,
            &f.target["agreement_id"],
            root_slot,
            &root.claim_ref,
            &f.prior["family_slot"],
            &link_ref,
        )?;
        let net_raw = text(m, &f.target["original_net"], "atoms")?;
        let net = arithmetic::integer(m, net_raw, true)?;
        let mut basis = vec![];
        for reference in shape::array(m, &f.target, "original_action_refs")? {
            basis.push(shape::reference(m, reference)?);
        }
        let basis = body::ref_set(m, basis)?;
        let family_key = text(m, &f.declaration, "family_key")?;
        let active = correction::verify_current(
            m, r, c, program, old, claim_ref, &p, family_key, &net, &basis, &effect, root_slot,
            &operands,
        )?;
        let next = meter::add(m, &revision, &1.into())?.to_string();
        let none = pricing::explicit_none(program, family_key, m)?;
        for (reference, action) in active.iter().rev() {
            m.visit("causal.dependent_inverse")?;
            let inverse = correction::inverse(m, c, reference, action, &next)?;
            let old_raw = text(m, &action["amount"], "atoms")?;
            let old_atoms = arithmetic::integer(m, old_raw, true)?;
            let atoms = meter::neg(m, &old_atoms)?;
            steps.push(json!({"instruction":none.ordinal,"component_slot":root_slot,"family_slot":f.prior["family_slot"],"predicate":"true","basis_refs":action["basis_refs"],"rational_numerator":atoms.to_string(),"rational_denominator":"1","rounded":{"currency":"USD","scale":18,"atoms":atoms.to_string()},"disposition":"inverse","action_ref":inverse.reference}));
            records.push(inverse);
        }
        let (_, evaluation) = load(
            m,
            r,
            c,
            &old["evaluation_ref"],
            "workflow-evaluation",
            "ledger-workflow-evaluation-proof/1",
        )?;
        let evaluation = body::document(
            m,
            "workflow-evaluation",
            json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":claim_ref,"policy_ref":p["policy_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"compiler_ref":program.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":p["binding_ref"],"fact_schema_ref":p["fact_schema_ref"],"facts":evaluation["facts"],"context":operands.binding["accepted_context"],"steps":steps}),
        )?;
        let appended = body::ref_set(m, records.iter().map(|x| x.reference.clone()).collect())?;
        let id = body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!([old["decision_id"], next]),
        )?;
        let decision = body::record(
            m,
            &c.key,
            "workflow-decision",
            id,
            json!({"schema":"ledger-workflow-decision-revision/1","decision_id":old["decision_id"],"revision":next,"prior_ref":old_ref,"subject_ref":claim_ref,"kind":"outcome","transition":"dependent_reverse","status":"reversed","original_terms_ref":p["binding_ref"],"authority_ref":operands.authority.reference,"evaluation_ref":evaluation.reference,"actions_appended":appended,"active_action_refs":[],"dependency_refs":old["dependency_refs"],"operation_key":c.key,"accepted_at":c.accepted_at}),
        )?;
        records.push(evaluation);
        Some(decision)
    } else {
        None
    };
    Ok(ClosedFamily {
        family: f,
        introduced: operands.introduced,
        authority: operands.authority,
        decision,
        records,
        steps,
        source_slot,
    })
}
pub(super) fn evidence_membership(
    m: &mut CoreMeter,
    _r: &mut Records,
    c: &DraftContextData,
    o: &authority::Terms,
    refs: &[ExactRef],
) -> Result<()> {
    for reference in refs {
        m.visit("causal.applicable_retained_evidence")?;
        let mut found = false;
        for candidate in [
            &o.binding["assent_refs"],
            &json!(c.authorities[0].original_grant_refs),
            &json!([c.authorities[0].current_grant_ref]),
        ] {
            for value in candidate
                .as_array()
                .ok_or_else(|| shape::fail("authority evidence membership array"))?
            {
                if shape::same(m, value, &json!(reference))? {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err(Error::new(
                "AUTHORITY_REQUIRED",
                "causal evidence not an applicable original assent/grant or current grant",
            ));
        }
    }
    Ok(())
}
impl ClosedFamily {
    pub(super) fn check(&self, m: &mut CoreMeter, root: &RootPlan) -> Result<Value> {
        m.visit("causal.closure_check")?;
        Ok(
            json!({"family_state_ref":self.family.prior_ref,"expected_revision":self.family.prior["revision"],"original_slot_terms_ref":root.operands.slot_terms_ref,"root_component_slot":self.family.target["root_component_slot"],"family_slot":self.family.prior["family_slot"],"dependent_authority_ref":self.authority.reference,"authority_evidence_refs":self.family.evidence}),
        )
    }
    pub(super) fn finalize(
        &self,
        m: &mut CoreMeter,
        c: &DraftContextData,
        root: &RootPlan,
        authority: &ExactRef,
    ) -> Result<(DraftEnvelope, String)> {
        let revision = counter(m, &self.family.prior, "revision")?;
        let revision = meter::add(m, &revision, &1.into())?.to_string();
        let slot = text(m, &self.family.prior, "family_slot")?;
        let id = body::semantic_id(
            m,
            "wfamily_",
            "workflow-family-state",
            &json!([self.family.target_ref.id, slot, revision]),
        )?;
        let mut value = json!({"schema":"ledger-workflow-family-state/2","target_ref":self.family.target_ref,"family_slot":slot,"revision":revision,"state":"closed_by_root","prior_ref":self.family.prior_ref,"closure_evidence_refs":self.family.evidence,"operation_key":c.key,"accepted_at":c.accepted_at,"causal_closure":{"root_decision_ref":root.current_ref,"root_expected_revision":root.current["revision"],"authority_ref":authority,"original_slot_terms_ref":root.operands.slot_terms_ref,"root_component_slot":self.family.target["root_component_slot"]}});
        for key in ["window", "anchor_work_ref", "claim_ref"] {
            m.visit("causal.preserve_original_footprint")?;
            if let Some(prior) = self.family.prior.get(key) {
                value[key] = prior.clone();
            }
        }
        match (&self.decision, self.family.prior.get("decision_ref")) {
            (Some(decision), Some(_)) => value["decision_ref"] = json!(decision.reference),
            (None, None) => {}
            _ => return Err(shape::fail("causal closed decision footprint")),
        }
        Ok((
            body::record(m, &c.key, "workflow-family-state", id, value)?,
            revision,
        ))
    }
}
