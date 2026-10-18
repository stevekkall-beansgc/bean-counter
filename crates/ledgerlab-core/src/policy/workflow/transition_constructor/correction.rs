//! Original-term standalone outcome correction DATA. No retained money is a
//! pricing operand: original named work is repriced, and the current outcome
//! evaluation/actions are checked against that same frozen program. Host origin,
//! complete index selection, reservations and resource admission remain gates.
use super::super::selected_program::SelectedProgram;
use super::emitted_record::ConstructedEnvelope;
use super::record_body::RecordBody;
use super::records::SuppliedRecords;
use super::{
    CoreMeter, arithmetic,
    authority::{self, EconomicMode},
    body, codec,
    data::*,
    inventory::entry,
    meter, outcome, outcome_proofs, pricing,
    records::{self, Records},
    representation::{InvocationToken, ParsedDraftTimes},
    shape, targets,
};
use crate::{Error, Result, domain::Timestamp};
use num_bigint::BigInt;
use serde_json::{Value, json};
macro_rules! call {
    ($function:path,$work:expr,$a0:expr $(,)?) => {{
        let (a0,) = ($a0,);
        $function($work, a0)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr $(,)?) => {{
        let (a0, a1) = ($a0, $a1);
        $function($work, a0, a1)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr $(,)?) => {{
        let (a0, a1, a2) = ($a0, $a1, $a2);
        $function($work, a0, a1, a2)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr $(,)?) => {{
        let (a0, a1, a2, a3) = ($a0, $a1, $a2, $a3);
        $function($work, a0, a1, a2, a3)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4) = ($a0, $a1, $a2, $a3, $a4);
        $function($work, a0, a1, a2, a3, a4)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5) = ($a0, $a1, $a2, $a3, $a4, $a5);
        $function($work, a0, a1, a2, a3, a4, a5)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6) = ($a0, $a1, $a2, $a3, $a4, $a5, $a6);
        $function($work, a0, a1, a2, a3, a4, a5, a6)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7) = ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7);
        $function($work, a0, a1, a2, a3, a4, a5, a6, a7)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8) = ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8);
        $function($work, a0, a1, a2, a3, a4, a5, a6, a7, a8)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9) =
            ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9);
        $function($work, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr,$a10:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10) =
            ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9, $a10);
        $function($work, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr,$a10:expr,$a11:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11) =
            ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9, $a10, $a11);
        $function($work, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr,$a10:expr,$a11:expr,$a12:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12) = (
            $a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9, $a10, $a11, $a12,
        );
        $function($work, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12)
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr,$a10:expr,$a11:expr,$a12:expr,$a13:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13) = (
            $a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9, $a10, $a11, $a12, $a13,
        );
        $function(
            $work, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13,
        )
    }};
    ($function:path,$work:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr,$a10:expr,$a11:expr,$a12:expr,$a13:expr,$a14:expr $(,)?) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14) = (
            $a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9, $a10, $a11, $a12, $a13, $a14,
        );
        $function(
            $work, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14,
        )
    }};
}
fn unsupported(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if call!(shape::same, m, a, b)? {
        Ok(())
    } else {
        Err(shape::fail("original correction relation"))
    }
}
fn field_copy(m: &mut CoreMeter, v: &Value, k: &str) -> Result<Value> {
    let value = call!(shape::field, m, v, k)?;
    // The whole-core allocation ledger must pre-admit this owning Value copy;
    // this slice remains noncommittable until that composition is complete.
    Ok(value.clone())
}
pub(super) fn verify_original_claim_namespace(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,claim_ref:&ExactRef,claim:&Value,decision_id:&str,binding:&Value)->Result<()> {
    let first_id=body::semantic_id(m,"wdrev_","workflow-decision-revision",&json!([decision_id,"1"]))?;
    let (_,first)=r.by_identity(m,&c.key,"workflow-decision",&first_id,"ledger-workflow-decision-revision/1")?;
    dictionary_decision(m,&first)?;
    equal(m,&first["decision_id"],&json!(decision_id))?;
    equal(m,&first["revision"],&json!("1"))?;
    equal(m,&first["kind"],&json!("outcome"))?;
    equal(m,&first["transition"],&json!("initial"))?;
    equal(m,&first["subject_ref"],&json!(claim_ref))?;
    equal(m,&first["original_terms_ref"],binding)?;
    equal(m,&first["accepted_at"],&claim["accepted_at"])?;
    authority::historical_namespace(m,r,c,&first["authority_ref"],&claim["source_namespace"],None,"outcome.submit",binding,&claim["accepted_at"])
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, RecordBody)> {
    let reference = call!(shape::reference, m, v)?;
    let value = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, value))
}
fn counter(m: &mut CoreMeter, v: &Value, k: &str) -> Result<BigInt> {
    let raw = call!(shape::text, m, v, k)?;
    let n = call!(arithmetic::integer, m, raw, false)?;
    if !call!(meter::cmp, m, &n, &0.into())?.is_gt() {
        return Err(shape::fail("positive correction revision"));
    }
    Ok(n)
}
fn at(m: &mut CoreMeter, v: &Value, k: &str) -> Result<BigInt> {
    let raw = call!(shape::text, m, v, k)?;
    Ok(Timestamp::parse_with_work(raw, m)?.micros().into())
}
fn amount(n: &BigInt) -> Value {
    json!({"currency":"USD","scale":18,"atoms":n.to_string()})
}
pub(super) fn dictionary_outcome(m: &mut CoreMeter, p: &Value) -> Result<()> {
    call!(
        shape::closed,
        m,
        p,
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
            "occurred_at"
        ],
        &[]
    )?;
    call!(equal, m, &p["schema"], &json!("ledger-workflow-outcome/1"))
}
pub(super) fn dictionary_decision(m: &mut CoreMeter, p: &Value) -> Result<()> {
    call!(
        shape::closed,
        m,
        p,
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
            "accepted_at"
        ],
        &["prior_ref"]
    )
}
fn dictionary_action(m: &mut CoreMeter, a: &Value) -> Result<()> {
    call!(
        shape::closed,
        m,
        a,
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
            "operation_key"
        ],
        &["inverse_of"]
    )?;
    call!(equal, m, &a["schema"], &json!("ledger-workflow-action/1"))?;
    call!(
        shape::closed,
        m,
        &a["amount"],
        &["currency", "scale", "atoms"],
        &[]
    )?;
    call!(equal, m, &a["amount"]["currency"], &json!("USD"))?;
    call!(equal, m, &a["amount"]["scale"], &json!(18))
}
/// Every inverse is an exact negation of the consumed current immutable action.
/// The prior amount has already been repriced from the current evaluation facts
/// under the original program; neither caller amounts nor invoice totals enter.
pub(super) fn inverse(
    m: &mut CoreMeter,
    c: &DraftContextData,
    old_ref: &ExactRef,
    old: &Value,
    revision: &str,
) -> Result<ConstructedEnvelope> {
    call!(dictionary_action, m, old)?;
    let atoms = call!(
        arithmetic::integer,
        m,
        call!(shape::text, m, &old["amount"], "atoms")?,
        true
    )?;
    let negative = call!(meter::neg, m, &atoms)?;
    let id = call!(
        body::semantic_id,
        m,
        "waction_",
        "workflow-action",
        &json!([old["effect_id"], revision, "inverse"])
    )?;
    let mut result = old.clone();
    result["revision"] = json!(revision);
    result["posting_role"] = json!("inverse");
    result["amount"] = amount(&negative);
    result["operation_key"] = json!(c.key);
    result["inverse_of"] = json!(old_ref);
    call!(body::record, m, &c.key, "workflow-action", id, result)
}
/// Called by the same fresh/replay constructor after its ordinary entry check.
pub(super) fn execute<'inv, 'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    mut request: super::working_value::WorkingValue,
    inputs: SuppliedRecords<'raw>,
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction<'inv, 'raw>> {
    entry::context_selected(m,c,program)?;
    call!(
        shape::closed,
        m,
        &request,
        &[
            "schema",
            "original_claim_ref",
            "expected_revision",
            "replacement"
        ],
        &[]
    )?;
    call!(
        equal,
        m,
        &request["schema"],
        &json!("ledger-workflow-outcome-correction/1")
    )?;
    let expected = call!(counter, m, &request, "expected_revision")?;
    let mut r = inputs.resolve(m, &c.key, &c.source_observation_inputs)?;
    if let Some((owner, evidence)) = intrinsic {
        r = r.with_intrinsic_evidence(m, &c.key, owner, evidence)?;
    }
    let (claim_ref, claim) = call!(
        load,
        m,
        &mut r,
        c,
        &request["original_claim_ref"],
        "workflow-outcome-claim",
        "ledger-workflow-outcome-claim/1"
    )?;
    call!(
        shape::closed,
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
            "accepted_at"
        ],
        &[]
    )?;
    call!(equal, m, &claim["scope"], &json!(c.key.scope))?;
    let original_value=call!(shape::field,m,&claim,"original_payload")?;
    let original=super::working_value::WorkingValue::copy(m,original_value)?;
    call!(dictionary_outcome, m, &original)?;
    for k in ["family_slot", "target_ref", "business_outcome_token"] {
        call!(equal, m, &claim[k], &original[k])?;
    }
    let stable_claim = call!(
        body::semantic_id,
        m,
        "woclaim_",
        "workflow-outcome-claim",
        &json!([
            c.key.scope,
            claim["agreement_id"],
            claim["family_slot"],
            claim["target_ref"]["id"]
        ])
    )?;
    if !call!(shape::same_text, m, &stable_claim, &claim_ref.id)? {
        return Err(shape::fail("permanent outcome claim identity"));
    }
    // V1 preserves its old selected restriction. RP01 binds the permanent
    // claim's namespace to its actual original authority below, independently
    // of the current corrector's source/enrollment namespace.
    if m.representation_arena().is_none() && !call!(
        shape::same_text,
        m,
        call!(shape::text, m, &claim, "source_namespace")?,
        &c.canonical_economic_namespace
    )? {
        return Err(unsupported("distinct historical assessor namespace"));
    }
    let none = request["replacement"].get("kind").is_some();
    let mut p = if none {
        let replacement = &request["replacement"];
        call!(
            shape::closed,
            m,
            replacement,
            &["kind", "evidence_refs", "reason"],
            &[]
        )?;
        call!(equal, m, &replacement["kind"], &json!("none"))?;
        let reason = call!(shape::text, m, replacement, "reason")?;
        if !call!(meter::cmp, m, &reason.len().into(), &0.into())?.is_gt()
            || call!(meter::cmp, m, &reason.len().into(), &1024.into())?.is_gt()
        {
            return Err(shape::fail("correction none reason"));
        }
        original.with_fields(m,&[("facts",&json!({})),("fact_proofs",&json!([])),("evidence_refs",&replacement["evidence_refs"])])?
    } else {
        call!(super::working_value::WorkingValue::copy,m,call!(shape::field,m,&request,"replacement")?)?
    };
    call!(dictionary_outcome, m, &p)?;
    for k in [
        "target_ref",
        "family_slot",
        "business_outcome_token",
        "activity",
        "product_ref",
        "policy_ref",
        "fact_schema_ref",
        "binding_ref",
    ] {
        call!(equal, m, &p[k], &original[k])?;
    }
    let evidence = if none {
        &request["replacement"]["evidence_refs"]
    } else {
        &p["evidence_refs"]
    };
    let evidence = evidence
        .as_array()
        .ok_or_else(|| shape::fail("correction evidence set"))?;
    if !call!(meter::cmp, m, &evidence.len().into(), &0.into())?.is_eq() {
        return Err(unsupported("supplemental correction evidence validation"));
    }
    if !call!(meter::cmp, m, &state.family_heads.len().into(), &1.into())?.is_eq()
        || !call!(meter::cmp, m, &state.decision_heads.len().into(), &2.into())?.is_eq()
    {
        return Err(unsupported("selected root and outcome heads only"));
    }
    if !call!(meter::cmp, m, &state.work_claims.len().into(), &0.into())?.is_eq()
        || !call!(meter::cmp, m, &state.outcome_claims.len().into(), &1.into())?.is_eq()
        || !call!(shape::same_ref, m, &state.outcome_claims[0], &claim_ref)?
        || !call!(meter::cmp, m, &state.links.len().into(), &1.into())?.is_eq()
    {
        return Err(unsupported(
            "selected permanent claim/link index composition",
        ));
    }
    let (target_ref, target) = call!(
        load,
        m,
        &mut r,
        c,
        &p["target_ref"],
        "workflow-target",
        "ledger-workflow-component-target/1"
    )?;
    call!(
        shape::closed,
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
            "original_work_accepted_at"
        ],
        &[]
    )?;
    call!(equal, m, &target["scope"], &json!(c.key.scope))?;
    call!(equal, m, &target["agreement_id"], &claim["agreement_id"])?;
    for k in ["product_ref", "policy_ref", "binding_ref"] {
        call!(equal, m, &target[k], &p[k])?;
    }
    call!(
        equal,
        m,
        &target["compiled_plan_ref"],
        &json!(program.compiled_plan().reference())
    )?;
    let (work_ref, work) = call!(
        load,
        m,
        &mut r,
        c,
        &target["work_ref"],
        "workflow-work",
        "ledger-workflow-work-record/1"
    )?;
    let (work_claim_ref, work_claim) = call!(
        load,
        m,
        &mut r,
        c,
        &target["work_claim_ref"],
        "workflow-work-claim",
        "ledger-workflow-work-claim/1"
    )?;
    call!(entry::work_claim, m, c, &work_claim_ref, &work_claim)?;
    call!(outcome::original_claim_association, m, &work_claim, &work)?;
    call!(equal, m, &work_claim["work_ref"], &json!(work_ref))?;
    call!(
        equal,
        m,
        &work["accepted_at"],
        &target["original_work_accepted_at"]
    )?;
    let root_slot = call!(shape::text, m, &target, "root_component_slot")?.to_owned();
    let target_id = call!(
        body::semantic_id,
        m,
        "wtarget_",
        "workflow-target",
        &json!([
            c.key.scope,
            target["agreement_id"],
            work_claim_ref.id,
            root_slot
        ])
    )?;
    if !call!(shape::same_text, m, &target_ref.id, &target_id)? {
        return Err(shape::fail("correction original target identity"));
    }
    let (root_ref, root) = call!(
        load,
        m,
        &mut r,
        c,
        &target["original_decision_ref"],
        "workflow-decision",
        "ledger-workflow-decision-revision/1"
    )?;
    let root_head = call!(
        select_head,
        m,
        state,
        call!(shape::text, m, &root, "decision_id")?
    )?;
    if !call!(shape::same_ref, m, &root_head.reference, &root_ref)?
        || !call!(shape::same_text, m, &root_head.revision, "1")?
    {
        return Err(Error::new("TARGET_CLOSED", "original root changed"));
    }
    let mut net = BigInt::from(0);
    let mut basis = vec![];
    for a in call!(shape::array, m, &root, "active_action_refs")? {
        let (reference, action) = call!(
            load,
            m,
            &mut r,
            c,
            a,
            "workflow-action",
            "ledger-workflow-action/1"
        )?;
        if call!(
            shape::same_text,
            m,
            call!(shape::text, m, &action, "root_component_slot")?,
            &root_slot
        )? {
            let atoms = call!(
                arithmetic::integer,
                m,
                call!(shape::text, m, &action["amount"], "atoms")?,
                true
            )?;
            net = call!(meter::add, m, &net, &atoms)?;
            basis.push(reference);
        }
    }
    let basis = call!(body::ref_set, m, basis)?;
    call!(equal, m, &target["original_action_refs"], &json!(basis))?;
    call!(equal, m, &target["original_net"], &amount(&net))?;
    let family_slot = call!(shape::text, m, &p, "family_slot")?.to_owned();
    let families = call!(shape::array, m, &target, "families")?;
    if !call!(meter::cmp, m, &families.len().into(), &1.into())?.is_eq() {
        return Err(unsupported("multiple correction families"));
    }
    let target_family = call!(shape::named, m, families, "family_slot", &family_slot)?;
    call!(
        shape::closed,
        m,
        target_family,
        &[
            "family_key",
            "family_slot",
            "prospective_decision_id",
            "window_terms",
            "maximum_premium",
            "maximum_discount"
        ],
        &[]
    )?;
    let key = call!(shape::text, m, target_family, "family_key")?.to_owned();
    let family = call!(select_family, m, program, &key)?;
    call!(equal, m, &family["trigger_activity"], &p["activity"])?;
    call!(
        equal,
        m,
        &family["selector"]["target_activity"],
        &work["payload"]["activity"]
    )?;
    let decision_id = call!(
        targets::prospective_decision,
        m,
        &target_ref.id,
        &family_slot
    )?;
    call!(
        equal,
        m,
        &target_family["prospective_decision_id"],
        &json!(decision_id)
    )?;
    call!(equal, m, &target_family["window_terms"], &family["window"])?;
    let (premium, discount) = call!(targets::maxima, m, family, &net)?;
    call!(
        equal,
        m,
        &target_family["maximum_premium"],
        &amount(&premium)
    )?;
    call!(
        equal,
        m,
        &target_family["maximum_discount"],
        &amount(&discount)
    )?;
    let head = &state.family_heads[0];
    if !call!(shape::same_ref, m, &head.target_ref, &target_ref)?
        || !call!(shape::same_text, m, &head.family_slot, &family_slot)?
    {
        return Err(shape::fail("correction family selection"));
    }
    let old_family = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-family-state",
        "ledger-workflow-family-state/1",
    )?;
    call!(
        shape::closed,
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
            "prior_ref",
            "window",
            "anchor_work_ref",
            "claim_ref",
            "decision_ref"
        ],
        &[]
    )?;
    call!(equal, m, &old_family["target_ref"], &json!(target_ref))?;
    call!(equal, m, &old_family["family_slot"], &json!(family_slot))?;
    call!(equal, m, &old_family["revision"], &json!(head.revision))?;
    call!(equal, m, &old_family["claim_ref"], &json!(claim_ref))?;
    call!(equal, m, &old_family["state"], &json!("claimed"))?;
    let family_revision = call!(counter, m, &old_family, "revision")?;
    let fid = call!(
        body::semantic_id,
        m,
        "wfamily_",
        "workflow-family-state",
        &json!([target_ref.id, family_slot, family_revision.to_string()])
    )?;
    if !call!(shape::same_text, m, &fid, &head.reference.id)? {
        return Err(shape::fail("family head identity"));
    }
    let (old_ref, old) = call!(
        load,
        m,
        &mut r,
        c,
        &old_family["decision_ref"],
        "workflow-decision",
        "ledger-workflow-decision-revision/1"
    )?;
    call!(dictionary_decision, m, &old)?;
    call!(equal, m, &old["decision_id"], &json!(decision_id))?;
    call!(equal, m, &old["subject_ref"], &json!(claim_ref))?;
    call!(equal, m, &old["kind"], &json!("outcome"))?;
    call!(equal, m, &old["original_terms_ref"], &p["binding_ref"])?;
    let old_revision = call!(counter, m, &old, "revision")?;
    if m.representation_arena().is_some(){
        verify_original_claim_namespace(m,&mut r,c,&claim_ref,&claim,&decision_id,&p["binding_ref"])?;
    }
    if !call!(meter::cmp, m, &old_revision, &expected)?.is_eq() {
        return Err(Error::new("STALE_REVISION", "outcome decision revision"));
    }
    let outcome_head = call!(select_head, m, state, &decision_id)?;
    if !call!(shape::same_ref, m, &outcome_head.reference, &old_ref)?
        || !call!(
            shape::same_text,
            m,
            &outcome_head.revision,
            call!(shape::text, m, &old, "revision")?
        )?
    {
        return Err(shape::fail("current outcome head"));
    }
    let rid = call!(
        body::semantic_id,
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision_id, old_revision.to_string()])
    )?;
    if !call!(shape::same_text, m, &rid, &old_ref.id)? {
        return Err(shape::fail("current outcome identity"));
    }
    let window = call!(
        targets::frozen_window,
        m,
        &family["window"],
        &work_ref,
        call!(shape::text, m, &target, "original_work_accepted_at")?
    )?
    .ok_or_else(|| unsupported("predecessor anchored correction"))?;
    call!(equal, m, &old_family["window"], &window)?;
    call!(equal, m, &old_family["anchor_work_ref"], &json!(work_ref))?;
    let owner = InvocationToken::new();
    let times = call!(
        ParsedDraftTimes::parse,
        m,
        &owner,
        &c.received_at,
        &c.accepted_at
    )?;
    let (received, accepted) = times.get(m, &owner)?;
    let received = BigInt::from(received.micros());
    let accepted = BigInt::from(accepted.micros());
    let deadline = call!(at, m, &window, "correction_until")?;
    if call!(meter::cmp, m, &received, &accepted)?.is_gt()
        || call!(meter::cmp, m, &accepted, &deadline)?.is_gt()
    {
        return Err(Error::new(
            "CORRECTION_WINDOW_CLOSED",
            "original correction horizon",
        ));
    }
    for v in [&old, &old_family] {
        let previous = call!(at, m, v, "accepted_at")?;
        if call!(meter::cmp, m, &accepted, &previous)?.is_lt() {
            return Err(Error::new("CLOCK_REGRESSION", "correction history time"));
        }
    }
    if !none {
        let occurrence = call!(at, m, &p, "occurred_at")?;
        let start = call!(at, m, &window, "starts_at")?;
        let end = call!(at, m, &window, "occurs_before")?;
        if call!(meter::cmp, m, &occurrence, &start)?.is_lt()
            || !call!(meter::cmp, m, &occurrence, &end)?.is_lt()
            || call!(meter::cmp, m, &occurrence, &received)?.is_gt()
        {
            return Err(shape::fail(
                "replacement occurrence outside original window",
            ));
        }
    }
    let product = call!(select_product, m, program, &p["product_ref"])?;
    let activity = call!(
        shape::named,
        m,
        call!(shape::array, m, product, "activities")?,
        "name",
        call!(shape::text, m, &p, "activity")?
    )?;
    call!(equal, m, &activity["primitive"], &json!("outcome"))?;
    call!(equal, m, &activity["fact_schema"], &p["fact_schema_ref"])?;
    let schema = call!(select_schema, m, program, &p["fact_schema_ref"])?;
    // The explicit no-replacement form carries no newly asserted facts. Its
    // original schema/anchor remain in the evaluation, with facts exactly {}.
    // A full submit using a declared none code still normalizes its full facts.
    let proofs = if none {
        outcome_proofs::Proofs {
            references: vec![],
            introduced: vec![],
        }
    } else {
        let normalized = super::working_value::NormalizedInput::normalize(m,schema,&p["facts"])?;
        p.replace(m,"facts",normalized.canonical_value())?;
        call!(
            outcome_proofs::resolve,
            m,
            &mut r,
            c,
            &p,
            schema,
            &normalized,
            state
        )?
    };
    p.normalize_set(m,"fact_proofs",true)?;
    let mut admin_payload = p.with_fields(m,&[("customer",&work["payload"]["customer"])])?;
    if none {admin_payload.replace(m,"evidence_refs",&request["replacement"]["evidence_refs"])?;}
    let correction_activity = super::working_value::WorkingValue::copy(m,activity)?;
    let correction_activity=correction_activity.with_fields(m,&[("source_slot",&family["correction_source_slot"])])?;
    let namespace=shape::text(m,&work,"source_namespace")?;
    let operands = if m.representation_arena().is_some(){authority::economic_original(
        m,&mut r,c,&admin_payload,&correction_activity,product,&owner,&times,program,EconomicMode::OutcomeCorrect,&proofs.references,namespace,
    )?}else{call!(
        authority::economic,
        m,
        &mut r,
        c,
        &admin_payload,
        &correction_activity,
        product,
        &owner,
        &times,
        program,
        EconomicMode::OutcomeCorrect,
        &proofs.references
    )?};
    call!(
        outcome::original_target_association,
        m,
        &operands,
        &target,
        target_family,
        family
    )?;
    call!(
        outcome::verify_original_actions,
        m,
        &mut r,
        c,
        program,
        &operands,
        &work_ref,
        &work,
        &work_claim_ref,
        &root_ref,
        &root
    )?;
    let (link_ref, link) = call!(
        load,
        m,
        &mut r,
        c,
        &json!(state.links[0]),
        "workflow-link",
        "ledger-workflow-link/1"
    )?;
    call!(
        shape::closed,
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
            "operation_key"
        ],
        &[]
    )?;
    for (k, v) in [
        ("scope", json!(c.key.scope)),
        ("child", json!(claim_ref)),
        ("product_ref", p["product_ref"].clone()),
        ("binding_ref", p["binding_ref"].clone()),
        ("slot", family["target_link_slot"].clone()),
        ("predecessor", json!(work_ref)),
        ("classification", json!("monetary")),
    ] {
        call!(equal, m, &link[k], &v)?;
    }
    let link_id = call!(
        body::semantic_id,
        m,
        "wlink_",
        "workflow-link",
        &json!([
            c.key.scope,
            claim_ref,
            p["product_ref"],
            family["target_link_slot"],
            work_ref
        ])
    )?;
    if !call!(shape::same_text, m, &link_ref.id, &link_id)? {
        return Err(shape::fail("original selected link identity"));
    }
    let effect = outcome::original_family_effect(
        m,
        &c.key.scope,
        &target["agreement_id"],
        &root_slot,
        &work_claim_ref,
        &json!(family_slot),
        &link_ref,
    )?;
    let active = call!(
        verify_current,
        m,
        &mut r,
        c,
        program,
        &old,
        &claim_ref,
        &p,
        &key,
        &net,
        &basis,
        &effect,
        &root_slot,
        &operands
    )?;
    let price = if none {
        pricing::explicit_none(program, &key, m)?
    } else {
        let code = call!(
            shape::text,
            m,
            &p["facts"],
            call!(shape::text, m, family, "code_field")?
        )?;
        pricing::outcome(program, &key, code, &net, m)?
    };
    let revision = call!(meter::add, m, &old_revision, &1.into())?.to_string();
    let mut actions = vec![];
    let mut steps = vec![];
    for (reference, action) in active.iter().rev() {
        let inverse = call!(inverse, m, c, reference, action, &revision)?;
        let inverse_body = call!(inverse_value, m, &inverse)?;
        let old_atoms = call!(
            arithmetic::integer,
            m,
            call!(shape::text, m, &inverse_body["amount"], "atoms")?,
            true
        )?;
        steps.push(json!({"instruction":price.ordinal,"component_slot":root_slot,"family_slot":family_slot,"predicate":"true","basis_refs":action["basis_refs"],"rational_numerator":old_atoms.to_string(),"rational_denominator":"1","rounded":amount(&old_atoms),"disposition":"inverse","action_ref":inverse.reference}));
        actions.push(inverse);
    }
    let replacement_none = none || price.none;
    let replacement_atoms = if replacement_none {
        BigInt::from(0)
    } else {
        price.atoms.into()
    };
    let total = call!(meter::add, m, &net, &replacement_atoms)?;
    call!(
        entry::exposure,
        m,
        &operands.binding,
        &operands.slot_terms,
        &total
    )?;
    let mut step = json!({"instruction":price.ordinal,"component_slot":root_slot,"family_slot":family_slot,"predicate":"true","basis_refs":basis,"rational_numerator":if replacement_none{"0".to_owned()}else{price.rational.numerator.to_string()},"rational_denominator":if replacement_none{"1".to_owned()}else{price.rational.denominator.to_string()},"rounded":amount(&replacement_atoms),"disposition":if replacement_none{"none"}else if call!(meter::cmp,m,&replacement_atoms,&0.into())?.is_eq(){"zero"}else{"posted"}});
    let mut replacement_refs = vec![];
    if !replacement_none && !call!(meter::cmp, m, &replacement_atoms, &0.into())?.is_eq() {
        let id = call!(
            body::semantic_id,
            m,
            "waction_",
            "workflow-action",
            &json!([effect, revision, "replacement"])
        )?;
        let action = call!(
            body::record,
            m,
            &c.key,
            "workflow-action",
            id,
            json!({"schema":"ledger-workflow-action/1","effect_id":effect,"decision_id":decision_id,"revision":revision,"posting_role":"replacement","book":"retail","component_slot":root_slot,"root_component_slot":root_slot,"role_ref":operands.role_ref,"slot_terms_ref":operands.slot_terms_ref,"funding_leg":operands.slot_terms["funding_leg"],"amount":amount(&replacement_atoms),"basis_refs":basis,"operation_key":c.key})
        )?;
        step["action_ref"] = json!(action.reference);
        replacement_refs.push(action.reference.clone());
        actions.push(action);
    }
    steps.push(step);
    let evaluation = call!(
        body::document,
        m,
        "workflow-evaluation",
        json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":claim_ref,"policy_ref":p["policy_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"compiler_ref":program.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":p["binding_ref"],"fact_schema_ref":p["fact_schema_ref"],"facts":p["facts"],"context":operands.binding["accepted_context"],"steps":steps})
    )?;
    let appended = call!(
        body::ref_set,
        m,
        actions.iter().map(|a| a.reference.clone()).collect()
    )?;
    let replacement_refs = call!(body::ref_set, m, replacement_refs)?;
    let did = call!(
        body::semantic_id,
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision_id, revision])
    )?;
    let decision = call!(
        body::record,
        m,
        &c.key,
        "workflow-decision",
        did,
        json!({"schema":"ledger-workflow-decision-revision/1","decision_id":decision_id,"revision":revision,"subject_ref":claim_ref,"kind":"outcome","transition":"outcome_correct","status":"active","original_terms_ref":p["binding_ref"],"authority_ref":operands.authority.reference,"evaluation_ref":evaluation.reference,"actions_appended":appended,"active_action_refs":replacement_refs,"dependency_refs":old["dependency_refs"],"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":old_ref})
    )?;
    let frevision = call!(meter::add, m, &family_revision, &1.into())?.to_string();
    let fid = call!(
        body::semantic_id,
        m,
        "wfamily_",
        "workflow-family-state",
        &json!([target_ref.id, family_slot, frevision])
    )?;
    let next_family = call!(
        body::record,
        m,
        &c.key,
        "workflow-family-state",
        fid,
        json!({"schema":"ledger-workflow-family-state/1","target_ref":target_ref,"family_slot":family_slot,"revision":frevision,"state":"claimed","window":window,"anchor_work_ref":work_ref,"closure_evidence_refs":old_family["closure_evidence_refs"],"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":head.reference,"claim_ref":claim_ref,"decision_ref":decision.reference})
    )?;
    let old_chain_ref = state
        .chain_head
        .as_ref()
        .ok_or_else(|| shape::fail("correction chain head"))?;
    let old_chain = r.get(
        m,
        &c.key,
        old_chain_ref,
        "workflow-chain-state",
        "ledger-workflow-chain-state/1",
    )?;
    let chain_payload = json!({"customer":work["payload"]["customer"],"chain":work["payload"]["chain"],"binding_ref":p["binding_ref"]});
    let (chain_revision, admitted) = entry::chain_head_with_links(
        m,
        &mut r,
        c,
        &chain_payload,
        old_chain_ref,
        &old_chain,
        &state.links,
    )?;
    let crevision = call!(meter::add, m, &chain_revision, &1.into())?.to_string();
    let cid = call!(
        body::semantic_id,
        m,
        "wchain_",
        "workflow-chain-state",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            chain_payload["customer"],
            chain_payload["chain"],
            crevision
        ])
    )?;
    let chain = call!(
        body::record,
        m,
        &c.key,
        "workflow-chain-state",
        cid,
        json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"customer":chain_payload["customer"],"chain":chain_payload["chain"],"binding_ref":p["binding_ref"],"accepted_agreement_ref":c.authorities[0].accepted_agreement_ref,"activation_ref":c.authorities[0].activation_ref,"revision":crevision,"admitted_events":admitted.to_string(),"links_root":old_chain["links_root"],"operation_key":c.key,"accepted_at":c.accepted_at,"prior_ref":old_chain_ref})
    )?;
    if !none {
        request.replace(m,"replacement",&p)?;
    }
    let normalized_payload = request.bytes(m)?;
    let mut envelopes = operands.introduced;
    envelopes.extend(proofs.introduced);
    envelopes.push(operands.authority);
    envelopes.extend(actions);
    envelopes.push(evaluation);
    let heads = ProposedHeads {
        decision: vec![(decision_id, revision, decision.reference.clone())],
        family: vec![(
            target_ref,
            family_slot,
            frevision,
            next_family.reference.clone(),
        )],
        chain: Some(chain.reference.clone()),
    };
    envelopes.push(decision);
    envelopes.push(next_family);
    envelopes.push(chain);
    Ok(Construction {
        original_reservations: None,financial_reservations:None,
        economic: EconomicConstruction {
            terms: operands.terms,
            original_work: OriginalWorkPayload::Retained(work),
        },
        normalized_payload,
        new_envelopes: envelopes,reused_result:None,
        evaluation_steps: steps,
        proposed_heads: heads,
        claim_introductions: vec![],
        link_introductions: vec![],
        prospective_obligations: vec![],
        records: r,
        history_edges: vec![],
        app_effects:None,app_causal:None,current_capacity:None,
        candidate_heads:None,candidate_introductions:None,financial_observation:None,funding_effects:None,control_owners:[None,None,None,None,None],
    })
}
fn inverse_value(m: &mut CoreMeter, v: &ConstructedEnvelope) -> Result<Value> {
    call!(codec::parse, m, &v.canonical_body, 4194304)
}
fn select_head<'a>(
    m: &mut CoreMeter,
    state: &'a DraftStateData,
    id: &str,
) -> Result<&'a DecisionHead> {
    let mut out = None;
    for h in &state.decision_heads {
        m.visit("correction.select_head")?;
        if call!(shape::same_text, m, &h.decision_id, id)? {
            if out.is_some() {
                return Err(shape::fail("duplicate decision selection"));
            }
            out = Some(h);
        }
    }
    out.ok_or_else(|| shape::fail("required current decision missing"))
}
pub(super) fn select_family<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    key: &str,
) -> Result<&'a Value> {
    let mut out = None;
    for i in call!(
        shape::array,
        m,
        p.compiled_plan().canonical_value(),
        "typed_instructions"
    )? {
        if call!(
            shape::same_text,
            m,
            call!(shape::text, m, i, "opcode")?,
            "evaluate_family"
        )? && call!(
            shape::same_text,
            m,
            call!(shape::text, m, &i["family"], "family_key")?,
            key
        )? {
            if out.is_some() {
                return Err(shape::fail("ambiguous family"));
            }
            out = Some(&i["family"]);
        }
    }
    out.ok_or_else(|| shape::fail("original named family missing"))
}
fn select_product<'a>(m: &mut CoreMeter, p: &'a SelectedProgram, r: &Value) -> Result<&'a Value> {
    for d in p.source_documents() {
        m.visit("correction.product")?;
        if call!(shape::same, m, &json!(d.reference()), r)? {
            return Ok(d.canonical_value());
        }
    }
    Err(shape::fail("original product missing"))
}
fn select_schema<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    r: &Value,
) -> Result<&'a crate::policy::workflow::FactSchema> {
    for (reference, schema) in p.fact_schemas() {
        if call!(shape::same, m, &json!(reference), r)? {
            return Ok(schema);
        }
    }
    Err(shape::fail("original schema missing"))
}
pub(super) fn verify_current(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    old: &Value,
    claim: &ExactRef,
    p: &Value,
    key: &str,
    net: &BigInt,
    basis: &[ExactRef],
    effect: &str,
    root: &str,
    operands: &authority::Terms,
) -> Result<Vec<(ExactRef, RecordBody)>> {
    let (_, evaluation) = call!(
        load,
        m,
        &mut *r,
        c,
        &old["evaluation_ref"],
        "workflow-evaluation",
        "ledger-workflow-evaluation-proof/1"
    )?;
    call!(
        shape::closed,
        m,
        &evaluation,
        &[
            "schema",
            "subject_ref",
            "policy_ref",
            "compiled_plan_ref",
            "compiler_ref",
            "binding_ref",
            "fact_schema_ref",
            "facts",
            "context",
            "steps"
        ],
        &[]
    )?;
    for (k, v) in [
        ("subject_ref", json!(claim)),
        ("policy_ref", p["policy_ref"].clone()),
        (
            "compiled_plan_ref",
            json!(program.compiled_plan().reference()),
        ),
        (
            "compiler_ref",
            program.compiled_plan().canonical_value()["compiler_ref"].clone(),
        ),
        ("binding_ref", p["binding_ref"].clone()),
        ("fact_schema_ref", p["fact_schema_ref"].clone()),
        ("context", operands.binding["accepted_context"].clone()),
    ] {
        call!(equal, m, &evaluation[k], &v)?;
    }
    let family = call!(select_family, m, program, key)?;
    if !call!(
        shape::same_text,
        m,
        call!(shape::text, m, old, "status")?,
        "active"
    )? {
        return Err(shape::fail(
            "current outcome must remain active even for none/zero",
        ));
    }
    let facts = call!(shape::field, m, &evaluation, "facts")?;
    let facts_object = facts
        .as_object()
        .ok_or_else(|| shape::fail("current outcome facts object"))?;
    let empty = call!(meter::cmp, m, &facts_object.len().into(), &0.into())?.is_eq();
    let price = if empty {
        // Only an explicit no-replacement correction has empty facts. Its
        // retained original recipe must establish that earlier transition;
        // this local use still checks its none status and original ordinal.
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, old, "transition")?,
            "outcome_correct"
        )? {
            return Err(shape::fail(
                "empty current facts require explicit none correction",
            ));
        }
        pricing::explicit_none(program, key, m)?
    } else {
        let schema = call!(select_schema, m, program, &p["fact_schema_ref"])?;
        let normalized = super::working_value::NormalizedInput::normalize(m, schema, facts)?;
        call!(equal, m, normalized.canonical_value(), facts)?;
        let code = call!(
            shape::text,
            m,
            facts,
            call!(shape::text, m, family, "code_field")?
        )?;
        pricing::outcome(program, key, code, net, m)?
    };
    let refs = call!(shape::array, m, old, "active_action_refs")?;
    let expected = if price.none || call!(meter::cmp, m, &price.atoms.into(), &0.into())?.is_eq() {
        0
    } else {
        1
    };
    if !call!(meter::cmp, m, &refs.len().into(), &expected.into())?.is_eq() {
        return Err(shape::fail("current action count versus original pricing"));
    }
    let mut out = vec![];
    for item in refs {
        let (reference, a) = call!(
            load,
            m,
            &mut *r,
            c,
            item,
            "workflow-action",
            "ledger-workflow-action/1"
        )?;
        call!(dictionary_action, m, &a)?;
        for (k, v) in [
            ("effect_id", json!(effect)),
            ("decision_id", old["decision_id"].clone()),
            ("revision", old["revision"].clone()),
            ("book", json!("retail")),
            ("component_slot", json!(root)),
            ("root_component_slot", json!(root)),
            ("role_ref", json!(operands.role_ref)),
            ("slot_terms_ref", json!(operands.slot_terms_ref)),
            ("funding_leg", operands.slot_terms["funding_leg"].clone()),
            ("amount", amount(&price.atoms.into())),
            ("basis_refs", json!(basis)),
            ("operation_key", old["operation_key"].clone()),
        ] {
            call!(equal, m, &a[k], &v)?;
        }
        let posting = if call!(
            shape::same_text,
            m,
            call!(shape::text, m, old, "revision")?,
            "1"
        )? {
            "original"
        } else {
            "replacement"
        };
        call!(equal, m, &a["posting_role"], &json!(posting))?;
        let id = call!(
            body::semantic_id,
            m,
            "waction_",
            "workflow-action",
            &json!([effect, old["revision"], posting])
        )?;
        if !call!(shape::same_text, m, &reference.id, &id)? {
            return Err(shape::fail("current original effect identity"));
        }
        if a.get("inverse_of").is_some() {
            return Err(shape::fail("active action cannot be inverse"));
        }
        out.push((reference, a));
    }
    Ok(out)
}
