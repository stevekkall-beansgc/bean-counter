//! Local pre-evaluation lookup DATA. These are exact existing identity recipes,
//! not pricing, authority, currentness, claim absence or a prepared candidate.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter,
    candidate::PermanentClaimKey,
    data::*,
    input_selection::{EconomicCapture, WorkHeadLocators},
    records::Records,
    shape,
    working_value::{OwnedExactRef, WorkingKey, parse_owned_reference},
};
use crate::{
    Result,
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
};
use serde_json::Value;
use std::mem;

pub struct LocalRecordIdentity {
    kind: &'static str,
    id: String,
    schema: &'static str,
}
impl LocalRecordIdentity {
    pub fn kind(&self) -> &str {
        self.kind
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn schema(&self) -> &str {
        self.schema
    }
}
pub struct LocalOperationLocators {
    subject: LocalSubjectBinding,
    work: Option<WorkHeadLocators>,
    records: Vec<LocalRecordIdentity>,
    claims: Vec<PermanentClaimKey>,
    outcome_link: Option<(ExactRef,String)>,
    required_slots: Vec<String>,
    _copies: Vec<ExternalBufferLease>,
    _ownership: ExternalBufferLease,
}
impl LocalOperationLocators {
    /// Original declaration slots needed by root reversal or close retention.
    /// Lookup DATA only: the child independently authenticates every frame and
    /// rejects missing, duplicate and unused nominations.
    pub fn required_authority_slots(&self) -> &[String] { &self.required_slots }
    pub fn family_slot(&self) -> Option<&str> {
        self.subject.family_slot()
    }
    pub fn component_slot(&self) -> Option<&str> {
        self.subject.component_slot()
    }
    pub fn original_work_ref(&self) -> Option<&ExactRef> {
        self.subject.original_work_ref()
    }
    pub fn target_ref(&self) -> Option<&ExactRef> {
        self.subject.target_ref()
    }
    pub fn original_binding_ref(&self) -> &ExactRef {
        self.subject.original_binding_ref()
    }
    pub fn work_heads(&self) -> Option<&WorkHeadLocators> {
        self.work.as_ref()
    }
    /// Revision-1/T0 and dependency lookup identities, queried within the same
    /// store/scope. A prospective decision may genuinely be absent; a held
    /// current head determines which retained row is mandatory. No digest or
    /// absence proof is fabricated here.
    pub fn record_identities(&self) -> &[LocalRecordIdentity] {
        &self.records
    }
    pub fn claim_keys(&self) -> &[PermanentClaimKey] {
        &self.claims
    }
    /// Exact immutable original child and selected link slot for a held point
    /// read. The returned DATA establishes neither a link nor its currentness.
    pub fn original_outcome_link(&self)->Option<(&ExactRef,&str)>{
        self.outcome_link.as_ref().map(|(child,slot)|(child,slot.as_str()))
    }
}
fn cp(
    m: &mut CoreMeter,
    a: &AuditArena,
    owners: &mut Vec<ExternalBufferLease>,
    v: &str,
) -> Result<String> {
    let n = w::usize_add(m, v.len(), mem::size_of::<String>())?;
    let o = a.reserve_external_payload(n, m)?;
    m.scan_bytes(v.len())?;
    m.write_bytes(n)?;
    owners.push(o);
    Ok(v.to_owned())
}
fn rf(
    m: &mut CoreMeter,
    a: &AuditArena,
    owners: &mut Vec<ExternalBufferLease>,
    v: &Value,
) -> Result<ExactRef> {
    let r = parse_owned_reference(m, a, v)?;
    Ok(ExactRef {
        kind: cp(m, a, owners, &r.reference().kind)?,
        id: cp(m, a, owners, &r.reference().id)?,
        digest: cp(m, a, owners, &r.reference().digest)?,
    })
}
fn txt<'a>(m: &mut CoreMeter, v: &'a Value, k: &str) -> Result<&'a str> {
    shape::text(m, v, k)
}
fn load_ref(
    m: &mut CoreMeter,
    a: &AuditArena,
    r: &mut Records,
    key: &OperationKey,
    value: &Value,
    kind: &str,
    schema: &str,
) -> Result<(OwnedExactRef, super::record_body::RecordBody)> {
    let reference = parse_owned_reference(m, a, value)?;
    let body = r.get(m, key, reference.reference(), kind, schema)?;
    Ok((reference, body))
}
fn initial_id(m: &mut CoreMeter, a: &AuditArena, decision: &str) -> Result<String> {
    let d = a.string_value(decision, m)?;
    let one = a.string_value("1", m)?;
    let tuple = a.array_from_values(&[d.value(), one.value()], m)?;
    super::body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())
}
fn dependency_id(
    m: &mut CoreMeter,
    a: &AuditArena,
    root: &str,
    dependent: &str,
    binding: &Value,
) -> Result<String> {
    let root = a.string_value(root, m)?;
    let dependent = a.string_value(dependent, m)?;
    let tuple = a.array_from_values(&[root.value(), dependent.value(), binding], m)?;
    super::body::semantic_id(m, "wdep_", "workflow-dependency", tuple.value())
}

