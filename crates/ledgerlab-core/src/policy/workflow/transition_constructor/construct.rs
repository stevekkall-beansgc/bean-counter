//! Shared fresh/replay pure draft. Unsupported selected branches refuse; no
//! financial append capability is created by this module.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, Phase, arithmetic, authority, body, codec,
    data::*,
    meter, pricing,
    records::{self, Records},
    representation::{InvocationToken, ParsedDraftTimes},
    shape,
};
use crate::{Error, Result, domain::Timestamp};
use num_bigint::BigInt;
use serde_json::{Value, json};
fn unsupported(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn exact(v: &Value) -> Result<ExactRef> {
    serde_json::from_value(v.clone()).map_err(|_| shape::fail("reference"))
}
pub fn draft_transition(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &[u8],
    inputs: &[RecordInput],
    state: &DraftStateData,
    m: &mut CoreMeter,
) -> Result<DraftTransition> {
    m.enter_draft()?;
    execute(program, c, payload, inputs, state, m, None)?.draft_projection(m)
}
/// A separate pure DATA entry. Intrinsic evidence is not a host origin proof;
/// outputs retain all unresolved admission gates. New/unmatched evidence still
/// executes the raw validator. The ordinary untrusted API is unchanged.
pub(in crate::policy::workflow) fn draft_with_intrinsic_evidence<'inv>(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &[u8],
    inputs: &[RecordInput],
    state: &DraftStateData,
    owner: &'inv super::intrinsic_record::IntrinsicDataScope,
    evidence: &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    m: &mut CoreMeter,
) -> Result<DraftTransition> {
    m.enter_draft()?;
    execute(
        program,
        c,
        payload,
        inputs,
        state,
        m,
        Some((owner, evidence)),
    )?
    .draft_projection(m)
}
pub(in crate::policy::workflow) fn replay_with_intrinsic_evidence<'inv>(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &[u8],
    inputs: &[RecordInput],
    state: &DraftStateData,
    owner: &'inv super::intrinsic_record::IntrinsicDataScope,
    evidence: &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    expected: &DraftTransition,
    m: &mut CoreMeter,
) -> Result<DraftTransition> {
    let draft =
        draft_with_intrinsic_evidence(program, c, payload, inputs, state, owner, evidence, m)?;
    compare_drafts(&draft, expected, m)?;
    Ok(draft)
}
pub(super) fn execute<'inv>(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &[u8],
    inputs: &[RecordInput],
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction> {
    m.begin_request(payload)?;
    m.phase(Phase::ParseNormalize);
    let mut p = codec::parse(m, payload, 262144)?;
    if shape::same(
        m,
        &p["schema"],
        &json!("ledger-workflow-outcome-correction/1"),
    )? {
        return super::correction::execute(program, c, p, inputs, state, m, intrinsic);
    }
    if shape::same(m, &p["schema"], &json!("ledger-workflow-outcome/1"))? {
        return super::outcome::execute(program, c, p, inputs, state, m, intrinsic);
    }
    if !shape::same(m, &p["schema"], &json!("ledger-workflow-work/1"))? {
        if shape::same(
            m,
            &p["schema"],
            &json!("ledger-workflow-quantity-correction/2"),
        )? || shape::same(m, &p["schema"], &json!("ledger-workflow-work-reverse/2"))?
        {
            return super::root_correction::execute(program, c, p, inputs, state, m, intrinsic);
        }
        return Err(unsupported(
            "selected transition branch not implemented yet",
        ));
    }
    shape::closed(
        m,
        &p,
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
            "status",
            "facts",
            "fact_proofs",
            "links",
            "evidence_refs",
            "invocation_refs",
        ],
        &["occurred_at", "stage_instance_ref"],
    )?;
    if !m.check("draft.no_stage", || p.get("stage_instance_ref").is_none())? {
        return Err(unsupported("staged work"));
    }
    for key in [
        "external_event_id",
        "business_operation_id",
        "customer",
        "chain",
        "activity",
    ] {
        let value = shape::text(m, &p, key)?;
        if !meter::cmp(m, &value.len().into(), &0usize.into())?.is_gt()
            || meter::cmp(m, &value.len().into(), &128usize.into())?.is_gt()
        {
            return Err(shape::fail("text bound"));
        }
        for ch in value.chars() {
            if !m.check("draft.text_character", || !ch.is_control())? {
                return Err(shape::fail("control character"));
            }
        }
    }
    for key in ["fact_proofs", "links", "invocation_refs"] {
        let items = shape::array(m, &p, key)?;
        if !meter::cmp(m, &items.len().into(), &0usize.into())?.is_eq() {
            return Err(unsupported(
                "field-proof/link/invocation construction pending",
            ));
        }
    }
    if !shape::same(m, &p["status"], &json!("succeeded"))? {
        return Err(unsupported("failed work construction pending"));
    }
    super::inventory::entry::validate(m, c, &p, state)?;
    let owner = InvocationToken::new();
    let times = ParsedDraftTimes::parse(m, &owner, &c.received_at, &c.accepted_at)?;
    let (received, at) = times.get(m, &owner)?;
    if meter::cmp(
        m,
        &BigInt::from(received.micros()),
        &BigInt::from(at.micros()),
    )?
    .is_gt()
    {
        return Err(shape::fail("received after accepted"));
    }
    let mut product = None;
    for d in program.source_documents() {
        m.visit("draft.product_ref")?;
        if shape::same(m, &json!(d.reference()), &p["product_ref"])? {
            product = Some(d.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("original product substitution"))?;
    let activity_name = shape::text(m, &p, "activity")?.to_owned();
    let activities = shape::array(m, product, "activities")?;
    let activity = shape::named(m, activities, "name", &activity_name)?;
    if !shape::same(m, &activity["primitive"], &json!("work"))? {
        return Err(shape::fail("work primitive"));
    }
    if !shape::same(m, &p["fact_schema_ref"], &activity["fact_schema"])? {
        return Err(shape::fail("fact schema relation"));
    }
    let mut schema = None;
    for (reference, s) in program.fact_schemas() {
        m.visit("draft.schema_lookup")?;
        if shape::same(m, &p["fact_schema_ref"], &json!(reference))? {
            schema = Some(s);
            break;
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("original fact schema"))?;
    let normalized = m.scoped("FactSchema::normalize_value_with_work.work_facts", |m| {
        schema.normalize_value_with_work(&p["facts"], m)
    })?;
    p["facts"] = normalized.canonical_value().clone();
    let normalized_payload = codec::encode(m, &p, 262144)?;
    let mut pool = inputs.to_vec();
    pool.extend_from_slice(&c.source_observation_inputs);
    let mut r = Records::new(m, &pool)?;
    if let Some((owner, evidence)) = intrinsic {
        r = r.with_intrinsic_evidence(m, &c.key, owner, evidence)?;
    }
    let a = authority::work(m, &mut r, c, &p, activity, product, &owner, &times, program)?;
    m.visit("draft.original_plan")?;
    if !shape::same(
        m,
        &a.binding["compiled_plan_ref"],
        &json!(program.compiled_plan().reference()),
    )? {
        return Err(shape::fail("original plan substitution"));
    }
    let agreement = &a.binding["agreement_id"];
    let claim_id = body::semantic_id(
        m,
        "wclaim_",
        "workflow-work-claim",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            c.canonical_economic_namespace,
            p["business_operation_id"],
            "completion"
        ]),
    )?;
    for claim in &state.work_claims {
        let prior = r.get(
            m,
            &c.key,
            claim,
            "workflow-work-claim",
            "ledger-workflow-work-claim/1",
        )?;
        super::inventory::entry::work_claim(m, c, claim, &prior)?;
        if shape::same_text(m, &claim.id, &claim_id)? {
            return Err(Error::new("IDENTITY_CONFLICT", "permanent work claim"));
        }
    }
    let work_id = body::semantic_id(
        m,
        "wevent_",
        "workflow-event",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            c.key.source,
            p["external_event_id"]
        ]),
    )?;
    let work = body::record(
        m,
        &c.key,
        "workflow-work",
        work_id,
        json!({"schema":"ledger-workflow-work-record/1","key":c.key,"payload":p,"claim_id":claim_id,"received_at":c.received_at,"accepted_at":c.accepted_at,"source_namespace":c.canonical_economic_namespace}),
    )?;
    let mut claim_tuple = vec![
        p["customer"].clone(),
        p["chain"].clone(),
        p["status"].clone(),
        p["activity"].clone(),
        p["product_ref"].clone(),
        p["policy_ref"].clone(),
        p["fact_schema_ref"].clone(),
        p["facts"].clone(),
        p["links"].clone(),
        p["binding_ref"].clone(),
        p["invocation_refs"].clone(),
    ];
    if let Some(occurrence) = p.get("occurred_at") {
        claim_tuple.push(occurrence.clone());
    }
    claim_tuple.push(p["evidence_refs"].clone());
    let facts_digest = records::hash(m, "workflow-claim-facts", &json!(claim_tuple))?;
    let claim = body::record(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id.clone(),
        json!({"schema":"ledger-workflow-work-claim/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"source_namespace":c.canonical_economic_namespace,"business_operation_id":p["business_operation_id"],"completion":"completion","work_ref":work.reference,"facts_digest":facts_digest}),
    )?;
    let decision_id = body::semantic_id(
        m,
        "wdecision_",
        "workflow-decision",
        &json!([claim_id, "work"]),
    )?;
    let prices = pricing::work(program, &activity_name, &p["facts"], m)?;
    super::inventory::entry::quantities(m, &a.binding, &a.slot_terms, &activity_name, &p["facts"])?;
    let mut total = num_bigint::BigInt::from(0);
    for price in &prices {
        m.visit("inventory.output_charge")?;
        total = meter::add(m, &total, &price.atoms.into())?;
    }
    super::inventory::entry::exposure(m, &a.binding, &a.slot_terms, &total)?;
    let mut actions = Vec::<DraftEnvelope>::new();
    let mut steps = vec![];
    let mut action_slots = Vec::<(String, ExactRef)>::new();
    for price in prices {
        let mapping = shape::named(
            m,
            a.instantiation["component_map"]
                .as_array()
                .ok_or_else(|| shape::fail("component map"))?,
            "template_key",
            &price.component_key,
        )?;
        let slot = shape::text(m, mapping, "slot_id")?.to_owned();
        let rootmap = shape::named(
            m,
            a.instantiation["component_map"].as_array().unwrap(),
            "template_key",
            &price.root_key,
        )?;
        let root_slot = shape::text(m, rootmap, "slot_id")?.to_owned();
        let catalog = shape::named(
            m,
            a.catalog["components"]
                .as_array()
                .ok_or_else(|| shape::fail("catalog components"))?,
            "slot_id",
            &slot,
        )?;
        if !shape::same(m, &catalog["root_slot"], &json!(root_slot))? {
            return Err(shape::fail("catalog root"));
        }
        if !shape::same(m, &catalog["role_slot"], &json!(price.role_slot))? {
            return Err(shape::fail("catalog role"));
        }
        if !shape::same(m, &catalog["book"], &json!("retail"))? {
            return Err(shape::fail("catalog book"));
        }
        let mut basis = vec![];
        for key in &price.basis_components {
            for (component, reference) in &action_slots {
                if shape::same_text(m, component, key)? {
                    basis.push(reference.clone());
                }
            }
        }
        let basis = body::ref_set(m, basis)?;
        let effect_id = body::semantic_id(
            m,
            "weffect_",
            "workflow-effect",
            &json!([c.key.scope, agreement, slot, claim_id, "self", "original"]),
        )?;
        let action_id = body::semantic_id(
            m,
            "waction_",
            "workflow-action",
            &json!([effect_id, "1", "original"]),
        )?;
        let amount = json!({"currency":"USD","scale":18,"atoms":price.atoms.to_string()});
        let nonzero = !meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq();
        let disposition = if nonzero { "posted" } else { "zero" };
        let mut step = json!({"instruction":price.ordinal,"component_slot":slot,"predicate":"true","basis_refs":basis,"rational_numerator":price.rational.numerator.to_string(),"rational_denominator":price.rational.denominator.to_string(),"rounded":amount,"disposition":disposition});
        if nonzero {
            let action = body::record(
                m,
                &c.key,
                "workflow-action",
                action_id,
                json!({"schema":"ledger-workflow-action/1","effect_id":effect_id,"decision_id":decision_id,"revision":"1","posting_role":"original","book":"retail","component_slot":slot,"root_component_slot":root_slot,"role_ref":a.role_ref,"slot_terms_ref":a.slot_terms_ref,"funding_leg":a.slot_terms["funding_leg"],"amount":amount,"basis_refs":basis,"operation_key":c.key}),
            )?;
            action_slots.push((price.component_key, action.reference.clone()));
            step["action_ref"] = json!(action.reference);
            actions.push(action);
        }
        steps.push(step);
    }
    let evaluation = body::document(
        m,
        "workflow-evaluation",
        json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":work.reference,"policy_ref":p["policy_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"compiler_ref":program.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":p["binding_ref"],"fact_schema_ref":p["fact_schema_ref"],"facts":p["facts"],"context":a.binding["accepted_context"],"steps":steps}),
    )?;
    let action_refs = body::ref_set(m, actions.iter().map(|x| x.reference.clone()).collect())?;
    let decision_revision = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision_id, "1"]),
    )?;
    let decision = body::record(
        m,
        &c.key,
        "workflow-decision",
        decision_revision,
        json!({"schema":"ledger-workflow-decision-revision/1","decision_id":decision_id,"revision":"1","subject_ref":work.reference,"kind":"work","transition":"initial","status":"active","original_terms_ref":p["binding_ref"],"authority_ref":a.authority.reference,"evaluation_ref":evaluation.reference,"actions_appended":action_refs,"active_action_refs":action_refs,"dependency_refs":[],"operation_key":c.key,"accepted_at":c.accepted_at}),
    )?;
    let targets = super::targets::build(program, c, &p, &a, &work, &claim, &decision, &actions, m)?;
    m.phase(Phase::Construct);
    let mut revision = BigInt::from(1);
    let mut admitted = BigInt::from(1);
    let mut prior = None;
    if let Some(head) = &state.chain_head {
        let old = r.get(
            m,
            &c.key,
            head,
            "workflow-chain-state",
            "ledger-workflow-chain-state/1",
        )?;
        let (old_revision, old_events) = if state.links.is_empty() {
            super::inventory::entry::chain_head(m, c, &p, head, &old)?
        } else {
            super::inventory::entry::chain_head_with_links(
                m,
                &mut r,
                c,
                &p,
                head,
                &old,
                &state.links,
            )?
        };
        revision = meter::add(m, &old_revision, &BigInt::from(1))?;
        admitted = meter::add(m, &old_events, &BigInt::from(1))?;
        if meter::cmp(m, &admitted, &1000.into())?.is_gt() {
            return Err(Error::new("CAPACITY_EXCEEDED", "chain event ceiling"));
        }
        prior = Some(head.clone());
    }
    let links_root = if meter::cmp(m, &state.links.len().into(), &0usize.into())?.is_eq() {
        records::hash(m, "workflow-link-set", &json!([]))?
    } else {
        let mut retained = vec![];
        for reference in &state.links {
            m.visit("work.existing_link")?;
            let link = r.get(
                m,
                &c.key,
                reference,
                "workflow-link",
                "ledger-workflow-link/1",
            )?;
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
            if !shape::same(m, &link["scope"], &json!(c.key.scope))? {
                return Err(shape::fail("work retained link scope"));
            }
            retained.push(reference.clone());
        }
        let retained = body::ref_set(m, retained)?;
        let root = records::hash(m, "workflow-link-set", &json!(retained))?;
        if let Some(prior) = &state.chain_head {
            let prior = r.get(
                m,
                &c.key,
                prior,
                "workflow-chain-state",
                "ledger-workflow-chain-state/1",
            )?;
            let prior_root = shape::text(m, &prior, "links_root")?;
            if !shape::same_text(m, &root, prior_root)? {
                return Err(shape::fail("work prior link root"));
            }
        } else {
            return Err(shape::fail("retained links without chain"));
        }
        root
    };
    let chain_id = body::semantic_id(
        m,
        "wchain_",
        "workflow-chain-state",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            p["customer"],
            p["chain"],
            revision.to_string()
        ]),
    )?;
    let mut chain_body = json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"customer":p["customer"],"chain":p["chain"],"binding_ref":p["binding_ref"],"accepted_agreement_ref":c.authorities[0].accepted_agreement_ref,"activation_ref":c.authorities[0].activation_ref,"revision":revision.to_string(),"admitted_events":admitted.to_string(),"links_root":links_root,"operation_key":c.key,"accepted_at":c.accepted_at});
    if let Some(pr) = prior {
        chain_body["prior_ref"] = json!(pr);
    }
    let chain = body::record(m, &c.key, "workflow-chain-state", chain_id, chain_body)?;
    let proposed_heads = ProposedHeads {
        decision: vec![(decision_id, "1".into(), decision.reference.clone())],
        family: targets.heads,
        chain: Some(chain.reference.clone()),
    };
    let mut envelopes = vec![work, claim.clone()];
    envelopes.extend(a.introduced);
    envelopes.push(a.authority);
    envelopes.extend(actions);
    envelopes.push(evaluation);
    envelopes.push(decision);
    envelopes.extend(targets.records);
    envelopes.push(chain);
    Ok(Construction {
        normalized_payload,
        new_envelopes: envelopes,
        evaluation_steps: steps,
        proposed_heads,
        claim_introductions: vec![claim.reference],
        link_introductions: vec![],
        prospective_obligations: targets.obligations,
        consumed_refs: body::ref_set(m, r.consumed())?,
        history_edges: vec![],
    })
}
#[cfg(test)]
pub(super) fn calibrate(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &[u8],
    inputs: &[RecordInput],
    state: &DraftStateData,
    m: &mut CoreMeter,
) -> Result<DraftTransition> {
    m.enter_calibration()?;
    m.diagnostic_scope("selected.A.setup", |m| {
        execute(program, c, payload, inputs, state, m, None)?.draft_projection(m)
    })
}
pub fn replay_draft(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &[u8],
    inputs: &[RecordInput],
    state: &DraftStateData,
    expected: &DraftTransition,
    m: &mut CoreMeter,
) -> Result<DraftTransition> {
    let draft = draft_transition(program, c, payload, inputs, state, m)?;
    compare_drafts(&draft, expected, m)?;
    Ok(draft)
}
pub(super) fn compare_drafts(
    draft: &DraftTransition,
    expected: &DraftTransition,
    m: &mut CoreMeter,
) -> Result<()> {
    m.phase(Phase::Compare);
    if !shape::same_bytes(m, &draft.normalized_payload, &expected.normalized_payload)? {
        return Err(shape::fail("replay payload"));
    }
    if !meter::cmp(
        m,
        &draft.new_envelopes.len().into(),
        &expected.new_envelopes.len().into(),
    )?
    .is_eq()
    {
        return Err(shape::fail("replay members"));
    }
    for (new, old) in draft.new_envelopes.iter().zip(&expected.new_envelopes) {
        m.visit("replay.reference")?;

        if !shape::same_ref(m, &new.reference, &old.reference)?
            || !shape::same_bytes(m, &new.canonical_body, &old.canonical_body)?
        {
            return Err(shape::fail("replay bytes"));
        }
    }
    let heads =
        |h: &ProposedHeads| json!({"decision":h.decision,"family":h.family,"chain":h.chain});
    if !shape::same(
        m,
        &heads(&draft.proposed_heads),
        &heads(&expected.proposed_heads),
    )? {
        return Err(shape::fail("replay heads"));
    }
    for (actual, old) in [
        (
            json!(draft.evaluation_steps),
            json!(expected.evaluation_steps),
        ),
        (
            json!(draft.claim_introductions),
            json!(expected.claim_introductions),
        ),
        (
            json!(draft.link_introductions),
            json!(expected.link_introductions),
        ),
        (
            json!(draft.prospective_obligations),
            json!(expected.prospective_obligations),
        ),
        (json!(draft.consumed_refs), json!(expected.consumed_refs)),
        (json!(draft.history_edges), json!(expected.history_edges)),
        (
            json!(draft.unresolved_admission_obligations),
            json!(expected.unresolved_admission_obligations),
        ),
    ] {
        if !shape::same(m, &actual, &old)? {
            return Err(shape::fail("replay DATA projection"));
        }
    }
    Ok(())
}
