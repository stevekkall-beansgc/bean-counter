//! Shared original-family submit DATA construction. Host origin/current-index,
//! capacity, funding and atomicity remain separate gates; no output can commit.
use super::super::selected_program::SelectedProgram;
use super::emitted_record::ConstructedEnvelope;
use super::record_body::RecordBody;
use super::records::SuppliedRecords;
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
) -> Result<(ExactRef, RecordBody)> {
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
// An outcome uses its ordinary root head; independently finalized app costs
// remain in the complete state consumed later by funding/capacity. A sibling
// is excluded from this local view only after its original identity and bodies
// are checked in this SAME Records table. No host-provided exclusion is used.
fn local_ordinary_head_count(
    m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,
    work_ref:&ExactRef,work_payload:&Value,root_id:&str,binding_ref:&ExactRef,
    instantiation:&Value,heads:&[DecisionHead],
)->Result<usize>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("local outcome head owner"))?;
    let _scratch=arena.reserve_external_payload(2*(std::mem::size_of::<String>()+128),m)?;
    let scope=super::work_keys::scope(m,&c.key)?;
    let work=super::working_value::owning_reference(m,&arena,work_ref)?;
    let binding=super::working_value::owning_reference(m,&arena,binding_ref)?;
    let mut ordinary=0usize;
    for(i,head)in heads.iter().enumerate(){
        m.visit("outcome.local_ordinary_head")?;
        for old in &heads[..i]{if shape::same_text(m,&old.decision_id,&head.decision_id)?{return Err(shape::fail("duplicate local outcome decision head"));}}
        if shape::same_text(m,&head.decision_id,root_id)?{ordinary=crate::validation_work::usize_add(m,ordinary,1)?;continue;}
        let mut sibling=false;
        super::input_selection::head_rows(program,work_payload,instantiation,m,&mut |slot,family,m|{
            if family.is_some(){return Ok(());}
            let id=super::work_keys::app_decision(m,scope.value(),&work_ref.id,slot)?;
            if !shape::same_text(m,&id,&head.decision_id)?{return Ok(());}
            if sibling{return Err(shape::fail("duplicate original app sibling mapping"));}
            let decision=r.get(m,&c.key,&head.reference,"workflow-decision","ledger-workflow-decision-revision/3")?;
            for(field,want)in [("kind","cost"),("decision_id",head.decision_id.as_str()),("revision",head.revision.as_str())]{
                let actual=shape::text(m,&decision,field)?;if !shape::same_text(m,actual,want)?{return Err(shape::fail("local outcome app head association"));}
            }
            positive(m,&decision,"revision")?;
            equal(m,&decision["original_terms_ref"],binding.value())?;
            let claim_ref=super::working_value::parse_owned_reference(m,&arena,&decision["subject_ref"])?;
            let claim=r.get(m,&c.key,claim_ref.reference(),"workflow-cost","ledger-workflow-app-cost-claim/1")?;
            equal(m,&claim["original_terms_ref"],binding.value())?;
            let subject=shape::field(m,&claim,"subject")?;
            equal(m,&subject["original_work_ref"],work.value())?;
            equal(m,&subject["original_binding_ref"],binding.value())?;
            equal(m,&subject["scope"],scope.value())?;
            let store=shape::text(m,subject,"logical_store_id")?;let component=shape::text(m,subject,"component_slot")?;
            if !shape::same_text(m,store,&c.key.logical_store_id)? || !shape::same_text(m,component,slot)?{return Err(shape::fail("local outcome app subject association"));}
            sibling=true;Ok(())
        })?;
        if !sibling{ordinary=crate::validation_work::usize_add(m,ordinary,1)?;}
    }
    Ok(ordinary)
}
/// Called only after the common entry charges/parses the original request.
/// It recomputes records from source/program/retained inputs, never saved money.
pub(super) fn execute<'inv, 'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    mut p: super::working_value::WorkingValue,
    inputs: SuppliedRecords<'raw>,
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction<'inv, 'raw>> {
    entry::context_selected(m,c,program)?;
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
    let mut r = inputs.resolve(m, &c.key, &c.source_observation_inputs)?;
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
    if m.representation_arena().is_some(){entry::historical_work_claim(m,c,&claim_ref,&claim,&work)?;}else{entry::work_claim(m,c,&claim_ref,&claim)?;}
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
    let ordinary_heads=if program.uses_local_execution_recipe() && state.decision_heads.len()>1 {
        let(binding_ref,binding)=load(m,&mut r,c,&target["binding_ref"],"workflow-binding","ledger-workflow-binding/2")?;
        let(_,instantiation)=load(m,&mut r,c,&binding["slot_instantiation_ref"],"workflow-slot-instantiation","ledger-workflow-slot-instantiation/1")?;
        local_ordinary_head_count(m,&mut r,c,program,&work_ref,work_payload,root_id,&binding_ref,&instantiation,&state.decision_heads)?
    }else{state.decision_heads.len()};
    if !call!(meter::cmp, m, &ordinary_heads.into(), &1.into())?.is_eq() {
        return Err(unsupported("extra outcome decision selection"));
    }
    let mut basis_owner=None;
    let mut basis=Vec::new();
    if let Some(arena)=m.representation_arena(){
        use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
        let count=shape::array(m,&root_decision,"active_action_refs")?.len();
        let per=std::mem::size_of::<ExactRef>()+64+128+71+std::mem::size_of::<&ExactRef>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
        let bytes=count.checked_mul(per).ok_or_else(||shape::fail("original family basis dimensions"))?;
        let bytes=w::usize_add(m,bytes,std::mem::size_of::<Vec<ExactRef>>()+std::mem::size_of::<Vec<&ExactRef>>())?;
        basis_owner=Some(arena.reserve_external_payload(bytes,m)?);m.write_bytes(bytes)?;
        basis.try_reserve_exact(count).map_err(|_|shape::fail("original family basis allocation"))?;
    }
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
    let basis=if basis_owner.is_some(){basis}else{body::ref_set(m,basis)?};
    let basis_value=if let Some(arena)=m.representation_arena(){
        let mut refs=Vec::new();refs.try_reserve_exact(basis.len()).map_err(|_|shape::fail("original family basis views"))?;
        for reference in &basis{m.visit("outcome.original_basis_view")?;refs.push(reference);}
        super::working_value::WorkingValue::Owned(super::app_cost::reference_set(m,&arena,&refs)?)
    }else{super::working_value::WorkingValue::Legacy(json!(basis))};
    call!(equal,m,&basis_value,call!(shape::field,m,&target,"original_action_refs")?)?;
    if let Some(arena)=m.representation_arena(){
        let amount=targets::owned_amount(m,&arena,&net)?;
        call!(equal,m,call!(shape::field,m,&target,"original_net")?,amount.value())?;
    }else{
        call!(equal,m,call!(shape::field,m,&target,"original_net")?,&json!({"currency":"USD","scale":18,"atoms":net.to_string()}))?;
    }
    let raw_slot = shape::text(m, &p, "family_slot")?;
    let legacy_slot=if m.representation_arena().is_some(){None}else{
        m.write_bytes(raw_slot.len())?;Some(raw_slot.to_owned())
    };
    let target_family = call!(
        shape::named,
        m,
        call!(shape::array, m, &target, "families")?,
        "family_slot",
        legacy_slot.as_deref().unwrap_or(raw_slot)
    )?;
    // The payload is normalized later; borrow the exact immutable target row
    // selected above rather than retain a reference into a replaced payload.
    let family_slot_owned=match legacy_slot{Some(slot)=>std::borrow::Cow::Owned(slot),None=>std::borrow::Cow::Borrowed(shape::text(m,target_family,"family_slot")?)};
    let family_slot=family_slot_owned.as_ref();
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
    let normalized = super::working_value::NormalizedInput::normalize(m,schema,&p["facts"])?;
    p.replace(m,"facts",normalized.canonical_value())?;
    let proofs = outcome_proofs::resolve(m, &mut r, c, &p, schema, &normalized, state)?;
    p.normalize_set(m,"fact_proofs",true)?;
    let authority_payload = p.with_fields(m,&[("customer",&work_payload["customer"])])?;
    let operands = if m.representation_arena().is_some() {
        let namespace=shape::text(m,&work,"source_namespace")?;
        authority::economic_original(
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
        namespace,
    )?
    }else{
authority::economic(
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
    )?
    };
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
    // Seven fixed emitted rows plus the optional family action. Reference
    // copies for claim/link, heads and introduction effects overlap those rows.
    // JSON body graphs keep their own arena owners; native arithmetic scratch
    // and whole-executor/stack bounds remain separate containing obligations.
    let mut posting_owner=None;
    if let Some(arena)=m.representation_arena(){
        let bytes=8*std::mem::size_of::<super::emitted_record::ConstructedEnvelope>()
            +15*(std::mem::size_of::<ExactRef>()+64+128+71)
            +12*(std::mem::size_of::<String>()+128)
            +8*std::mem::size_of::<Vec<Value>>()
            +20*std::mem::size_of::<(&str,&Value)>();
        posting_owner=Some(arena.reserve_external_payload(bytes,m)?);m.write_bytes(bytes)?;
    }
    let (claim,link)=if let Some(arena)=m.representation_arena(){
        let constants=m.parse_owned_representation(&arena,
            br#"{"claim":"ledger-workflow-outcome-claim/1","link":"ledger-workflow-link/1","classification":"monetary"}"#,
            crate::validation_work::RepresentationKind::SourceDefinition)?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let family_slot=arena.string_value(family_slot,m)?;
        let target_id=arena.string_value(&target_ref.id,m)?;
        let tuple=arena.array_from_values(&[&key.value()["scope"],&target["agreement_id"],family_slot.value(),target_id.value()],m)?;
        let id=body::semantic_id(m,"woclaim_","workflow-outcome-claim",tuple.value())?;
        let target_value=super::working_value::owning_reference(m,&arena,&target_ref)?;
        let namespace=arena.string_value(&c.canonical_economic_namespace,m)?;
        let received=arena.string_value(&c.received_at,m)?;let accepted=arena.string_value(&c.accepted_at,m)?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["claim"]),("scope",&key.value()["scope"]),("agreement_id",&target["agreement_id"]),
            ("family_slot",family_slot.value()),("target_ref",target_value.value()),("source_namespace",namespace.value()),
            ("business_outcome_token",&p["business_outcome_token"]),("original_payload",&p),
            ("received_at",received.value()),("accepted_at",accepted.value()),
        ],m)?;
        let claim=body::record_borrowed(m,&c.key,"workflow-outcome-claim",id,value.value())?;
        let child=super::working_value::owning_reference(m,&arena,&claim.reference)?;
        let work=super::working_value::owning_reference(m,&arena,&work_ref)?;
        let authority=super::working_value::owning_reference(m,&arena,&operands.authority.reference)?;
        let tuple=arena.array_from_values(&[&key.value()["scope"],child.value(),&p["product_ref"],&family["target_link_slot"],work.value()],m)?;
        let id=body::semantic_id(m,"wlink_","workflow-link",tuple.value())?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["link"]),("scope",&key.value()["scope"]),("child",child.value()),
            ("product_ref",&p["product_ref"]),("binding_ref",&p["binding_ref"]),("slot",&family["target_link_slot"]),
            ("predecessor",work.value()),("classification",&constants.value()["classification"]),
            ("authority_ref",authority.value()),("operation_key",key.value()),
        ],m)?;
        let link=body::record_borrowed(m,&c.key,"workflow-link",id,value.value())?;
        (claim,link)
    }else{
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
        (claim,link)
    };
    let effect_id=if m.representation_arena().is_some(){
        original_family_effect(m,&c.key.scope,&target["agreement_id"],root_slot,&claim_ref,&target_family["family_slot"],&link.reference)?
    }else{
    let effect_id = original_family_effect(
        m,
        &c.key.scope,
        &target["agreement_id"],
        root_slot,
        &claim_ref,
        &json!(family_slot),
        &link.reference,
    )?;        effect_id
    };
    let (step,actions)=if let Some(arena)=m.representation_arena(){
        let root=arena.string_value(root_slot,m)?;let family=arena.string_value(family_slot,m)?;
        let posting=super::cost_postings::family_posting(m,c,super::cost_postings::FamilyPosting{
            effect_id:&effect_id,decision_id:&expected_decision,revision:&1.into(),
            root_slot:root.value(),family_slot:family.value(),basis_refs:&basis_value,
            role_ref:&operands.role_ref,terms_ref:&operands.slot_terms_ref,terms:&operands.slot_terms,replacement:false,
        },&price)?;
        let mut actions=Vec::new();actions.try_reserve_exact(1).map_err(|_|shape::fail("family action output"))?;
        if let Some(action)=posting.action{actions.push(action);}
        (super::working_value::WorkingValue::Owned(posting.step),actions)
    }else{
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
        (super::working_value::WorkingValue::Legacy(step),actions)
    };
    let (evaluation,decision)=if let Some(arena)=m.representation_arena(){
        let constants=m.parse_owned_representation(&arena,
            br#"{"evaluation":"ledger-workflow-evaluation-proof/1","decision":"ledger-workflow-decision-revision/1","revision":"1","kind":"outcome","transition":"initial","status":"active"}"#,
            crate::validation_work::RepresentationKind::SourceDefinition)?;
        let subject=super::working_value::owning_reference(m,&arena,&claim.reference)?;
        let definition=program.compiled_plan().reference();
        let kind=arena.string_value(definition.kind(),m)?;let id=arena.string_value(definition.id(),m)?;let digest=arena.string_value(definition.digest(),m)?;
        let compiled=arena.object_from_fields(&[("kind",kind.value()),("id",id.value()),("digest",digest.value())],m)?;
        let steps=arena.array_from_values(&[&step],m)?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["evaluation"]),("subject_ref",subject.value()),("policy_ref",&p["policy_ref"]),
            ("compiled_plan_ref",compiled.value()),("compiler_ref",&program.compiled_plan().canonical_value()["compiler_ref"]),
            ("binding_ref",&p["binding_ref"]),("fact_schema_ref",&p["fact_schema_ref"]),("facts",&p["facts"]),
            ("context",&operands.binding["accepted_context"]),("steps",steps.value()),
        ],m)?;
        let evaluation=body::document_borrowed(m,"workflow-evaluation",value.value())?;
        m.visit("outcome.owning_action_ref_presence")?;
        let action_refs=if let Some(action)=actions.first(){super::app_cost::reference_set(m,&arena,&[&action.reference])?}
            else{super::app_cost::reference_set(m,&arena,&[])?};
        let decision_id=arena.string_value(&expected_decision,m)?;
        let tuple=arena.array_from_values(&[decision_id.value(),&constants.value()["revision"]],m)?;
        let id=body::semantic_id(m,"wdrev_","workflow-decision-revision",tuple.value())?;
        let authority=super::working_value::owning_reference(m,&arena,&operands.authority.reference)?;
        let eval=super::working_value::owning_reference(m,&arena,&evaluation.reference)?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let accepted=arena.string_value(&c.accepted_at,m)?;
        let empty=arena.array_from_values(&[],m)?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["decision"]),("decision_id",decision_id.value()),("revision",&constants.value()["revision"]),
            ("subject_ref",subject.value()),("kind",&constants.value()["kind"]),("transition",&constants.value()["transition"]),
            ("status",&constants.value()["status"]),("original_terms_ref",&p["binding_ref"]),("authority_ref",authority.value()),
            ("evaluation_ref",eval.value()),("actions_appended",action_refs.value()),("active_action_refs",action_refs.value()),
            ("dependency_refs",empty.value()),("operation_key",key.value()),("accepted_at",accepted.value()),
        ],m)?;
        let decision=body::record_borrowed(m,&c.key,"workflow-decision",id,value.value())?;
        (evaluation,decision)
    }else{
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
        (evaluation,decision)
    };
    let revision = meter::add(m, &old_revision, &1.into())?;
    let family_state=if let Some(arena)=m.representation_arena(){
        let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-family-state/1","state":"claimed"}"#,
            crate::validation_work::RepresentationKind::SourceDefinition)?;
        let target=super::working_value::owning_reference(m,&arena,&target_ref)?;
        let target_id=arena.string_value(&target_ref.id,m)?;
        let family=arena.string_value(family_slot,m)?;let revision=super::cost_postings::scalar(m,&arena,&revision)?;
        let tuple=arena.array_from_values(&[target_id.value(),family.value(),revision.value()],m)?;
        let id=body::semantic_id(m,"wfamily_","workflow-family-state",tuple.value())?;
        let work=super::working_value::owning_reference(m,&arena,&work_ref)?;
        let prior=super::working_value::owning_reference(m,&arena,&head.reference)?;
        let claimed=super::working_value::owning_reference(m,&arena,&claim.reference)?;
        let decision=super::working_value::owning_reference(m,&arena,&decision.reference)?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let accepted=arena.string_value(&c.accepted_at,m)?;let empty=arena.array_from_values(&[],m)?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["schema"]),("target_ref",target.value()),("family_slot",family.value()),
            ("revision",revision.value()),("state",&constants.value()["state"]),("window",&original_window),
            ("anchor_work_ref",work.value()),("closure_evidence_refs",empty.value()),("operation_key",key.value()),
            ("accepted_at",accepted.value()),("prior_ref",prior.value()),("claim_ref",claimed.value()),("decision_ref",decision.value()),
        ],m)?;
        body::record_borrowed(m,&c.key,"workflow-family-state",id,value.value())?
    }else{
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
        family_state
    };
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
    let chain_payload=if let Some(arena)=m.representation_arena(){
        super::working_value::WorkingValue::Owned(arena.object_from_fields(&[("customer",&work_payload["customer"]),("chain",&work_payload["chain"]),("binding_ref",&p["binding_ref"])],m)?)
    }else{super::working_value::WorkingValue::Legacy(json!({"customer":work_payload["customer"],"chain":work_payload["chain"],"binding_ref":p["binding_ref"]}))};
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
    let chain=if let Some(arena)=m.representation_arena(){
        let links=super::app_cost::reference_set(m,&arena,&[&link.reference])?;
        let root=records::hash(m,"workflow-link-set",links.value())?;
        let root=arena.string_value(&root,m)?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let revision=super::cost_postings::scalar(m,&arena,&chain_revision)?;
        let admitted=super::cost_postings::scalar(m,&arena,&admitted)?;
        let tuple=arena.array_from_values(&[&key.value()["logical_store_id"],&key.value()["scope"],&work_payload["customer"],&work_payload["chain"],revision.value()],m)?;
        let id=body::semantic_id(m,"wchain_","workflow-chain-state",tuple.value())?;
        let agreement=super::working_value::owning_reference(m,&arena,&c.authorities[0].accepted_agreement_ref)?;
        let activation=super::working_value::owning_reference(m,&arena,&c.authorities[0].activation_ref)?;
        let prior=super::working_value::owning_reference(m,&arena,old_chain_ref)?;
        let schema=arena.string_value("ledger-workflow-chain-state/1",m)?;
        let accepted=arena.string_value(&c.accepted_at,m)?;
        let value=arena.object_from_fields(&[
            ("schema",schema.value()),("logical_store_id",&key.value()["logical_store_id"]),("scope",&key.value()["scope"]),
            ("customer",&work_payload["customer"]),("chain",&work_payload["chain"]),("binding_ref",&p["binding_ref"]),
            ("accepted_agreement_ref",agreement.value()),("activation_ref",activation.value()),
            ("revision",revision.value()),("admitted_events",admitted.value()),("links_root",root.value()),
            ("operation_key",key.value()),("accepted_at",accepted.value()),("prior_ref",prior.value()),
        ],m)?;
        body::record_borrowed(m,&c.key,"workflow-chain-state",id,value.value())?
    }else{
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
        chain
    };
    let mut result_owner=None;
    let heads=if let Some(arena)=m.representation_arena(){
        use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
        let n=w::usize_add(m,8,operands.introduced.len())?;let n=w::usize_add(m,n,proofs.introduced.len())?;
        let per=std::mem::size_of::<super::emitted_record::ConstructedEnvelope>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
        let bytes=n.checked_mul(per).ok_or_else(||shape::fail("outcome result metadata dimensions"))?;
        let fixed=std::mem::size_of::<Vec<super::emitted_record::ConstructedEnvelope>>()
            +std::mem::size_of::<ProposedHeads>()+std::mem::size_of::<(String,String,ExactRef)>()
            +std::mem::size_of::<(ExactRef,String,String,ExactRef)>()
            +3*(std::mem::size_of::<ExactRef>()+64+128+71)
            +3*(std::mem::size_of::<String>()+128);
        let bytes=w::usize_add(m,bytes,fixed)?;
        result_owner=Some(arena.reserve_external_payload(bytes,m)?);m.write_bytes(bytes)?;
        let copy=|m:&mut CoreMeter,r:&ExactRef|->Result<ExactRef>{
            for raw in [&r.kind,&r.id,&r.digest]{m.scan_bytes(raw.len())?;m.write_bytes(raw.len())?;}
            Ok(r.clone())
        };
        let revision=super::cost_postings::scalar(m,&arena,&revision)?;
        let revision=revision.value().as_str().ok_or_else(||shape::fail("outcome head revision encoding"))?;
        m.scan_bytes(family_slot.len())?;m.write_bytes(family_slot.len())?;
        m.scan_bytes(revision.len())?;m.write_bytes(revision.len())?;m.scan_bytes(1)?;m.write_bytes(1)?;
        ProposedHeads{
            decision:vec![(expected_decision,"1".to_owned(),copy(m,&decision.reference)?)],
            family:vec![(target_ref,family_slot.to_owned(),revision.to_owned(),copy(m,&family_state.reference)?)],
            chain:Some(copy(m,&chain.reference)?),
        }
    }else{
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
        heads
    };
    let normalized_payload = p.bytes(m)?;
    let mut envelopes=if result_owner.is_some(){
        let n=crate::validation_work::usize_add(m,8,operands.introduced.len())?;
        let n=crate::validation_work::usize_add(m,n,proofs.introduced.len())?;
        let mut rows=Vec::new();rows.try_reserve_exact(n).map_err(|_|shape::fail("outcome result allocation"))?;
        for row in [&claim,&link]{for raw in [&row.reference.kind,&row.reference.id,&row.reference.digest]{m.scan_bytes(raw.len())?;m.write_bytes(raw.len())?;}}
        rows.push(claim.clone());rows
    }else{vec![claim.clone()]};
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
        original_reservations: None,financial_reservations:None,
        economic: EconomicConstruction {
            terms: operands.terms,
            original_work: OriginalWorkPayload::Retained(work),
        },
        normalized_payload,
        new_envelopes: envelopes,reused_result:None,
        evaluation_steps: match step {super::working_value::WorkingValue::Legacy(value)=>vec![value],super::working_value::WorkingValue::Owned(_)=>vec![]},
        proposed_heads: heads,
        claim_introductions: vec![claim.reference],
        link_introductions: vec![link.reference],
        prospective_obligations: vec![],
        records: r,
        history_edges: vec![],
        app_effects:None,app_causal:None,current_capacity:None,
        candidate_heads:None,candidate_introductions:None,financial_observation:None,funding_effects:None,control_owners:[posting_owner,result_owner,basis_owner,None,None],
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
    if m.representation_arena().is_some() {
        let namespace=shape::text(m,work,"source_namespace")?;
        let original_claim=r.get(m,&c.key,claim_ref,"workflow-work-claim","ledger-workflow-work-claim/1")?;
        equal(m,&original_claim["source_namespace"],&json!(namespace))?;
        entry::historical_work_claim(m,c,claim_ref,&original_claim,work)?;
        authority::historical_namespace(m,r,c,&decision["authority_ref"],&work["source_namespace"],Some(&work["key"]["source"]),"work.accept",&p["binding_ref"],&work["accepted_at"])?;
    }else{
        call!(equal,m,call!(shape::field,m,work,"source_namespace")?,&json!(c.canonical_economic_namespace))?;
    }
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
    let key_value=shape::field(m,work,"key")?;
    let old_key=super::working_value::WorkingKey::parse(m,key_value)?;
    let _identity_owner=if let Some(arena)=m.representation_arena(){
        let bytes=8*std::mem::size_of::<String>()+8*128+4*std::mem::size_of::<(&str,&Value)>();
        let owner=arena.reserve_external_payload(bytes,m)?;m.write_bytes(bytes)?;Some(owner)
    }else{None};
    let (stable,revision)=if let Some(arena)=m.representation_arena(){
        if !shape::same_text(m,&old_key.logical_store_id,&c.key.logical_store_id)?{return Err(shape::fail("original work store"));}
        for (original,current) in old_key.scope.iter().zip(c.key.scope.iter()){
            m.visit("outcome.original_work_scope")?;
            if !shape::same_text(m,original,current)?{return Err(shape::fail("original work scope"));}
        }
        let stable=super::work_keys::decision(m,&claim_ref.id)?;
        let id=arena.string_value(&stable,m)?;let first=arena.string_value("1",m)?;
        let tuple=arena.array_from_values(&[id.value(),first.value()],m)?;
        let revision=body::semantic_id(m,"wdrev_","workflow-decision-revision",tuple.value())?;
        (stable,revision)
    }else{
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
        (stable,revision)
    };
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
    if let Some(arena)=m.representation_arena(){
        let constants=m.parse_owned_representation(&arena,br#"{"revision":"1","kind":"work","transition":"initial","status":"active"}"#,
            crate::validation_work::RepresentationKind::SourceDefinition)?;
        let stable=arena.string_value(&stable,m)?;
        for(key,want)in[("decision_id",stable.value()),("revision",&constants.value()["revision"]),("kind",&constants.value()["kind"]),("transition",&constants.value()["transition"]),("status",&constants.value()["status"]),("operation_key",key_value),("original_terms_ref",&p["binding_ref"]),("accepted_at",&work["accepted_at"])]{
            call!(equal,m,call!(shape::field,m,decision,key)?,want)?;
        }
    }else{
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
    let event_id=if m.representation_arena().is_some(){super::work_keys::event(m,&old_key,p)?}else{
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
        event_id
    };
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
    let facts = super::working_value::NormalizedInput::normalize(m, schema, &p["facts"])?;
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
    if m.representation_arena().is_some(){
        return verify_original_owned_postings(m,r,c,&old_key,operands,claim_ref,&stable,&prices,decision);
    }
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
/// Reconstruct with the SAME original action producer used by initial work.
/// Current authority/origin checks above remain mandatory; this compares exact
/// original captured-key bytes and never substitutes the correcting source.
fn verify_original_owned_postings(
    m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,key:&OperationKey,
    terms:&authority::Terms,claim:&ExactRef,decision_id:&str,prices:&pricing::PricedInstructions,decision:&Value,
)->Result<()>{
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning original posting verification"))?;
    let n=prices.len();let per=std::mem::size_of::<(&str,ExactRef)>()+64+128+71
        +std::mem::size_of::<&ExactRef>();
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
    let size=n.checked_mul(per).ok_or_else(||shape::fail("original posting comparison metadata"))?;
    let fixed=2*std::mem::size_of::<Vec<(&str,ExactRef)>>()
        +3*(std::mem::size_of::<ExactRef>()+64+128+71)
        +8*(std::mem::size_of::<String>()+128)
        +20*std::mem::size_of::<(&str,&Value)>();
    let size=w::usize_add(m,size,fixed)?;let _owner=arena.reserve_external_payload(size,m)?;m.write_bytes(size)?;
    let mut expected=Vec::<(&str,ExactRef)>::new();expected.try_reserve_exact(n).map_err(|_|shape::fail("original posting comparison allocation"))?;
    let original=shape::array(m,decision,"active_action_refs")?;
    for price in prices{
        m.visit("outcome.reconstruct_original_posting")?;
        let mapping=call!(shape::named,m,call!(shape::array,m,&terms.instantiation,"component_map")?,"template_key",&price.component_key)?;
        let slot=shape::text(m,mapping,"slot_id")?;
        let root_mapping=call!(shape::named,m,call!(shape::array,m,&terms.instantiation,"component_map")?,"template_key",&price.root_key)?;
        let root=shape::text(m,root_mapping,"slot_id")?;
        if meter::cmp(m,&price.atoms.into(),&0.into())?.is_eq(){continue;}
        let count=price.basis_components.len();let item=std::mem::size_of::<&ExactRef>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&item.into()])?;
        let bytes=count.checked_mul(item).ok_or_else(||shape::fail("original named basis dimensions"))?;
        let bytes=w::usize_add(m,bytes,std::mem::size_of::<Vec<&ExactRef>>())?;
        let _basis_owner=arena.reserve_external_payload(bytes,m)?;m.write_bytes(bytes)?;
        let mut refs=Vec::new();refs.try_reserve_exact(count).map_err(|_|shape::fail("original named basis allocation"))?;
        for component in &price.basis_components{
            m.visit("outcome.reconstruct_original_named_basis")?;
            let mut found=None;
            for(prior,reference)in &expected{
                if shape::same_text(m,prior,component)?{found=Some(reference);break;}
            }
            refs.push(found.ok_or_else(||shape::fail("original named pricing basis"))?);
        }
        let basis=super::app_cost::reference_set(m,&arena,&refs)?;
        let slot=arena.string_value(slot,m)?;let root=arena.string_value(root,m)?;let book=arena.string_value("retail",m)?;
        let posting=super::cost_postings::original_cost_posting_key(m,key,super::cost_postings::OriginalCostPosting{
            binding:&terms.binding,original_work_claim:claim,component_slot:slot.value(),root_slot:root.value(),book:book.value(),
            terms_ref:&terms.slot_terms_ref,terms:&terms.slot_terms,role_ref:&terms.role_ref,decision_id,revision:&1.into(),replacement:false,basis_refs:Some(basis.value()),
        },price)?;
        m.visit("outcome.reconstructed_original_action_presence")?;
        let expected_action=posting.action.ok_or_else(||shape::fail("nonzero original price lacks action"))?;
        let mut selected=None;
        for item in original{
            let reference=shape::reference(m,item)?;
            if shape::same_text(m,&reference.id,&expected_action.reference.id)?{
                m.visit("outcome.reconstructed_original_action_unique")?;
                if selected.is_some(){return Err(shape::fail("duplicate original effect"));}
                selected=Some(reference);
            }
        }
        let actual=selected.ok_or_else(||shape::fail("original priced effect missing"))?;
        r.get(m,&c.key,&actual,"workflow-action","ledger-workflow-action/1")?;
        let bytes=r.canonical(m,&actual)?;
        if !shape::same_ref(m,&actual,&expected_action.reference)?||!shape::same_bytes(m,bytes,&expected_action.canonical_body)?{
            return Err(shape::fail("original action differs from exact original producer"));
        }
        expected.push((&price.component_key,expected_action.reference));
    }
    let count=expected.len();let per=std::mem::size_of::<&ExactRef>();
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
    let bytes=count.checked_mul(per).ok_or_else(||shape::fail("original action set views"))?;
    m.write_bytes(bytes)?;
    let mut refs=Vec::new();refs.try_reserve_exact(count).map_err(|_|shape::fail("original action set allocation"))?;
    for(_,reference)in &expected{m.visit("outcome.reconstructed_original_set")?;refs.push(reference);}
    let set=super::app_cost::reference_set(m,&arena,&refs)?;
    equal(m,set.value(),&decision["active_action_refs"])
}

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
    if let Some(arena) = m.representation_arena() {
        // The original WorkClaim recipe has eleven fixed fields, optional
        // occurrence, then evidence. Borrow the exact immutable operands into
        // a fixed stack index, and pre-admit the ONE owning tuple before any
        // JSON copy. No reconstructed amount or today's source namespace is
        // substituted into this original facts digest.
        let absent = Value::Null;
        let mut fields = [&absent; 13];
        let mut n = 0usize;
        for key in ["customer", "chain", "status", "activity", "product_ref", "policy_ref", "fact_schema_ref", "facts", "links", "binding_ref", "invocation_refs"] {
            fields[n] = shape::field(m,p,key)?;
            n = crate::validation_work::usize_add(m,n,1)?;
        }
        m.visit("outcome.original_occurrence_presence")?;
        if let Some(value) = p.get("occurred_at") {
            fields[n] = value;
            n = crate::validation_work::usize_add(m,n,1)?;
        }
        fields[n] = shape::field(m,p,"evidence_refs")?;
        n = crate::validation_work::usize_add(m,n,1)?;
        let tuple = arena.array_from_values(&fields[..n],m)?;
        // hash's fixed SHA256 hexadecimal String has its own short lifetime;
        // this allowance precedes its formatting and outlives the result.
        let _digest = arena.reserve_external_payload(512,m)?;
        let digest = records::hash(m,"workflow-claim-facts",tuple.value())?;
        let declared = shape::text(m,claim,"facts_digest")?;
        if !shape::same_text(m,declared,&digest)? {return Err(shape::fail("original work claim facts association"));}
        return Ok(());
    }
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
                binding: super::super::record_body::RecordBody::Legacy(
                    json!({"agreement_id":"agreement"}),
                ),
                accepted: super::super::record_body::RecordBody::Legacy(json!({})),
                activation: super::super::record_body::RecordBody::Legacy(json!({})),
                catalog: super::super::record_body::RecordBody::Legacy(
                    json!({"components":[{"slot_id":"root","root_slot":"root","role_slot":"payer"}],"families":[{"slot_id":"assessment","target_component_slot":"root","role_slot":"payer"}]}),
                ),
                instantiation: super::super::record_body::RecordBody::Legacy(
                    json!({"component_map":[{"template_key":"units","slot_id":"root"}],"family_map":[{"template_key":"assess","slot_id":"assessment"}]}),
                ),
                slot_terms: super::super::record_body::RecordBody::Legacy(json!({})),
                slot_terms_ref: r.clone(),
                role_ref: r.clone(),
            },
            authority: ConstructedEnvelope::from_bytes(r, vec![]),
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
                o.terms.catalog.legacy_mut()["families"][0]["role_slot"] = json!("other-role")
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
    if let Some(arena)=m.representation_arena(){
        let first=arena.string_value(&scope[0],m)?;let second=arena.string_value(&scope[1],m)?;
        let scope=arena.array_from_values(&[first.value(),second.value()],m)?;
        let root=arena.string_value(root_slot,m)?;
        let claim=arena.string_value(&work_claim.id,m)?;
        let link=arena.string_value(&link.id,m)?;
        let links=arena.array_from_values(&[link.value()],m)?;
        let family=arena.array_from_values(&[family_slot,links.value(),root.value()],m)?;
        let original=arena.string_value("original",m)?;
        let tuple=arena.array_from_values(&[scope.value(),agreement,root.value(),claim.value(),family.value(),original.value()],m)?;
        body::semantic_id(m,"weffect_","workflow-effect",tuple.value())
    }else{
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
}

