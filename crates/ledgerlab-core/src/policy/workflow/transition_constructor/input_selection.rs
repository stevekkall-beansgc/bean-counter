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
    _ownership: ExternalBufferLease,
}
impl<'a> WorkInputSeeds<'a> {
    pub fn source(&self) -> &WorkSourceLocator<'a> {
        &self.source
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
        let mut r = Records::borrowed_parent_app(m, &arena, capture.key, inputs)?;
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
        let rows = shape::array(m, &binding, "bindings")?;
        if !w::compare(m, rows.len(), 1usize)?.is_eq() {
            return Err(crate::Error::new("UNSUPPORTED_COMPOSITION","work parent app binding selector requires the same implemented single retail binding"));
        }
        let terms_value = shape::field(m, &rows[0], "binding_ref")?;
        let terms_ref = super::working_value::parse_owned_reference(m, &arena, terms_value)?;
        let schema = if program.uses_app_cost_causal_recipe() {
            "ledger-workflow-slot-binding-terms/7"
        } else {
            "ledger-workflow-slot-binding-terms/6"
        };
        let terms = r.get(
            m,
            capture.key,
            terms_ref.reference(),
            "workflow-slot-terms",
            schema,
        )?;
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
        let owner = arena.reserve_external_payload(mem::size_of::<WorkInputSeeds>(), m)?;
        m.write_bytes(mem::size_of::<WorkInputSeeds>())?;
        Ok(WorkInputSeeds {
            source,
            records: r.into_consumed_owned(),
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
    p: &Value,
    inst: &Value,
    m: &mut CoreMeter,
    f: &mut HeadRowVisitor<'_>,
) -> Result<()> {
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
        let mut records = Records::borrowed_parent_app(m, &arena, capture.key, inputs)?;
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
    head_rows(program, p, inst, m, &mut |_, family, m| {
        if let Some(slot) = family {
            families = w::usize_add(m, families, 1)?;
            bytes = w::usize_add(m, bytes, mem::size_of::<WorkFamilyHeadLocator>() + 2 * 128)?;
            bytes = w::usize_add(m, bytes, slot.len())?;
        } else {
            apps = w::usize_add(m, apps, 1)?;
            bytes = w::usize_add(m, bytes, mem::size_of::<String>() + 128)?;
        }
        Ok(())
    })?;
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
    head_rows(program, p, inst, m, &mut |slot, family, m| {
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
    })?;
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