/// The supplied pool contains actual retained operands selected under the host
/// snapshot. Missing required bodies refuse; no search through arbitrary
/// history, current-policy fallback or priced construction occurs here.
pub fn select_local_operation_locators(
    program: &SelectedProgram,
    capture: &EconomicCapture<'_>,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<LocalOperationLocators> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        if !program.uses_local_execution_recipe() {
            return Err(shape::fail("exact local locator recipe required"));
        }
        let a = s.arena().shared_handle();
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        super::codec::validate_selected_types(m, p.value())?;
        let schema = txt(m, p.value(), "schema")?;
        super::candidate::assembly::operation(m, p.value())?;
        let mut r = Records::borrowed_parent_app(m, &a, capture.key, inputs)?;
        let header = mem::size_of::<LocalOperationLocators>()
            + 4 * mem::size_of::<PermanentClaimKey>()
            + 32 * mem::size_of::<ExternalBufferLease>()
            + 3 * mem::size_of::<Vec<usize>>();
        let ownership = a.reserve_external_payload(header, m)?;
        m.write_bytes(header)?;
        let mut copies = Vec::new();
        copies
            .try_reserve_exact(32)
            .map_err(|_| shape::fail("local locator string controls"))?;
        let mut claims = Vec::new();
        claims
            .try_reserve_exact(4)
            .map_err(|_| shape::fail("local locator claim controls"))?;
        let subject = subject_refs(m, &a, &mut r, capture.key, p.value())?;
        let work_ref = &subject.work_ref;
        let target_ref = &subject.target_ref;
        let v = p.value();
        let mut work = None;
        if let Some(reference) = &work_ref {
            let body = r.get(
                m,
                capture.key,
                reference.reference(),
                "workflow-work",
                "ledger-workflow-work-record/1",
            )?;
            let key = WorkingKey::parse(m, &body["key"])?;
            if !shape::same_text(m, &key.logical_store_id, &capture.key.logical_store_id)?
                || !shape::same_text(m, &key.scope[0], &capture.key.scope[0])?
                || !shape::same_text(m, &key.scope[1], &capture.key.scope[1])?
            {
                return Err(shape::fail("local locator original work scope"));
            }
            let payload = &body["payload"];
            super::input_selection::work_activity(program, payload, m)?;
            let policy = super::working_value::owning_definition(m, &a, program.root_policy_ref())?;
            shape::equal(m, &payload["policy_ref"], policy.value())?;
            let (binding_ref, binding) = load_ref(
                m,
                &a,
                &mut r,
                capture.key,
                &payload["binding_ref"],
                "workflow-binding",
                "ledger-workflow-binding/2",
            )?;
            for field in ["customer", "policy_ref", "product_ref"] {
                shape::equal(m, &binding[field], &payload[field])?;
            }
            let plan = super::working_value::owning_definition(
                m,
                &a,
                program.compiled_plan().reference(),
            )?;
            shape::equal(m, &binding["compiled_plan_ref"], plan.value())?;
            let (_, inst) = load_ref(
                m,
                &a,
                &mut r,
                capture.key,
                &binding["slot_instantiation_ref"],
                "workflow-slot-instantiation",
                "ledger-workflow-slot-instantiation/1",
            )?;
            let namespace = txt(m, &body, "source_namespace")?;
            let heads = super::input_selection::derive_work_head_locators(
                program, &key, namespace, payload, &binding, &inst, m,
            )?;
            let actual_claim = txt(m, &body, "claim_id")?;
            if !shape::same_text(m, heads.work_event_id(), &reference.reference().id)?
                || !shape::same_text(m, heads.work_claim_id(), actual_claim)?
            {
                return Err(shape::fail("local locator original work identity"));
            }
            let _binding_ref = binding_ref;
            work = Some(heads);
        }
        let n = if let Some(heads) = &work {
            let count = w::usize_add(m, heads.family_keys().len(), heads.app_decision_ids().len())?;
            let both = w::usize_add(m, count, count)?;
            w::usize_add(m, 1, both)?
        } else {
            0
        };
        crate::validation_work::ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Multiply,
            &[
                &n.into(),
                &(mem::size_of::<LocalRecordIdentity>() + 128).into(),
            ],
        )?;
        let bytes = n
            .checked_mul(mem::size_of::<LocalRecordIdentity>() + 128)
            .ok_or_else(|| shape::fail("local locator record dimensions"))?;
        let record_owner = a.reserve_external_payload(bytes, m)?;
        m.write_bytes(bytes)?;
        copies.push(record_owner);
        let mut records = Vec::new();
        records
            .try_reserve_exact(n)
            .map_err(|_| shape::fail("local locator record allocation"))?;
        let apps = work.as_ref().map_or(0, |h| h.app_decision_ids().len());
        let claim_capacity = w::usize_add(m, 4, apps)?;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Multiply,
            &[&claim_capacity.into(), &16usize.into()],
        )?;
        let copy_capacity = claim_capacity
            .checked_mul(16)
            .ok_or_else(|| shape::fail("local locator copy dimensions"))?;
        let copy_capacity = w::usize_add(m, copy_capacity, 2)?;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Multiply,
            &[
                &claim_capacity.into(),
                &mem::size_of::<PermanentClaimKey>().into(),
            ],
        )?;
        let claim_bytes = claim_capacity
            .checked_mul(mem::size_of::<PermanentClaimKey>())
            .ok_or_else(|| shape::fail("local locator claim dimensions"))?;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Multiply,
            &[
                &copy_capacity.into(),
                &mem::size_of::<ExternalBufferLease>().into(),
            ],
        )?;
        let copy_bytes = copy_capacity
            .checked_mul(mem::size_of::<ExternalBufferLease>())
            .ok_or_else(|| shape::fail("local locator owner dimensions"))?;
        let growth = w::usize_add(m, claim_bytes, copy_bytes)?;
        let growth_owner = a.reserve_external_payload(growth, m)?;
        m.write_bytes(growth)?;
        copies
            .try_reserve_exact(copy_capacity - copies.len())
            .map_err(|_| shape::fail("local locator owner growth"))?;
        claims
            .try_reserve_exact(claim_capacity)
            .map_err(|_| shape::fail("local locator claim growth"))?;
        copies.push(growth_owner);
        if let Some(heads) = &work {
            records.push(LocalRecordIdentity {
                kind: "workflow-decision",
                id: initial_id(m, &a, heads.root_decision_id())?,
                schema: "ledger-workflow-decision-revision/1",
            });
            let original = r.get(
                m,
                capture.key,
                work_ref.as_ref().unwrap().reference(),
                "workflow-work",
                "ledger-workflow-work-record/1",
            )?;
            let binding = &original["payload"]["binding_ref"];
            for family in heads.family_keys() {
                m.visit("local.locator_family_identity")?;
                records.push(LocalRecordIdentity {
                    kind: "workflow-decision",
                    id: initial_id(m, &a, family.decision_id())?,
                    schema: "ledger-workflow-decision-revision/1",
                });
                records.push(LocalRecordIdentity {
                    kind: "workflow-dependency",
                    id: dependency_id(
                        m,
                        &a,
                        heads.root_decision_id(),
                        family.decision_id(),
                        binding,
                    )?,
                    schema: "ledger-workflow-dependency/2",
                });
            }
            for decision in heads.app_decision_ids() {
                m.visit("local.locator_app_identity")?;
                records.push(LocalRecordIdentity {
                    kind: "workflow-decision",
                    id: initial_id(m, &a, decision)?,
                    schema: "ledger-workflow-decision-revision/3",
                });
                records.push(LocalRecordIdentity {
                    kind: "workflow-dependency",
                    id: dependency_id(m, &a, heads.root_decision_id(), decision, binding)?,
                    schema: "ledger-workflow-dependency/2",
                });
            }
            let (_, bound) = load_ref(
                m,
                &a,
                &mut r,
                capture.key,
                binding,
                "workflow-binding",
                "ledger-workflow-binding/2",
            )?;
            let (_, inst) = load_ref(
                m,
                &a,
                &mut r,
                capture.key,
                &bound["slot_instantiation_ref"],
                "workflow-slot-instantiation",
                "ledger-workflow-slot-instantiation/1",
            )?;
            let worked = super::working_value::owning_reference(
                m,
                &a,
                work_ref.as_ref().unwrap().reference(),
            )?;
            super::input_selection::head_rows(
                program,
                &original["payload"],
                &inst,
                m,
                &mut |slot, family, m| {
                    if family.is_none() {
                        claims.push(PermanentClaimKey::AppCostComponent {
                            logical_store_id: cp(
                                m,
                                &a,
                                &mut copies,
                                &capture.key.logical_store_id,
                            )?,
                            original_work_ref: rf(m, &a, &mut copies, worked.value())?,
                            component_slot: cp(m, &a, &mut copies, slot)?,
                        });
                    }
                    Ok(())
                },
            )?;
        }
        claim_keys(
            m,
            &a,
            &mut r,
            capture,
            v,
            target_ref.as_ref(),
            &mut copies,
            &mut claims,
        )?;
        let outcome_link=if shape::same_text(m,schema,"ledger-workflow-outcome-correction/1")?{
            let (claim_ref,claim)=load_ref(m,&a,&mut r,capture.key,&v["original_claim_ref"],"workflow-outcome-claim","ledger-workflow-outcome-claim/1")?;
            let target=subject.target_ref().ok_or_else(||shape::fail("outcome correction locator target"))?;
            let expected=super::working_value::owning_reference(m,&a,target)?;
            shape::equal(m,&claim["target_ref"],expected.value())?;
            let binding=super::working_value::owning_reference(m,&a,subject.original_binding_ref())?;
            shape::equal(m,&claim["original_payload"]["binding_ref"],binding.value())?;
            let bound=r.get(m,capture.key,subject.original_binding_ref(),"workflow-binding","ledger-workflow-binding/2")?;
            let(_,inst)=load_ref(m,&a,&mut r,capture.key,&bound["slot_instantiation_ref"],"workflow-slot-instantiation","ledger-workflow-slot-instantiation/1")?;
            let slot=original_outcome_link_slot(program,m,&claim,&inst)?;
            let value=super::working_value::owning_reference(m,&a,claim_ref.reference())?;
            Some((rf(m,&a,&mut copies,value.value())?,cp(m,&a,&mut copies,slot)?))
        }else{None};
        let required_slots = authority_slots(program,m,&a,&mut r,capture.key,v,&subject,&mut copies)?;
        Ok(LocalOperationLocators {
            subject,
            work,
            records,
            claims,
            outcome_link,
            required_slots,
            _copies: copies,
            _ownership: ownership,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}


// Select the ORIGINAL family from the retained Target declaration and its
// original instantiation. Never take a requested source-slot string as authority.
fn locator_family<'p>(program: &'p SelectedProgram, m: &mut CoreMeter,
    target: &Value, work: &Value, inst: &Value, slot: &str) -> Result<&'p Value> {
    shape::equal(m,&target["binding_ref"],&work["payload"]["binding_ref"])?;
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning locator family"))?;
    let policy=super::working_value::owning_definition(m,&arena,program.root_policy_ref())?;
    let plan=super::working_value::owning_definition(m,&arena,program.compiled_plan().reference())?;
    shape::equal(m,&target["policy_ref"],policy.value())?;
    shape::equal(m,&target["compiled_plan_ref"],plan.value())?;
    let declarations=shape::array(m,target,"families")?;
    let declaration=shape::named(m,declarations,"family_slot",slot)?;
    let key=txt(m,declaration,"family_key")?;
    let maps=shape::array(m,inst,"family_map")?;
    let mapping=shape::named(m,maps,"template_key",key)?;
    let mapped_slot=txt(m,mapping,"slot_id")?;
    if !shape::same_text(m,mapped_slot,slot)? {return Err(shape::fail("original family slot mapping"));}
    let mut found=None;
    for instruction in shape::array(m,program.compiled_plan().canonical_value(),"typed_instructions")? {
        m.visit("local.authority_slot_family")?;
        let opcode=txt(m,instruction,"opcode")?;
        if !shape::same_text(m,opcode,"evaluate_family")? {continue;}
        let family=&instruction["family"];
        let family_key=txt(m,family,"family_key")?;
        if !shape::same_text(m,family_key,key)? {continue;}
        shape::equal(m,&family["selector"]["target_activity"],&work["payload"]["activity"])?;
        let components=shape::array(m,inst,"component_map")?;
        let component_key=txt(m,&family["selector"],"component_key")?;
        let component=shape::named(m,components,"template_key",component_key)?;
        shape::equal(m,&target["root_component_slot"],&component["slot_id"])?;
        if found.is_some(){return Err(shape::fail("duplicate original locator family"));}
        found=Some(family);
    }
    found.ok_or_else(||shape::fail("original locator family missing"))
}
fn push_authority_slot(m:&mut CoreMeter,a:&AuditArena,owners:&mut Vec<ExternalBufferLease>,out:&mut Vec<String>,slot:&str)->Result<()> {
    for previous in out.iter(){m.visit("local.authority_slot_dedup")?;if shape::same_text(m,previous,slot)? {return Ok(());}}
    out.push(cp(m,a,owners,slot)?);Ok(())
}
fn authority_slots(program:&SelectedProgram,m:&mut CoreMeter,a:&AuditArena,r:&mut Records,key:&OperationKey,
    request:&Value,subject:&LocalSubjectBinding,owners:&mut Vec<ExternalBufferLease>)->Result<Vec<String>> {
    let schema=txt(m,request,"schema")?;
    let reverse=shape::same_text(m,schema,"ledger-workflow-work-reverse/2")?;
    let close=shape::same_text(m,schema,"ledger-workflow-family-close-attest/1")?;
    if !reverse && !close {return Ok(Vec::new());}
    let count=if reverse{let closures=shape::array(m,request,"family_closures")?;w::usize_add(m,closures.len(),1)?}else{1};
    let extra=w::usize_add(m,count,1)?;
    let capacity=w::usize_add(m,owners.capacity(),extra)?;
    crate::validation_work::ValidationWork::arithmetic(m,w::ArithmeticOperation::Multiply,&[&count.into(),&mem::size_of::<String>().into()])?;
    let strings=count.checked_mul(mem::size_of::<String>()).ok_or_else(||shape::fail("authority slot controls"))?;
    crate::validation_work::ValidationWork::arithmetic(m,w::ArithmeticOperation::Multiply,&[&capacity.into(),&mem::size_of::<ExternalBufferLease>().into()])?;
    let leases=capacity.checked_mul(mem::size_of::<ExternalBufferLease>()).ok_or_else(||shape::fail("authority slot lease controls"))?;
    let bytes=w::usize_add(m,strings,leases)?;
    let controls=a.reserve_external_payload(bytes,m)?;m.write_bytes(bytes)?;
    owners.try_reserve_exact(capacity-owners.len()).map_err(|_|shape::fail("authority slot owner allocation"))?;
    owners.push(controls);
    let mut out=Vec::new();out.try_reserve_exact(count).map_err(|_|shape::fail("authority slot allocation"))?;
    let work_ref=subject.original_work_ref().ok_or_else(||shape::fail("authority slot original work"))?;
    let work=r.get(m,key,work_ref,"workflow-work","ledger-workflow-work-record/1")?;
    let binding=r.get(m,key,subject.original_binding_ref(),"workflow-binding","ledger-workflow-binding/2")?;
    let binding_value=super::working_value::owning_reference(m,a,subject.original_binding_ref())?;
    shape::equal(m,&work["payload"]["binding_ref"],binding_value.value())?;
    let(_,inst)=load_ref(m,a,r,key,&binding["slot_instantiation_ref"],"workflow-slot-instantiation","ledger-workflow-slot-instantiation/1")?;
    let work_value=super::working_value::owning_reference(m,a,work_ref)?;
    if reverse {
        let bindings=shape::array(m,&binding,"bindings")?;
        if !w::compare(m,bindings.len(),1usize)?.is_eq(){return Err(shape::fail("selected root binding count"));}
        let(_,terms)=load_ref(m,a,r,key,&bindings[0]["binding_ref"],"workflow-slot-terms","ledger-workflow-slot-binding-terms/7")?;
        let source_slot=txt(m,&terms,"correction_source_slot")?;
        push_authority_slot(m,a,owners,&mut out,source_slot)?;
        for closure in shape::array(m,request,"family_closures")? {
            m.visit("local.requested_family_authority_slot")?;
            let(_,state)=load_ref(m,a,r,key,&closure["family_state_ref"],"workflow-family-state","ledger-workflow-family-state/1")?;
            shape::equal(m,&closure["expected_revision"],&state["revision"])?;
            let(_,target)=load_ref(m,a,r,key,&state["target_ref"],"workflow-target","ledger-workflow-component-target/1")?;
            shape::equal(m,&target["work_ref"],work_value.value())?;
            let slot=txt(m,&state,"family_slot")?;
            let family=locator_family(program,m,&target,&work,&inst,slot)?;
            let source_slot=txt(m,family,"correction_source_slot")?;
            push_authority_slot(m,a,owners,&mut out,source_slot)?;
        }
    } else {
        let target_ref=subject.target_ref().ok_or_else(||shape::fail("close locator target"))?;
        let target=r.get(m,key,target_ref,"workflow-target","ledger-workflow-component-target/1")?;
        shape::equal(m,&target["work_ref"],work_value.value())?;
        let slot=subject.family_slot().ok_or_else(||shape::fail("close locator family slot"))?;
        let family=locator_family(program,m,&target,&work,&inst,slot)?;
        let source_slot=txt(m,&family["no_success_close"],"close_source_slot")?;
        push_authority_slot(m,a,owners,&mut out,source_slot)?;
    }
    Ok(out)
}