#[cfg(test)]
mod local_head_view_tests {
    use super::*;
    use crate::{canonical::{self,audit_arena::AuditArena},policy::workflow::{DefinitionRef,document_digest,selected_program::{SelectedCompilerSupport,SelectedSourceInput,compile_selected_publication}},validation_work::{OriginalAuditSession,ResourceLimits}};
    #[test]
    fn local_outcome_view_keeps_app_sibling_in_full_funding_total() {
        let arena=AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut compilation=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support=SelectedCompilerSupport::local_execution_with_session(&mut compilation).unwrap();
        let fixture:Value=serde_json::from_slice(include_bytes!("../selected_program/fixtures/local-first-policy-input.json")).unwrap();
        let mut documents=fixture["documents"].as_array().unwrap().clone();
        let policy=documents.iter_mut().find(|d|d["schema"]=="ledger-workflow-policy/4").unwrap();
        policy["compiler_ref"]=serde_json::to_value(support.reference()).unwrap();
        let digest=document_digest("ledger-workflow-policy/4",policy).unwrap();
        let root=DefinitionRef::new("workflow-policy",&format!("wdoc_{}",&digest[7..]),&digest).unwrap();
        documents.push(support.canonical_value().clone());
        let program=compile_selected_publication(SelectedSourceInput{documents:documents.iter().map(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect(),root_policy_ref:root},&support,&mut compilation).unwrap();
        let key=OperationKey{logical_store_id:"test".into(),scope:["test".into(),"local".into()],source:"source".into(),operation_id:"outcome".into()};
        let c=DraftContextData{key:key.clone(),actor:"operator".into(),canonical_economic_namespace:"source".into(),installation_mode:"isolated_synthetic".into(),received_at:"2026-10-18T00:00:00.000000Z".into(),accepted_at:"2026-10-18T00:00:00.000000Z".into(),authorities:vec![],source_observation_inputs:vec![]};
        // Small canonical operands for these two kernels, not an admission or
        // historical-authority fixture and never input to the real CLI.
        let mut producer=CoreMeter::for_compile(1048576).unwrap();
        let work_payload=json!({"activity":"support:complete"});
        let work=body::record(&mut producer,&key,"workflow-work",format!("wevent_{}","1".repeat(64)),json!({"schema":"ledger-workflow-work-record/1","key":key,"payload":work_payload})).unwrap();
        let binding=body::document(&mut producer,"workflow-binding",json!({"schema":"ledger-workflow-binding/2","scope":key.scope})).unwrap();
        let inst=json!({"component_map":[{"template_key":"support:service","slot_id":"support:service"},{"template_key":"support:model-retail","slot_id":"support:model-retail"}],"family_map":[{"template_key":"support:quality","slot_id":"support:quality"}]});
        let target=body::record(&mut producer,&key,"workflow-target",format!("wtarget_{}","2".repeat(64)),json!({"schema":"ledger-workflow-component-target/1","work_ref":work.reference})).unwrap();
        let claim=body::document(&mut producer,"workflow-cost",json!({"schema":"ledger-workflow-app-cost-claim/1","original_terms_ref":binding.reference,"subject":{"logical_store_id":key.logical_store_id,"scope":key.scope,"original_work_ref":work.reference,"original_binding_ref":binding.reference,"component_slot":"support:model-retail"}})).unwrap();
        let root_id=format!("wdecision_{}","3".repeat(64));
        let app_id=body::semantic_id(&mut producer,"wdecision_","workflow-decision",&json!([key.scope,work.reference.id,"support:model-retail","app_cost"])).unwrap();
        fn action(m:&mut CoreMeter,key:&OperationKey,id:&str,tag:&str,atoms:&str)->ConstructedEnvelope{
            body::record(m,key,"workflow-action",format!("waction_{}",tag.repeat(64)),json!({"schema":"ledger-workflow-action/1","decision_id":id,"posting_role":"original","book":"retail","amount":{"currency":"USD","scale":18,"atoms":atoms}})).unwrap()
        }
        fn decision(m:&mut CoreMeter,key:&OperationKey,id:&str,subject:&ExactRef,binding:&ExactRef,action:&ExactRef,app:bool,tag:&str)->ConstructedEnvelope{
            body::record(m,key,"workflow-decision",format!("wdrev_{}",tag.repeat(64)),json!({"schema":if app{"ledger-workflow-decision-revision/3"}else{"ledger-workflow-decision-revision/1"},"decision_id":id,"revision":"1","subject_ref":subject,"kind":if app{"cost"}else{"work"},"original_terms_ref":binding,"active_action_refs":[action]})).unwrap()
        }
        let root_action=action(&mut producer,&key,&root_id,"4","1000000000000000000");
        let app_action=action(&mut producer,&key,&app_id,"5","2000000000000000000");
        let root_decision=decision(&mut producer,&key,&root_id,&work.reference,&binding.reference,&root_action.reference,false,"6");
        let app_decision=decision(&mut producer,&key,&app_id,&claim.reference,&binding.reference,&app_action.reference,true,"7");
        let premium_id=format!("wdecision_{}","8".repeat(64));
        let premium_action=action(&mut producer,&key,&premium_id,"9","250000000000000000");
        let premium_decision=decision(&mut producer,&key,&premium_id,&target.reference,&binding.reference,&premium_action.reference,false,"a");
        let old=[work,claim,target,root_action,app_action,root_decision,app_decision];
        let views=old.iter().map(|v|EconomicRecordView{reference:&v.reference,canonical_body:&v.canonical_body}).collect::<Vec<_>>();
        let outputs=vec![premium_action,premium_decision];
        let mut state=DraftStateData{chain_head:None,decision_heads:vec![DecisionHead{decision_id:root_id.clone(),revision:"1".into(),reference:old[5].reference.clone()},DecisionHead{decision_id:app_id,revision:"1".into(),reference:old[6].reference.clone()}],family_heads:vec![],work_claims:vec![],outcome_claims:vec![],links:vec![],evidence_verifications:vec![]};
        let proposed=ProposedHeads{decision:vec![(premium_id,"1".into(),outputs[1].reference.clone())],family:vec![],chain:None};
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=session.meter_mut().local_economic_meter().unwrap();
        let mut records=Records::borrowed_app(&mut child,&arena,&key,&views,&[]).unwrap();
        assert_eq!(local_ordinary_head_count(&mut child,&mut records,&c,&program,&old[0].reference,&work_payload,&root_id,&binding.reference,&inst,&state.decision_heads).unwrap(),1);
        assert_eq!(state.decision_heads.len(),2);
        let before=super::super::funding::held_work_total(&mut child,&mut records,&c,&[],&state,&ProposedHeads{decision:vec![],family:vec![],chain:None},&old[0].reference,&binding.reference,&root_id).unwrap();
        assert_eq!(before,BigInt::from(3)*BigInt::from(10u8).pow(18));
        let after=super::super::funding::held_work_total(&mut child,&mut records,&c,&outputs,&state,&proposed,&old[0].reference,&binding.reference,&root_id).unwrap();
        assert_eq!(after,BigInt::from(325)*BigInt::from(10u8).pow(16));
        state.decision_heads[1].revision="2".into();
        assert!(local_ordinary_head_count(&mut child,&mut records,&c,&program,&old[0].reference,&work_payload,&root_id,&binding.reference,&inst,&state.decision_heads).is_err());
        state.decision_heads[1].revision="1".into();
        state.decision_heads.push(state.decision_heads[1].clone());
        assert!(local_ordinary_head_count(&mut child,&mut records,&c,&program,&old[0].reference,&work_payload,&root_id,&binding.reference,&inst,&state.decision_heads).is_err());
        state.decision_heads.pop();state.decision_heads[1].decision_id="unrecognized-decision".into();
        assert_eq!(local_ordinary_head_count(&mut child,&mut records,&c,&program,&old[0].reference,&work_payload,&root_id,&binding.reference,&inst,&state.decision_heads).unwrap(),2);
    }
}
