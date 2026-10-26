//! Fixed Stage membership and the actual Work-driven declared→started edge.
//! Supplier authority and monetary closure remain separate consumers.
use super::super::selected_program::SelectedProgram;
use super::{
    body,
    data::*,
    emitted_record::ConstructedEnvelope,
    records::Records,
    shape,
    working_value::{owning_reference, parse_owned_reference, OwnedExactRef},
    CoreMeter,
};
use crate::{
    canonical::{
        self,
        audit_arena::{AuditArena, ExternalBufferLease},
    },
    domain::Timestamp,
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
    Result,
};
use num_traits::ToPrimitive;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::mem;

fn arena(m: &mut CoreMeter) -> Result<AuditArena> {
    m.representation_arena()
        .ok_or_else(|| shape::fail("owning Stage consumer"))
}
fn text(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn same(m: &mut CoreMeter, v: &Value, k: &str, want: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, want)? {
        return Err(shape::fail("Stage literal association"));
    }
    Ok(())
}
fn id(m: &mut CoreMeter, prefix: &str, domain: &[u8], tuple: &Value) -> Result<String> {
    let a = arena(m)?;
    let bytes = m.canonical_owned_representation(&a, tuple, RepresentationKind::Record)?;
    let count = w::usize_add(m, domain.len(), bytes.bytes().len())?;
    m.hash_bytes(count)?;
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes.bytes());
    let hex = canonical::hex_with_work(&hash.finalize(), m)?;
    let count = w::usize_add(m, prefix.len(), hex.len())?;
    m.write_bytes(count)?;
    Ok(format!("{prefix}{hex}"))
}
fn instance_id(m: &mut CoreMeter, key: &OperationKey, v: &Value) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = super::work_keys::scope(m, key)?;
    let tuple = a.array_from_values(
        &[
            store.value(),
            scope.value(),
            &v["agreement_id"],
            &v["chain"],
            &v["stage_name"],
        ],
        m,
    )?;
    id(
        m,
        "wstage_",
        b"bean-counter/workflow/1/stage-instance\0",
        tuple.value(),
    )
}
fn state_id(
    m: &mut CoreMeter,
    key: &OperationKey,
    instance: &ExactRef,
    revision: &Value,
) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = super::work_keys::scope(m, key)?;
    let instance = a.string_value(&instance.id, m)?;
    let tuple = a.array_from_values(
        &[store.value(), scope.value(), instance.value(), revision],
        m,
    )?;
    id(
        m,
        "wstage_state_",
        b"bean-counter/workflow/1/stage-state\0",
        tuple.value(),
    )
}
fn definition<'a>(m: &mut CoreMeter, p: &'a SelectedProgram, name: &str) -> Result<&'a Value> {
    let mut found = None;
    for d in p.source_documents() {
        m.visit("stage.original_policy")?;
        let r = d.reference();
        let expected = p.root_policy_ref();
        if shape::same_text(m, r.kind(), expected.kind())?
            && shape::same_text(m, r.id(), expected.id())?
            && shape::same_text(m, r.digest(), expected.digest())?
        {
            found = Some(d.canonical_value());
            break;
        }
    }
    let policy = found.ok_or_else(|| shape::fail("Stage original Policy"))?;
    same(m, policy, "mode", "closure_share")?;
    let stages = shape::array(m, policy, "stages")?;
    shape::named(m, stages, "name", name)
}
fn closed_instance(m: &mut CoreMeter, key: &OperationKey, r: &ExactRef, v: &Value) -> Result<()> {
    shape::closed(
        m,
        v,
        &[
            "schema",
            "instance_id",
            "customer",
            "chain",
            "agreement_id",
            "binding_ref",
            "stage_name",
            "expected",
            "authority_refs",
            "declared_at",
        ],
        &[],
    )?;
    same(m, v, "schema", "ledger-workflow-stage-instance/1")?;
    if !shape::same_text(m, &r.kind, "workflow-stage-instance")? {
        return Err(shape::fail("Stage instance Ref kind"));
    }
    let expected = instance_id(m, key, v)?;
    if !shape::same_text(m, &r.id, &expected)? {
        return Err(shape::fail("Stage permanent instance identity"));
    }
    same(m, v, "instance_id", &expected)?;
    Timestamp::parse_with_work(shape::text(m, v, "declared_at")?, m)?;
    let expected = shape::array(m, v, "expected")?;
    if w::compare(m, expected.len(), 1usize)?.is_lt()
        || w::compare(m, expected.len(), 32usize)?.is_gt()
    {
        return Err(shape::fail("Stage fixed membership bound"));
    }
    super::inventory::slots::keyed_order(m, expected, "template_input_key", false)?;
    for member in expected {
        m.visit("stage.fixed_expected_shape")?;
        shape::closed(
            m,
            member,
            &[
                "template_input_key",
                "source",
                "business_operation_id",
                "activity",
                "component_slots",
            ],
            &[],
        )?;
        let slots = shape::array(m, member, "component_slots")?;
        if w::compare(m, slots.len(), 1usize)?.is_lt()
            || w::compare(m, slots.len(), 128usize)?.is_gt()
        {
            return Err(shape::fail("Stage expected components bound"));
        }
        let mut previous: Option<&str> = None;
        for slot in slots {
            m.visit("stage.fixed_component_order")?;
            let raw = slot
                .as_str()
                .ok_or_else(|| shape::fail("Stage component Name"))?;
            if let Some(old) = previous {
                let count = w::usize_add(m, old.len(), raw.len())?;
                m.scan_bytes(count)?;
                if old >= raw {
                    return Err(shape::fail("Stage component order/duplicate"));
                }
            }
            previous = Some(raw);
        }
    }
    Ok(())
}
/// Lookup DATA for the one actual Stage head. The host must read the projection
/// under the held transaction and retain its owning instance/body/cutoff.
pub struct StageWorkHeadLocator {
    agreement: String,
    chain: String,
    stage: String,
    instance: OwnedExactRef,
    _ownership: ExternalBufferLease,
}
impl StageWorkHeadLocator {
    pub fn agreement(&self) -> &str {
        &self.agreement
    }
    pub fn chain(&self) -> &str {
        &self.chain
    }
    pub fn stage(&self) -> &str {
        &self.stage
    }
    pub fn instance_ref(&self) -> &ExactRef {
        self.instance.reference()
    }
}
pub struct WorkInvocationHeadLocator {
    original: OwnedExactRef,
    id: String,
}
impl WorkInvocationHeadLocator {
    pub fn original_ref(&self) -> &ExactRef {
        self.original.reference()
    }
    pub fn invocation_id(&self) -> &str {
        &self.id
    }
}
pub struct WorkInvocationHeadLocators {
    rows: Vec<WorkInvocationHeadLocator>,
    _ownership: ExternalBufferLease,
}
impl WorkInvocationHeadLocators {
    pub fn heads(&self) -> &[WorkInvocationHeadLocator] {
        &self.rows
    }
}
/// Actual Work's named original Invocation DATA only. The host reads the exact
/// held mutable heads and original supplier producer/authority inputs; neither
/// this list nor a current head certifies original authorization or exposure.
pub fn select_work_invocation_head_locators(
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<WorkInvocationHeadLocators> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("exact Invocation lifecycle recipe required"));
        }
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let a = s.arena().shared_handle();
        let m = s.meter_mut();
        same(m, p.value(), "schema", "ledger-workflow-work/1")?;
        let refs = shape::array(m, p.value(), "invocation_refs")?;
        if w::compare(m, refs.len(), 16usize)?.is_gt() {
            return Err(shape::fail("Work Invocation bound"));
        }
        let mut size = mem::size_of::<WorkInvocationHeadLocators>();
        for _ in refs {
            m.visit("work.invocation_locator_dimension")?;
            size = w::usize_add(m, size, mem::size_of::<WorkInvocationHeadLocator>() + 128)?;
        }
        let ownership = a.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(refs.len())
            .map_err(|_| shape::fail("Work Invocation locator allocation"))?;
        let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        for reference in refs {
            m.visit("work.named_original_invocation")?;
            let original = parse_owned_reference(m, &a, reference)?;
            let invocation = r.get(
                m,
                key,
                original.reference(),
                "workflow-invocation",
                "ledger-workflow-invocation/1",
            )?;
            same(m, &invocation, "revision", "1")?;
            same(m, &invocation, "state", "authorized")?;
            if invocation.get("prior_ref").is_some() {
                return Err(shape::fail("original Invocation has predecessor"));
            }
            for (field, work_field) in [
                ("original_binding_ref", "binding_ref"),
                ("customer", "customer"),
                ("business_operation_id", "business_operation_id"),
                ("chain_id", "chain"),
                ("activity", "activity"),
            ] {
                shape::equal(m, &invocation[field], &p.value()[work_field])?;
            }
            let invocation_id = shape::text(m, &invocation, "invocation_id")?;
            if !shape::same_text(m, invocation_id, &original.reference().id)? {
                return Err(shape::fail("original Invocation full identity"));
            }
            let scope = super::work_keys::scope(m, key)?;
            let tuple = a.array_from_values(
                &[
                    scope.value(),
                    &invocation["agreement_id"],
                    &invocation["business_operation_id"],
                ],
                m,
            )?;
            let id = body::semantic_id(m, "winv_", "workflow-invocation", tuple.value())?;
            if !shape::same_text(m, &id, invocation_id)? {
                return Err(shape::fail("Invocation permanent semantic identity"));
            }
            for old in &rows {
                m.visit("work.invocation_locator_unique")?;
                let old: &WorkInvocationHeadLocator = old;
                if shape::same_text(m, &old.id, &id)? {
                    return Err(shape::fail("duplicate original Work Invocation"));
                }
            }
            rows.push(WorkInvocationHeadLocator { original, id });
        }
        Ok(WorkInvocationHeadLocators {
            rows,
            _ownership: ownership,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
pub fn select_stage_work_head_locator(
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<Option<StageWorkHeadLocator>> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let Some(reference) = p.value().get("stage_instance_ref") else {
            return Ok(None);
        };
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("exact Stage recipe required"));
        }
        let a = s.arena().shared_handle();
        let m = s.meter_mut();
        let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        let owned = parse_owned_reference(m, &a, reference)?;
        let instance = r.get(
            m,
            key,
            owned.reference(),
            "workflow-stage-instance",
            "ledger-workflow-stage-instance/1",
        )?;
        closed_instance(m, key, owned.reference(), &instance)?;
        for k in ["customer", "chain", "binding_ref"] {
            shape::equal(m, &instance[k], &p.value()[k])?;
        }
        let agreement = shape::text(m, &instance, "agreement_id")?;
        let chain = shape::text(m, &instance, "chain")?;
        let stage = shape::text(m, &instance, "stage_name")?;
        definition(m, program, stage)?;
        let mut n = w::usize_add(m, agreement.len(), chain.len())?;
        n = w::usize_add(m, n, stage.len())?;
        n = w::usize_add(m, n, mem::size_of::<StageWorkHeadLocator>())?;
        let owner = a.reserve_external_payload(n, m)?;
        Ok(Some(StageWorkHeadLocator {
            agreement: text(m, agreement)?,
            chain: text(m, chain)?,
            stage: text(m, stage)?,
            instance: owned,
            _ownership: owner,
        }))
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

pub(super) struct WorkTransition {
    pub record: Option<ConstructedEnvelope>,
    pub ownership: ExternalBufferLease,
}
/// Called only after the SAME Work interpreter has normalized facts, checked
/// original/current Work authority and constructed its permanent Work/claim.
pub(super) fn work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    selected: (&ExactRef, &ExactRef, &Value, &DraftStateData),
) -> Result<Option<WorkTransition>> {
    let Some(raw) = p.get("stage_instance_ref") else {
        return Ok(None);
    };
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("staged Work requires exact Stage recipe"));
    }
    let (work, authority, binding, state) = selected;
    let a = arena(m)?;
    let inst_ref = shape::reference(m, &binding["slot_instantiation_ref"])?;
    let inst = r.get(
        m,
        &c.key,
        &inst_ref,
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let ownership = a.reserve_external_payload(
        2 * mem::size_of::<WorkTransition>() + mem::size_of::<ExactRef>() + 64 + 128 + 71 + 1024,
        m,
    )?;
    let reference = parse_owned_reference(m, &a, raw)?;
    let instance = r.get(
        m,
        &c.key,
        reference.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    closed_instance(m, &c.key, reference.reference(), &instance)?;
    for k in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &instance[k], &p[k])?;
    }
    shape::equal(m, &instance["agreement_id"], &binding["agreement_id"])?;
    let name = shape::text(m, &instance, "stage_name")?;
    let def = definition(m, program, name)?;
    same(
        m,
        &def["window"],
        "anchor",
        "verified_predecessor_occurred_at",
    )?;
    let declared = Timestamp::parse_with_work(shape::text(m, &instance, "declared_at")?, m)?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?;
    if w::compare(m, declared.micros(), now.micros())?.is_gt() {
        return Err(shape::fail("Work before Stage declaration"));
    }
    let expected = shape::array(m, &instance, "expected")?;
    let templates = shape::array(m, def, "expected_slots")?;
    if !w::compare(m, expected.len(), templates.len())?.is_eq() {
        return Err(shape::fail("Stage exact fixed template membership"));
    }
    let mut member = None;
    for row in expected {
        m.visit("stage.expected_work_association")?;
        let key = shape::text(m, row, "template_input_key")?;
        let template = shape::named(m, templates, "key", key)?;
        shape::equal(m, &row["activity"], &template["activity"])?;
        let actual = shape::array(m, row, "component_slots")?;
        let keys = shape::array(m, template, "component_keys")?;
        if !w::compare(m, actual.len(), keys.len())?.is_eq() {
            return Err(shape::fail("Stage exact component inventory"));
        }
        let maps = shape::array(m, &inst, "component_map")?;
        for key in keys {
            m.visit("stage.original_instantiated_component")?;
            let key = key
                .as_str()
                .ok_or_else(|| shape::fail("Stage template component Name"))?;
            let map = shape::named(m, maps, "template_key", key)?;
            if !shape::member(m, actual, &map["slot_id"])? {
                return Err(shape::fail("Stage expected component mapping"));
            }
        }
        let slot = shape::text(m, template, "source_slot")?;
        let nominations = shape::array(m, binding, "source_slots")?;
        let nomination = shape::named(m, nominations, "name", slot)?;
        let mut nominated = false;
        for nominee in shape::array(m, nomination, "sources")? {
            m.visit("stage.expected_source_nominee")?;
            if shape::same(m, nominee, &row["source"])? {
                nominated = true;
            }
        }
        if !nominated {
            return Err(shape::fail("Stage expected accepted source nomination"));
        }
        let row_source = shape::text(m, row, "source")?;
        if shape::same(
            m,
            &row["business_operation_id"],
            &p["business_operation_id"],
        )? && shape::same_text(m, row_source, &c.key.source)?
        {
            if member.is_some() {
                return Err(shape::fail("duplicate Stage business completion"));
            }
            shape::equal(m, &row["activity"], &p["activity"])?;
            member = Some(row);
        }
    }
    if member.is_none() {
        return Err(shape::fail("Work is not an expected Stage member"));
    }
    let mut head = None;
    let agreement = shape::text(m, &instance, "agreement_id")?;
    let chain = shape::text(m, &instance, "chain")?;
    for h in &state.stage_heads {
        m.visit("stage.actual_held_head")?;
        if shape::same_text(m, &h.agreement, agreement)?
            && shape::same_text(m, &h.chain, chain)?
            && shape::same_text(m, &h.stage, name)?
        {
            if head.is_some() {
                return Err(shape::fail("ambiguous Stage head"));
            }
            head = Some(h);
        }
    }
    let head = head.ok_or_else(|| shape::fail("missing declared Stage head"))?;
    let old = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-stage-state",
        "ledger-workflow-stage-state/1",
    )?;
    let instance_value = owning_reference(m, &a, reference.reference())?;
    shape::equal(m, &old["stage_instance_ref"], instance_value.value())?;
    same(m, &old, "revision", &head.revision)?;
    let identity = state_id(m, &c.key, reference.reference(), &old["revision"])?;
    if !shape::same_text(m, &identity, &head.reference.id)? {
        return Err(shape::fail("Stage state identity"));
    }
    let status = shape::text(m, &old, "state")?;
    if shape::same_text(m, status, "started")? {
        shape::closed(
            m,
            &old,
            &[
                "schema",
                "stage_instance_ref",
                "revision",
                "state",
                "operation_key",
                "authority_ref",
                "accepted_at",
                "prior_ref",
                "first_work_ref",
                "first_work_accepted_at",
                "activation_deadline",
            ],
            &[],
        )?;
        same(m, &old, "revision", "2")?;
        // Preserve this immutable clock; later members do not emit a new state.
        let first = shape::reference(m, &old["first_work_ref"])?;
        let first = r.get(
            m,
            &c.key,
            &first,
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        shape::equal(
            m,
            &first["payload"]["stage_instance_ref"],
            instance_value.value(),
        )?;
        shape::equal(m, &first["accepted_at"], &old["first_work_accepted_at"])?;
        shape::equal(m, &first["key"], &old["operation_key"])?;
        shape::equal(m, &first["accepted_at"], &old["accepted_at"])?;
        for field in ["customer", "chain", "binding_ref"] {
            shape::equal(m, &first["payload"][field], &instance[field])?;
        }
        let first_source = shape::text(m, &first["key"], "source")?;
        let mut matched = false;
        for member in expected {
            m.visit("stage.first_expected_work")?;
            let source = shape::text(m, member, "source")?;
            if shape::same_text(m, source, first_source)?
                && shape::same(
                    m,
                    &member["business_operation_id"],
                    &first["payload"]["business_operation_id"],
                )?
            {
                shape::equal(m, &member["activity"], &first["payload"]["activity"])?;
                if matched {
                    return Err(shape::fail("ambiguous first Stage member"));
                }
                matched = true;
            }
        }
        if !matched {
            return Err(shape::fail("first Work outside fixed Stage membership"));
        }
        let claim_id = shape::text(m, &first, "claim_id")?;
        let (claim_ref, claim) = r.by_identity(
            m,
            &c.key,
            "workflow-work-claim",
            claim_id,
            "ledger-workflow-work-claim/1",
        )?;
        super::inventory::entry::historical_work_claim(m, c, &claim_ref, &claim, &first)?;
        let permanent = super::work_keys::decision(m, claim_id)?;
        let permanent = a.string_value(&permanent, m)?;
        let revision = a.string_value("1", m)?;
        let tuple = a.array_from_values(&[permanent.value(), revision.value()], m)?;
        let decision_id =
            body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
        let (_, decision) = r.by_identity(
            m,
            &c.key,
            "workflow-decision",
            &decision_id,
            "ledger-workflow-decision-revision/1",
        )?;
        shape::equal(m, &decision["subject_ref"], &old["first_work_ref"])?;
        shape::equal(m, &decision["authority_ref"], &old["authority_ref"])?;
        shape::equal(m, &decision["accepted_at"], &first["accepted_at"])?;
        same(m, &decision, "revision", "1")?;
        let initial = shape::reference(m, &old["prior_ref"])?;
        let initial = r.get(
            m,
            &c.key,
            &initial,
            "workflow-stage-state",
            "ledger-workflow-stage-state/1",
        )?;
        shape::closed(
            m,
            &initial,
            &[
                "schema",
                "stage_instance_ref",
                "revision",
                "state",
                "operation_key",
                "authority_ref",
                "accepted_at",
            ],
            &[],
        )?;
        same(m, &initial, "revision", "1")?;
        same(m, &initial, "state", "declared")?;
        shape::equal(m, &initial["stage_instance_ref"], instance_value.value())?;
        let first_at = Timestamp::parse_with_work(shape::text(m, &first, "accepted_at")?, m)?;
        if w::compare(m, first_at.micros(), declared.micros())?.is_lt()
            || w::compare(m, first_at.micros(), now.micros())?.is_gt()
        {
            return Err(shape::fail("Stage first Work chronological bound"));
        }
        let duration = shape::text(m, &def["window"], "activation_deadline_us")?;
        let duration = super::arithmetic::integer(m, duration, false)?;
        if super::meter::cmp(m, &duration, &0.into())?.is_lt() {
            return Err(shape::fail("Stage activation duration"));
        }
        let deadline = super::meter::add(m, &first_at.micros().into(), &duration)?;
        let raw = shape::text(m, &old, "activation_deadline")?;
        let actual = Timestamp::parse_with_work(raw, m)?;
        if !super::meter::cmp(m, &deadline, &actual.micros().into())?.is_eq() {
            return Err(shape::fail("Stage immutable original activation deadline"));
        }
        return Ok(None);
    }
    if !shape::same_text(m, status, "declared")? {
        return Err(crate::Error::new("TARGET_CLOSED", "Stage is terminal"));
    }
    shape::closed(
        m,
        &old,
        &[
            "schema",
            "stage_instance_ref",
            "revision",
            "state",
            "operation_key",
            "authority_ref",
            "accepted_at",
        ],
        &[],
    )?;
    same(m, &old, "revision", "1")?;
    let duration = shape::text(m, &def["window"], "activation_deadline_us")?;
    let duration = super::arithmetic::integer(m, duration, false)?;
    if super::meter::cmp(m, &duration, &0.into())?.is_lt() {
        return Err(shape::fail("Stage activation duration"));
    }
    let deadline = super::meter::add(m, &now.micros().into(), &duration)?
        .to_i64()
        .ok_or_else(|| shape::fail("Stage deadline overflow"))?;
    let deadline = Timestamp::from_micros_with_work(deadline, m)?;
    let deadline = a.string_value(deadline.as_str(), m)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let authority = owning_reference(m, &a, authority)?;
    let work = owning_reference(m, &a, work)?;
    let prior = owning_reference(m, &a, &head.reference)?;
    let constants = m.parse_owned_representation(
        &a,
        br#"{"schema":"ledger-workflow-stage-state/1","revision":"2","state":"started"}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let value = a.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            ("stage_instance_ref", instance_value.value()),
            ("revision", &constants.value()["revision"]),
            ("state", &constants.value()["state"]),
            ("operation_key", key.value()),
            ("authority_ref", authority.value()),
            ("accepted_at", at.value()),
            ("prior_ref", prior.value()),
            ("first_work_ref", work.value()),
            ("first_work_accepted_at", at.value()),
            ("activation_deadline", deadline.value()),
        ],
        m,
    )?;
    let id = state_id(
        m,
        &c.key,
        reference.reference(),
        &constants.value()["revision"],
    )?;
    let record = body::record_borrowed(m, &c.key, "workflow-stage-state", id, value.value())?;
    Ok(Some(WorkTransition {
        record: Some(record),
        ownership,
    }))
}
