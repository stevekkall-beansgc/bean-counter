//! Shared fresh/replay pure draft. Unsupported selected branches refuse; no
//! financial append capability is created by this module.
use super::super::selected_program::SelectedProgram;
use super::emitted_record::ConstructedEnvelope;
use super::records::SuppliedRecords;
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
    execute(
        program,
        c,
        payload,
        SuppliedRecords::Legacy(inputs),
        state,
        m,
        None,
    )?
    .draft_projection(m)
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
        SuppliedRecords::Legacy(inputs),
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
pub(super) fn execute<'inv, 'raw>(
    program: &SelectedProgram,
    c: &'raw DraftContextData,
    payload: &[u8],
    inputs: SuppliedRecords<'raw>,
    state: &DraftStateData,
    m: &mut CoreMeter,
    intrinsic: Option<(
        &'inv super::intrinsic_record::IntrinsicDataScope,
        &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    )>,
) -> Result<Construction<'inv, 'raw>> {
    m.begin_request(payload)?;
    m.phase(Phase::ParseNormalize);
    let mut p = super::working_value::WorkingValue::parse(m, payload)?;
    execute_parsed(program,c,p,inputs,state,None,m,intrinsic)
}
/// SAME monetary interpreter with the actual held extra app/capacity/party
/// state. No caller-ready flag and no independent meter/budget is introduced.
pub(super) fn execute_complete<'raw>(program:&SelectedProgram,c:&'raw DraftContextData,payload:&[u8],inputs:&'raw [EconomicRecordView<'raw>],state:&EconomicStateData<'raw>,m:&mut CoreMeter)->Result<Construction<'raw,'raw>>{
    m.begin_request(payload)?;m.phase(Phase::ParseNormalize);
    let p=super::working_value::WorkingValue::parse(m,payload)?;
    let arena=m.representation_arena().ok_or_else(||shape::fail("complete constructor requires owning resource session"))?;
    let schema=shape::text(m,&p,"schema")?;
    if shape::same_text(m,schema,"ledger-workflow-fact-attest/1")?||shape::same_text(m,schema,"ledger-workflow-family-close-attest/1")?{
        let mut construction=super::retention::execute_complete(program,c,p.into_owned()?,inputs,state,&arena,m)?;
        construction.candidate_heads=Some(super::candidate::heads::derive(m,c,state,&construction)?);
        construction.candidate_introductions=Some(super::candidate::introductions::derive(m,c,&construction)?);
        if !program.uses_local_execution_recipe(){construction.financial_observation=Some(super::financial_observation::complete::select_and_introduce(m,program,&mut construction,c,state)?);}
        return Ok(construction);
    }
    if program.uses_app_owned_cost_recipe(){
        let schema=shape::text(m,&p,"schema")?;
        if shape::same_text(m,schema,"ledger-workflow-app-observe/1")?||shape::same_text(m,schema,"ledger-workflow-app-cost-attest/1")?{
            let mut construction=super::app_cost::retain_complete(program,c,p.into_owned()?,inputs,state,&arena,m)?;
            construction.candidate_heads=Some(super::candidate::heads::derive(m,c,state,&construction)?);
            construction.candidate_introductions=Some(super::candidate::introductions::derive(m,c,&construction)?);
            if !program.uses_local_execution_recipe(){construction.financial_observation=Some(super::financial_observation::complete::select_and_introduce(m,program,&mut construction,c,state)?);}
            // This is unattached evidence retention. No monetary association,
            // funding mutation or ancestor capacity revision is introduced.
            return Ok(construction);
        }
        if shape::same_text(m,schema,"ledger-workflow-cost-finalize/2")?||shape::same_text(m,schema,"ledger-workflow-cost-correct/2")?{
            let mut construction=super::app_cost::produce_parsed(program,c,p,inputs,state,m)?.into_construction(c,m)?;
            construction.candidate_heads=Some(super::candidate::heads::derive(m,c,state,&construction)?);
            construction.candidate_introductions=Some(super::candidate::introductions::derive(m,c,&construction)?);
            if !program.uses_local_execution_recipe(){construction.financial_observation=Some(super::financial_observation::complete::select_and_introduce(m,program,&mut construction,c,state)?);}
            construction.funding_effects=Some(super::funding::construction_effect(m,program,c,state,&mut construction)?);
            return Ok(construction);
        }
    }
    let mut construction=execute_parsed(program,c,p,SuppliedRecords::BorrowedApp{records:inputs,arena,sources:state.initial_source_correspondences},state.financial,Some(state),m,None)?;
    if program.uses_app_owned_cost_recipe(){super::app_cost::check_current_consent(program,m,&mut construction.records,c,&construction.economic.terms.binding,&construction.economic.terms.accepted,state)?;}
    construction.candidate_heads=Some(super::candidate::heads::derive(m,c,state,&construction)?);
    construction.candidate_introductions=Some(super::candidate::introductions::derive(m,c,&construction)?);
    if program.uses_app_owned_cost_recipe(){
        if !program.uses_local_execution_recipe(){construction.financial_observation=Some(super::financial_observation::complete::select_and_introduce(m,program,&mut construction,c,state)?);}
        construction.funding_effects=Some(super::funding::construction_effect(m,program,c,state,&mut construction)?);
    }
    Ok(construction)
}
fn execute_parsed<'inv,'raw>(program:&SelectedProgram,c:&'raw DraftContextData,mut p:super::working_value::WorkingValue,inputs:SuppliedRecords<'raw>,state:&DraftStateData,complete_state:Option<&EconomicStateData>,m:&mut CoreMeter,intrinsic:Option<(&'inv super::intrinsic_record::IntrinsicDataScope,&[&'inv super::intrinsic_record::IntrinsicRecord<'inv>])>)->Result<Construction<'inv,'raw>>{
    if m.representation_arena().is_some() {
        if shape::same(m,&p["schema"],&json!("ledger-workflow-outcome-close/2"))? {
            return super::close::execute(program,c,p,inputs,state,m,intrinsic);
        }
    }
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
            if let Some(complete)=complete_state{return super::root_correction::execute_complete(program,c,p,inputs,complete,m);}
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
    super::inventory::entry::validate_selected(m,c,&p,state,program)?;
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
    let (product,activity,legacy_activity_name)=if m.representation_arena().is_some(){
        // Parent DATA preselection and this consuming child execute the SAME
        // locator independently. No parent tuple replaces semantic work here.
        let(product,activity)=super::input_selection::work_activity(program,&p,m)?;
        (product,activity,None)
    }else{
    // Preserve the old non-RP01 draft's exact traversal and paid trace.
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
    (product,activity,Some(activity_name))
    };
    let activity_name=match &legacy_activity_name{Some(name)=>name.as_str(),None=>shape::text(m,activity,"name")?};
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
        super::working_value::NormalizedInput::normalize(m,schema,&p["facts"])
    })?;
    p.replace(m,"facts",normalized.canonical_value())?;
    let normalized_payload = p.bytes(m)?;
    let mut r = inputs.resolve(m, &c.key, &c.source_observation_inputs)?;
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
    let mut construction_owners=[None,None,None,None,None];
    if let Some(arena)=m.representation_arena(){
        // Ref payloads/control strings are admitted BEFORE either semantic ID
        // is constructed. Canonical/decoded graphs have independent owners.
        let controls=3*(std::mem::size_of::<ExactRef>()+64+128+71)
            +2*128+71+11*std::mem::size_of::<String>()
            +13*std::mem::size_of::<&Value>()
            +15*std::mem::size_of::<(&str,&Value)>()
            +2*std::mem::size_of::<ConstructedEnvelope>()
            +std::mem::size_of::<Vec<ConstructedEnvelope>>()
            +std::mem::size_of::<Vec<ExactRef>>();
        construction_owners[0]=Some(arena.reserve_external_payload(controls,m)?);
        m.write_bytes(controls)?;
    }
    let claim_id = if m.representation_arena().is_some(){super::work_keys::claim(m,&c.key,&c.canonical_economic_namespace,&p)?}else{
        body::semantic_id(
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
    )?
    };
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
    let work_id = if m.representation_arena().is_some(){super::work_keys::event(m,&c.key,&p)?}else{
        body::semantic_id(
        m,
        "wevent_",
        "workflow-event",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            c.key.source,
            p["external_event_id"]
        ]),
    )?
    };
    let (work,claim)=if let Some(arena)=m.representation_arena(){
        // The complete path constructs these actual Value graphs before body
        // emission; output byte leases alone never owned the old json! copies.
        let constants=m.parse_owned_representation(&arena,
            br#"{"work":"ledger-workflow-work-record/1","claim":"ledger-workflow-work-claim/1","completion":"completion"}"#,
            crate::validation_work::RepresentationKind::SourceDefinition)?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let claimed=arena.string_value(&claim_id,m)?;
        let received=arena.string_value(&c.received_at,m)?;
        let accepted=arena.string_value(&c.accepted_at,m)?;
        let namespace=arena.string_value(&c.canonical_economic_namespace,m)?;
        let work_value=arena.object_from_fields(&[
            ("schema",&constants.value()["work"]),("key",key.value()),
            ("payload",&p),("claim_id",claimed.value()),
            ("received_at",received.value()),("accepted_at",accepted.value()),
            ("source_namespace",namespace.value()),
        ],m)?;
        let work=body::record_borrowed(m,&c.key,"workflow-work",work_id,work_value.value())?;
        // Optional occurrence precedes evidence_refs in the original exact
        // tuple. Both branches use the SAME array and hash producers.
        let tuple=if let Some(occurrence)=p.get("occurred_at"){
            arena.array_from_values(&[&p["customer"],&p["chain"],&p["status"],&p["activity"],
                &p["product_ref"],&p["policy_ref"],&p["fact_schema_ref"],&p["facts"],
                &p["links"],&p["binding_ref"],&p["invocation_refs"],occurrence,&p["evidence_refs"]],m)?
        }else{
            arena.array_from_values(&[&p["customer"],&p["chain"],&p["status"],&p["activity"],
                &p["product_ref"],&p["policy_ref"],&p["fact_schema_ref"],&p["facts"],
                &p["links"],&p["binding_ref"],&p["invocation_refs"],&p["evidence_refs"]],m)?
        };
        let facts_digest=records::hash(m,"workflow-claim-facts",tuple.value())?;
        let facts_digest=arena.string_value(&facts_digest,m)?;
        let work_ref=super::working_value::owning_reference(m,&arena,&work.reference)?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["claim"]),("logical_store_id",&key.value()["logical_store_id"]),
            ("scope",&key.value()["scope"]),("source_namespace",namespace.value()),
            ("business_operation_id",&p["business_operation_id"]),("completion",&constants.value()["completion"]),
            ("work_ref",work_ref.value()),("facts_digest",facts_digest.value()),
        ],m)?;
        m.scan_bytes(claim_id.len())?;m.write_bytes(claim_id.len())?;
        let claim=body::record_borrowed(m,&c.key,"workflow-work-claim",claim_id.clone(),value.value())?;
        (work,claim)
    }else{
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
        (work,claim)
    };
    let decision_id = if m.representation_arena().is_some(){super::work_keys::decision(m,&claim_id)?}else{
        body::semantic_id(
        m,
        "wdecision_",
        "workflow-decision",
        &json!([claim_id, "work"]),
    )?
    };
    let prices = pricing::work(program, &activity_name, &p["facts"], m)?;
    super::inventory::entry::quantities(m, &a.binding, &a.slot_terms, &activity_name, &p["facts"])?;
    let mut total = num_bigint::BigInt::from(0);
    for price in &prices {
        m.visit("inventory.output_charge")?;
        total = meter::add(m, &total, &price.atoms.into())?;
    }
    super::inventory::entry::exposure(m, &a.binding, &a.slot_terms, &total)?;
    let mut actions = Vec::<ConstructedEnvelope>::new();
    let mut steps = vec![];
    let mut owned_steps=Vec::<crate::canonical::audit_arena::AuditedValue>::new();
    let mut action_slots = Vec::<(String, ExactRef)>::new();
    if let Some(arena)=m.representation_arena(){
        use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
        let n=prices.len();
        let per=std::mem::size_of::<ConstructedEnvelope>()+std::mem::size_of::<crate::canonical::audit_arena::AuditedValue>()
            +std::mem::size_of::<(String,ExactRef)>()+128+2*(64+128+71)
            +128+8*std::mem::size_of::<String>()
            +22*std::mem::size_of::<(&str,&Value)>()
            +2*std::mem::size_of::<&ExactRef>()+std::mem::size_of::<&Value>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
        let bytes=n.checked_mul(per).ok_or_else(||shape::fail("work posting control bound"))?;
        let fixed=5*std::mem::size_of::<Vec<Value>>()
            +2*(std::mem::size_of::<ExactRef>()+64+128+71)
            +8*std::mem::size_of::<String>()+4*128+71
            +30*std::mem::size_of::<(&str,&Value)>();
        let bytes=w::usize_add(m,bytes,fixed)?;
        construction_owners[2]=Some(arena.reserve_external_payload(bytes,m)?);
        m.write_bytes(bytes)?;
        actions.try_reserve_exact(n).map_err(|_|shape::fail("work action vector"))?;
        owned_steps.try_reserve_exact(n).map_err(|_|shape::fail("work step owner vector"))?;
        action_slots.try_reserve_exact(n).map_err(|_|shape::fail("work action basis vector"))?;
    }
    for price in prices {
        let mapping = shape::named(
            m,
            a.instantiation["component_map"]
                .as_array()
                .ok_or_else(|| shape::fail("component map"))?,
            "template_key",
            &price.component_key,
        )?;
        let slot_text=shape::text(m,mapping,"slot_id")?;
        let slot=if m.representation_arena().is_some(){std::borrow::Cow::Borrowed(slot_text)}else{std::borrow::Cow::Owned(slot_text.to_owned())};
        let rootmap = shape::named(
            m,
            a.instantiation["component_map"].as_array().unwrap(),
            "template_key",
            &price.root_key,
        )?;
        let root_text=shape::text(m,rootmap,"slot_id")?;
        let root_slot=if m.representation_arena().is_some(){std::borrow::Cow::Borrowed(root_text)}else{std::borrow::Cow::Owned(root_text.to_owned())};
        let catalog = shape::named(
            m,
            a.catalog["components"]
                .as_array()
                .ok_or_else(|| shape::fail("catalog components"))?,
            "slot_id",
            &slot,
        )?;
        if let Some(arena)=m.representation_arena(){
            for(field,expected)in[("root_slot",root_slot.as_ref()),("role_slot",price.role_slot.as_str()),("book","retail")]{
                let actual=shape::text(m,catalog,field)?;
                if !shape::same_text(m,actual,expected)?{return Err(shape::fail("catalog original posting association"));}
            }
            // Reuse the SAME customer posting producer used by attributable
            // costs. Original rule/basis selection still executes here.
            let mut selected=Vec::new();
            // The fixed workspace is part of control_owners[2]; writes are
            // paid each iteration, including early allocation/lookup refusal.
            let n=action_slots.len();
            use crate::validation_work::{ArithmeticOperation,ValidationWork};
            let per=std::mem::size_of::<&ExactRef>();
            ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
            let bytes=n.checked_mul(per).ok_or_else(||shape::fail("work basis pointer bound"))?;
            m.write_bytes(bytes+std::mem::size_of::<Vec<&ExactRef>>())?;
            selected.try_reserve_exact(n).map_err(|_|shape::fail("work basis pointer vector"))?;
            for key in &price.basis_components{
                for(component,reference)in &action_slots{
                    if shape::same_text(m,component,key)?{selected.push(reference);}
                }
            }
            let basis=super::app_cost::reference_set(m,&arena,&selected)?;
            let slot=arena.string_value(&slot,m)?;
            let root=arena.string_value(&root_slot,m)?;
            let posting=super::cost_postings::original_cost_posting(m,c,super::cost_postings::OriginalCostPosting{
                binding:&a.binding,original_work_claim:&claim.reference,component_slot:slot.value(),root_slot:root.value(),
                book:&catalog["book"],terms_ref:&a.slot_terms_ref,terms:&a.slot_terms,role_ref:&a.role_ref,
                decision_id:&decision_id,revision:&1.into(),replacement:false,basis_refs:Some(basis.value()),
            },&price)?;
            if let Some(action)=posting.action{
                for field in [&action.reference.kind,&action.reference.id,&action.reference.digest]{m.scan_bytes(field.len())?;m.write_bytes(field.len())?;}
                action_slots.push((price.component_key,action.reference.clone()));actions.push(action);
            }
            owned_steps.push(posting.step);
            continue;
        }
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
    let (evaluation,decision)=if let Some(arena)=m.representation_arena(){
        let constants=m.parse_owned_representation(&arena,
            br#"{"evaluation":"ledger-workflow-evaluation-proof/1","decision":"ledger-workflow-decision-revision/1","revision":"1","kind":"work","transition":"initial","status":"active"}"#,
            crate::validation_work::RepresentationKind::SourceDefinition)?;
        let mut borrowed=Vec::new();
        use crate::validation_work::{ArithmeticOperation,ValidationWork};
        let n=owned_steps.len();let per=std::mem::size_of::<&Value>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
        m.write_bytes(n.checked_mul(per).ok_or_else(||shape::fail("work evaluation step views"))?+std::mem::size_of::<Vec<&Value>>())?;
        borrowed.try_reserve_exact(n).map_err(|_|shape::fail("work evaluation step views"))?;
        for step in &owned_steps{m.visit("work.owning_evaluation_step")?;borrowed.push(step.value());}
        let all_steps=arena.array_from_values(&borrowed,m)?;
        let subject=super::working_value::owning_reference(m,&arena,&work.reference)?;
        let definition=program.compiled_plan().reference();
        let kind=arena.string_value(definition.kind(),m)?;let id=arena.string_value(definition.id(),m)?;let digest=arena.string_value(definition.digest(),m)?;
        let compiled=arena.object_from_fields(&[("kind",kind.value()),("id",id.value()),("digest",digest.value())],m)?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["evaluation"]),("subject_ref",subject.value()),("policy_ref",&p["policy_ref"]),
            ("compiled_plan_ref",compiled.value()),("compiler_ref",&program.compiled_plan().canonical_value()["compiler_ref"]),
            ("binding_ref",&p["binding_ref"]),("fact_schema_ref",&p["fact_schema_ref"]),("facts",&p["facts"]),
            ("context",&a.binding["accepted_context"]),("steps",all_steps.value()),
        ],m)?;
        let evaluation=body::document_borrowed(m,"workflow-evaluation",value.value())?;
        drop(borrowed);drop(owned_steps);
        let mut refs=Vec::new();let n=actions.len();let per=std::mem::size_of::<&ExactRef>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
        m.write_bytes(n.checked_mul(per).ok_or_else(||shape::fail("work action reference views"))?+std::mem::size_of::<Vec<&ExactRef>>())?;
        refs.try_reserve_exact(n).map_err(|_|shape::fail("work action reference views"))?;
        for action in &actions{m.visit("work.owning_action_ref")?;refs.push(&action.reference);}
        let action_refs=super::app_cost::reference_set(m,&arena,&refs)?;
        let decision_id_value=arena.string_value(&decision_id,m)?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let accepted=arena.string_value(&c.accepted_at,m)?;
        let authority=super::working_value::owning_reference(m,&arena,&a.authority.reference)?;
        let eval=super::working_value::owning_reference(m,&arena,&evaluation.reference)?;
        let empty=arena.array_from_values(&[],m)?;
        let tuple=arena.array_from_values(&[decision_id_value.value(),&constants.value()["revision"]],m)?;
        let revision_id=body::semantic_id(m,"wdrev_","workflow-decision-revision",tuple.value())?;
        let value=arena.object_from_fields(&[
            ("schema",&constants.value()["decision"]),("decision_id",decision_id_value.value()),("revision",&constants.value()["revision"]),
            ("subject_ref",subject.value()),("kind",&constants.value()["kind"]),("transition",&constants.value()["transition"]),
            ("status",&constants.value()["status"]),("original_terms_ref",&p["binding_ref"]),("authority_ref",authority.value()),
            ("evaluation_ref",eval.value()),("actions_appended",action_refs.value()),("active_action_refs",action_refs.value()),
            ("dependency_refs",empty.value()),("operation_key",key.value()),("accepted_at",accepted.value()),
        ],m)?;
        let decision=body::record_borrowed(m,&c.key,"workflow-decision",revision_id,value.value())?;
        (evaluation,decision)
    }else{
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
        (evaluation,decision)
    };
    let mut targets = super::targets::build(program, c, &p, &a, &work, &claim, &decision, &actions, m)?;
    construction_owners[4]=targets._ownership.take();
    m.phase(Phase::Construct);
    if let Some(arena)=m.representation_arena(){
        let controls=3*(std::mem::size_of::<ExactRef>()+64+128+71)
            +8*std::mem::size_of::<String>()+3*71
            +14*std::mem::size_of::<(&str,&Value)>()
            +5*std::mem::size_of::<&Value>();
        construction_owners[1]=Some(arena.reserve_external_payload(controls,m)?);
        m.write_bytes(controls)?;
    }
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
        if m.representation_arena().is_some(){
            for field in [&head.kind,&head.id,&head.digest]{m.scan_bytes(field.len())?;m.write_bytes(field.len())?;}
        }
        prior = Some(head.clone());
    }
    let links_root=if let Some(arena)=m.representation_arena(){
        use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
        let count=state.links.len();let per=std::mem::size_of::<&ExactRef>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
        let bytes=count.checked_mul(per).ok_or_else(||shape::fail("work link view bound"))?;
        let bytes=w::usize_add(m,bytes,std::mem::size_of::<Vec<&ExactRef>>())?;
        let _link_views=arena.reserve_external_payload(bytes,m)?;m.write_bytes(bytes)?;
        let mut selected=Vec::new();selected.try_reserve_exact(count).map_err(|_|shape::fail("work link view allocation"))?;
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        for reference in &state.links{
            m.visit("work.existing_link")?;
            let link=r.get(m,&c.key,reference,"workflow-link","ledger-workflow-link/1")?;
            shape::closed(m,&link,&["schema","scope","child","product_ref","binding_ref","slot","predecessor","classification","authority_ref","operation_key"],&[])?;
            if !shape::same(m,&link["scope"],&key.value()["scope"])?{return Err(shape::fail("work retained link scope"));}
            selected.push(reference);
        }
        let retained=super::app_cost::reference_set(m,&arena,&selected)?;
        let digest=records::hash(m,"workflow-link-set",retained.value())?;
        if !w::compare(m,count,0usize)?.is_eq(){
            if let Some(reference)=&state.chain_head{
                let prior=r.get(m,&c.key,reference,"workflow-chain-state","ledger-workflow-chain-state/1")?;
                let prior_root=shape::text(m,&prior,"links_root")?;
                if !shape::same_text(m,&digest,prior_root)?{return Err(shape::fail("work prior link root"));}
            }else{return Err(shape::fail("retained links without chain"));}
        }
        digest
    }else{
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
        links_root
    };
    let chain=if let Some(arena)=m.representation_arena(){
        let key=super::cost_postings::operation_key(m,&arena,c)?;
        let revision=super::cost_postings::scalar(m,&arena,&revision)?;
        let admitted=super::cost_postings::scalar(m,&arena,&admitted)?;
        let links=arena.string_value(&links_root,m)?;
        let accepted=arena.string_value(&c.accepted_at,m)?;
        let agreement=super::working_value::owning_reference(m,&arena,&c.authorities[0].accepted_agreement_ref)?;
        let activation=super::working_value::owning_reference(m,&arena,&c.authorities[0].activation_ref)?;
        let schema=arena.string_value("ledger-workflow-chain-state/1",m)?;
        let seed=arena.array_from_values(&[&key.value()["logical_store_id"],&key.value()["scope"],
            &p["customer"],&p["chain"],revision.value()],m)?;
        let chain_id=body::semantic_id(m,"wchain_","workflow-chain-state",seed.value())?;
        let value=arena.object_from_fields(&[
            ("schema",schema.value()),("logical_store_id",&key.value()["logical_store_id"]),
            ("scope",&key.value()["scope"]),("customer",&p["customer"]),("chain",&p["chain"]),
            ("binding_ref",&p["binding_ref"]),("accepted_agreement_ref",agreement.value()),
            ("activation_ref",activation.value()),("revision",revision.value()),
            ("admitted_events",admitted.value()),("links_root",links.value()),
            ("operation_key",key.value()),("accepted_at",accepted.value()),
        ],m)?;
        let value=if let Some(reference)=&prior{
            let reference=super::working_value::owning_reference(m,&arena,reference)?;
            super::working_value::WorkingValue::Owned(value).with_fields(m,&[("prior_ref",reference.value())])?.into_owned()?
        }else{value};
        body::record_borrowed(m,&c.key,"workflow-chain-state",chain_id,value.value())?
    }else{
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
        chain
    };
    let proposed_heads = ProposedHeads {
        decision: vec![(decision_id, "1".into(), decision.reference.clone())],
        family: targets.heads,
        chain: Some(chain.reference.clone()),
    };
    let mut financial_reservations=None;
    let original_reservations=if let Some(arena)=m.representation_arena() {
        if program.uses_local_execution_recipe(){financial_reservations=Some(super::capacity::original::derive_financial(&arena,m,&mut r,c,program,&p,&decision,&targets.records,&work,&claim,&a.terms)?);None}else{
        Some(super::capacity::original::derive(&arena,m,&mut r,c,program,&p,&decision,&targets.records,&work,&claim,&a.terms)?)
        }
    } else {None};
    let envelopes=if let Some(arena)=m.representation_arena(){
        use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
        let mut count=w::usize_add(m,2,a.introduced.len())?;
        count=w::usize_add(m,count,actions.len())?;count=w::usize_add(m,count,targets.records.len())?;
        count=w::usize_add(m,count,4)?; //authority, evaluation, decision, chain
        let per=std::mem::size_of::<ConstructedEnvelope>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
        let bytes=count.checked_mul(per).ok_or_else(||shape::fail("work result vector bound"))?;
        let bytes=w::usize_add(m,bytes,std::mem::size_of::<Vec<ConstructedEnvelope>>())?;
        construction_owners[3]=Some(arena.reserve_external_payload(bytes,m)?);m.write_bytes(bytes)?;
        // Ref payload of this one original claim clone is admitted by [0].
        for field in [&claim.reference.kind,&claim.reference.id,&claim.reference.digest]{m.scan_bytes(field.len())?;m.write_bytes(field.len())?;}
        let mut envelopes=Vec::new();envelopes.try_reserve_exact(count).map_err(|_|shape::fail("work result vector allocation"))?;
        envelopes.push(work);envelopes.push(claim.clone());envelopes.extend(a.introduced);
        envelopes.push(a.authority);envelopes.extend(actions);envelopes.push(evaluation);envelopes.push(decision);
        envelopes.extend(targets.records);envelopes.push(chain);envelopes
    }else{
    if m.representation_arena().is_some(){
        // The retained claim and the output clone coexist through original
        // reservation derivation; the initial-row lease owns both Ref graphs.
        for field in [&claim.reference.kind,&claim.reference.id,&claim.reference.digest]{
            m.scan_bytes(field.len())?;m.write_bytes(field.len())?;
        }
    }
    let mut envelopes = vec![work, claim.clone()];
    envelopes.extend(a.introduced);
    envelopes.push(a.authority);
    envelopes.extend(actions);
    envelopes.push(evaluation);
    envelopes.push(decision);
    envelopes.extend(targets.records);
    envelopes.push(chain);
        envelopes
    };
    Ok(Construction {
        original_reservations,financial_reservations,
        economic: EconomicConstruction {
            terms: a.terms,
            original_work: OriginalWorkPayload::New(p),
        },
        normalized_payload,
        new_envelopes: envelopes,reused_result:None,
        evaluation_steps: steps,
        proposed_heads,
        claim_introductions: vec![claim.reference],
        link_introductions: vec![],
        prospective_obligations: targets.obligations,
        records: r,
        history_edges: vec![],
        app_effects:None,app_causal:None,current_capacity:None,
        candidate_heads:None,candidate_introductions:None,financial_observation:None,funding_effects:None,control_owners:construction_owners,
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
        execute(
            program,
            c,
            payload,
            SuppliedRecords::Legacy(inputs),
            state,
            m,
            None,
        )?
        .draft_projection(m)
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
