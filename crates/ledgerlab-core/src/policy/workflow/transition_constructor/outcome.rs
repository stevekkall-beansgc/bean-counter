//! Shared original-family submit DATA construction. Host origin/current-index,
//! capacity, funding and atomicity remain separate gates; no output can commit.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, Phase, arithmetic,
    authority::{self, EconomicMode},
    body, codec,
    data::*,
    inventory::entry,
    meter, outcome_proofs, pricing,
    records::{self, Records},
    representation::{InvocationToken, ParsedDraftTimes},
    shape, targets,
};
use crate::{Error, Result, domain::Timestamp};
use num_bigint::BigInt;
use serde_json::{Value, json};
macro_rules! call {
    ($f:path,$m:expr,$a:expr $(,)?) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr $(,)?) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr $(,)?) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr,$d:expr $(,)?) => {{
        let (a, b, c, d) = ($a, $b, $c, $d);
        $f($m, a, b, c, d)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr,$d:expr,$e:expr $(,)?) => {{
        let (a, b, c, d, e) = ($a, $b, $c, $d, $e);
        $f($m, a, b, c, d, e)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr,$d:expr,$e:expr,$f0:expr $(,)?) => {{
        let (a, b, c, d, e, f0) = ($a, $b, $c, $d, $e, $f0);
        $f($m, a, b, c, d, e, f0)
    }};
}
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if shape::same(m, a, b)? {
        Ok(())
    } else {
        Err(shape::fail("original outcome relation"))
    }
}
fn unsupported(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, Value)> {
    let reference = shape::reference(m, v)?;
    let body = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, body))
}
fn timestamp(m: &mut CoreMeter, v: &Value, key: &str) -> Result<BigInt> {
    let s = shape::text(m, v, key)?;
    Ok(Timestamp::parse_with_work(s, m)?.micros().into())
}
fn positive(m: &mut CoreMeter, v: &Value, key: &str) -> Result<BigInt> {
    let s = shape::text(m, v, key)?;
    let value = arithmetic::integer(m, s, false)?;
    if !call!(meter::cmp, m, &value, &0.into())?.is_gt() {
        return Err(shape::fail("positive outcome revision"));
    }
    Ok(value)
}
/// Called only after the common entry charges/parses the original request.
/// It recomputes records from source/program/retained inputs, never saved money.
pub(super) fn execute<'inv>(
    program: &SelectedProgram,
    c: &DraftContextData,
    mut p: Value,
    inputs: &[RecordInput],
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction> {
    entry::context(m, c)?;
    shape::closed(
        m,
        &p,
        &[
            "schema",
            "target_ref",
            "family_slot",
            "business_outcome_token",
            "activity",
            "product_ref",
            "policy_ref",
            "fact_schema_ref",
            "binding_ref",
            "facts",
            "fact_proofs",
            "evidence_refs",
            "occurred_at",
        ],
        &[],
    )?;
    for key in ["family_slot", "business_outcome_token"] {
        let text = shape::text(m, &p, key)?;
        if !call!(meter::cmp, m, &text.len().into(), &0.into())?.is_gt()
            || call!(meter::cmp, m, &text.len().into(), &128.into())?.is_gt()
        {
            return Err(shape::fail("outcome identifier bound"));
        }
        for ch in text.chars() {
            m.scan_bytes(ch.len_utf8())?;
            if m.check("outcome.identifier", || ch.is_control())? {
                return Err(shape::fail("outcome identifier"));
            }
        }
    }
    if !call!(
        meter::cmp,
        m,
        &shape::array(m, &p, "evidence_refs")?.len().into(),
        &0.into(),
    )?
    .is_eq()
    {
        return Err(unsupported("supplemental outcome evidence"));
    }
    if !call!(meter::cmp, m, &state.family_heads.len().into(), &1.into())?.is_eq() {
        return Err(shape::fail("one selected family head"));
    }
    if !meter::cmp(m, &state.work_claims.len().into(), &0.into())?.is_eq()
        || !meter::cmp(m, &state.outcome_claims.len().into(), &0.into())?.is_eq()
        || !meter::cmp(m, &state.links.len().into(), &0.into())?.is_eq()
    {
        return Err(unsupported(
            "noninitial selected outcome claim/link composition",
        ));
    }
    let owner = InvocationToken::new();
    let times = ParsedDraftTimes::parse(m, &owner, &c.received_at, &c.accepted_at)?;
    let (received, accepted) = times.get(m, &owner)?;
    let received = BigInt::from(received.micros());
    let accepted = BigInt::from(accepted.micros());
    if call!(meter::cmp, m, &received, &accepted)?.is_gt() {
        return Err(shape::fail("outcome receipt after acceptance"));
    }
    let mut pool = inputs.to_vec();
    pool.extend_from_slice(&c.source_observation_inputs);
    let mut r = Records::new(m, &pool)?;
    if let Some((scope, records)) = intrinsic {
        r = r.with_intrinsic_evidence(m, &c.key, scope, records)?;
    }
    let target_input = shape::field(m, &p, "target_ref")?;
    let (target_ref, target) = call!(
        load,
        m,
        &mut r,
        c,
        target_input,
        "workflow-target",
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
    call!(
        equal,
        m,
        call!(shape::field, m, &target, "scope")?,
        &json!(c.key.scope)
    )?;
    for key in ["product_ref", "policy_ref", "binding_ref"] {
        call!(
            equal,
            m,
            call!(shape::field, m, &target, key)?,
            call!(shape::field, m, &p, key)?
        )?;
    }
    call!(
        equal,
        m,
        call!(shape::field, m, &target, "compiled_plan_ref")?,
        &json!(program.compiled_plan().reference())
    )?;
    let (work_ref, work) = call!(
        load,
        m,
        &mut r,
        c,
        shape::field(m, &target, "work_ref")?,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let work_payload = shape::field(m, &work, "payload")?;
    call!(
        equal,
        m,
        call!(shape::field, m, work_payload, "status")?,
        &json!("succeeded")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &work, "accepted_at")?,
        call!(shape::field, m, &target, "original_work_accepted_at")?
    )?;
    let (claim_ref, claim) = call!(
        load,
        m,
        &mut r,
        c,
        shape::field(m, &target, "work_claim_ref")?,
        "workflow-work-claim",
        "ledger-workflow-work-claim/1",
    )?;
    entry::work_claim(m, c, &claim_ref, &claim)?;
    call!(
        equal,
        m,
        call!(shape::field, m, &claim, "work_ref")?,
        &json!(work_ref)
    )?;
    let root_slot = shape::text(m, &target, "root_component_slot")?;
    let id = body::semantic_id(
        m,
        "wtarget_",
        "workflow-target",
        &json!([c.key.scope, target["agreement_id"], claim_ref.id, root_slot]),
    )?;
    if !shape::same_text(m, &id, &target_ref.id)? {
        return Err(shape::fail("original target semantic identity"));
    }
    let (root_decision_ref, root_decision) = call!(
        load,
        m,
        &mut r,
        c,
        shape::field(m, &target, "original_decision_ref")?,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    let root_id = shape::text(m, &root_decision, "decision_id")?;
    let mut root_head = None;
    for head in &state.decision_heads {
        m.visit("outcome.root_head")?;
        if shape::same_text(m, &head.decision_id, root_id)? {
            if root_head.is_some() {
                return Err(shape::fail("ambiguous root head"));
            }
            root_head = Some(head);
        }
    }
    let current_root =
        root_head.ok_or_else(|| shape::fail("current original root head missing"))?;
    if !shape::same_ref(m, &current_root.reference, &root_decision_ref)?
        || !shape::same_text(m, &current_root.revision, "1")?
    {
        return Err(Error::new("TARGET_CLOSED", "original root was corrected"));
    }
    if !call!(meter::cmp, m, &state.decision_heads.len().into(), &1.into())?.is_eq() {
        return Err(unsupported("extra outcome decision selection"));
    }
    let mut basis = vec![];
    let mut net = BigInt::from(0);
    for item in shape::array(m, &root_decision, "active_action_refs")? {
        let (reference, action) = call!(
            load,
            m,
            &mut r,
            c,
            item,
            "workflow-action",
            "ledger-workflow-action/1",
        )?;
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, &action, "root_component_slot")?,
            root_slot
        )? {
            continue;
        }
        let scalar = shape::text(m, &action["amount"], "atoms")?;
        let atoms = arithmetic::integer(m, scalar, true)?;
        net = meter::add(m, &net, &atoms)?;
        basis.push(reference);
    }
    let basis = body::ref_set(m, basis)?;
    call!(
        equal,
        m,
        &json!(basis),
        call!(shape::field, m, &target, "original_action_refs")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &target, "original_net")?,
        &json!({"currency":"USD","scale":18,"atoms":net.to_string()})
    )?;
    let raw_slot = shape::text(m, &p, "family_slot")?;
    m.write_bytes(raw_slot.len())?;
    let family_slot_owned = raw_slot.to_owned();
    let family_slot = family_slot_owned.as_str();
    let target_family = call!(
        shape::named,
        m,
        call!(shape::array, m, &target, "families")?,
        "family_slot",
        family_slot
    )?;
    shape::closed(
        m,
        target_family,
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
    // This constructor slice consumes exactly one original family. Additional
    // families require their own named-context validation, never ignored rows.
    if !call!(
        meter::cmp,
        m,
        &shape::array(m, &target, "families")?.len().into(),
        &1.into()
    )?
    .is_eq()
    {
        return Err(unsupported("multiple original target families"));
    }
    let family_key = shape::text(m, target_family, "family_key")?;
    let mut instruction = None;
    for item in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        if call!(
            shape::same_text,
            m,
            call!(shape::text, m, item, "opcode")?,
            "evaluate_family"
        )? && call!(
            shape::same_text,
            m,
            call!(shape::text, m, &item["family"], "family_key")?,
            family_key
        )? {
            if instruction.is_some() {
                return Err(shape::fail("ambiguous original family"));
            }
            instruction = Some(item);
        }
    }
    let instruction = instruction.ok_or_else(|| shape::fail("original family missing"))?;
    let family = shape::field(m, instruction, "family")?;
    call!(
        equal,
        m,
        call!(shape::field, m, family, "trigger_activity")?,
        call!(shape::field, m, &p, "activity")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &family["selector"], "target_activity")?,
        call!(shape::field, m, work_payload, "activity")?
    )?;
    if !call!(
        meter::cmp,
        m,
        &shape::array(m, &family["selector"], "path")?.len().into(),
        &1.into(),
    )?
    .is_eq()
    {
        return Err(unsupported("non-direct selected outcome path"));
    }
    call!(
        equal,
        m,
        &family["selector"]["path"][0],
        call!(shape::field, m, family, "target_link_slot")?
    )?;
    let expected_decision = targets::prospective_decision(m, &target_ref.id, family_slot)?;
    call!(
        equal,
        m,
        call!(shape::field, m, target_family, "prospective_decision_id")?,
        &json!(expected_decision)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, target_family, "window_terms")?,
        call!(shape::field, m, family, "window")?
    )?;
    let (premium, discount) = targets::maxima(m, family, &net)?;
    for (key, atoms) in [("maximum_premium", premium), ("maximum_discount", discount)] {
        call!(
            equal,
            m,
            call!(shape::field, m, target_family, key)?,
            &json!({"currency":"USD","scale":18,"atoms":atoms.to_string()})
        )?;
    }
    let head = &state.family_heads[0];
    call!(equal, m, &json!(head.target_ref), &json!(target_ref))?;
    if !shape::same_text(m, &head.family_slot, family_slot)? {
        return Err(shape::fail("selected family slot"));
    }
    let old_family = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-family-state",
        "ledger-workflow-family-state/1",
    )?;
    shape::closed(
        m,
        &old_family,
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
            "prior_ref",
            "window",
            "anchor_work_ref",
            "claim_ref",
            "decision_ref",
        ],
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &old_family, "target_ref")?,
        &json!(target_ref)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &old_family, "family_slot")?,
        &json!(family_slot)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &old_family, "revision")?,
        &json!(head.revision)
    )?;
    let old_revision = positive(m, &old_family, "revision")?;
    if !meter::cmp(m, &old_revision, &1.into())?.is_eq() {
        return Err(unsupported("noninitial open family history"));
    }
    for optional in ["prior_ref", "claim_ref", "decision_ref"] {
        m.visit("outcome.initial_family_optional")?;
        if old_family.get(optional).is_some() {
            return Err(shape::fail("initial open family carries later history"));
        }
    }
    if !call!(
        meter::cmp,
        m,
        &shape::array(m, &old_family, "closure_evidence_refs")?
            .len()
            .into(),
        &0.into()
    )?
    .is_eq()
    {
        return Err(unsupported("open family closure evidence"));
    }
    let family_id = body::semantic_id(
        m,
        "wfamily_",
        "workflow-family-state",
        &json!([target_ref.id, family_slot, old_revision.to_string()]),
    )?;
    if !shape::same_text(m, &head.reference.id, &family_id)? {
        return Err(shape::fail("family state semantic identity"));
    }
    if !call!(
        shape::same_text,
        m,
        call!(shape::text, m, &old_family, "state")?,
        "open"
    )? {
        return Err(Error::new("TARGET_CLOSED", "original family is not open"));
    }
    let original_window = call!(
        targets::frozen_window,
        m,
        &family["window"],
        &work_ref,
        shape::text(m, &target, "original_work_accepted_at")?,
    )?
    .ok_or_else(|| unsupported("predecessor-anchored outcome"))?;
    call!(
        equal,
        m,
        call!(shape::field, m, &old_family, "window")?,
        &original_window
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &old_family, "anchor_work_ref")?,
        &json!(work_ref)
    )?;
    let occurred = timestamp(m, &p, "occurred_at")?;
    let starts = timestamp(m, &original_window, "starts_at")?;
    let before = timestamp(m, &original_window, "occurs_before")?;
    let received_by = timestamp(m, &original_window, "received_by")?;
    let accepted_by = timestamp(m, &original_window, "accepted_by")?;
    let previous = timestamp(m, &old_family, "accepted_at")?;
    if call!(meter::cmp, m, &occurred, &starts)?.is_lt()
        || !call!(meter::cmp, m, &occurred, &before)?.is_lt()
        || call!(meter::cmp, m, &occurred, &received)?.is_gt()
        || call!(meter::cmp, m, &received, &received_by)?.is_gt()
        || call!(meter::cmp, m, &accepted, &accepted_by)?.is_gt()
    {
        return Err(Error::new(
            "CORRECTION_WINDOW_CLOSED",
            "original outcome window",
        ));
    }
    if call!(meter::cmp, m, &accepted, &previous)?.is_lt() {
        return Err(Error::new("CLOCK_REGRESSION", "original family time"));
    }
    let mut product = None;
    for d in program.source_documents() {
        if shape::same(m, &json!(d.reference()), &p["product_ref"])? {
            product = Some(d.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("original product"))?;
    let activity = call!(
        shape::named,
        m,
        call!(shape::array, m, product, "activities")?,
        "name",
        shape::text(m, &p, "activity")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, activity, "primitive")?,
        &json!("outcome")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, activity, "fact_schema")?,
        call!(shape::field, m, &p, "fact_schema_ref")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, family, "source_slot")?,
        &json!(c.authorities[0].source_slot)
    )?;
    let mut schema = None;
    for (reference, s) in program.fact_schemas() {
        if shape::same(m, &json!(reference), &p["fact_schema_ref"])? {
            schema = Some(s);
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("original outcome schema"))?;
    let normalized = schema.normalize_value_with_work(&p["facts"], m)?;
    p["facts"] = normalized.canonical_value().clone();
    let proofs = outcome_proofs::resolve(m, &mut r, c, &p, schema, &normalized, state)?;
    normalize_proofs(m, &mut p)?;
    let mut authority_payload = p.clone();
    authority_payload["customer"] = work_payload["customer"].clone();
    let operands = authority::economic(
        m,
        &mut r,
        c,
        &authority_payload,
        activity,
        product,
        &owner,
        &times,
        program,
        EconomicMode::OutcomeSubmit,
        &proofs.references,
    )?;
    original_target_association(m, &operands, &target, target_family, family)?;
    original_claim_association(m, &claim, &work)?;
    let nomination = call!(
        shape::named,
        m,
        call!(shape::array, m, &operands.binding, "source_slots")?,
        "name",
        &c.authorities[0].source_slot
    )?;
    for name in normalized.required_proof_fields() {
        let definition = call!(
            shape::named,
            m,
            call!(shape::array, m, schema.canonical_value(), "fields")?,
            "name",
            name
        )?;
        let kind = shape::field(m, &definition["provenance"], "evidence_kind")?;
        let nominated = shape::array(m, nomination, "evidence_kinds")?;
        if !shape::member(m, nominated, kind)? {
            return Err(shape::fail("original nominated evidence kind"));
        }
    }
    verify_original_actions(
        m,
        &mut r,
        c,
        program,
        &operands,
        &work_ref,
        &work,
        &claim_ref,
        &root_decision_ref,
        &root_decision,
    )?;
    let code_field = shape::text(m, family, "code_field")?;
    let code = shape::text(m, &p["facts"], code_field)?;
    let price = pricing::outcome(program, family_key, code, &net, m)?;
    let root_and_outcome = meter::add(m, &net, &price.atoms.into())?;
    super::inventory::entry::exposure(
        m,
        &operands.binding,
        &operands.slot_terms,
        &root_and_outcome,
    )?;
    let claim_id = body::semantic_id(
        m,
        "woclaim_",
        "workflow-outcome-claim",
        &json!([
            c.key.scope,
            target["agreement_id"],
            family_slot,
            target_ref.id
        ]),
    )?;
    let claim = body::record(
        m,
        &c.key,
        "workflow-outcome-claim",
        claim_id,
        json!({"schema":"ledger-workflow-outcome-claim/1","scope":c.key.scope,"agreement_id":target["agreement_id"],"family_slot":family_slot,"target_ref":target_ref,"source_namespace":c.canonical_economic_namespace,"business_outcome_token":p["business_outcome_token"],"original_payload":p,"received_at":c.received_at,"accepted_at":c.accepted_at}),
    )?;
    let link_id = body::semantic_id(
        m,
        "wlink_",
        "workflow-link",
        &json!([
            c.key.scope,
            claim.reference,
            p["product_ref"],
            family["target_link_slot"],
            work_ref
        ]),
    )?;
    let link = body::record(
        m,
        &c.key,
        "workflow-link",
        link_id,
        json!({"schema":"ledger-workflow-link/1","scope":c.key.scope,"child":claim.reference,"product_ref":p["product_ref"],"binding_ref":p["binding_ref"],"slot":family["target_link_slot"],"predecessor":work_ref,"classification":"monetary","authority_ref":operands.authority.reference,"operation_key":c.key}),
    )?;
    let effect_id = original_family_effect(
        m,
        &c.key.scope,
        &target["agreement_id"],
        root_slot,
        &claim_ref,
        &json!(family_slot),
        &link.reference,
    )?;
    let nonzero = !call!(meter::cmp, m, &price.atoms.into(), &0.into())?.is_eq();
    let amount = json!({"currency":"USD","scale":18,"atoms":price.atoms.to_string()});
    let mut step = json!({"instruction":price.ordinal,"component_slot":root_slot,"family_slot":family_slot,"predicate":"true","basis_refs":basis,"rational_numerator":price.rational.numerator.to_string(),"rational_denominator":price.rational.denominator.to_string(),"rounded":amount,"disposition":if price.none{"none"}else if nonzero{"posted"}else{"zero"}});
    let mut actions = vec![];
    if nonzero && !price.none {
        let id = body::semantic_id(
            m,
            "waction_",
            "workflow-action",
            &json!([effect_id, "1", "original"]),
        )?;
        let action = body::record(
            m,
            &c.key,
            "workflow-action",
            id,
            json!({"schema":"ledger-workflow-action/1","effect_id":effect_id,"decision_id":expected_decision,"revision":"1","posting_role":"original","book":"retail","component_slot":root_slot,"root_component_slot":root_slot,"role_ref":operands.role_ref,"slot_terms_ref":operands.slot_terms_ref,"funding_leg":operands.slot_terms["funding_leg"],"amount":amount,"basis_refs":basis,"operation_key":c.key}),
        )?;
        step["action_ref"] = json!(action.reference);
        actions.push(action);
    }
    let evaluation = body::document(
        m,
        "workflow-evaluation",
        json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":claim.reference,"policy_ref":p["policy_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"compiler_ref":program.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":p["binding_ref"],"fact_schema_ref":p["fact_schema_ref"],"facts":p["facts"],"context":operands.binding["accepted_context"],"steps":[step]}),
    )?;
    let action_refs = body::ref_set(m, actions.iter().map(|a| a.reference.clone()).collect())?;
    let decision_ref = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([expected_decision, "1"]),
    )?;
    let decision = body::record(
        m,
        &c.key,
        "workflow-decision",
        decision_ref,
        json!({"schema":"ledger-workflow-decision-revision/1","decision_id":expected_decision,"revision":"1","subject_ref":claim.reference,"kind":"outcome","transition":"initial","status":"active","original_terms_ref":p["binding_ref"],"authority_ref":operands.authority.reference,"evaluation_ref":evaluation.reference,"actions_appended":action_refs,"active_action_refs":action_refs,"dependency_refs":[],"operation_key":c.key,"accepted_at":c.accepted_at}),
    )?;
    let revision = meter::add(m, &old_revision, &1.into())?;
    let family_id = body::semantic_id(
        m,
        "wfamily_",
        "workflow-family-state",
        &json!([target_ref.id, family_slot, revision.to_string()]),
    )?;
    let family_state = body::record(
        m,
        &c.key,
        "workflow-family-state",
        family_id,
        json!({"schema":"ledger-workflow-family-state/1","target_ref":target_ref,"family_slot":family_slot,"revision":revision.to_string(),"state":"claimed","window":original_window,"anchor_work_ref":work_ref,"closure_evidence_refs":[],"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":head.reference,"claim_ref":claim.reference,"decision_ref":decision.reference}),
    )?;
    let old_chain_ref = state
        .chain_head
        .as_ref()
        .ok_or_else(|| shape::fail("selected outcome chain head"))?;
    let old_chain = r.get(
        m,
        &c.key,
        old_chain_ref,
        "workflow-chain-state",
        "ledger-workflow-chain-state/1",
    )?;
    let chain_payload = json!({"customer":work_payload["customer"],"chain":work_payload["chain"],"binding_ref":p["binding_ref"]});
    let (old_revision, admitted) = if state.links.is_empty() {
        entry::chain_head(m, c, &chain_payload, old_chain_ref, &old_chain)?
    } else {
        entry::chain_head_with_links(
            m,
            &mut r,
            c,
            &chain_payload,
            old_chain_ref,
            &old_chain,
            &state.links,
        )?
    };
    let chain_revision = meter::add(m, &old_revision, &1.into())?;
    let admitted = meter::add(m, &admitted, &1.into())?;
    if call!(meter::cmp, m, &admitted, &1000.into())?.is_gt() {
        return Err(Error::new("CAPACITY_EXCEEDED", "chain event ceiling"));
    }
    let links = body::ref_set(m, vec![link.reference.clone()])?;
    let links_root = records::hash(m, "workflow-link-set", &json!(links))?;
    let chain_id = body::semantic_id(
        m,
        "wchain_",
        "workflow-chain-state",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            work_payload["customer"],
            work_payload["chain"],
            chain_revision.to_string()
        ]),
    )?;
    let chain = body::record(
        m,
        &c.key,
        "workflow-chain-state",
        chain_id,
        json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"customer":work_payload["customer"],"chain":work_payload["chain"],"binding_ref":p["binding_ref"],"accepted_agreement_ref":c.authorities[0].accepted_agreement_ref,"activation_ref":c.authorities[0].activation_ref,"revision":chain_revision.to_string(),"admitted_events":admitted.to_string(),"links_root":links_root,"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":old_chain_ref}),
    )?;
    let heads = ProposedHeads {
        decision: vec![(expected_decision, "1".into(), decision.reference.clone())],
        family: vec![(
            target_ref,
            family_slot.to_owned(),
            revision.to_string(),
            family_state.reference.clone(),
        )],
        chain: Some(chain.reference.clone()),
    };
    let normalized_payload = codec::encode(m, &p, 262144)?;
    let mut envelopes = vec![claim.clone()];
    envelopes.extend(operands.introduced);
    envelopes.extend(proofs.introduced);
    envelopes.push(operands.authority);
    envelopes.push(link.clone());
    envelopes.extend(actions);
    envelopes.push(evaluation);
    envelopes.push(decision);
    envelopes.push(family_state);
    envelopes.push(chain);
    Ok(Construction {
        normalized_payload,
        new_envelopes: envelopes,
        evaluation_steps: vec![step],
        proposed_heads: heads,
        claim_introductions: vec![claim.reference],
        link_introductions: vec![link.reference],
        prospective_obligations: vec![],
        consumed_refs: body::ref_set(m, r.consumed())?,
        history_edges: vec![],
    })
}