fn original_outcome_link_slot<'a>(program:&'a SelectedProgram,m:&mut CoreMeter,claim:&Value,inst:&Value)->Result<&'a str>{
    let payload=&claim["original_payload"];
    for field in ["target_ref","family_slot","business_outcome_token"]{shape::equal(m,&claim[field],&payload[field])?;}
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning original outcome locator"))?;
    let policy=super::working_value::owning_definition(m,&arena,program.root_policy_ref())?;
    shape::equal(m,&payload["policy_ref"],policy.value())?;
    let mut found=None;
    for instruction in shape::array(m,program.compiled_plan().canonical_value(),"typed_instructions")?{
        m.visit("local.original_outcome_link_family")?;
        let opcode=txt(m,instruction,"opcode")?;
        if !shape::same_text(m,opcode,"evaluate_family")?{continue;}
        let family=&instruction["family"];
        if !shape::same(m,&family["trigger_activity"],&payload["activity"])?{continue;}
        let key=txt(m,family,"family_key")?;
        let mappings=shape::array(m,inst,"family_map")?;
        let mapping=shape::named(m,mappings,"template_key",key)?;
        if !shape::same(m,&mapping["slot_id"],&claim["family_slot"])?{continue;}
        if found.is_some(){return Err(shape::fail("ambiguous original outcome link family"));}
        found=Some(txt(m,family,"target_link_slot")?);
    }
    found.ok_or_else(||shape::fail("original outcome link family missing"))
}

