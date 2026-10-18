//! Monetary reservations derived from the ORIGINAL constructor's targets and
//! compiled family alternatives, not submitted dependency/capacity assertions.
//! This is the structural part of the full future proof, not a work/byte proof.
use super::super::{CoreMeter,emitted_record::ConstructedEnvelope,data::ExactRef,meter,shape};
use super::super::super::selected_program::SelectedProgram;
use crate::{Result,canonical::audit_arena::{AuditArena,ExternalBufferLease},validation_work::{self as w,ArithmeticOperation,ValidationWork}};
use serde_json::Value;
use std::mem;
mod retained;
pub(in crate::policy::workflow::transition_constructor) use retained::{derive_retained,derive_for_work};

pub(in crate::policy::workflow::transition_constructor) struct FamilyReservation {
    pub target_ref:ExactRef,
    pub family_slot:String,
    pub dependent_decision_id:String,
    pub maximum_inverse_actions:usize,
}
pub(in crate::policy::workflow::transition_constructor) struct OriginalReservations {
    pub root_decision_id:String,
    pub original_root_ref:ExactRef,
    pub original_binding_ref:ExactRef,
    pub maximum_root_actions:usize,
    pub families:Vec<FamilyReservation>,
    pub delayed_costs:Vec<DelayedCostReservation>,
    pub app_costs:Vec<AppCostReservation>,
    pub future_records:Option<super::future_records::FutureRecords>,
    pub app_body_bytes:Option<super::app_bytes::AppBodyBounds>,
    pub family_body_bytes:Option<super::app_bytes::FamilyBodyBounds>,
    pub root_body_bytes:Option<super::app_bytes::RootBodyBounds>,
    pub source_bounds:super::source::OriginalSourceBounds,
    pub pricing_kernels:Option<super::pricing::PricingKernels>,
    _metadata:ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) struct DelayedCostReservation {
    pub target_ref:ExactRef,
    pub component_slot:String,
    pub supplier_binding_slot:String,
    pub dependent_decision_id:String,
    pub maximum_inverse_actions:usize,
}
pub(in crate::policy::workflow::transition_constructor) struct AppCostReservation {
 pub target_ref:ExactRef,pub component_slot:String,pub dependent_decision_id:String,
 pub maximum_inverse_actions:usize,
}
fn add(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{w::usize_add(m,a,b)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
    a.checked_mul(b).ok_or_else(||shape::fail("original reservation allocation product"))
}
fn bound(m:&mut CoreMeter,n:usize,minimum:usize,maximum:usize)->Result<()> {
    if w::compare(m,n,minimum)?.is_lt() || w::compare(m,n,maximum)?.is_gt(){return Err(shape::fail("original reservation identity bound"));}Ok(())
}
fn copy(m:&mut CoreMeter,s:&str)->Result<String>{m.scan_bytes(s.len())?;m.write_bytes(s.len())?;Ok(s.to_owned())}
fn copy_ref(m:&mut CoreMeter,r:&ExactRef)->Result<ExactRef>{Ok(ExactRef{kind:copy(m,&r.kind)?,id:copy(m,&r.id)?,digest:copy(m,&r.digest)?})}
/// Private inputs are the SAME work constructor's emitted target and decision
/// objects. Every current/original authority check still runs in that caller.
pub(in crate::policy::workflow::transition_constructor) fn derive(
    arena:&AuditArena,m:&mut CoreMeter,r:&mut super::super::records::Records,c:&super::super::data::DraftContextData,program:&SelectedProgram,payload:&Value,
    decision:&ConstructedEnvelope,targets:&[ConstructedEnvelope],work:&ConstructedEnvelope,
    claim:&ConstructedEnvelope,terms:&super::super::authority::Terms,
)->Result<OriginalReservations>{
    let count=targets.len();
    let per=mem::size_of::<(ExactRef,super::super::record_body::RecordBody)>()+64+128+71;
    let bytes=mul(m,count,per)?;
    let bytes=add(m,bytes,mem::size_of::<Vec<(ExactRef,super::super::record_body::RecordBody)>>())?;
    let _views=arena.reserve_external_payload(bytes,m)?;
    m.write_bytes(bytes)?;
    let mut selected=Vec::new();selected.try_reserve_exact(count).map_err(|_|shape::fail("original target views"))?;
    for target in targets{
        m.visit("capacity.fresh_target_view")?;
        if !shape::same_text(m,&target.reference.kind,"workflow-target")?{continue;}
        selected.push((copy_ref(m,&target.reference)?,target.decoded(m,4194304)?));
    }
    let decision_value=decision.decoded(m,4194304)?;
    let work_value=work.decoded(m,4194304)?;let claim_value=claim.decoded(m,4194304)?;
    derive_data(arena,m,r,c,program,payload,&decision.reference,&decision_value,&selected,&work.reference,&work_value,&claim.reference,&claim_value,terms)
}

/// The same original reservation derivation receives actual decoded fresh OR
/// retained bodies. A retained input never impersonates an emitted envelope.
fn derive_data(
    arena:&AuditArena,m:&mut CoreMeter,r:&mut super::super::records::Records,c:&super::super::data::DraftContextData,program:&SelectedProgram,payload:&Value,
    decision_ref:&ExactRef,original_decision:&Value,targets:&[(ExactRef,super::super::record_body::RecordBody)],
    work_ref:&ExactRef,original_work:&Value,claim_ref:&ExactRef,original_claim:&Value,terms:&super::super::authority::Terms,
)->Result<OriginalReservations>{
    let instructions=shape::array(m,program.compiled_plan().canonical_value(),"typed_instructions")?;
    let mut family_count=0usize;
    let mut roots=0usize;
    let mut delayed_count=0usize;
    let mut app_count=0usize;
    for instruction in instructions {
        m.visit("capacity.original_instruction_dimension")?;
        let opcode=shape::text(m,instruction,"opcode")?;
        if shape::same_text(m,opcode,"evaluate_rule")? {
            if shape::same(m,&instruction["rule"]["activity"],&payload["activity"])? {
                let kind=shape::text(m,&instruction["rule"]["formula"],"kind")?;
                if shape::same_text(m,kind,"app_final_cost")?{app_count=add(m,app_count,1)?;}
                else if shape::same_text(m,kind,"supplier_final_cost")?{delayed_count=add(m,delayed_count,1)?;}
                else if !shape::same_text(m,kind,"final_cost")?{roots=add(m,roots,1)?;}
            }
        } else if shape::same_text(m,opcode,"evaluate_family")? {
            if shape::same(m,&instruction["family"]["selector"]["target_activity"],&payload["activity"])? {
                family_count=add(m,family_count,1)?;
            }
        } else {return Err(shape::fail("unknown compiled capacity opcode"));}
    }
    if w::compare(m,family_count,128usize)?.is_gt(){return Err(shape::fail("original dependency count"));}
    // One Vec with exact capacity. Each row has <=128-byte slot/decision IDs
    // and one exact Ref (kind64/id128/digest71). Root ID and Binding Ref have
    // independent payloads. All are validated producer identities before copy.
    let per_row=add(m,mem::size_of::<FamilyReservation>(),128+128+64+128+71)?;
    let bytes=mul(m,family_count,per_row)?;
    let delayed_row=add(m,mem::size_of::<DelayedCostReservation>(),128+128+128+64+128+71)?;
    let delayed_bytes=mul(m,delayed_count,delayed_row)?;
    let bytes=add(m,bytes,delayed_bytes)?;
    let app_row=add(m,mem::size_of::<AppCostReservation>(),128+128+64+128+71)?;
    let app_bytes=mul(m,app_count,app_row)?;
    let bytes=add(m,bytes,app_bytes)?;
    let bytes=add(m,bytes,mem::size_of::<OriginalReservations>()+128+2*(64+128+71))?;
    let metadata=arena.reserve_external_payload(bytes,m)?;
    // All three Vec controls/rows and the returned struct are written only
    // after this debit, including allocation-failure prefixes. String copies
    // below keep their own actual scan/write callbacks.
    m.write_bytes(bytes)?;
    // One semantic-ID call at a time overlaps its fixed domain prefix,
    // hexadecimal digest and returned ID before the latter moves into a row.
    let _id_scratch=arena.reserve_external_payload(5*mem::size_of::<String>()+512,m)?;
    let mut families=Vec::new();
    families.try_reserve_exact(family_count).map_err(|_|shape::fail("original dependency vector allocation"))?;
    let first=shape::text(m,&original_decision,"revision")?;
    if !shape::same_text(m,first,"1")?{return Err(shape::fail("capacity original root must be revision1"));}
    let root_id=shape::text(m,&original_decision,"decision_id")?;
    bound(m,root_id.len(),1,128)?;
    let root_decision_id=copy(m,root_id)?;
    let original_binding_ref=shape::reference(m,&payload["binding_ref"])?;
    let mut delayed_costs=Vec::new();
    delayed_costs.try_reserve_exact(delayed_count).map_err(|_|shape::fail("delayed reservation allocation"))?;
    let mut app_costs=Vec::new();app_costs.try_reserve_exact(app_count).map_err(|_|shape::fail("app reservation allocation"))?;
    super::super::outcome::original_claim_association(m,&original_claim,&original_work)?;
    let work_value=super::super::working_value::owning_reference(m,arena,work_ref)?;
    let decision_value=super::super::working_value::owning_reference(m,arena,decision_ref)?;
    if !shape::same(m,&original_claim["work_ref"],work_value.value())?
        || !shape::same(m,&original_decision["subject_ref"],work_value.value())?{
        return Err(shape::fail("original delayed reservation work/claim/decision"));
    }
    for instruction in instructions{
        m.visit("capacity.delayed_original_instruction")?;
        let opcode=shape::text(m,instruction,"opcode")?;
        if !shape::same_text(m,opcode,"evaluate_rule")?{continue;}
        let rule=&instruction["rule"];
        if !shape::same(m,&rule["activity"],&payload["activity"])?{continue;}
        let kind=shape::text(m,&rule["formula"],"kind")?;
        if !shape::same_text(m,kind,"supplier_final_cost")?{continue;}
        let component=shape::text(m,rule,"component_key")?;
        let bindingslot=shape::text(m,rule,"binding_slot")?;
        let maps=shape::array(m,&terms.instantiation,"component_map")?;
        let mapping=shape::named(m,maps,"template_key",component)?;
        let slot=shape::text(m,mapping,"slot_id")?;
        let catalog=shape::array(m,&terms.catalog,"components")?;
        let declared=shape::named(m,catalog,"slot_id",slot)?;
        let book=shape::text(m,declared,"book")?;
        if !shape::same_text(m,book,"supplier")?{return Err(shape::fail("delayed reservation catalog book"));}
        let mut maximum_inverse_actions=1usize;
        for candidate in instructions{
            m.visit("capacity.delayed_original_retail_producer")?;
            let opcode=shape::text(m,candidate,"opcode")?;
            if !shape::same_text(m,opcode,"evaluate_rule")?{continue;}
            let retail=&candidate["rule"];
            if !shape::same(m,&retail["activity"],&payload["activity"])?{continue;}
            let kind=shape::text(m,&retail["formula"],"kind")?;
            if shape::same_text(m,kind,"final_cost")?{
                let supplier=shape::text(m,&retail["formula"],"supplier_slot")?;
                if shape::same_text(m,supplier,bindingslot)?{maximum_inverse_actions=add(m,maximum_inverse_actions,1)?;}
            }
        }
        shape::equal(m,&original_claim["scope"],&original_work["key"]["scope"])?;
        let claim_id=arena.string_value(&claim_ref.id,m)?;let slot_value=arena.string_value(slot,m)?;
        let discriminator=arena.string_value("cost",m)?;
        let tuple=arena.array_from_values(&[&original_claim["scope"],claim_id.value(),slot_value.value(),discriminator.value()],m)?;
        let dependent=super::super::body::semantic_id(m,"wdecision_","workflow-decision",tuple.value())?;
        delayed_costs.push(DelayedCostReservation{target_ref:copy_ref(m,&work_ref)?,component_slot:copy(m,slot)?,supplier_binding_slot:copy(m,bindingslot)?,dependent_decision_id:dependent,maximum_inverse_actions});
    }
    for instruction in instructions{
        m.visit("capacity.app_original_instruction")?;
        let opcode=shape::text(m,instruction,"opcode")?;
        if !shape::same_text(m,opcode,"evaluate_rule")?{continue;}
        let rule=&instruction["rule"];
        if !shape::same(m,&rule["activity"],&payload["activity"])?{continue;}
        let kind=shape::text(m,&rule["formula"],"kind")?;
        if !shape::same_text(m,kind,"app_final_cost")?{continue;}
        if !program.uses_app_owned_cost_recipe(){return Err(shape::fail("app reservation original recipe"));}
        let component=shape::text(m,rule,"component_key")?;
        let maps=shape::array(m,&terms.instantiation,"component_map")?;
        let mapping=shape::named(m,maps,"template_key",component)?;
        let slot=shape::text(m,mapping,"slot_id")?;
        let declared=shape::array(m,&terms.catalog,"components")?;
        let declared=shape::named(m,declared,"slot_id",slot)?;
        let book=shape::text(m,declared,"book")?;
        if !shape::same_text(m,book,"retail")?{return Err(shape::fail("app reservation catalog book"));}
        let dependent=super::super::work_keys::app_decision(m,&original_work["key"]["scope"],&work_ref.id,slot)?;
        // Each original declaration selects one terminal customer cost posting.
        // Its zero alternative retains the decision vertex; a root inverse
        // requires at most that one active action, never an AP action.
        app_costs.push(AppCostReservation{target_ref:copy_ref(m,&work_ref)?,component_slot:copy(m,slot)?,dependent_decision_id:dependent,maximum_inverse_actions:1});
    }
    if !w::compare(m,app_costs.len(),app_count)?.is_eq(){return Err(shape::fail("app reservation omission"));}
    for (target_ref,value) in targets {
        m.visit("capacity.emitted_target")?;
        if !shape::same(m,&value["original_decision_ref"],decision_value.value())?
            || !shape::same(m,&value["binding_ref"],&payload["binding_ref"])? {
            return Err(shape::fail("emitted original reservation association"));
        }
        for original_family in shape::array(m,&value,"families")? {
            m.visit("capacity.original_family_reservation")?;
            let key=shape::text(m,original_family,"family_key")?;
            let slot=shape::text(m,original_family,"family_slot")?;
            let dependent=shape::text(m,original_family,"prospective_decision_id")?;
            bound(m,slot.len(),1,128)?;
            bound(m,dependent.len(),1,128)?;
            let mut family=None;
            for instruction in instructions {
                m.visit("capacity.original_family_program_join")?;
                let opcode=shape::text(m,instruction,"opcode")?;
                let original_key=if shape::same_text(m,opcode,"evaluate_family")? {Some(shape::text(m,&instruction["family"],"family_key")?)}else{None};
                let matches=match original_key {Some(original_key)=>shape::same_text(m,original_key,key)?,None=>false};
                if matches {
                    if family.is_some(){return Err(shape::fail("ambiguous original family reservation"));}
                    family=Some(&instruction["family"]);
                }
            }
            let family=family.ok_or_else(||shape::fail("reservation missing original family"))?;
            if !shape::same(m,&family["selector"]["target_activity"],&payload["activity"])? {
                return Err(shape::fail("reservation original family activity"));
            }
            // Every allowed ordinary code contributes at most one active
            // posting. NONE/zero/unclaimed still retain this decision vertex.
            // Alternatives are exclusive, so take MAX instead of SUM.
            let mut maximum_inverse_actions=0;
            for code in shape::array(m,family,"codes")? {
                m.visit("capacity.original_family_effect_alternative")?;
                let kind=shape::text(m,&code["effect"],"kind")?;
                if shape::same_text(m,kind,"none")?{continue;}
                if !shape::same_text(m,kind,"premium")? && !shape::same_text(m,kind,"discount")? {
                    return Err(shape::fail("unknown original family effect"));
                }
                maximum_inverse_actions=1;
            }
            for prior in &families {
                let prior:&FamilyReservation=prior;
                m.visit("capacity.original_dependency_identity")?;
                if shape::same_text(m,&prior.dependent_decision_id,dependent)? {
                    return Err(shape::fail("duplicate original dependent decision"));
                }
            }
            if !w::compare(m,families.len(),family_count)?.is_lt(){return Err(shape::fail("original family dimension widened"));}
            families.push(FamilyReservation{target_ref:copy_ref(m,&target_ref)?,family_slot:copy(m,slot)?,dependent_decision_id:copy(m,dependent)?,maximum_inverse_actions});
        }
    }
    if !w::compare(m,families.len(),family_count)?.is_eq(){return Err(shape::fail("original family reservation omission"));}
    // Original rule instructions are one posting each, including a zero
    // original alternative. A root correction may inverse all and replace all.
    let maximum_root_actions=mul(m,roots,2)?;
    if w::compare(m,maximum_root_actions,128usize)?.is_gt(){return Err(shape::fail("original root action capacity"));}
    let future_records=if delayed_costs.is_empty(){Some(super::future_records::derive(m,program,payload,maximum_root_actions,families.len(),app_costs.len())?)}else{None};
    let app_body_bytes=if app_costs.is_empty(){None}else{Some(super::app_bytes::derive(m,&original_work["key"]["logical_store_id"],payload,terms)?)};
    let family_body_bytes=if families.is_empty(){None}else if let Some(records)=&future_records{
        let bounds=super::app_bytes::family_bodies(m,program,payload,terms,&original_work["key"]["logical_store_id"],roots,records.maximum_attested_family_fields)?;
        if !w::compare(m,bounds.facts.family_instructions,family_count)?.is_eq(){return Err(shape::fail("family changing-body declaration coverage"));}
        Some(bounds)
    }else{None};
    let root_body_bytes=if delayed_costs.is_empty(){Some(super::app_bytes::root_bodies(m,program,payload,terms,&original_work["key"]["logical_store_id"],roots,families.len(),app_costs.len(),app_body_bytes.as_ref())?)}else{None};
    let source_bounds=super::source::original_frames(m,r,c,terms,program)?;
    let pricing_kernels=if delayed_costs.is_empty(){Some(super::pricing::derive(m,program.compiled_plan().canonical_value(),&payload["activity"])?)}else{None};
    let reservations=OriginalReservations{root_decision_id,original_root_ref:copy_ref(m,&decision_ref)?,original_binding_ref,maximum_root_actions,families,delayed_costs,app_costs,future_records,app_body_bytes,family_body_bytes,root_body_bytes,source_bounds,pricing_kernels,_metadata:metadata};
    let checked=reservations.structural_union(arena,m)?;
    drop(checked);
    Ok(reservations)
}
impl OriginalReservations {
    pub(in crate::policy::workflow::transition_constructor) fn structural_union<'a>(&'a self,arena:&AuditArena,m:&mut CoreMeter)->Result<super::topology::Union<'a>> {
        // This temporary borrowed-edge vector has its own pre-admission and
        // remains live only through the common distinct-decision DAG traversal.
        let count=add(m,self.families.len(),self.delayed_costs.len())?;
        let count=add(m,count,self.app_costs.len())?;
        let bytes=mul(m,count,mem::size_of::<super::topology::Reservation>())?;
        let bytes=add(m,bytes,mem::size_of::<Vec<super::topology::Reservation>>())?;
        let _edges=arena.reserve_external_payload(bytes,m)?;
        m.write_bytes(bytes)?;
        let mut edges=Vec::new();edges.try_reserve_exact(count).map_err(|_|shape::fail("original capacity edge allocation"))?;
        for family in &self.families {m.visit("capacity.original_edge")?;edges.push(super::topology::Reservation{ancestor_decision_id:&self.root_decision_id,dependent_decision_id:&family.dependent_decision_id,maximum_inverse_actions:family.maximum_inverse_actions});}
        for cost in &self.delayed_costs {m.visit("capacity.original_delayed_edge")?;edges.push(super::topology::Reservation{ancestor_decision_id:&self.root_decision_id,dependent_decision_id:&cost.dependent_decision_id,maximum_inverse_actions:cost.maximum_inverse_actions});}
        for cost in &self.app_costs {m.visit("capacity.original_app_edge")?;edges.push(super::topology::Reservation{ancestor_decision_id:&self.root_decision_id,dependent_decision_id:&cost.dependent_decision_id,maximum_inverse_actions:cost.maximum_inverse_actions});}
        super::topology::union(arena,m,&self.root_decision_id,self.maximum_root_actions,&edges)
    }
}
