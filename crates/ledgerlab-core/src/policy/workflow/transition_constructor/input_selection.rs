//! Coordinator locator DATA before content-addressed observations exist.
//! This is not authority, a completed immediate closure, or an economic result.
//! The economic child repeats the same selector and all semantic checks.
use super::super::selected_program::SelectedProgram;
use super::{
    data::{EconomicRecordView, ExactRef, OperationKey},
    records::{OwnedConsumed, Records},
    shape, CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
    Result,
};
use serde_json::Value;
use std::mem;

/// Actual held source operands, without an invented observation Ref. Bodies and
/// original/current custody stay in the host's existing owning inventory.
pub struct SourceAuthorityCapture<'a> {
    pub source_slot: &'a str,
    pub source: &'a str,
    pub principal: &'a str,
    pub canonical_economic_namespace: &'a str,
    pub enrollment_ref: &'a ExactRef,
    pub current_state_ref: &'a ExactRef,
    pub current_grant_ref: &'a ExactRef,
    pub original_grant_refs: &'a [ExactRef],
    pub binding_ref: &'a ExactRef,
    pub accepted_agreement_ref: &'a ExactRef,
    pub activation_ref: &'a ExactRef,
}
/// Borrowed real capture. No current observation, caller-complete boolean,
/// economic child, or new resource allowance is supplied by this view.
pub struct EconomicCapture<'a> {
    pub key: &'a OperationKey,
    pub actor: &'a str,
    pub canonical_economic_namespace: &'a str,
    pub installation_mode: &'a str,
    pub received_at: &'a str,
    pub accepted_at: &'a str,
    pub sources: &'a [SourceAuthorityCapture<'a>],
}
pub struct WorkSourceLocator<'a> {
    source_slot: &'a str,
    frame: &'a SourceAuthorityCapture<'a>,
    _ownership: ExternalBufferLease,
}
impl<'a> WorkSourceLocator<'a> {
    pub fn source_slot(&self) -> &str {
        self.source_slot
    }
    pub fn captured_source(&self) -> &SourceAuthorityCapture<'a> {
        self.frame
    }
}
/// Direct original/current locator seeds only. The coordinator must expand the
/// exact recipe closure, implicit locators and held heads before making its F
/// header. These rows cannot stand in for that closure or authorize admission.
pub struct WorkInputSeeds<'a> {
    source: WorkSourceLocator<'a>,
    records: OwnedConsumed,
    predecessor: Option<WorkHeadLocators>,
    _ownership: ExternalBufferLease,
}
impl<'a> WorkInputSeeds<'a> {
    pub fn source(&self) -> &WorkSourceLocator<'a> {
        &self.source
    }
    pub fn into_predecessor_work_heads(self) -> Option<WorkHeadLocators> {
        self.predecessor
    }
    pub fn predecessor_work_heads(&self) -> Option<&WorkHeadLocators> {
        self.predecessor.as_ref()
    }
    pub fn direct_refs(&self) -> &[ExactRef] {
        &self.records.references
    }
}

/// SAME product/activity/source-slot locator used again inside the economic
/// work constructor. Each invocation pays its own actual lookup/comparisons.
pub(super) fn work_activity<'a>(
    program: &'a SelectedProgram,
    p: &Value,
    m: &mut CoreMeter,
) -> Result<(&'a Value, &'a Value)> {
    let product_ref = shape::field(m, p, "product_ref")?;
    shape::closed(m, product_ref, &["kind", "id", "digest"], &[])?;
    let kind = shape::text(m, product_ref, "kind")?;
    let id = shape::text(m, product_ref, "id")?;
    let digest = shape::text(m, product_ref, "digest")?;
    super::records::reference_parts(m, kind, id, digest)?;
    let mut selected = None;
    for document in program.source_documents() {
        m.visit("work.selected_product_locator")?;
        let reference = document.reference();
        if shape::same_text(m, kind, reference.kind())?
            && shape::same_text(m, id, reference.id())?
            && shape::same_text(m, digest, reference.digest())?
        {
            if selected.is_some() {
                return Err(shape::fail("ambiguous selected original product"));
            }
            selected = Some(document.canonical_value());
        }
    }
    let product = selected.ok_or_else(|| shape::fail("original product substitution"))?;
    let name = shape::text(m, p, "activity")?;
    let activities = shape::array(m, product, "activities")?;
    let activity = shape::named(m, activities, "name", name)?;
    let primitive = shape::text(m, activity, "primitive")?;
    if !shape::same_text(m, primitive, "work")? {
        return Err(shape::fail("work primitive"));
    }
    let actual_schema = shape::field(m, p, "fact_schema_ref")?;
    let selected_schema = shape::field(m, activity, "fact_schema")?;
    shape::equal(m, actual_schema, selected_schema)?;
    // Validate the selector now; the borrowed value remains program-owned.
    shape::text(m, activity, "source_slot")?;
    Ok((product, activity))
}