/// Raw DATA cannot make saved money authoritative merely by rehashing it. Replay
/// the original work's actual pricing and compare every nonzero effect. A future
/// checked original may reuse this completed semantic result only via its exact
/// private original-origin DATA owner; current contextual checks still apply.
pub(super) fn verify_original_actions(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    operands: &authority::Terms,
    work_ref: &ExactRef,
    work: &Value,
    claim_ref: &ExactRef,
    decision_ref: &ExactRef,
    decision: &Value,
) -> Result<()> {
    shape::closed(
        m,
        work,
        &[
            "schema",
            "key",
            "payload",
            "claim_id",
            "received_at",
            "accepted_at",
            "source_namespace",
        ],
        &[],
    )?;
    let p = shape::field(m, work, "payload")?;
    call!(
        equal,
        m,
        call!(shape::field, m, work, "source_namespace")?,
        &json!(c.canonical_economic_namespace)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, p, "binding_ref")?,
        &json!(c.authorities[0].binding_ref)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, p, "product_ref")?,
        call!(shape::field, m, &operands.binding, "product_ref")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, p, "policy_ref")?,
        call!(shape::field, m, &operands.binding, "policy_ref")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, work, "claim_id")?,
        &json!(claim_ref.id)
    )?;
    let old_key: OperationKey = serde_json::from_value(shape::field(m, work, "key")?.clone())
        .map_err(|_| shape::fail("original work key"))?;
    call!(
        equal,
        m,
        &json!(old_key.logical_store_id),
        &json!(c.key.logical_store_id)
    )?;
    call!(equal, m, &json!(old_key.scope), &json!(c.key.scope))?;
    let stable = body::semantic_id(
        m,
        "wdecision_",
        "workflow-decision",
        &json!([claim_ref.id, "work"]),
    )?;
    let revision = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([stable, "1"]),
    )?;
    if !shape::same_text(m, &revision, &decision_ref.id)? {
        return Err(shape::fail("original work decision identity"));
    }
    shape::closed(
        m,
        decision,
        &[
            "schema",
            "decision_id",
            "revision",
            "subject_ref",
            "kind",
            "transition",
            "status",
            "original_terms_ref",
            "authority_ref",
            "evaluation_ref",
            "actions_appended",
            "active_action_refs",
            "dependency_refs",
            "operation_key",
            "accepted_at",
        ],
        &[],
    )?;
    for (key, want) in [
        ("decision_id", json!(stable)),
        ("revision", json!("1")),
        ("kind", json!("work")),
        ("transition", json!("initial")),
        ("status", json!("active")),
        ("operation_key", json!(old_key)),
        ("original_terms_ref", p["binding_ref"].clone()),
        ("accepted_at", work["accepted_at"].clone()),
    ] {
        call!(equal, m, call!(shape::field, m, decision, key)?, &want)?;
    }
    call!(
        equal,
        m,
        call!(shape::field, m, decision, "actions_appended")?,
        call!(shape::field, m, decision, "active_action_refs")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, decision, "subject_ref")?,
        &json!(work_ref)
    )?;
    let event_id = body::semantic_id(
        m,
        "wevent_",
        "workflow-event",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            old_key.source,
            p["external_event_id"]
        ]),
    )?;
    if !shape::same_text(m, &event_id, &work_ref.id)? {
        return Err(shape::fail("original work event identity"));
    }
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
            "status",
            "facts",
            "fact_proofs",
            "links",
            "evidence_refs",
            "invocation_refs",
        ],
        &["occurred_at", "stage_instance_ref"],
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, p, "schema")?,
        &json!("ledger-workflow-work/1")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, p, "customer")?,
        call!(shape::field, m, &operands.binding, "customer")?
    )?;
    let authority_ref = shape::field(m, decision, "authority_ref")?;
    shape::reference(m, authority_ref)?;
    let evaluation_ref = shape::field(m, decision, "evaluation_ref")?;
    shape::reference(m, evaluation_ref)?;
    for key in ["fact_proofs", "links", "invocation_refs"] {
        if !call!(
            meter::cmp,
            m,
            &shape::array(m, p, key)?.len().into(),
            &0.into()
        )?
        .is_eq()
        {
            return Err(unsupported("original work proof/link/invocation recipe"));
        }
    }
    m.visit("outcome.original_stage")?;
    if p.get("stage_instance_ref").is_some() {
        return Err(unsupported("original staged work"));
    }
    let activity = shape::text(m, p, "activity")?;
    let mut original_product = None;
    for document in program.source_documents() {
        m.visit("outcome.original_product")?;
        if shape::same(m, &json!(document.reference()), &p["product_ref"])? {
            original_product = Some(document.canonical_value());
        }
    }
    let original_product = original_product.ok_or_else(|| shape::fail("original work product"))?;
    let original_activity = call!(
        shape::named,
        m,
        call!(shape::array, m, original_product, "activities")?,
        "name",
        activity
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, original_activity, "primitive")?,
        &json!("work")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, original_activity, "fact_schema")?,
        call!(shape::field, m, p, "fact_schema_ref")?
    )?;
    let mut schema = None;
    for (reference, candidate) in program.fact_schemas() {
        if shape::same(m, &json!(reference), &p["fact_schema_ref"])? {
            schema = Some(candidate);
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("original work fact schema"))?;
    let facts = schema.normalize_value_with_work(&p["facts"], m)?;
    equal(m, facts.canonical_value(), &p["facts"])?;
    if !call!(
        meter::cmp,
        m,
        &facts.required_proof_fields().len().into(),
        &0.into()
    )?
    .is_eq()
    {
        return Err(unsupported("original attested work facts"));
    }
    let prices = pricing::work(program, activity, facts.canonical_value(), m)?;
    let mut expected_actions = vec![];
    let original_actions = shape::array(m, decision, "active_action_refs")?;
    for price in prices {
        let mapping = call!(
            shape::named,
            m,
            call!(shape::array, m, &operands.instantiation, "component_map")?,
            "template_key",
            &price.component_key
        )?;
        let slot = shape::text(m, mapping, "slot_id")?;
        let root_mapping = call!(
            shape::named,
            m,
            call!(shape::array, m, &operands.instantiation, "component_map")?,
            "template_key",
            &price.root_key
        )?;
        let root = shape::text(m, root_mapping, "slot_id")?;
        if meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq() {
            continue;
        }
        let effect = body::semantic_id(
            m,
            "weffect_",
            "workflow-effect",
            &json!([
                c.key.scope,
                operands.binding["agreement_id"],
                slot,
                claim_ref.id,
                "self",
                "original"
            ]),
        )?;
        let id = body::semantic_id(
            m,
            "waction_",
            "workflow-action",
            &json!([effect, "1", "original"]),
        )?;
        let mut found = None;
        for item in original_actions {
            let reference = shape::reference(m, item)?;
            if shape::same_text(m, &reference.id, &id)? {
                if found.is_some() {
                    return Err(shape::fail("duplicate original effect"));
                }
                found = Some(reference);
            }
        }
        let reference = found.ok_or_else(|| shape::fail("original priced effect missing"))?;
        let action = r.get(
            m,
            &c.key,
            &reference,
            "workflow-action",
            "ledger-workflow-action/1",
        )?;
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
        for (key, want) in [
            ("effect_id", json!(effect)),
            ("decision_id", json!(stable)),
            ("revision", json!("1")),
            ("posting_role", json!("original")),
            ("book", json!("retail")),
            ("component_slot", json!(slot)),
            ("root_component_slot", json!(root)),
            ("role_ref", json!(operands.role_ref)),
            ("slot_terms_ref", json!(operands.slot_terms_ref)),
            ("funding_leg", operands.slot_terms["funding_leg"].clone()),
            (
                "amount",
                json!({"currency":"USD","scale":18,"atoms":price.atoms.to_string()}),
            ),
            ("operation_key", json!(old_key)),
        ] {
            call!(equal, m, call!(shape::field, m, &action, key)?, &want)?;
        }
        // Every original basis must resolve to a previously matched named effect.
        let mut basis = vec![];
        for component in &price.basis_components {
            let map = call!(
                shape::named,
                m,
                call!(shape::array, m, &operands.instantiation, "component_map")?,
                "template_key",
                component
            )?;
            let basis_slot = shape::text(m, map, "slot_id")?;
            let effect = body::semantic_id(
                m,
                "weffect_",
                "workflow-effect",
                &json!([
                    c.key.scope,
                    operands.binding["agreement_id"],
                    basis_slot,
                    claim_ref.id,
                    "self",
                    "original"
                ]),
            )?;
            let id = body::semantic_id(
                m,
                "waction_",
                "workflow-action",
                &json!([effect, "1", "original"]),
            )?;
            let mut selected = None;
            for prior in &expected_actions {
                let prior: &ExactRef = prior;
                if shape::same_text(m, &prior.id, &id)? {
                    selected = Some(prior.clone());
                    break;
                }
            }
            basis.push(selected.ok_or_else(|| shape::fail("original named pricing basis"))?);
        }
        let basis = body::ref_set(m, basis)?;
        call!(
            equal,
            m,
            call!(shape::field, m, &action, "basis_refs")?,
            &json!(basis)
        )?;
        expected_actions.push(reference);
    }
    let expected_actions = body::ref_set(m, expected_actions)?;
    call!(
        equal,
        m,
        &json!(expected_actions),
        call!(shape::field, m, decision, "active_action_refs")?
    )?;
    Ok(())
}