fn claim_keys(
    m: &mut CoreMeter,
    a: &AuditArena,
    r: &mut Records,
    c: &EconomicCapture<'_>,
    p: &Value,
    target: Option<&OwnedExactRef>,
    owners: &mut Vec<ExternalBufferLease>,
    out: &mut Vec<PermanentClaimKey>,
) -> Result<()> {
    let schema = txt(m, p, "schema")?;
    m.visit("local.locator_claim_recipe")?;
    match schema {
        "ledger-workflow-fact-attest/1" | "ledger-workflow-family-close-attest/1" => {
            let att = &p["attestation"];
            let source = txt(m, att, "source")?;
            if !shape::same_text(m, source, &c.key.source)? {
                return Err(shape::fail("assertion locator actual source"));
            }
            let id = txt(m, att, "attestation_id")?;
            out.push(PermanentClaimKey::Assertion {
                logical_store_id: cp(m, a, owners, &c.key.logical_store_id)?,
                scope: [
                    cp(m, a, owners, &c.key.scope[0])?,
                    cp(m, a, owners, &c.key.scope[1])?,
                ],
                verified_source: cp(m, a, owners, source)?,
                attestation_id: cp(m, a, owners, id)?,
            });
        }
        "ledger-workflow-app-observe/1" => {
            let att = &p["attestation"];
            let subject = &att["subject"];
            let call = &subject["app_call"];
            let namespace = txt(m, call, "source_namespace")?;
            let id = txt(m, call, "call_id")?;
            out.push(PermanentClaimKey::AppCall {
                logical_store_id: cp(m, a, owners, &c.key.logical_store_id)?,
                source_namespace: cp(m, a, owners, namespace)?,
                call_id: cp(m, a, owners, id)?,
            });
            let slot = txt(m, subject, "component_slot")?;
            out.push(PermanentClaimKey::AppComponentObservation {
                logical_store_id: cp(m, a, owners, &c.key.logical_store_id)?,
                original_work_ref: rf(m, a, owners, &subject["original_work_ref"])?,
                component_slot: cp(m, a, owners, slot)?,
            });
            if let Some(provider) = att.get("provider_key") {
                let name = txt(m, provider, "provider")?;
                let account = txt(m, provider, "account")?;
                let id = txt(m, provider, "request_id")?;
                out.push(PermanentClaimKey::AppProviderCall {
                    logical_store_id: cp(m, a, owners, &c.key.logical_store_id)?,
                    provider: cp(m, a, owners, name)?,
                    account: cp(m, a, owners, account)?,
                    request_id: cp(m, a, owners, id)?,
                });
            }
        }
        "ledger-workflow-cost-finalize/2" | "ledger-workflow-cost-correct/2" => {
            let proof = if schema == "ledger-workflow-cost-finalize/2" {
                &p["final_cost_proof"]
            } else {
                &p["replacement_final_cost_proof"]
            };
            let (evidence, body) = load_ref(
                m,
                a,
                r,
                c.key,
                &proof["attestation_ref"],
                "workflow-fact-attestation",
                "ledger-workflow-app-final-cost/1",
            )?;
            // Every original app component key was emitted by the SAME work
            // declaration selector above, including this finalized component.
            let value = super::working_value::owning_reference(m, a, evidence.reference())?;
            out.push(PermanentClaimKey::AppFinalEvidence {
                logical_store_id: cp(m, a, owners, &c.key.logical_store_id)?,
                final_evidence_ref: rf(m, a, owners, value.value())?,
            });
            let raw = txt(m, &body, "cost_revision")?;
            let counter = crate::policy::workflow::facts::counter_with_work(raw, m)?;
            let counter = super::close::counter_value(m, a, counter)?;
            out.push(PermanentClaimKey::AppObservationRevision {
                logical_store_id: cp(m, a, owners, &c.key.logical_store_id)?,
                observation_ref: rf(m, a, owners, &body["observation_ref"])?,
                cost_revision: cp(
                    m,
                    a,
                    owners,
                    counter
                        .value()
                        .as_str()
                        .ok_or_else(|| shape::fail("cost revision counter"))?,
                )?,
            });
        }
        "ledger-workflow-outcome/1" => {
            let target = target.ok_or_else(|| shape::fail("outcome locator target"))?;
            let body = r.get(
                m,
                c.key,
                target.reference(),
                "workflow-target",
                "ledger-workflow-component-target/1",
            )?;
            let agreement = txt(m, &body, "agreement_id")?;
            let family = txt(m, p, "family_slot")?;
            let token = txt(m, p, "business_outcome_token")?;
            out.push(PermanentClaimKey::Outcome {
                scope: [
                    cp(m, a, owners, &c.key.scope[0])?,
                    cp(m, a, owners, &c.key.scope[1])?,
                ],
                agreement_id: cp(m, a, owners, agreement)?,
                family_slot: cp(m, a, owners, family)?,
                target_id: cp(m, a, owners, &target.reference().id)?,
                source_namespace: cp(m, a, owners, c.canonical_economic_namespace)?,
                business_outcome_token: cp(m, a, owners, token)?,
            });
        }
        "ledger-workflow-outcome-correction/1" => {
            let (_,claim)=load_ref(m,a,r,c.key,&p["original_claim_ref"],"workflow-outcome-claim","ledger-workflow-outcome-claim/1")?;
            let scope=super::work_keys::scope(m,c.key)?;shape::equal(m,&claim["scope"],scope.value())?;
            let target=target.ok_or_else(||shape::fail("outcome correction permanent target"))?;
            let expected=super::working_value::owning_reference(m,a,target.reference())?;shape::equal(m,&claim["target_ref"],expected.value())?;
            let agreement=txt(m,&claim,"agreement_id")?;
            let family=txt(m,&claim,"family_slot")?;
            let namespace=txt(m,&claim,"source_namespace")?;
            let token=txt(m,&claim,"business_outcome_token")?;
            out.push(PermanentClaimKey::Outcome{
                scope:[cp(m,a,owners,&c.key.scope[0])?,cp(m,a,owners,&c.key.scope[1])?],
                agreement_id:cp(m,a,owners,agreement)?,
                family_slot:cp(m,a,owners,family)?,
                target_id:cp(m,a,owners,&target.reference().id)?,
                source_namespace:cp(m,a,owners,namespace)?,
                business_outcome_token:cp(m,a,owners,token)?,
            });
        }
        _ => {}
    }
    Ok(())
}

