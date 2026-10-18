//! Customer-local positive-obligation bounds for ONE original work. This is
//! neither a cash pool nor a payable/pricing engine. Exact original pricing is
//! reused; discounts cannot buy contingent premium headroom. A closed family
//! cannot coexist with replacement postings after a root correction.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, arithmetic,
    authority::Terms,
    data::ExactRef,
    inventory::entry,
    meter, pricing, shape, targets,
    working_value::{NormalizedInput, WorkingValue},
};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde_json::{Value, json};
pub(super) struct RootExposureBound {
    pub root_key: String,
    pub original_positive: BigInt,
    pub original_net: BigInt,
    pub simultaneous_family_premium: BigInt,
    pub corrected_positive: BigInt,
    pub corrected_net: BigInt,
    pub positive_bound: BigInt,
}
pub(super) struct WorkExposureBound {
    pub work_claim_ref: ExactRef,
    pub binding_slot: String,
    pub roots: Vec<RootExposureBound>,
    pub maximum_positive: BigInt,
    // Owns the returned root vector and copied Ref/name payload. BigInt
    // arithmetic scratch remains part of the composed interpreter ledger.
    _metadata: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
pub(super) struct OwnedFundingEffects {
    pub effects:Vec<super::candidate::CustomerLocalFundingEffect>,
    _vector:crate::canonical::audit_arena::ExternalBufferLease,
}
/// Execute the initial-work funding obligation inside the SAME construction,
/// before it can become a complete candidate. Original pricing is reused;
/// evidence/roles/ceilings come from the actual resolved accepted terms.
pub(super) fn initial_construction_effect(m:&mut CoreMeter,p:&SelectedProgram,t:&super::data::Construction)->Result<Option<OwnedFundingEffects>>{
    let Some(original)=&t.original_reservations else{return Ok(None)};
    let arena=m.representation_arena().ok_or_else(||fail("owning original-work funding"))?;
    let mut found=None;
    for claim in &t.claim_introductions{m.visit("funding.initial_constructed_claim")?;if shape::same_text(m,&claim.kind,"workflow-work-claim")?{if found.is_some(){return Err(fail("multiple original-work funding claims"));}found=Some(claim);}}
    let claim=found.ok_or_else(||fail("original-work funding claim missing"))?;
    let payload=t.economic.original_work.payload();
    let expected=super::working_value::owning_reference(m,&arena,&original.original_binding_ref)?;shape::equal(m,&payload["binding_ref"],expected.value())?;
    let bound=derive_per_work_bound(m,p,payload,claim,&t.economic.terms)?;
    let effect=initial_work_effect(&arena,m,&original.original_binding_ref,&t.economic.terms,&bound)?;
    let vector=arena.reserve_external_payload(std::mem::size_of::<super::candidate::CustomerLocalFundingEffect>()+std::mem::size_of::<OwnedFundingEffects>(),m)?;
    let mut effects=Vec::new();effects.try_reserve_exact(1).map_err(|_|fail("initial-work funding effect vector"))?;effects.push(effect);
    Ok(Some(OwnedFundingEffects{effects,_vector:vector}))
}
fn selected_body(m:&mut CoreMeter,r:&mut super::records::Records,c:&super::data::DraftContextData,output:&[super::emitted_record::ConstructedEnvelope],reference:&ExactRef,kind:&str,schemas:&[&str])->Result<super::record_body::RecordBody>{
    let mut found=None;for row in output{m.visit("funding.actual_result_locator")?;if shape::same_ref(m,&row.reference,reference)?{if found.is_some(){return Err(fail("duplicate funding result Ref"));}found=Some(row);}}
    if let Some(row)=found{
        if !shape::same_text(m,&reference.kind,kind)?{return Err(fail("funding result kind"));}
        let value=row.decoded(m,4194304)?;let schema=shape::text(m,&value,"schema")?;let mut selected=false;for expected in schemas{if shape::same_text(m,schema,expected)?{selected=true;}}
        if !selected{return Err(fail("funding result schema"));}Ok(value)
    }else{r.get_schemas(m,&c.key,reference,kind,schemas)}
}
fn active_work_total(m:&mut CoreMeter,r:&mut super::records::Records,c:&super::data::DraftContextData,output:&[super::emitted_record::ConstructedEnvelope],decision:&ExactRef,work:&ExactRef,binding:&ExactRef)->Result<BigInt>{
    use super::working_value::parse_owned_reference;
    let arena=m.representation_arena().ok_or_else(||fail("owning funding current actions"))?;
    let value=selected_body(m,r,c,output,decision,"workflow-decision",&["ledger-workflow-decision-revision/1","ledger-workflow-decision-revision/3"])?;
    let subject=parse_owned_reference(m,&arena,&value["subject_ref"])?;let subject=subject.reference();
    let related=if shape::same_text(m,&subject.kind,"workflow-work")?{shape::same_ref(m,subject,work)?}
    else if shape::same_text(m,&subject.kind,"workflow-target")?{
        let target=selected_body(m,r,c,output,subject,"workflow-target",&["ledger-workflow-component-target/1"])?;
        let original=parse_owned_reference(m,&arena,&target["work_ref"])?;shape::same_ref(m,original.reference(),work)?
    }else if shape::same_text(m,&subject.kind,"workflow-cost")?{
        let claim=selected_body(m,r,c,output,subject,"workflow-cost",&["ledger-workflow-app-cost-claim/1"])?;
        let original=parse_owned_reference(m,&arena,&claim["subject"]["original_work_ref"])?;shape::same_ref(m,original.reference(),work)?
    }else{return Err(fail("unsupported app funding decision subject"));};
    if !related{return Ok(0.into());}
    let original=parse_owned_reference(m,&arena,&value["original_terms_ref"])?;if !shape::same_ref(m,original.reference(),binding)?{return Err(fail("same-work funding original Binding association"));}
    let actions=shape::array(m,&value,"active_action_refs")?;if crate::validation_work::compare(m,actions.len(),128usize)?.is_gt(){return Err(fail("funding active action count"));}
    let mut total=BigInt::from(0);
    for(i,action)in actions.iter().enumerate(){
        m.visit("funding.actual_active_action")?;for previous in &actions[..i]{m.visit("funding.active_action_unique")?;if shape::same(m,previous,action)?{return Err(fail("funding duplicate active action"));}}
        let reference=parse_owned_reference(m,&arena,action)?;let action=selected_body(m,r,c,output,reference.reference(),"workflow-action",&["ledger-workflow-action/1"])?;
        shape::equal(m,&action["decision_id"],&value["decision_id"])?;
        let role=shape::text(m,&action,"posting_role")?;if !shape::same_text(m,role,"original")?&&!shape::same_text(m,role,"replacement")?{return Err(fail("funding active inverse forbidden"));}
        let book=shape::text(m,&action,"book")?;if !shape::same_text(m,book,"retail")?{return Err(fail("app funding cannot include AP action"));}
        let currency=shape::text(m,&action["amount"],"currency")?;if !shape::same_text(m,currency,"USD")?||!shape::same(m,&action["amount"]["scale"],&json!(18))?{return Err(fail("funding current Amount units"));}
        let atom_text=shape::text(m,&action["amount"],"atoms")?;
        let atoms=arithmetic::integer(m,atom_text,true)?;total=meter::add(m,&total,&atoms)?;
    }Ok(total)
}
/// Update the same per-original-work projection using actual effective heads:
/// newly produced decisions replace their exact old heads; independent work
/// rows do not spend this work's exposure. Complete head/absence provenance is
/// still the caller's held-prefix and final role/capacity proof obligation.
pub(super) fn construction_effect(m:&mut CoreMeter,p:&SelectedProgram,c:&super::data::DraftContextData,state:&super::data::EconomicStateData,t:&mut super::data::Construction)->Result<OwnedFundingEffects>{
    if m.check("funding.original_work_introduction",||matches!(&t.economic.original_work,super::data::OriginalWorkPayload::New(_)))?{return initial_construction_effect(m,p,t)?.ok_or_else(||fail("initial work funding effect missing"));}
    let work=match &t.economic.original_work{super::data::OriginalWorkPayload::Retained(work)=>work.clone(),_=>return Err(fail("retained funding work selection"))};
    let arena=m.representation_arena().ok_or_else(||fail("owning per-work funding update"))?;
    let _references=arena.reserve_external_payload(2*(std::mem::size_of::<ExactRef>()+64+128+71),m)?;
    let claim_id=shape::text(m,&work,"claim_id")?;let(claim_ref,claim)=t.records.by_identity(m,&c.key,"workflow-work-claim",claim_id,"ledger-workflow-work-claim/1")?;
    super::outcome::original_claim_association(m,&claim,&work)?;
    let work_ref=super::working_value::parse_owned_reference(m,&arena,&claim["work_ref"])?;
    let binding_ref=super::working_value::parse_owned_reference(m,&arena,&work["payload"]["binding_ref"])?;
    let bound=derive_per_work_bound(m,p,&work["payload"],&claim_ref,&t.economic.terms)?;
    let root_seed=arena.string_value(&claim_ref.id,m)?;let discriminator=arena.string_value("work",m)?;let root_tuple=arena.array_from_values(&[root_seed.value(),discriminator.value()],m)?;
    let root_id=super::body::semantic_id(m,"wdecision_","workflow-decision",root_tuple.value())?;
    let mut root_seen=false;let mut total=BigInt::from(0);
    for head in &state.financial.decision_heads{
        m.visit("funding.held_work_decision")?;let mut selected=&head.reference;
        for(id,_,reference)in &t.proposed_heads.decision{m.visit("funding.replaced_decision_head")?;if shape::same_text(m,id,&head.decision_id)?{selected=reference;}}
        if shape::same_text(m,&head.decision_id,&root_id)?{if root_seen{return Err(fail("funding duplicate original work root head"));}root_seen=true;}
        let amount=active_work_total(m,&mut t.records,c,&t.new_envelopes,selected,work_ref.reference(),binding_ref.reference())?;total=meter::add(m,&total,&amount)?;
    }
    if !root_seen{return Err(fail("funding original work current root head missing"));}
    for(id,_,reference)in &t.proposed_heads.decision{
        m.visit("funding.new_work_decision")?;let mut existed=false;for head in &state.financial.decision_heads{if shape::same_text(m,id,&head.decision_id)?{existed=true;}}
        if !existed{let amount=active_work_total(m,&mut t.records,c,&t.new_envelopes,reference,work_ref.reference(),binding_ref.reference())?;total=meter::add(m,&total,&amount)?;}
    }
    let effect=work_effect(&arena,m,binding_ref.reference(),&t.economic.terms,&bound,Some(&total))?;
    let vector=arena.reserve_external_payload(std::mem::size_of::<super::candidate::CustomerLocalFundingEffect>()+std::mem::size_of::<OwnedFundingEffects>(),m)?;
    let mut effects=Vec::new();effects.try_reserve_exact(1).map_err(|_|fail("per-work funding effect vector"))?;effects.push(effect);
    Ok(OwnedFundingEffects{effects,_vector:vector})
}
fn fail(s: &str) -> Error {
    shape::fail(s)
}
fn same(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn schema<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    r: &Value,
) -> Result<&'a crate::policy::workflow::FactSchema> {
    for (reference, schema) in p.fact_schemas() {
        m.visit("funding.original_fact_schema")?;
        if shape::same(m, &json!(reference), r)? {
            return Ok(schema);
        }
    }
    Err(fail("funding original schema missing"))
}
/// Derive a maximum permitted normalized integer vector from original schema
/// AND both accepted quantity ceilings. It is an abstract prospective operand,
/// not a new assertion, claim, proof, request, or substituted billed quantity.
fn maximum_facts(
    m: &mut CoreMeter,
    p: &SelectedProgram,
    payload: &Value,
    t: &Terms,
) -> Result<WorkingValue> {
    let activity = shape::text(m, payload, "activity")?;
    let original_schema = schema(m, p, &payload["fact_schema_ref"])?;
    let mut facts = WorkingValue::copy(m, &payload["facts"])?;
    let instructions = shape::array(m, p.compiled_plan().canonical_value(), "typed_instructions")?;
    // Borrow names from the immutable compiled plan. Retain a pre-admitted
    // exact vector while building the owning prospective facts; no name clone
    // and no naked normalized Value escapes this function on RP01.
    let metadata = if let Some(arena) = m.representation_arena() {
        use crate::validation_work::{ArithmeticOperation as Op, ValidationWork};
        ValidationWork::arithmetic(
            m,
            Op::Multiply,
            &[
                &instructions.len().into(),
                &std::mem::size_of::<&str>().into(),
            ],
        )?;
        let bytes = instructions
            .len()
            .checked_mul(std::mem::size_of::<&str>())
            .ok_or_else(|| fail("prospective fact name allocation"))?;
        Some(arena.reserve_external_payload(bytes, m)?)
    } else {
        None
    };
    let mut fields: Vec<&str> = vec![];
    if metadata.is_some() {
        fields
            .try_reserve_exact(instructions.len())
            .map_err(|_| fail("prospective fact name allocation"))?;
    }
    for instruction in instructions {
        m.visit("funding.original_quantity_producer")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_rule")? {
            continue;
        }
        let rule = &instruction["rule"];
        let declared_activity = shape::text(m, rule, "activity")?;
        if !shape::same_text(m, declared_activity, activity)? {
            continue;
        }
        let formula = &rule["formula"];
        let kind = shape::text(m, formula, "kind")?;
        if !shape::same_text(m, kind, "unit")? {
            continue;
        }
        let field = shape::text(m, formula, "quantity_field")?;
        let mut seen = false;
        for prior in &fields {
            if shape::same_text(m, prior, field)? {
                seen = true;
                break;
            }
        }
        if seen {
            continue;
        }
        fields.push(field);
        let declarations = shape::array(m, original_schema.canonical_value(), "fields")?;
        let declaration = shape::named(m, declarations, "name", field)?;
        same(m, &declaration["required"], &json!(true))?;
        same(m, &declaration["type"]["kind"], &json!("integer"))?;
        let raw = shape::text(m, &declaration["type"], "max")?;
        let mut maximum = arithmetic::integer(m, raw, true)?;
        for limits in [
            &t.binding["quantity_ceilings"],
            &t.slot_terms["quantity_ceilings"],
        ] {
            for bound in limits
                .as_array()
                .ok_or_else(|| fail("funding quantity ceiling array"))?
            {
                m.visit("funding.accepted_quantity_ceiling")?;
                let name = shape::text(m, bound, "activity")?;
                let quantity = shape::text(m, bound, "field")?;
                if !shape::same_text(m, name, activity)? || !shape::same_text(m, quantity, field)? {
                    continue;
                }
                same(m, &bound["unit"], &declaration["type"]["unit"])?;
                let raw = shape::text(m, bound, "maximum")?;
                let bound = arithmetic::decimal(m, raw)?;
                if !meter::cmp(m, &bound.numerator, &0.into())?.is_gt() {
                    return Err(fail("nonpositive future quantity ceiling"));
                }
                let integral = meter::div(m, &bound.numerator, &bound.denominator)?;
                if meter::cmp(m, &integral, &maximum)?.is_lt() {
                    maximum = integral;
                }
            }
        }
        let raw_min = shape::text(m, &declaration["type"], "min")?;
        let minimum = arithmetic::integer(m, raw_min, true)?;
        if !meter::cmp(m, &maximum, &0.into())?.is_gt()
            || meter::cmp(m, &maximum, &minimum)?.is_lt()
        {
            return Err(fail("no permitted positive future quantity"));
        }
        if let Some(arena) = m.representation_arena() {
            let scalar = super::cost_postings::scalar(m, &arena, &maximum)?;
            facts.replace(m, field, scalar.value())?;
        } else {
            facts.replace(m, field, &json!(maximum.to_string()))?;
        }
    }
    let normalized = NormalizedInput::normalize(m, original_schema, &facts)?;
    entry::quantities(
        m,
        &t.binding,
        &t.slot_terms,
        activity,
        normalized.canonical_value(),
    )?;
    WorkingValue::copy(m, normalized.canonical_value())
}
fn root_position(
    m: &mut CoreMeter,
    roots: &[RootExposureBound],
    key: &str,
) -> Result<Option<usize>> {
    for (i, root) in roots.iter().enumerate() {
        m.visit("funding.original_root")?;
        if shape::same_text(m, &root.root_key, key)? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}
/// The caller must already resolve the exact accepted terms/program/work and
/// validate the additive exposure declaration. This result is pure derived DATA
/// and has no constructor for a store balance or host authorization capability.
pub(super) fn derive_per_work_bound(
    m: &mut CoreMeter,
    p: &SelectedProgram,
    payload: &Value,
    claim: &ExactRef,
    t: &Terms,
) -> Result<WorkExposureBound> {
    let schema=if p.uses_app_cost_causal_recipe(){"ledger-workflow-slot-binding-terms/7"}else if p.uses_app_owned_cost_recipe(){"ledger-workflow-slot-binding-terms/6"}else{"ledger-workflow-slot-binding-terms/3"};
    let actual=shape::text(m,&t.slot_terms,"schema")?;
    if !shape::same_text(m,actual,schema)?{return Err(fail("original exposure terms recipe"));}
    same(m, &t.slot_terms["exposure_scope"], &json!("original_work"))?;
    same(m, &t.slot_terms["book"], &json!("retail"))?;
    let activity = shape::text(m, payload, "activity")?;
    let slot = shape::text(m, &t.slot_terms, "slot")?;
    let instructions = shape::array(m, p.compiled_plan().canonical_value(), "typed_instructions")?;
    // Bound only the returned metadata here. Pricing and 512-bit arithmetic
    // have their own actual work/scratch obligations in the complete proof.
    let metadata = if let Some(arena) = m.representation_arena() {
        use crate::validation_work::{self as w, ArithmeticOperation as Op, ValidationWork};
        ValidationWork::arithmetic(
            m,
            Op::Multiply,
            &[
                &instructions.len().into(),
                &std::mem::size_of::<RootExposureBound>().into(),
            ],
        )?;
        let mut bytes = instructions
            .len()
            .checked_mul(std::mem::size_of::<RootExposureBound>())
            .ok_or_else(|| fail("funding root metadata allocation"))?;
        bytes = w::usize_add(m, bytes, std::mem::size_of::<WorkExposureBound>())?;
        for value in [&claim.kind, &claim.id, &claim.digest] {
            bytes = w::usize_add(m, bytes, value.len())?;
        }
        bytes = w::usize_add(m, bytes, slot.len())?;
        for instruction in instructions {
            m.visit("funding.root_metadata_dimension")?;
            let opcode = shape::text(m, instruction, "opcode")?;
            if !shape::same_text(m, opcode, "evaluate_rule")? {
                continue;
            }
            let key = shape::text(m, &instruction["rule"], "component_key")?;
            // root_key is either this original producer or its named basis;
            // count the actual compiled component root rather than a constant.
            let components =
                shape::array(m, p.compiled_plan().canonical_value(), "component_slots")?;
            let component = shape::named(m, components, "key", key)?;
            let root = shape::text(m, component, "root_key")?;
            bytes = w::usize_add(m, bytes, root.len())?;
        }
        Some(arena.reserve_external_payload(bytes, m)?)
    } else {
        None
    };
    if metadata.is_some() {
        m.scan_bytes(slot.len())?;
        m.write_bytes(slot.len())?;
    }
    let binding_slot = slot.to_owned();
    let original = pricing::work(p, activity, &payload["facts"], m)?;
    let maximum = maximum_facts(m, p, payload, t)?;
    let corrected = pricing::work(p, activity, &maximum, m)?;
    let mut roots: Vec<RootExposureBound> = vec![];
    if metadata.is_some() {
        roots
            .try_reserve_exact(instructions.len())
            .map_err(|_| fail("funding root metadata allocation"))?;
    }
    for price in original {
        m.visit("funding.original_component")?;
        if !shape::same_text(m, &price.binding_slot, &binding_slot)? {
            return Err(fail("funding component binding slot"));
        }
        let index = match root_position(m, &roots, &price.root_key)? {
            Some(i) => i,
            None => {
                let index = roots.len();
                if metadata.is_some() {
                    m.scan_bytes(price.root_key.len())?;
                    m.write_bytes(price.root_key.len())?;
                }
                roots.push(RootExposureBound {
                    root_key: price.root_key.clone(),
                    original_positive: 0.into(),
                    original_net: 0.into(),
                    simultaneous_family_premium: 0.into(),
                    corrected_positive: 0.into(),
                    corrected_net: 0.into(),
                    positive_bound: 0.into(),
                });
                index
            }
        };
        let atoms = BigInt::from(price.atoms);
        roots[index].original_net = meter::add(m, &roots[index].original_net, &atoms)?;
        if meter::cmp(m, &atoms, &0.into())?.is_gt() {
            roots[index].original_positive =
                meter::add(m, &roots[index].original_positive, &atoms)?;
        }
    }
    for price in corrected {
        m.visit("funding.corrected_component")?;
        let index = root_position(m, &roots, &price.root_key)?
            .ok_or_else(|| fail("future price introduced an undeclared root"))?;
        let atoms = BigInt::from(price.atoms);
        roots[index].corrected_net = meter::add(m, &roots[index].corrected_net, &atoms)?;
        if meter::cmp(m, &atoms, &0.into())?.is_gt() {
            roots[index].corrected_positive =
                meter::add(m, &roots[index].corrected_positive, &atoms)?;
        }
    }
    if p.uses_app_owned_cost_recipe(){
        for declaration in shape::array(m,p.original_policy_value(),"app_delayed_costs")?{
            m.visit("funding.original_app_cost_maximum")?;
            if !shape::same(m,&declaration["activity"],&payload["activity"])?{continue;}
            let key=shape::text(m,declaration,"component_key")?;
            let accepted=shape::array(m,&t.slot_terms,"app_cost_terms")?;
            let mut found=false;
            for row in accepted{
                m.visit("funding.original_app_maximum_assent")?;
                if shape::same(m,&row["component_key"],&declaration["component_key"])?&&shape::same(m,&row["activity"],&declaration["activity"])?{
                    if found{return Err(fail("duplicate accepted app maximum"));}
                    shape::equal(m,row,declaration)?;found=true;
                }
            }
            if !found{return Err(fail("app maximum not covered by original terms"));}
            let raw=shape::text(m,&declaration["maximum_final_amount"],"atoms")?;
            let maximum=arithmetic::integer(m,raw,false)?;
            let prices=pricing::app_final_cost(p,activity,key,&payload["facts"],&maximum,m)?;
            if !crate::validation_work::compare(m,prices.len(),1usize)?.is_eq(){return Err(fail("app prospective producer cardinality"));}
            for price in prices{
                if !shape::same_text(m,&price.binding_slot,&binding_slot)?{return Err(fail("app maximum customer binding association"));}
                if root_position(m,&roots,&price.root_key)?.is_some(){return Err(fail("app cost cannot alias an already booked root"));}
                if meter::cmp(m,&price.atoms.into(),&0.into())?.is_lt(){return Err(fail("negative prospective app obligation"));}
                m.scan_bytes(price.root_key.len())?;m.write_bytes(price.root_key.len())?;
                roots.push(RootExposureBound{root_key:price.root_key,original_positive:0.into(),original_net:0.into(),simultaneous_family_premium:0.into(),corrected_positive:price.atoms.into(),corrected_net:price.atoms.into(),positive_bound:0.into()});
            }
        }
    }
    for instruction in shape::array(m, p.compiled_plan().canonical_value(), "typed_instructions")? {
        m.visit("funding.original_family_alternatives")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_family")? {
            continue;
        }
        let family = &instruction["family"];
        same(m, &family["selector"]["binding_slot"], &json!(binding_slot))?;
        if !shape::same(
            m,
            &family["selector"]["target_activity"],
            &payload["activity"],
        )? {
            continue;
        }
        let key = shape::text(m, &family["selector"], "component_key")?;
        let index = root_position(m, &roots, key)?
            .ok_or_else(|| fail("family positive reserve lacks original root"))?;
        if meter::cmp(m, &roots[index].original_net, &0.into())?.is_lt() {
            return Err(fail("original named root has excessive aggregate discount"));
        }
        // MAX over exclusive codes happens in the existing production helper;
        // SUM over distinct families happens here. Discounts never offset this.
        let component_map = shape::array(m, &t.instantiation, "component_map")?;
        let component = shape::named(m, component_map, "template_key", key)?;
        let families = shape::array(m, &t.instantiation, "family_map")?;
        let family_key = shape::text(m, family, "family_key")?;
        let mapped = shape::named(m, families, "template_key", family_key)?;
        let mut found = false;
        for clause in shape::array(m, &t.slot_terms, "causal_reversal_clauses")? {
            m.visit("funding.future_permanent_closure_clause")?;
            if shape::same(m, &clause["root_component_slot"], &component["slot_id"])?
                && shape::same(m, &clause["family_slot"], &mapped["slot_id"])?
            {
                if found {
                    return Err(fail("duplicate exposure causal clause"));
                }
                same(
                    m,
                    &clause["correction_source_slot"],
                    &family["correction_source_slot"],
                )?;
                same(
                    m,
                    &clause["mode"],
                    &json!("exact_inverse_and_permanent_close"),
                )?;
                same(
                    m,
                    &clause["standalone_deadline_effect"],
                    &json!("does_not_expire_preaccepted_causal_reversal"),
                )?;
                found = true;
            }
        }
        if !found {
            return Err(fail(
                "exposure branch requires original permanent closure clause",
            ));
        }
        let (premium, _discount) = targets::maxima(m, family, &roots[index].original_net)?;
        roots[index].simultaneous_family_premium =
            meter::add(m, &roots[index].simultaneous_family_premium, &premium)?;
    }
    let mut total = BigInt::from(0);
    for root in &mut roots {
        m.visit("funding.simultaneously_permitted_branch")?;
        if meter::cmp(m, &root.original_net, &0.into())?.is_lt()
            || meter::cmp(m, &root.corrected_net, &0.into())?.is_lt()
        {
            return Err(fail("aggregate original modifiers exceed their named root"));
        }
        let original = meter::add(
            m,
            &root.original_positive,
            &root.simultaneous_family_premium,
        )?;
        root.positive_bound = if meter::cmp(m, &original, &root.corrected_positive)?.is_gt() {
            original
        } else {
            root.corrected_positive.clone()
        };
        total = meter::add(m, &total, &root.positive_bound)?;
    }
    entry::exposure(m, &t.binding, &t.slot_terms, &total)?;
    if metadata.is_some() {
        for value in [&claim.kind, &claim.id, &claim.digest] {
            m.scan_bytes(value.len())?;
            m.write_bytes(value.len())?;
        }
    }
    Ok(WorkExposureBound {
        work_claim_ref: claim.clone(),
        binding_slot,
        roots,
        maximum_positive: total,
        _metadata: metadata,
    })
}
/// Construct the immutable customer-local effect from the exact resolved terms
/// and the derived per-work bound. This is not a balance, liability reset or
/// candidate success. Contextual evidence was verified by authority::resolve;
/// the complete factory must retain that relationship and all source rights.
pub(super) fn initial_work_effect(
    arena: &crate::canonical::audit_arena::AuditArena,
    m: &mut CoreMeter,
    binding_ref: &ExactRef,
    t: &Terms,
    bound: &WorkExposureBound,
) -> Result<super::candidate::CustomerLocalFundingEffect> {
    work_effect(arena,m,binding_ref,t,bound,None)
}
fn work_effect(
    arena:&crate::canonical::audit_arena::AuditArena,m:&mut CoreMeter,binding_ref:&ExactRef,
    t:&Terms,bound:&WorkExposureBound,current_after:Option<&BigInt>,
) -> Result<super::candidate::CustomerLocalFundingEffect> {
    use crate::validation_work::{self as w, ArithmeticOperation as Op, ValidationWork};
    use num_traits::ToPrimitive;
    use std::mem;
    fn add(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
        w::usize_add(m, a, b)
    }
    fn size(m: &mut CoreMeter, r: &ExactRef) -> Result<usize> {
        let n = add(m, r.kind.len(), r.id.len())?;
        add(m, n, r.digest.len())
    }
    fn text(m: &mut CoreMeter, v: &str) -> Result<String> {
        m.scan_bytes(v.len())?;
        m.write_bytes(v.len())?;
        Ok(v.to_owned())
    }
    fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
        Ok(ExactRef {
            kind: text(m, &r.kind)?,
            id: text(m, &r.id)?,
            digest: text(m, &r.digest)?,
        })
    }
    let leg_name = shape::text(m, &t.slot_terms, "funding_leg")?;
    let funding = shape::array(m, &t.binding, "funding_legs")?;
    let leg = shape::named(m, funding, "name", leg_name)?;
    let mode = shape::text(m, leg, "mode")?;
    if !shape::same_text(m, mode, "customer_local")? {
        return Err(fail("customer-local funding effect mode"));
    }
    let evidence = shape::array(m, leg, "evidence_refs")?;
    let ceiling = &t.slot_terms["exposure_ceiling"];
    let ceiling_text = shape::text(m, ceiling, "atoms")?;
    let currency = shape::text(m, ceiling, "currency")?;
    if !shape::same_text(m, currency, "USD")? {
        return Err(fail("funding currency"));
    }
    let mut bytes = mem::size_of::<super::candidate::CustomerLocalFundingEffect>();
    for reference in [
        &bound.work_claim_ref,
        binding_ref,
        &t.slot_terms_ref,
        &t.role_ref,
    ] {
        let n = size(m, reference)?;
        bytes = add(m, bytes, n)?;
    }
    for value in [&bound.binding_slot, leg_name, currency, ceiling_text] {
        bytes = add(m, bytes, value.len())?;
    }
    ValidationWork::arithmetic(
        m,
        Op::Multiply,
        &[&evidence.len().into(), &mem::size_of::<ExactRef>().into()],
    )?;
    let positions = evidence
        .len()
        .checked_mul(mem::size_of::<ExactRef>())
        .ok_or_else(|| fail("funding evidence allocation"))?;
    bytes = add(m, bytes, positions)?;
    for reference in evidence {
        m.visit("funding.effect_evidence_size")?;
        for field in ["kind", "id", "digest"] {
            let s = shape::text(m, reference, field)?;
            bytes = add(m, bytes, s.len())?;
        }
    }
    // Each exact Amount is <=30 coefficient digits after accepted ceilings.
    // unsigned_string owns at most a32-byte digit Vec plus a30-byte result.
    // Reserving both strings' entire growth/copy sums before either formatter
    // also covers their simultaneous retained outputs.
    bytes = add(m, bytes, 172)?; // two complete (8+16+32+30) allocation sums
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let mut current = BigInt::from(0);
    if let Some(after)=current_after{m.visit("funding.actual_post_transition_total")?;current=meter::add(m,&current,after)?;}
    else{for root in &bound.roots {
        m.visit("funding.current_original_root")?;
        current = meter::add(m, &current, &root.original_net)?;
    }}
    let original_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, &current, &0.into())?.is_lt()
        || meter::cmp(m, &current, &original_limit)?.is_gt()
        || meter::cmp(m, &bound.maximum_positive, &original_limit)?.is_gt()
    {
        return Err(fail("funding effect amount bound"));
    }
    let current = current
        .to_u128()
        .ok_or_else(|| fail("funding current amount"))?;
    let prospective = bound
        .maximum_positive
        .to_u128()
        .ok_or_else(|| fail("funding future amount"))?;
    let current = w::unsigned_string(current, m)?;
    let prospective = w::unsigned_string(prospective, m)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(evidence.len())
        .map_err(|_| fail("funding evidence allocation"))?;
    for reference in evidence {
        let kind = shape::text(m, reference, "kind")?;
        let id = shape::text(m, reference, "id")?;
        let digest = shape::text(m, reference, "digest")?;
        refs.push(ExactRef {
            kind: text(m, kind)?,
            id: text(m, id)?,
            digest: text(m, digest)?,
        });
    }
    Ok(super::candidate::CustomerLocalFundingEffect::from_derived(
        copy_ref(m, &bound.work_claim_ref)?,
        text(m, &bound.binding_slot)?,
        copy_ref(m, binding_ref)?,
        copy_ref(m, &t.slot_terms_ref)?,
        copy_ref(m, &t.role_ref)?,
        text(m, leg_name)?,
        refs,
        text(m, currency)?,
        18,
        text(m, ceiling_text)?,
        current,
        prospective,
        ownership,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical;
    use crate::policy::workflow::selected_program::{
        SelectedCompilerSupport, SelectedSourceInput, compile_selected_program,
    };
    #[test]
    fn original_work_bound_covers_future_quantity_without_summing_exclusive_or_closed_branches() {
        // Ordinary immutable source inputs, not a saved economic result/oracle.
        let source:Value=canonical::parse(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let inputs = SelectedSourceInput {
            documents: source["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
        };
        let support = SelectedCompilerSupport::builtin().unwrap();
        let mut compile = CoreMeter::for_compile(1_048_576).unwrap();
        let program = compile_selected_program(inputs, &support, &mut compile).unwrap();
        let raw: Value =
            serde_json::from_slice(include_bytes!("fixtures/prerequisite-input-data.json"))
                .unwrap();
        let get = |name: &str| -> (ExactRef, Value) {
            let row = raw["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["name"] == name)
                .unwrap();
            let bytes = row["canonical_body_hex"]
                .as_str()
                .unwrap()
                .as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect::<Vec<_>>();
            (
                serde_json::from_value(row["reference"].clone()).unwrap(),
                serde_json::from_slice(&bytes).unwrap(),
            )
        };
        let (terms_ref, mut terms) = get("A.slot_terms");
        terms["schema"] = json!("ledger-workflow-slot-binding-terms/3");
        terms["exposure_scope"] = json!("original_work");
        let (binding_ref, binding) = get("A.binding");
        let (_, instantiation) = get("A.instantiation");
        let (_, catalog) = get("A.catalog");
        let unit = program.compiled_plan().canonical_value()["typed_instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["rule"]["formula"]["kind"] == "unit")
            .unwrap();
        let field = unit["rule"]["formula"]["quantity_field"].as_str().unwrap();
        let activity = &unit["rule"]["activity"];
        let schema_ref = program
            .source_documents()
            .iter()
            .find(|d| d.canonical_value()["schema"] == "ledger-workflow-product/1")
            .unwrap()
            .canonical_value()["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| &a["name"] == activity)
            .unwrap()["fact_schema"]
            .clone();
        let payload = json!({"activity":activity,"fact_schema_ref":schema_ref,"facts":{field:"3"}});
        let claim = ExactRef {
            kind: "workflow-work-claim".into(),
            id: format!("wclaim_{}", "1".repeat(64)),
            digest: format!("sha256:{}", "2".repeat(64)),
        };
        let mut terms = Terms {
            binding: super::super::record_body::RecordBody::Legacy(binding),
            accepted: super::super::record_body::RecordBody::Legacy(json!({})),
            activation: super::super::record_body::RecordBody::Legacy(json!({})),
            catalog: super::super::record_body::RecordBody::Legacy(catalog),
            instantiation: super::super::record_body::RecordBody::Legacy(instantiation),
            slot_terms: super::super::record_body::RecordBody::Legacy(terms),
            slot_terms_ref: terms_ref,
            role_ref: claim.clone(),
        };
        let mut m = CoreMeter::new(65_536).unwrap();
        let bound = derive_per_work_bound(&mut m, &program, &payload, &claim, &terms).unwrap();
        assert_eq!(
            bound.maximum_positive,
            BigInt::from(37) * BigInt::from(10u8).pow(18)
        );
        assert_eq!(bound.roots.len(), 1);
        assert_eq!(
            bound.roots[0].original_net,
            BigInt::from(999) * BigInt::from(10u8).pow(15)
        );
        assert_eq!(
            bound.roots[0].simultaneous_family_premium,
            BigInt::from(1998) * BigInt::from(10u8).pow(14)
        );
        let arena = crate::canonical::audit_arena::AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let effect = initial_work_effect(&arena, &mut m, &binding_ref, &terms, &bound).unwrap();
        assert_eq!(effect.original_work_claim_ref(), &claim);
        assert_eq!(effect.binding_slot(), bound.binding_slot);
        assert_eq!(effect.current_exposure_atoms(), "999000000000000000");
        assert_eq!(effect.prospective_exposure_atoms(), "37000000000000000000");
        assert!(arena.usage().unwrap().live_bytes > before);
        drop(effect);
        assert_eq!(arena.usage().unwrap().live_bytes, before);
        // Each independent original work has its own aggregate; deriving one
        // bound does not create/spend/reset a shared lifetime balance.
        let mut independent = claim.clone();
        independent.id = format!("wclaim_{}", "3".repeat(64));
        assert_eq!(
            derive_per_work_bound(&mut m, &program, &payload, &independent, &terms)
                .unwrap()
                .maximum_positive,
            bound.maximum_positive
        );
        // The prospective fact projection uses the same normalizer under
        // RP01 and retains its returned metadata through the bound lifetime.
        // This is the per-work exposure unit, not a capacity/admission proof.
        let mut session =
            crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child = session.meter_mut().original_economic_meter().unwrap();
        let owned = derive_per_work_bound(&mut child, &program, &payload, &claim, &terms).unwrap();
        assert_eq!(owned.maximum_positive, bound.maximum_positive);
        assert_eq!(owned.roots[0].original_net, bound.roots[0].original_net);
        assert_eq!(
            owned.roots[0].corrected_positive,
            bound.roots[0].corrected_positive
        );
        assert!(arena.usage().unwrap().live_bytes > before);
        session.meter_mut().finish_original_economic(child).unwrap();
        drop(owned);
        assert_eq!(arena.usage().unwrap().live_bytes, before);
        terms.slot_terms.legacy_mut()["exposure_ceiling"]["atoms"] = json!("1000000000000000000");
        assert!(derive_per_work_bound(&mut m, &program, &payload, &claim, &terms).is_err());
    }
}