/// Named proof transport is normalized only after every field is consumed and
/// duplicates rejected. Order is the canonical field-name order, not Ref order.
pub(super) fn normalize_proofs(m: &mut CoreMeter, p: &mut Value) -> Result<()> {
    let mut ordered: Vec<(Vec<u8>, Value)> = vec![];
    for item in shape::array(m, p, "fact_proofs")? {
        let field = shape::field(m, item, "field")?;
        let key = codec::encode(m, field, 262144)?;
        let mut position = ordered.len();
        for (i, (prior, _)) in ordered.iter().enumerate() {
            m.visit("outcome.proof_sort")?;
            match shape::byte_order(m, &key, prior)? {
                std::cmp::Ordering::Equal => return Err(shape::fail("duplicate proof field")),
                std::cmp::Ordering::Less => {
                    position = i;
                    break;
                }
                _ => {}
            }
        }
        m.visit("outcome.proof_insert")?;
        for _ in position..ordered.len() {
            m.visit("validator.sort_move")?;
        }
        ordered.insert(position, (key, item.clone()));
    }
    p["fact_proofs"] = Value::Array(ordered.into_iter().map(|(_, item)| item).collect());
    Ok(())
}
pub(super) fn original_target_association(
    m: &mut CoreMeter,
    o: &authority::Terms,
    target: &Value,
    target_family: &Value,
    family: &Value,
) -> Result<()> {
    call!(
        equal,
        m,
        call!(shape::field, m, target, "agreement_id")?,
        call!(shape::field, m, &o.binding, "agreement_id")?
    )?;
    let key = shape::text(m, &family["selector"], "component_key")?;
    let mapped = call!(
        shape::named,
        m,
        call!(shape::array, m, &o.instantiation, "component_map")?,
        "template_key",
        key
    )?;
    let root = shape::text(m, mapped, "slot_id")?;
    call!(
        equal,
        m,
        call!(shape::field, m, target, "root_component_slot")?,
        &json!(root)
    )?;
    let component = call!(
        shape::named,
        m,
        call!(shape::array, m, &o.catalog, "components")?,
        "slot_id",
        root
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, component, "root_slot")?,
        &json!(root)
    )?;
    let family_key = shape::text(m, family, "family_key")?;
    let family_map = call!(
        shape::named,
        m,
        call!(shape::array, m, &o.instantiation, "family_map")?,
        "template_key",
        family_key
    )?;
    let slot = shape::text(m, family_map, "slot_id")?;
    call!(
        equal,
        m,
        call!(shape::field, m, target_family, "family_slot")?,
        &json!(slot)
    )?;
    let declaration = call!(
        shape::named,
        m,
        call!(shape::array, m, &o.catalog, "families")?,
        "slot_id",
        slot
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, declaration, "target_component_slot")?,
        &json!(root)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, declaration, "role_slot")?,
        call!(shape::field, m, component, "role_slot")?
    )?;
    Ok(())
}
pub(super) fn original_claim_association(
    m: &mut CoreMeter,
    claim: &Value,
    work: &Value,
) -> Result<()> {
    let p = shape::field(m, work, "payload")?;
    call!(
        equal,
        m,
        call!(shape::field, m, claim, "business_operation_id")?,
        call!(shape::field, m, p, "business_operation_id")?
    )?;
    let mut tuple = vec![];
    for key in [
        "customer",
        "chain",
        "status",
        "activity",
        "product_ref",
        "policy_ref",
        "fact_schema_ref",
        "facts",
        "links",
        "binding_ref",
        "invocation_refs",
    ] {
        tuple.push(shape::field(m, p, key)?.clone());
    }
    m.visit("outcome.original_occurrence_presence")?;
    if let Some(v) = p.get("occurred_at") {
        tuple.push(v.clone())
    }
    tuple.push(shape::field(m, p, "evidence_refs")?.clone());
    let digest = records::hash(m, "workflow-claim-facts", &json!(tuple))?;
    call!(
        equal,
        m,
        call!(shape::field, m, claim, "facts_digest")?,
        &json!(digest)
    )?;
    Ok(())
}