/// Exact immutable subject associations, before the host knows which original
/// Binding/PUB to load. This DATA lookup uses no compiler or source authority.
pub struct LocalSubjectBinding {
    work_ref: Option<OwnedExactRef>,
    target_ref: Option<OwnedExactRef>,
    binding_ref: OwnedExactRef,
    family_slot: Option<LocatorText>,
    component_slot: Option<LocatorText>,
    _ownership: ExternalBufferLease,
}
struct LocatorText {
    value: String,
    _owner: ExternalBufferLease,
}
fn locator_text(m: &mut CoreMeter, a: &AuditArena, value: &str) -> Result<LocatorText> {
    let bytes = w::usize_add(m, mem::size_of::<LocatorText>(), value.len())?;
    let owner = a.reserve_external_payload(bytes, m)?;
    m.scan_bytes(value.len())?;
    m.write_bytes(bytes)?;
    Ok(LocatorText {
        value: value.to_owned(),
        _owner: owner,
    })
}
impl LocalSubjectBinding {
    pub fn family_slot(&self) -> Option<&str> {
        self.family_slot.as_ref().map(|v| v.value.as_str())
    }
    pub fn component_slot(&self) -> Option<&str> {
        self.component_slot.as_ref().map(|v| v.value.as_str())
    }
    pub fn original_work_ref(&self) -> Option<&ExactRef> {
        self.work_ref.as_ref().map(|v| v.reference())
    }
    pub fn target_ref(&self) -> Option<&ExactRef> {
        self.target_ref.as_ref().map(|v| v.reference())
    }
    pub fn original_binding_ref(&self) -> &ExactRef {
        self.binding_ref.reference()
    }
}
fn subject_refs(
    m: &mut CoreMeter,
    a: &AuditArena,
    r: &mut Records,
    key: &OperationKey,
    p: &Value,
) -> Result<LocalSubjectBinding> {
    let owner = a.reserve_external_payload(mem::size_of::<LocalSubjectBinding>(), m)?;
    m.write_bytes(mem::size_of::<LocalSubjectBinding>())?;
    let schema = txt(m, p, "schema")?;
    super::candidate::assembly::operation(m, p)?;
    let mut family_slot = None;
    let mut component_slot = None;
    let mut work_ref = None;
    let mut target_ref = None;
    let v = p;
    m.visit("local.locator_payload_dispatch")?;
    let original_work = match schema {
        "ledger-workflow-app-observe/1" | "ledger-workflow-app-cost-attest/1" => {
            Some(&v["attestation"]["subject"]["original_work_ref"])
        }
        "ledger-workflow-cost-finalize/2" => Some(&v["original_work_ref"]),
        "ledger-workflow-work-reverse/2" | "ledger-workflow-quantity-correction/2" => {
            Some(&v["work_ref"])
        }
        "ledger-workflow-cost-correct/2" => {
            let (_, claim) = load_ref(
                m,
                a,
                r,
                key,
                &v["original_cost_claim_ref"],
                "workflow-cost",
                "ledger-workflow-app-cost-claim/1",
            )?;
            work_ref = Some(parse_owned_reference(
                m,
                a,
                &claim["subject"]["original_work_ref"],
            )?);
            let slot = txt(m, &claim["subject"], "component_slot")?;
            component_slot = Some(locator_text(m, a, slot)?);
            None
        }
        "ledger-workflow-outcome/1" | "ledger-workflow-outcome-close/2" => {
            target_ref = Some(parse_owned_reference(m, a, &v["target_ref"])?);
            None
        }
        "ledger-workflow-outcome-correction/1" => {
            let (_, claim) = load_ref(
                m,
                a,
                r,
                key,
                &v["original_claim_ref"],
                "workflow-outcome-claim",
                "ledger-workflow-outcome-claim/1",
            )?;
            target_ref = Some(parse_owned_reference(m, a, &claim["target_ref"])?);
            let slot = txt(m, &claim, "family_slot")?;
            family_slot = Some(locator_text(m, a, slot)?);
            None
        }
        "ledger-workflow-fact-attest/1" => {
            let kind = txt(m, &v["attestation"]["subject"], "kind")?;
            if shape::same_text(m, kind, "outcome")? {
                target_ref = Some(parse_owned_reference(
                    m,
                    a,
                    &v["attestation"]["subject"]["target_ref"],
                )?);
            } else if !shape::same_text(m, kind, "work")? {
                return Err(shape::fail("unattached fact subject kind"));
            }
            None
        }
        "ledger-workflow-family-close-attest/1" => {
            target_ref = Some(parse_owned_reference(
                m,
                a,
                &v["attestation"]["subject"]["target_ref"],
            )?);
            None
        }
        _ => return Err(shape::fail("non-work local locator payload unavailable")),
    };
    if let Some(value) = original_work {
        work_ref = Some(parse_owned_reference(m, a, value)?);
    }
    if matches!(
        schema,
        "ledger-workflow-app-observe/1" | "ledger-workflow-app-cost-attest/1"
    ) {
        let slot = txt(m, &v["attestation"]["subject"], "component_slot")?;
        component_slot = Some(locator_text(m, a, slot)?);
    }
    if schema == "ledger-workflow-cost-finalize/2" {
        let slot = txt(m, v, "component_slot")?;
        component_slot = Some(locator_text(m, a, slot)?);
    }
    if matches!(
        schema,
        "ledger-workflow-outcome/1" | "ledger-workflow-outcome-close/2"
    ) {
        let slot = txt(m, v, "family_slot")?;
        family_slot = Some(locator_text(m, a, slot)?);
    }
    if matches!(
        schema,
        "ledger-workflow-fact-attest/1" | "ledger-workflow-family-close-attest/1"
    ) && target_ref.is_some()
    {
        let slot = txt(m, &v["attestation"]["subject"], "family_slot")?;
        family_slot = Some(locator_text(m, a, slot)?);
    }
    if let Some(target) = &target_ref {
        let body = r.get(
            m,
            key,
            target.reference(),
            "workflow-target",
            "ledger-workflow-component-target/1",
        )?;
        work_ref = Some(parse_owned_reference(m, a, &body["work_ref"])?);
    }

    let binding_ref = if let Some(work) = &work_ref {
        let body = r.get(
            m,
            key,
            work.reference(),
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        parse_owned_reference(m, a, &body["payload"]["binding_ref"])?
    } else {
        parse_owned_reference(m, a, &p["attestation"]["subject"]["binding_ref"])?
    };
    if !shape::same_text(m, &binding_ref.reference().kind, "workflow-binding")? {
        return Err(shape::fail("local subject binding kind"));
    }
    Ok(LocalSubjectBinding {
        work_ref,
        target_ref,
        binding_ref,
        family_slot,
        component_slot,
        _ownership: owner,
    })
}
pub fn locate_local_subject_binding(
    key: &OperationKey,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<LocalSubjectBinding> {
    let result = (|| {
        s.check_cancelled()?;
        let a = s.arena().shared_handle();
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        super::codec::validate_selected_types(m, p.value())?;
        let mut r = Records::borrowed_parent_app(m, &a, key, inputs)?;
        subject_refs(m, &a, &mut r, key, p.value())
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

#[cfg(test)]
mod correction_lookup_tests{
    use super::*;
    use crate::{canonical,policy::workflow::{DefinitionRef,document_digest,selected_program::{SelectedCompilerSupport,SelectedSourceInput,compile_selected_publication}},validation_work::ResourceLimits};
    use serde_json::json;
    #[test]
    fn correction_lookups_retain_original_claim_namespace_and_selected_link(){
        let a=AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s=OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let support=SelectedCompilerSupport::local_execution_with_session(&mut s).unwrap();
        let fixture:Value=serde_json::from_slice(include_bytes!("../selected_program/fixtures/local-first-policy-input.json")).unwrap();
        let mut docs=fixture["documents"].as_array().unwrap().clone();
        let policy=docs.iter_mut().find(|v|v["schema"]=="ledger-workflow-policy/4").unwrap();
        policy["compiler_ref"]=json!(support.reference());
        let digest=document_digest("ledger-workflow-policy/4",policy).unwrap();
        let root=DefinitionRef::new("workflow-policy",&format!("wdoc_{}",&digest[7..]),&digest).unwrap();
        docs.push(support.canonical_value().clone());
        let program=compile_selected_publication(SelectedSourceInput{documents:docs.iter().map(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect(),root_policy_ref:root},&support,&mut s).unwrap();
        let key=OperationKey{logical_store_id:"store".into(),scope:["test".into(),"local".into()],source:"new-corrector".into(),operation_id:"correction".into()};
        let capture=EconomicCapture{key:&key,actor:"corrector",canonical_economic_namespace:"new-corrector-namespace",installation_mode:"isolated_synthetic",received_at:"2026-10-19T00:00:00.000000Z",accepted_at:"2026-10-19T00:00:00.000000Z",sources:&[]};
        // Minimal lookup operands only, not a full authority/original replay.
        let mut producer=CoreMeter::for_compile(1048576).unwrap();
        let target=super::super::body::record(&mut producer,&key,"workflow-target",format!("wtarget_{}","1".repeat(64)),json!({"schema":"ledger-workflow-component-target/1"})).unwrap();
        let mut value=json!({"schema":"ledger-workflow-outcome-claim/1","scope":key.scope,"agreement_id":"agreement","family_slot":"support:quality","target_ref":target.reference,"source_namespace":"original-assessor-namespace","business_outcome_token":"original-token","original_payload":{"target_ref":target.reference,"family_slot":"support:quality","business_outcome_token":"original-token","activity":"support:assess","policy_ref":program.root_policy_ref()}});
        let claim=super::super::body::record(&mut producer,&key,"workflow-outcome-claim",format!("woclaim_{}","2".repeat(64)),value.clone()).unwrap();
        let views=[EconomicRecordView{reference:&claim.reference,canonical_body:&claim.canonical_body}];
        let request=json!({"schema":"ledger-workflow-outcome-correction/1","original_claim_ref":claim.reference});
        // Publication compilation and a later request have distinct original
        // ledgers, retaining the same attempt-owned program/arena.
        let mut selection=OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let m=selection.meter_mut();let mut r=Records::borrowed_parent_app(m,&a,&key,&views).unwrap();
        let target_ref=parse_owned_reference(m,&a,&json!(target.reference)).unwrap();
        let mut owners=Vec::new();let mut keys=Vec::new();
        claim_keys(m,&a,&mut r,&capture,&request,Some(&target_ref),&mut owners,&mut keys).unwrap();
        match &keys[0]{PermanentClaimKey::Outcome{source_namespace,business_outcome_token,target_id,..}=>{assert_eq!(source_namespace,"original-assessor-namespace");assert_eq!(business_outcome_token,"original-token");assert_eq!(target_id,&target.reference.id);},_=>panic!("outcome key")}
        let inst=json!({"family_map":[{"template_key":"support:quality","slot_id":"support:quality"}]});
        assert_eq!(original_outcome_link_slot(&program,m,&value,&inst).unwrap(),"support:assesses");
        value["family_slot"]=json!("other:family");value["original_payload"]["family_slot"]=json!("other:family");
        assert!(original_outcome_link_slot(&program,m,&value,&inst).is_err());
        // Lookup-only bodies exercise original slot selection; these are not
        // complete original authorization/financial replay fixtures.
        let terms=super::super::body::document(&mut producer,"workflow-slot-terms",json!({"schema":"ledger-workflow-slot-binding-terms/7","correction_source_slot":"root:corrector"})).unwrap();
        let inst=super::super::body::document(&mut producer,"workflow-slot-instantiation",json!({"schema":"ledger-workflow-slot-instantiation/1","family_map":[{"template_key":"support:quality","slot_id":"actual:family"}],"component_map":[{"template_key":"support:service","slot_id":"actual:service"}]})).unwrap();
        let bound=super::super::body::document(&mut producer,"workflow-binding",json!({"schema":"ledger-workflow-binding/2","slot_instantiation_ref":inst.reference,"bindings":[{"binding_ref":terms.reference}]})).unwrap();
        let work=super::super::body::record(&mut producer,&key,"workflow-work",format!("wevent_{}","7".repeat(64)),json!({"schema":"ledger-workflow-work-record/1","payload":{"binding_ref":bound.reference,"activity":"support:complete"}})).unwrap();
        let target=super::super::body::record(&mut producer,&key,"workflow-target",format!("wtarget_{}","3".repeat(64)),json!({"schema":"ledger-workflow-component-target/1","work_ref":work.reference,"binding_ref":bound.reference,"policy_ref":program.root_policy_ref(),"compiled_plan_ref":program.compiled_plan().reference(),"root_component_slot":"actual:service","families":[{"family_key":"support:quality","family_slot":"actual:family"}]})).unwrap();
        let family=super::super::body::record(&mut producer,&key,"workflow-family-state",format!("wfamily_{}","8".repeat(64)),json!({"schema":"ledger-workflow-family-state/1","target_ref":target.reference,"family_slot":"actual:family","revision":"3"})).unwrap();
        let rows=[&terms,&inst,&bound,&work,&target,&family];
        let views=rows.iter().map(|x|EconomicRecordView{reference:&x.reference,canonical_body:&x.canonical_body}).collect::<Vec<_>>();
        let mut r=Records::borrowed_parent_app(m,&a,&key,&views).unwrap();
        let reverse=json!({"schema":"ledger-workflow-work-reverse/2","work_ref":work.reference,"family_closures":[{"family_state_ref":family.reference,"expected_revision":"3"}]});
        let subject=subject_refs(m,&a,&mut r,&key,&reverse).unwrap();
        let slots=authority_slots(&program,m,&a,&mut r,&key,&reverse,&subject,&mut owners).unwrap();
        assert_eq!(slots,["root:corrector","support:assessor"]);
        let close=json!({"schema":"ledger-workflow-family-close-attest/1","attestation":{"subject":{"target_ref":target.reference,"family_slot":"actual:family"}}});
        let subject=subject_refs(m,&a,&mut r,&key,&close).unwrap();
        assert_eq!(authority_slots(&program,m,&a,&mut r,&key,&close,&subject,&mut owners).unwrap(),["support:assessor"]);
        let mut wrong=reverse.clone();wrong["family_closures"][0]["expected_revision"]=json!("2");
        assert!(authority_slots(&program,m,&a,&mut r,&key,&wrong,&subject,&mut owners).is_err());

    }
}