/// First parent-paid locator step for work.accept. It deliberately exposes no
/// input-set or prefix digest: complete recipe closure and independent current
/// authority are still required before observation construction/admission.
pub fn select_work_source_frame<'a>(
    program: &'a SelectedProgram,
    capture: &'a EconomicCapture<'a>,
    raw: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<WorkSourceLocator<'a>> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        let payload = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        super::codec::validate_selected_types(m, payload.value())?;
        let schema = shape::text(m, payload.value(), "schema")?;
        if !shape::same_text(m, schema, "ledger-workflow-work/1")? {
            return Err(shape::fail("work source locator payload"));
        }
        let (_, activity) = work_activity(program, payload.value(), m)?;
        let slot = shape::text(m, activity, "source_slot")?;
        let binding = shape::field(m, payload.value(), "binding_ref")?;
        shape::closed(m, binding, &["kind", "id", "digest"], &[])?;
        let k = shape::text(m, binding, "kind")?;
        let id = shape::text(m, binding, "id")?;
        let d = shape::text(m, binding, "digest")?;
        super::records::reference_parts(m, k, id, d)?;
        let mut frame = None;
        for source in capture.sources {
            m.visit("work.captured_source_slot_locator")?;
            if shape::same_text(m, source.source_slot, slot)? {
                // This is the actual submitting work source. A dependent's
                // separately captured identity must never rewrite the root key.
                if !shape::same_text(m, source.source, &capture.key.source)?
                    || !shape::same_text(m, source.principal, capture.actor)?
                    || !shape::same_text(
                        m,
                        source.canonical_economic_namespace,
                        capture.canonical_economic_namespace,
                    )?
                {
                    continue;
                }
                if !shape::same_text(m, k, &source.binding_ref.kind)?
                    || !shape::same_text(m, id, &source.binding_ref.id)?
                    || !shape::same_text(m, d, &source.binding_ref.digest)?
                {
                    continue;
                }
                if frame.is_some() {
                    return Err(shape::fail("ambiguous captured source slot"));
                }
                frame = Some(source);
            }
        }
        let frame = frame.ok_or_else(|| shape::fail("actual captured work source slot missing"))?;
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning work source locator"))?;
        let owner = arena.reserve_external_payload(mem::size_of::<WorkSourceLocator>(), m)?;
        m.write_bytes(mem::size_of::<WorkSourceLocator>())?;
        Ok(WorkSourceLocator {
            source_slot: slot,
            frame,
            _ownership: owner,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

/// Fixed locator DATA from the same original activity/rule/Policy association
/// that the consuming Stage binding interpreter repeats. No first-row fallback.
fn work_terms_row<'a>(
    program: &SelectedProgram,
    payload: &Value,
    binding: &'a Value,
    m: &mut CoreMeter,
) -> Result<(&'a Value, &'static str)> {
    let rows = shape::array(m, binding, "bindings")?;
    if program.uses_stage_lifecycle_recipe() {
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning Stage work selector"))?;
        let policy = super::working_value::owning_definition(m, &arena, program.root_policy_ref())?;
        shape::equal(m, &binding["policy_ref"], policy.value())?;
        for field in ["policy_ref", "product_ref"] {
            shape::equal(m, &binding[field], &payload[field])?;
        }
        if rows.is_empty() || w::compare(m, rows.len(), 16usize)?.is_gt() {
            return Err(shape::fail("Stage work binding slot bound"));
        }
        super::inventory::slots::keyed_order(m, rows, "slot", false)?;
        let definitions = shape::array(m, program.original_policy_value(), "binding_slots")?;
        if !w::compare(m, rows.len(), definitions.len())?.is_eq() {
            return Err(shape::fail("Stage work complete original binding slots"));
        }
        for row in rows {
            let name = shape::text(m, row, "slot")?;
            shape::named(m, definitions, "name", name)?;
        }
        let mut selected = None;
        for instruction in shape::array(
            m,
            program.compiled_plan().canonical_value(),
            "typed_instructions",
        )? {
            m.visit("work.parent_stage_actual_retail_slot")?;
            if !shape::same(m, &instruction["rule"]["activity"], &payload["activity"])? {
                continue;
            }
            let name = shape::text(m, &instruction["rule"], "binding_slot")?;
            let definition = shape::named(m, definitions, "name", name)?;
            let book = shape::text(m, definition, "book")?;
            if !shape::same_text(m, book, "retail")? {
                continue;
            }
            let candidate = shape::named(m, rows, "slot", name)?;
            if let Some(old) = selected {
                shape::equal(m, old, candidate)?;
            }
            selected = Some(candidate);
        }
        let row = selected.ok_or_else(|| shape::fail("Stage work's exact retail binding slot"))?;
        Ok((row, "ledger-workflow-slot-binding-terms/3"))
    } else {
        if !w::compare(m, rows.len(), 1usize)?.is_eq() {
            return Err(crate::Error::new("UNSUPPORTED_COMPOSITION", "work parent app binding selector requires the same implemented single retail binding"));
        }
        Ok((
            &rows[0],
            if program.uses_app_cost_causal_recipe() {
                "ledger-workflow-slot-binding-terms/7"
            } else {
                "ledger-workflow-slot-binding-terms/6"
            },
        ))
    }
}

fn stage_work_terms_association(
    m: &mut CoreMeter,
    key: &OperationKey,
    payload: &Value,
    binding: &Value,
    selected_row: &Value,
    terms: &Value,
) -> Result<()> {
    // Binding2 deliberately has no scope member. The actual held operation
    // key is the scoped association, just as in the consuming interpreter.
    let scope = super::work_keys::scope(m, key)?;
    shape::equal(m, &terms["scope"], scope.value())?;
    for field in ["agreement_id", "customer"] {
        shape::equal(m, &terms[field], &binding[field])?;
    }
    shape::equal(m, &terms["slot"], &selected_row["slot"])?;
    let activities = shape::array(m, terms, "allowed_activities")?;
    if !shape::member(m, activities, &payload["activity"])? {
        return Err(shape::fail(
            "Stage work activity absent original retail Terms",
        ));
    }
    Ok(())
}

/// Actual original Binding/Terms source group and initial-state locator. No
/// authority predicate or original receipt is inferred from this selection.
/// In particular the matched original group is DERIVED from accepted Terms,
/// rather than accepting a possibly incomplete capture slice as its proof.
pub fn select_work_input_seeds<'a>(
    program: &'a SelectedProgram,
    capture: &'a EconomicCapture<'a>,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<WorkInputSeeds<'a>> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        s.meter_mut().visit("work.parent_original_app_recipe")?;
        if !program.uses_app_owned_cost_recipe() {
            return Err(crate::Error::new(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "app work seed selector requires its exact original app compiler recipe",
            ));
        }
        let source = select_work_source_frame(program, capture, raw, s)?;
        let frame = source.captured_source();
        let arena = s.arena().shared_handle();
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        let mut r = Records::borrowed_parent_selected(m, &arena, capture.key, inputs, program)?;
        let binding = r.get(
            m,
            capture.key,
            frame.binding_ref,
            "workflow-binding",
            "ledger-workflow-binding/2",
        )?;
        let accepted = r.get(
            m,
            capture.key,
            frame.accepted_agreement_ref,
            "workflow-accepted-agreement",
            "ledger-workflow-accepted-agreement/1",
        )?;
        let activation = r.get(
            m,
            capture.key,
            frame.activation_ref,
            "workflow-activation",
            "ledger-workflow-activation-transition/1",
        )?;
        for body in [&*accepted, &*activation] {
            m.visit("work.parent_binding_edge")?;
            let actual = shape::field(m, body, "binding_ref")?;
            let expected = shape::field(m, p.value(), "binding_ref")?;
            shape::equal(m, actual, expected)?;
        }
        let (selected_row, schema) = work_terms_row(program, p.value(), &binding, m)?;
        let terms_value = shape::field(m, selected_row, "binding_ref")?;
        let terms_ref = super::working_value::parse_owned_reference(m, &arena, terms_value)?;
        let terms = r.get(
            m,
            capture.key,
            terms_ref.reference(),
            "workflow-slot-terms",
            schema,
        )?;
        if program.uses_stage_lifecycle_recipe() {
            stage_work_terms_association(
                m,
                capture.key,
                p.value(),
                &binding,
                selected_row,
                &terms,
            )?;
        }
        let originals = shape::array(m, &terms, "source_grant_refs")?;
        let binding_grants = shape::array(m, &binding, "authority_refs")?;
        let mut count = 0usize;
        for value in originals {
            m.visit("work.parent_original_grant_locator")?;
            if !shape::member(m, binding_grants, value)? {
                return Err(shape::fail("work parent grant absent original Binding"));
            }
            let reference = super::working_value::parse_owned_reference(m, &arena, value)?;
            let grant = r.get(
                m,
                capture.key,
                reference.reference(),
                "workflow-source-grant",
                "ledger-workflow-source-grant/1",
            )?;
            if !source_grant_matches(
                m,
                &grant,
                frame.source,
                frame.principal,
                frame.enrollment_ref,
            )? {
                continue;
            }
            let mut matches = 0usize;
            for captured in frame.original_grant_refs {
                m.visit("work.parent_exact_original_group")?;
                if shape::same_ref(m, reference.reference(), captured)? {
                    matches = w::usize_add(m, matches, 1)?;
                }
            }
            if !w::compare(m, matches, 1usize)?.is_eq() {
                return Err(shape::fail("work parent original source group mismatch"));
            }
            count = w::usize_add(m, count, 1)?;
        }
        if !w::compare(m, count, frame.original_grant_refs.len())?.is_eq()
            || w::compare(m, count, 0usize)?.is_eq()
        {
            return Err(shape::fail(
                "work parent original source group omitted or extra",
            ));
        }
        let enrollment = r.get(
            m,
            capture.key,
            frame.enrollment_ref,
            "workflow-source-enrollment",
            "ledger-workflow-source-enrollment/1",
        )?;
        r.get(
            m,
            capture.key,
            frame.current_state_ref,
            "workflow-source-state",
            "ledger-workflow-source-state/1",
        )?;
        r.get(
            m,
            capture.key,
            frame.current_grant_ref,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        let enrollment_raw = shape::text(m, &enrollment, "enrollment_id")?;
        let enrollment_id = arena.string_value(enrollment_raw, m)?;
        let store = arena.string_value(&capture.key.logical_store_id, m)?;
        let left = arena.string_value(&capture.key.scope[0], m)?;
        let right = arena.string_value(&capture.key.scope[1], m)?;
        let scope = arena.array_from_values(&[left.value(), right.value()], m)?;
        let revision = arena.string_value("1", m)?;
        let tuple = arena.array_from_values(
            &[
                store.value(),
                scope.value(),
                enrollment_id.value(),
                revision.value(),
            ],
            m,
        )?;
        let initial_id =
            super::body::semantic_id(m, "wsource_", "workflow-source-state", tuple.value())?;
        let (_, initial) = r.by_identity(
            m,
            capture.key,
            "workflow-source-state",
            &initial_id,
            "ledger-workflow-source-state/1",
        )?;
        let initial_value = shape::field(m, &initial, "current_grant_ref")?;
        let initial_grant = super::working_value::parse_owned_reference(m, &arena, initial_value)?;
        r.get(
            m,
            capture.key,
            initial_grant.reference(),
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        let predecessor = if program.uses_declared_work_link_recipe() {
            match super::declared_links::validate_inputs(m, program, p.value())? {
                Some(input) => Some(retained_work_heads(
                    program,
                    m,
                    &mut r,
                    capture.key,
                    &input["predecessor"],
                )?),
                None => None,
            }
        } else {
            None
        };
        let owner = arena.reserve_external_payload(mem::size_of::<WorkInputSeeds>(), m)?;
        m.write_bytes(mem::size_of::<WorkInputSeeds>())?;
        Ok(WorkInputSeeds {
            source,
            records: r.into_consumed_owned(),
            predecessor,
            _ownership: owner,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

/// SAME original grant grouping in the parent DATA selector and the consuming
/// source validator. The latter still performs every grant/ceiling/right check.
pub(super) fn source_grant_matches(
    m: &mut CoreMeter,
    grant: &Value,
    source: &str,
    actor: &str,
    enrollment: &ExactRef,
) -> Result<bool> {
    let actual_source = shape::text(m, grant, "source")?;
    let principal = shape::text(m, grant, "principal_id")?;
    if !shape::same_text(m, actual_source, source)? || !shape::same_text(m, principal, actor)? {
        return Ok(false);
    }
    let value = shape::field(m, grant, "original_ceiling_ref")?;
    shape::closed(m, value, &["kind", "id", "digest"], &[])?;
    for (field, expected) in [
        ("kind", &enrollment.kind),
        ("id", &enrollment.id),
        ("digest", &enrollment.digest),
    ] {
        let actual = shape::text(m, value, field)?;
        if !shape::same_text(m, actual, expected)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Lookup DATA only. Target IDs deliberately do not pretend to be full Refs:
/// their bodies include pricing results produced only inside the economic child.
pub struct WorkFamilyHeadLocator {
    target_id: String,
    family_slot: String,
    decision_id: String,
}
impl WorkFamilyHeadLocator {
    pub fn target_id(&self) -> &str {
        &self.target_id
    }
    pub fn family_slot(&self) -> &str {
        &self.family_slot
    }
    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }
}
pub struct WorkHeadLocators {
    claim: super::candidate::PermanentClaimKey,
    claim_id: String,
    event_id: String,
    root_decision_id: String,
    chain: super::candidate::FinancialHeadKey,
    families: Vec<WorkFamilyHeadLocator>,
    app_decisions: Vec<String>,
    _ownership: ExternalBufferLease,
}
impl WorkHeadLocators {
    pub fn work_claim_key(&self) -> &super::candidate::PermanentClaimKey {
        &self.claim
    }
    pub fn work_claim_id(&self) -> &str {
        &self.claim_id
    }
    pub fn work_event_id(&self) -> &str {
        &self.event_id
    }
    pub fn root_decision_id(&self) -> &str {
        &self.root_decision_id
    }
    pub fn chain_key(&self) -> &super::candidate::FinancialHeadKey {
        &self.chain
    }
    pub fn family_keys(&self) -> &[WorkFamilyHeadLocator] {
        &self.families
    }
    pub fn app_decision_ids(&self) -> &[String] {
        &self.app_decisions
    }
}
type HeadRowVisitor<'a> = dyn FnMut(&str, Option<&str>, &mut CoreMeter) -> Result<()> + 'a;
pub(super) fn head_rows(
    program: &SelectedProgram,
    inputs: (&Value, &Value, Option<(&Value, &str)>),
    inst: &Value,
    m: &mut CoreMeter,
    f: &mut HeadRowVisitor<'_>,
) -> Result<()> {
    let (p, context, operational) = inputs;
    if program.uses_work_lifecycle_recipe() {
        let status = shape::text(m, p, "status")?;
        if shape::same_text(m, status, "failed")? {
            return Ok(());
        }
    }
    let activity = shape::field(m, p, "activity")?;
    for instruction in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        m.visit("work.parent_head_instruction")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if shape::same_text(m, opcode, "evaluate_family")? {
            let family = shape::field(m, instruction, "family")?;
            let selector = shape::field(m, family, "selector")?;
            let target_activity = shape::field(m, selector, "target_activity")?;
            if !shape::same(m, target_activity, activity)? {
                continue;
            }
            let component = shape::text(m, selector, "component_key")?;
            if !program.root_is_selected_for_work_operational(
                m,
                p,
                component,
                (context, operational),
            )? {
                continue;
            }
            let mappings = shape::array(m, inst, "component_map")?;
            let mapping = shape::named(m, mappings, "template_key", component)?;
            let root_slot = shape::text(m, mapping, "slot_id")?;
            let family_key = shape::text(m, family, "family_key")?;
            let mappings = shape::array(m, inst, "family_map")?;
            let mapping = shape::named(m, mappings, "template_key", family_key)?;
            let slot = shape::text(m, mapping, "slot_id")?;
            f(root_slot, Some(slot), m)?;
        } else if shape::same_text(m, opcode, "evaluate_rule")? {
            let rule = shape::field(m, instruction, "rule")?;
            let rule_activity = shape::field(m, rule, "activity")?;
            if !shape::same(m, rule_activity, activity)? {
                continue;
            }
            let formula = shape::field(m, rule, "formula")?;
            let kind = shape::text(m, formula, "kind")?;
            if !shape::same_text(m, kind, "app_final_cost")? {
                continue;
            }
            let component = shape::text(m, rule, "component_key")?;
            let mappings = shape::array(m, inst, "component_map")?;
            let mapping = shape::named(m, mappings, "template_key", component)?;
            let slot = shape::text(m, mapping, "slot_id")?;
            f(slot, None, m)?;
        }
    }
    Ok(())
}
fn head_copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
/// SAME semantic-ID recipes as owning work/target/original-app reservation
/// construction. This performs no pricing or authority check and proves no
/// head/claim absence. Actual held query/current/original proof remains host
/// owned; the single economic child repeats every semantic selection.
pub fn select_work_head_locators(
    program: &SelectedProgram,
    capture: &EconomicCapture<'_>,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<WorkHeadLocators> {
    let result = (|| {
        let source = select_work_source_frame(program, capture, raw, s)?;
        let arena = s.arena().shared_handle();
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        let mut records =
            Records::borrowed_parent_selected(m, &arena, capture.key, inputs, program)?;
        let binding_schema = if program.uses_app_owned_cost_recipe() {
            "ledger-workflow-binding/2"
        } else {
            "ledger-workflow-binding/1"
        };
        let binding = records.get(
            m,
            capture.key,
            source.captured_source().binding_ref,
            "workflow-binding",
            binding_schema,
        )?;
        for field in ["customer", "policy_ref", "product_ref"] {
            m.visit("work.parent_head_binding_subject")?;
            let actual = shape::field(m, &binding, field)?;
            let expected = shape::field(m, p.value(), field)?;
            shape::equal(m, actual, expected)?;
        }
        let inst_value = shape::field(m, &binding, "slot_instantiation_ref")?;
        let inst_ref = super::working_value::parse_owned_reference(m, &arena, inst_value)?;
        let inst = records.get(
            m,
            capture.key,
            inst_ref.reference(),
            "workflow-slot-instantiation",
            "ledger-workflow-slot-instantiation/1",
        )?;
        derive_work_head_locators(
            program,
            capture.key,
            capture.canonical_economic_namespace,
            p.value(),
            &binding,
            &inst,
            m,
        )
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

// SAME owning identity producer, also used by targeted local non-work DATA acquisition.
/// DATA derivation for one exact retained Work; the consuming child repeats
/// scope/claim/authority/currentness checks. No temporal or source substitution.
pub(super) fn retained_work_heads(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    key: &OperationKey,
    reference: &Value,
) -> Result<WorkHeadLocators> {
    let reference = shape::reference(m, reference)?;
    let work = r.get(
        m,
        key,
        &reference,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let original_key = super::working_value::WorkingKey::parse(m, &work["key"])?;
    for (actual, expected) in [
        (&original_key.logical_store_id, &key.logical_store_id),
        (&original_key.scope[0], &key.scope[0]),
        (&original_key.scope[1], &key.scope[1]),
    ] {
        if !shape::same_text(m, actual, expected)? {
            return Err(shape::fail("retained relation Work scope"));
        }
    }
    let payload = shape::field(m, &work, "payload")?;
    let binding_ref = shape::reference(m, &payload["binding_ref"])?;
    let binding = r.get(
        m,
        key,
        &binding_ref,
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let inst_ref = shape::reference(m, &binding["slot_instantiation_ref"])?;
    let inst = r.get(
        m,
        key,
        &inst_ref,
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let namespace = shape::text(m, &work, "source_namespace")?;
    let heads = derive_work_head_locators(
        program,
        &original_key,
        namespace,
        payload,
        &binding,
        &inst,
        m,
    )?;
    let claim_id = shape::text(m, &work, "claim_id")?;
    if !shape::same_text(m, heads.work_event_id(), &reference.id)?
        || !shape::same_text(m, heads.work_claim_id(), claim_id)?
    {
        return Err(shape::fail("retained relation Work identity"));
    }
    Ok(heads)
}

pub(super) fn derive_work_head_locators(
    program: &SelectedProgram,
    key: &super::OperationKey,
    namespace: &str,
    p: &Value,
    binding: &Value,
    inst: &Value,
    m: &mut CoreMeter,
) -> Result<WorkHeadLocators> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning original work head locators"))?;
    let customer = shape::text(m, p, "customer")?;
    let chain = shape::text(m, p, "chain")?;
    let business = shape::text(m, p, "business_operation_id")?;
    let mut bytes = 2 * mem::size_of::<WorkHeadLocators>()
        + 8 * mem::size_of::<String>()
        + 8 * 128
        + mem::size_of::<super::candidate::PermanentClaimKey>()
        + mem::size_of::<super::candidate::FinancialHeadKey>();
    for text in [&key.logical_store_id, &key.scope[0], &key.scope[1]] {
        bytes = w::usize_add(m, bytes, text.len())?;
        bytes = w::usize_add(m, bytes, text.len())?;
    }
    for text in [namespace, business, customer, chain, "completion"] {
        bytes = w::usize_add(m, bytes, text.len())?;
    }
    let (mut families, mut apps) = (0usize, 0usize);
    head_rows(
        program,
        (
            p,
            &binding["accepted_context"],
            Some((binding, &key.source)),
        ),
        inst,
        m,
        &mut |_, family, m| {
            if let Some(slot) = family {
                families = w::usize_add(m, families, 1)?;
                bytes = w::usize_add(m, bytes, mem::size_of::<WorkFamilyHeadLocator>() + 2 * 128)?;
                bytes = w::usize_add(m, bytes, slot.len())?;
            } else {
                apps = w::usize_add(m, apps, 1)?;
                bytes = w::usize_add(m, bytes, mem::size_of::<String>() + 128)?;
            }
            Ok(())
        },
    )?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut family_keys = Vec::new();
    family_keys
        .try_reserve_exact(families)
        .map_err(|_| shape::fail("work family locator allocation"))?;
    let mut app_decisions = Vec::new();
    app_decisions
        .try_reserve_exact(apps)
        .map_err(|_| shape::fail("work app locator allocation"))?;
    let claim_id = super::work_keys::claim(m, key, namespace, p)?;
    let event_id = super::work_keys::event(m, key, p)?;
    let root_decision_id = super::work_keys::decision(m, &claim_id)?;
    let scope = super::work_keys::scope(m, key)?;
    let agreement = shape::field(m, binding, "agreement_id")?;
    head_rows(
        program,
        (
            p,
            &binding["accepted_context"],
            Some((binding, &key.source)),
        ),
        inst,
        m,
        &mut |slot, family, m| {
            if let Some(family) = family {
                let target_id = super::work_keys::target(m, key, agreement, &claim_id, slot)?;
                let decision_id = super::targets::prospective_decision(m, &target_id, family)?;
                family_keys.push(WorkFamilyHeadLocator {
                    target_id,
                    family_slot: head_copy(m, family)?,
                    decision_id,
                });
            } else {
                app_decisions.push(super::work_keys::app_decision(
                    m,
                    scope.value(),
                    &event_id,
                    slot,
                )?);
            }
            Ok(())
        },
    )?;
    let claim = super::candidate::PermanentClaimKey::Work {
        logical_store_id: head_copy(m, &key.logical_store_id)?,
        scope: [head_copy(m, &key.scope[0])?, head_copy(m, &key.scope[1])?],
        source_namespace: head_copy(m, namespace)?,
        business_operation_id: head_copy(m, business)?,
        completion: head_copy(m, "completion")?,
    };
    let chain = super::candidate::FinancialHeadKey::Chain {
        logical_store_id: head_copy(m, &key.logical_store_id)?,
        scope: [head_copy(m, &key.scope[0])?, head_copy(m, &key.scope[1])?],
        customer: head_copy(m, customer)?,
        chain: head_copy(m, chain)?,
    };
    Ok(WorkHeadLocators {
        claim,
        claim_id,
        event_id,
        root_decision_id,
        chain,
        families: family_keys,
        app_decisions,
        _ownership: ownership,
    })
}

#[cfg(test)]
mod tests {
    use super::super::super::selected_program::{
        compile_selected_publication, SelectedCompilerSupport, SelectedSourceInput,
    };
    use super::*;
    use crate::canonical::{self, audit_arena::AuditArena};
    #[test]
    fn stage_work_terms_locator_uses_original_activity_and_complete_binding_without_legacy_fallback(
    ) {
        use serde_json::json;
        let arena = AuditArena::new().unwrap();
        let mut compiler = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support =
            SelectedCompilerSupport::local_stage_lifecycle_v2_execution_with_session(&mut compiler)
                .unwrap();
        let source: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-stage/source-platform-funding.json"
        ))
        .unwrap();
        let mut documents = source["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
            .collect::<Vec<_>>();
        documents.push(support.canonical_bytes().to_vec());
        let program = compile_selected_publication(
            SelectedSourceInput {
                documents,
                root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
            },
            &support,
            &mut compiler,
        )
        .unwrap();
        let product = program
            .source_documents()
            .iter()
            .find(|v| v.canonical_value()["schema"] == "ledger-workflow-product/1")
            .unwrap();
        let activity = product.canonical_value()["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "support:prepare")
            .unwrap();
        let payload = json!({"activity":"support:prepare","product_ref":product.reference(),"policy_ref":program.root_policy_ref(),"fact_schema_ref":activity["fact_schema"]});
        // Closed Binding2/Terms3 bodies from the saved private39 Stage case.
        // Only Program refs below are rebound to this immutable compiler fixture;
        // this structural DATA test does not assert their producer authority.
        let mut binding: Value = serde_json::from_str(r#"{"accepted_at":"2026-10-28T00:30:57.771741Z","accepted_by":"example-operator","accepted_context":{"fact_schema_ref":{"digest":"sha256:9047f6746f773f4b33a5ad6b04d7f8b9cc820353210a7beae62a3766c7e09369","id":"wdoc_9047f6746f773f4b33a5ad6b04d7f8b9cc820353210a7beae62a3766c7e09369","kind":"workflow-fact-schema"},"facts":{}},"agreement_id":"stage-support","assent_refs":[{"digest":"sha256:06677e18263549a3fca53efe2f5067e1d206df1b904b4c8a183fb0e41845929e","id":"wdoc_06677e18263549a3fca53efe2f5067e1d206df1b904b4c8a183fb0e41845929e","kind":"workflow-assent"},{"digest":"sha256:ccf0eff66c1a22edf9f237c3bcd66d0b83ce36da479686137b21785d3066fc98","id":"wdoc_ccf0eff66c1a22edf9f237c3bcd66d0b83ce36da479686137b21785d3066fc98","kind":"workflow-assent"},{"digest":"sha256:cfc4b036d6fcbb8c24393df834f4ea43e2d79a3b0b70300035cd58ea0d9e4c03","id":"wdoc_cfc4b036d6fcbb8c24393df834f4ea43e2d79a3b0b70300035cd58ea0d9e4c03","kind":"workflow-assent"}],"authority_refs":[{"digest":"sha256:230eb9176b3ed8a29b45511f66e16082edeee2beba23513dc2ec9e037c650d26","id":"wdoc_230eb9176b3ed8a29b45511f66e16082edeee2beba23513dc2ec9e037c650d26","kind":"workflow-source-grant"},{"digest":"sha256:53f41ede58047505403098ce4efe9ad9f9e4aa581b2dcf034d7df801bf777d00","id":"wdoc_53f41ede58047505403098ce4efe9ad9f9e4aa581b2dcf034d7df801bf777d00","kind":"workflow-authority"},{"digest":"sha256:63af239330c177b1b292665a7c9a446aaa6d5f26e7f4029bc77c864999050448","id":"wdoc_63af239330c177b1b292665a7c9a446aaa6d5f26e7f4029bc77c864999050448","kind":"workflow-source-grant"},{"digest":"sha256:75673b0ba6c7340c24d5f58e2d2fc6cd07e85d7fed5e2274fdb857c99a656cc2","id":"wdoc_75673b0ba6c7340c24d5f58e2d2fc6cd07e85d7fed5e2274fdb857c99a656cc2","kind":"workflow-source-grant"},{"digest":"sha256:860701d2b7b6c756df38ad4d614610970768e4d9a74207ec076572027d356860","id":"wdoc_860701d2b7b6c756df38ad4d614610970768e4d9a74207ec076572027d356860","kind":"workflow-source-grant"},{"digest":"sha256:afe90befe301c8a9a4668acaa66e4a0fa974f3435e51e46ed759ec81d123ba5f","id":"wdoc_afe90befe301c8a9a4668acaa66e4a0fa974f3435e51e46ed759ec81d123ba5f","kind":"workflow-authority"},{"digest":"sha256:d5c977e2114b3f7d1cca6aaed0b7a8a8de140b8da1fc63b22299b65728ac22ab","id":"wdoc_d5c977e2114b3f7d1cca6aaed0b7a8a8de140b8da1fc63b22299b65728ac22ab","kind":"workflow-source-grant"}],"bindings":[{"binding_ref":{"digest":"sha256:20e61a781ba4676ab7736f569863c581c2941393cfb8d2fb362834be807e3d32","id":"wdoc_20e61a781ba4676ab7736f569863c581c2941393cfb8d2fb362834be807e3d32","kind":"workflow-slot-terms"},"slot":"support:retail"},{"binding_ref":{"digest":"sha256:027377f192a33c538fef707fc1ff939fa89aa3f6b25d3278a752b35f15c9196e","id":"wdoc_027377f192a33c538fef707fc1ff939fa89aa3f6b25d3278a752b35f15c9196e","kind":"workflow-slot-terms"},"slot":"support:supplier"}],"compiled_plan_ref":{"digest":"sha256:f28f5a8feb06b5c3c021dbf9dbcbfb7d905dc8c222991f20c4270eb44068594e","id":"wplan_f28f5a8feb06b5c3c021dbf9dbcbfb7d905dc8c222991f20c4270eb44068594e","kind":"workflow-compiled-plan"},"customer":"customer-stage","evaluator":"ledger-workflow-evaluator/1","exposure_ceilings":[{"binding_slot":"support:retail","maximum":{"atoms":"4000000000000000000","currency":"USD","scale":18}},{"binding_slot":"support:supplier","maximum":{"atoms":"1000000000000000000","currency":"USD","scale":18}}],"fact_schema_refs":[{"digest":"sha256:02ec83b01c78d8e7ae72d7d799c01770e5ba7e7457ca88cc7e05c5163dbaca67","id":"wdoc_02ec83b01c78d8e7ae72d7d799c01770e5ba7e7457ca88cc7e05c5163dbaca67","kind":"workflow-fact-schema"},{"digest":"sha256:0bbb98a2dcb00202b49224edfa2ab834abb47abb1b3174eba5fcb2bf39b9be81","id":"wdoc_0bbb98a2dcb00202b49224edfa2ab834abb47abb1b3174eba5fcb2bf39b9be81","kind":"workflow-fact-schema"}],"funding_legs":[{"evidence_refs":[],"mode":"customer_local","name":"support:customer","payer":"customer-stage","resource_owner":"customer-stage"},{"evidence_refs":[{"digest":"sha256:303f07451b2f3ae877858fd0e1afdcd57646a9168912ff2f50c5a5e2fdd7056c","id":"wdoc_303f07451b2f3ae877858fd0e1afdcd57646a9168912ff2f50c5a5e2fdd7056c","kind":"workflow-evidence"}],"mode":"platform","name":"support:provider","payer":"platform-P","resource_owner":"platform-P","supplier_binding_slot":"support:supplier"}],"policy_ref":{"digest":"sha256:342f2472e0e9cf6451e952ddc9b4e8bd1aa86419c25f70aa8bedd5f4b561b6a2","id":"wdoc_342f2472e0e9cf6451e952ddc9b4e8bd1aa86419c25f70aa8bedd5f4b561b6a2","kind":"workflow-policy"},"product_ref":{"digest":"sha256:48524e0263104d1ae6a54f770286c2d43626edfb70fd08358fe7ea7c0669c85f","id":"wdoc_48524e0263104d1ae6a54f770286c2d43626edfb70fd08358fe7ea7c0669c85f","kind":"workflow-product"},"quantity_ceilings":[{"activity":"support:prepare","field":"support:quantity","maximum":"1","unit":"support:item"}],"roles_refs":[{"digest":"sha256:2469b759b94fc14ee11293df2efe7cbda2decf1ed0ed993df29bf6b902d06b85","id":"wdoc_2469b759b94fc14ee11293df2efe7cbda2decf1ed0ed993df29bf6b902d06b85","kind":"workflow-roles"},{"digest":"sha256:7571af1d7348b9588366ab2463a25eb441cc86392ca1774bd2181dc3b39fc26e","id":"wdoc_7571af1d7348b9588366ab2463a25eb441cc86392ca1774bd2181dc3b39fc26e","kind":"workflow-roles"}],"schema":"ledger-workflow-binding/2","slot_catalog_ref":{"digest":"sha256:54aa3326a51170f83eea8d40f530070a7ad674a132e9bd6a7210322525b3b9d8","id":"wdoc_54aa3326a51170f83eea8d40f530070a7ad674a132e9bd6a7210322525b3b9d8","kind":"workflow-slot-catalog"},"slot_instantiation_ref":{"digest":"sha256:2b1c9874a4207842297a7023da93df719ae1486f6f466602afbabd67b4eec659","id":"wdoc_2b1c9874a4207842297a7023da93df719ae1486f6f466602afbabd67b4eec659","kind":"workflow-slot-instantiation"},"source_namespace":"urn:example:stage-worker","source_slots":[{"evidence_kinds":["occurrence","outcome"],"name":"support:closer","sources":["urn:example:stage-closer"]},{"evidence_kinds":[],"name":"support:corrector","sources":["urn:example:stage-corrector"]},{"evidence_kinds":[],"name":"support:stage-authorizer","sources":["urn:example:stage-admin"]},{"evidence_kinds":[],"name":"support:supplier-authorizer","sources":["urn:example:stage-closer"]},{"evidence_kinds":["occurrence"],"name":"support:worker","sources":["urn:example:stage-reporter","urn:example:stage-worker"]}],"starts_at":"2000-01-01T00:00:00.000000Z","version":"1"}"#).unwrap();
        let terms: Value = serde_json::from_str(r#"{"agreement_id":"stage-support","allowed_activities":["support:close","support:prepare"],"book":"retail","causal_reversal_clauses":[],"correction_source_slot":"support:corrector","customer":"customer-stage","exposure_ceiling":{"atoms":"4000000000000000000","currency":"USD","scale":18},"exposure_scope":"original_work","funding_leg":"support:customer","quantity_ceilings":[{"activity":"support:prepare","field":"support:quantity","maximum":"1","unit":"support:item"}],"role_ref":{"digest":"sha256:7571af1d7348b9588366ab2463a25eb441cc86392ca1774bd2181dc3b39fc26e","id":"wdoc_7571af1d7348b9588366ab2463a25eb441cc86392ca1774bd2181dc3b39fc26e","kind":"workflow-roles"},"role_slot":"support:retail-role","schema":"ledger-workflow-slot-binding-terms/3","scope":["example-company","local"],"slot":"support:retail","source_grant_refs":[{"digest":"sha256:230eb9176b3ed8a29b45511f66e16082edeee2beba23513dc2ec9e037c650d26","id":"wdoc_230eb9176b3ed8a29b45511f66e16082edeee2beba23513dc2ec9e037c650d26","kind":"workflow-source-grant"},{"digest":"sha256:63af239330c177b1b292665a7c9a446aaa6d5f26e7f4029bc77c864999050448","id":"wdoc_63af239330c177b1b292665a7c9a446aaa6d5f26e7f4029bc77c864999050448","kind":"workflow-source-grant"},{"digest":"sha256:75673b0ba6c7340c24d5f58e2d2fc6cd07e85d7fed5e2274fdb857c99a656cc2","id":"wdoc_75673b0ba6c7340c24d5f58e2d2fc6cd07e85d7fed5e2274fdb857c99a656cc2","kind":"workflow-source-grant"},{"digest":"sha256:860701d2b7b6c756df38ad4d614610970768e4d9a74207ec076572027d356860","id":"wdoc_860701d2b7b6c756df38ad4d614610970768e4d9a74207ec076572027d356860","kind":"workflow-source-grant"},{"digest":"sha256:d5c977e2114b3f7d1cca6aaed0b7a8a8de140b8da1fc63b22299b65728ac22ab","id":"wdoc_d5c977e2114b3f7d1cca6aaed0b7a8a8de140b8da1fc63b22299b65728ac22ab","kind":"workflow-source-grant"}]}"#).unwrap();
        assert!(binding.get("scope").is_none());
        binding["policy_ref"] = serde_json::to_value(program.root_policy_ref()).unwrap();
        binding["product_ref"] = serde_json::to_value(product.reference()).unwrap();

        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        work_activity(&program, &payload, session.meter_mut()).unwrap();
        let (row, schema) =
            work_terms_row(&program, &payload, &binding, session.meter_mut()).unwrap();
        assert_eq!(row["slot"], "support:retail");
        assert_eq!(row["binding_ref"], binding["bindings"][0]["binding_ref"]);
        assert_eq!(schema, "ledger-workflow-slot-binding-terms/3");
        let key = OperationKey {
            logical_store_id: "scope-test-store".into(),
            scope: ["example-company".into(), "local".into()],
            source: "worker".into(),
            operation_id: "scope-test".into(),
        };
        stage_work_terms_association(session.meter_mut(), &key, &payload, &binding, row, &terms)
            .unwrap();
        let mut wrong_scope = terms.clone();
        wrong_scope["scope"][1] = json!("another-scope");
        assert!(stage_work_terms_association(
            session.meter_mut(),
            &key,
            &payload,
            &binding,
            row,
            &wrong_scope
        )
        .is_err());

        // Locator DATA only: supplier promises and original/current source
        // authority are still independently checked by the actual consumer.
        for mut invalid in [binding.clone(), binding.clone(), binding.clone()]
            .into_iter()
            .enumerate()
        {
            match invalid.0 {
                0 => {
                    invalid.1["bindings"].as_array_mut().unwrap().pop();
                }
                1 => {
                    invalid.1["bindings"][1] = invalid.1["bindings"][0].clone();
                }
                _ => {
                    invalid.1["policy_ref"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
                }
            }
            let mut check = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(work_terms_row(&program, &payload, &invalid.1, check.meter_mut()).is_err());
        }
        for (raw,causal) in [(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json").as_slice(),false),(include_bytes!("../../../../../../examples/workflow-work-lifecycle/source.json").as_slice(),true)] {
            let mut compiler=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let support=if causal {SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut compiler).unwrap()} else {SelectedCompilerSupport::builtin().unwrap()};
            let source:Value=serde_json::from_slice(raw).unwrap();
            let mut documents=source["documents"].as_array().unwrap().iter().map(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect::<Vec<_>>();
            if causal {documents.push(support.canonical_bytes().to_vec());}
            let legacy=compile_selected_publication(SelectedSourceInput{documents,root_policy_ref:serde_json::from_value(source["root_policy_ref"].clone()).unwrap()},&support,&mut compiler).unwrap();
            assert!(!legacy.uses_stage_lifecycle_recipe());
            let mut check=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(work_terms_row(&legacy,&payload,&binding,check.meter_mut()).is_err());
            let mut single=binding.clone(); single["bindings"].as_array_mut().unwrap().pop();
            let (_,schema)=work_terms_row(&legacy,&payload,&single,check.meter_mut()).unwrap();
            assert_eq!(schema,if causal {"ledger-workflow-slot-binding-terms/7"} else {"ledger-workflow-slot-binding-terms/6"});
        }
    }
    #[test]
    fn parent_work_locator_uses_same_compiled_activity_without_observation_refs() {
        let (_, c, raw, inputs) = super::super::first_work_tests::test_inputs();
        let source:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let arena = AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support = SelectedCompilerSupport::builtin().unwrap();
        let program = compile_selected_publication(
            SelectedSourceInput {
                documents: source["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                    .collect(),
                root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
            },
            &support,
            &mut session,
        )
        .unwrap();
        // A publication compiler and its consuming work origin have separate
        // existing original ledgers in the SAME attempt. No work child/reset
        // is used to acquire the source or head locator DATA.
        drop(session);
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let a = &c.authorities[0];
        let frame = || SourceAuthorityCapture {
            source_slot: &a.source_slot,
            source: &c.key.source,
            principal: &c.actor,
            canonical_economic_namespace: &c.canonical_economic_namespace,
            enrollment_ref: &a.enrollment_ref,
            current_state_ref: &a.current_state_ref,
            current_grant_ref: &a.current_grant_ref,
            original_grant_refs: &a.original_grant_refs,
            binding_ref: &a.binding_ref,
            accepted_agreement_ref: &a.accepted_agreement_ref,
            activation_ref: &a.activation_ref,
        };
        let mut unrelated = frame();
        unrelated.principal = "another captured principal";
        let sources = [unrelated, frame()];
        let capture = EconomicCapture {
            key: &c.key,
            actor: &c.actor,
            canonical_economic_namespace: &c.canonical_economic_namespace,
            installation_mode: &c.installation_mode,
            received_at: &c.received_at,
            accepted_at: &c.accepted_at,
            sources: &sources,
        };
        let locator = select_work_source_frame(&program, &capture, &raw, &mut session).unwrap();
        assert_eq!(locator.source_slot(), a.source_slot);
        assert!(std::ptr::eq(locator.captured_source(), &sources[1]));
        let payload: Value = serde_json::from_slice(&raw).unwrap();
        let (_, activity) = work_activity(&program, &payload, session.meter_mut()).unwrap();
        assert_eq!(
            locator.source_slot(),
            activity["source_slot"].as_str().unwrap()
        );
        // Neither capture nor returned locator has an observation Ref, I set,
        // digest, or origin/admission bit. The draft support stays nonadmitting.
        assert_eq!(program.execution_registry(), None);
        drop(locator);
        let views: Vec<_> = inputs
            .iter()
            .map(|r| EconomicRecordView {
                reference: &r.reference,
                canonical_body: &r.canonical_body,
            })
            .collect();
        let heads =
            select_work_head_locators(&program, &capture, &raw, &views, &mut session).unwrap();
        // Comparison-only frozen independent oracle, never selector input.
        let oracle: Value =
            serde_json::from_slice(include_bytes!("fixtures/first-work-expected.json")).unwrap();
        let rows = oracle["expected"]["new_records"].as_array().unwrap();
        for row in rows {
            let reference = &row["ref"];
            let kind = reference["kind"].as_str().unwrap();
            let value = &row["body"];
            match kind {
                "workflow-work-claim" => {
                    assert_eq!(heads.work_claim_id(), reference["id"].as_str().unwrap())
                }
                "workflow-work" => {
                    assert_eq!(heads.work_event_id(), reference["id"].as_str().unwrap())
                }
                "workflow-decision" => assert_eq!(
                    heads.root_decision_id(),
                    value["decision_id"].as_str().unwrap()
                ),
                "workflow-family-state" => assert!(heads.family_keys().iter().any(|h| h
                    .target_id()
                    == value["target_ref"]["id"].as_str().unwrap()
                    && h.family_slot() == value["family_slot"].as_str().unwrap())),
                _ => {}
            }
        }
        match heads.chain_key() {
            super::super::candidate::FinancialHeadKey::Chain {
                customer, chain, ..
            } => {
                assert_eq!(customer, payload["customer"].as_str().unwrap());
                assert_eq!(chain, payload["chain"].as_str().unwrap());
            }
            _ => panic!("wrong work chain locator"),
        }
        drop(heads);
        let mut other =
            OriginalAuditSession::for_resource_profile_v2(&AuditArena::new().unwrap()).unwrap();
        assert!(select_work_source_frame(&program, &capture, &raw, &mut other).is_err());
        let duplicate = [frame(), frame()];
        let duplicate_capture = EconomicCapture {
            sources: &duplicate,
            ..capture
        };
        assert!(
            select_work_source_frame(&program, &duplicate_capture, &raw, &mut session).is_err()
        );
        drop(program);
        drop(session);
        assert_eq!(arena.usage().unwrap().live_bytes, before);
    }
}