#[cfg(test)]
mod relation_tests {
    use super::*;
    fn reference(kind: &str, prefix: &str) -> ExactRef {
        ExactRef {
            kind: kind.into(),
            id: format!("{prefix}{}", "1".repeat(64)),
            digest: format!("sha256:{}", "2".repeat(64)),
        }
    }
    fn key() -> OperationKey {
        OperationKey {
            logical_store_id: "test".into(),
            scope: ["test".into(), "sandbox".into()],
            source: "test-source".into(),
            operation_id: "outcome".into(),
        }
    }
    fn operands() -> authority::Operands {
        let r = reference("workflow-authority", "wdoc_");
        authority::Operands {
            terms: authority::Terms {
                binding: json!({"agreement_id":"agreement"}),
                accepted: json!({}),
                activation: json!({}),
                catalog: json!({"components":[{"slot_id":"root","root_slot":"root","role_slot":"payer"}],"families":[{"slot_id":"assessment","target_component_slot":"root","role_slot":"payer"}]}),
                instantiation: json!({"component_map":[{"template_key":"units","slot_id":"root"}],"family_map":[{"template_key":"assess","slot_id":"assessment"}]}),
                slot_terms: json!({}),
                slot_terms_ref: r.clone(),
                role_ref: r.clone(),
            },
            authority: DraftEnvelope {
                reference: r,
                canonical_body: vec![],
            },
            introduced: vec![],
        }
    }
    #[test]
    fn rehashed_target_still_requires_original_agreement_and_catalog_joins() {
        for variant in 0..5 {
            let mut m = CoreMeter::new(65536).unwrap();
            let mut o = operands();
            let mut target = json!({"agreement_id":"agreement","root_component_slot":"root"});
            let mut family = json!({"family_slot":"assessment"});
            if variant == 1 {
                target["agreement_id"] = json!("other")
            }
            if variant == 2 {
                target["root_component_slot"] = json!("unrelated")
            }
            if variant == 3 {
                family["family_slot"] = json!("another-family")
            }
            if variant == 4 {
                o.terms.catalog["families"][0]["role_slot"] = json!("other-role")
            }
            // The synthetic body hash is rebound after each relation mutation;
            // this checks the join itself, not stale digest rejection.
            let envelope = body::record(
                &mut m,
                &key(),
                "workflow-target",
                format!("wtarget_{}", "3".repeat(64)),
                target,
            )
            .unwrap();
            let target = codec::parse(&mut m, &envelope.canonical_body, 4194304).unwrap();
            let rule = json!({"family_key":"assess","selector":{"component_key":"units"}});
            assert_eq!(
                original_target_association(&mut m, &o, &target, &family, &rule).is_ok(),
                variant == 0,
                "variant {variant}"
            );
        }
    }
    #[test]
    fn named_proofs_normalize_by_field_and_never_coalesce_duplicate_fields() {
        let mut m = CoreMeter::new(65536).unwrap();
        let r = reference("workflow-fact-attestation", "wdoc_");
        let mut payload = json!({"fact_proofs":[{"field":"z","attestation_ref":r},{"field":"a","attestation_ref":r}]});
        normalize_proofs(&mut m, &mut payload).unwrap();
        assert_eq!(payload["fact_proofs"][0]["field"], "a");
        let first = payload.clone();
        normalize_proofs(&mut m, &mut payload).unwrap();
        assert_eq!(payload, first);
        payload["fact_proofs"][1]["field"] = json!("a");
        assert!(normalize_proofs(&mut m, &mut payload).is_err());
    }
    #[test]
    fn rehashed_claim_must_bind_original_business_id_and_normalized_facts_tuple() {
        let p = json!({"customer":"customer","chain":"chain","status":"succeeded","activity":"work","product_ref":"product","policy_ref":"policy","fact_schema_ref":"schema","facts":{"units":"3"},"links":[],"binding_ref":"binding","invocation_refs":[],"evidence_refs":[],"business_operation_id":"business"});
        for variant in 0..4 {
            let mut m = CoreMeter::new(65536).unwrap();
            let mut tuple = vec![];
            for key in [
                "customer",
                "chain",
                "status",
                "activity",
                "product_ref",
                "policy_ref",
                "fact_schema_ref",
                "facts",
                "links",
                "binding_ref",
                "invocation_refs",
                "evidence_refs",
            ] {
                tuple.push(p[key].clone());
            }
            let digest = records::hash(&mut m, "workflow-claim-facts", &json!(tuple)).unwrap();
            let mut claim = json!({"business_operation_id":"business","facts_digest":digest});
            let mut original = p.clone();
            if variant == 1 {
                claim["business_operation_id"] = json!("different")
            }
            if variant == 2 {
                original["facts"]["units"] = json!("2")
            }
            if variant == 3 {
                original["occurred_at"] = json!("2026-10-13T01:00:00.000000Z")
            }
            let envelope = body::record(
                &mut m,
                &key(),
                "workflow-work-claim",
                format!("wclaim_{}", "3".repeat(64)),
                claim,
            )
            .unwrap();
            let claim = codec::parse(&mut m, &envelope.canonical_body, 4194304).unwrap();
            assert_eq!(
                original_claim_association(&mut m, &claim, &json!({"payload":original})).is_ok(),
                variant == 0,
                "variant {variant}"
            );
        }
    }
}

/// One immutable original-effect tuple for submit and every later correction.
/// Its work-claim identity never changes into the outcome-claim identity.
pub(super) fn original_family_effect(
    m: &mut CoreMeter,
    scope: &[String; 2],
    agreement: &Value,
    root_slot: &str,
    work_claim: &ExactRef,
    family_slot: &Value,
    link: &ExactRef,
) -> Result<String> {
    body::semantic_id(
        m,
        "weffect_",
        "workflow-effect",
        &json!([
            scope,
            agreement,
            root_slot,
            work_claim.id,
            [family_slot, [link.id], root_slot],
            "original"
        ]),
    )
}
