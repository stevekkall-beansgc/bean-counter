//! Paid original/current source intersection for the selected supplier operation.
//! Observation DATA does not authenticate its prefix. The held host supplies and
//! independently completes that exact origin before any candidate can be prepared.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    CoreMeter,
    authority::{self, EconomicMode},
    body,
    data::*,
    emitted_record::ConstructedEnvelope,
    inventory, meter,
    record_body::RecordBody,
    records::Records,
    representation::InvocationToken,
    shape,
    working_value::owning_reference,
};
use super::{operands::InvocationSelection, plan::CostPlan};
use crate::{
    Error, Result,
    canonical::audit_arena::{AuditedValue, ExternalBufferLease},
    domain::Timestamp,
    validation_work::{self as w, RepresentationKind},
};
use serde_json::{Value, json};

pub(super) use super::super::source_authority::SourceSelection;
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
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
pub(super) fn source(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,selected:&InvocationSelection,source_slot:&str,mode:EconomicMode)->Result<SourceSelection>{
    super::super::source_authority::source(m,r,c,program,super::super::source_authority::OriginalSourceSubject{binding_ref:&selected.binding_ref,binding:&selected.binding,terms:&selected.terms,payload:&selected.work["payload"]},source_slot,mode)
}

/// Construct the cost authority DATA from the actual selected current source,
/// original accepted Binding/terms and the executor's derived closure objects.
/// This is not a completed original/party-consent/capacity certificate. Those
/// independent gates remain prerequisites of the enclosing candidate factory.
pub(super) fn document(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &CostPlan<'_>,
    causal_checks: &AuditedValue,
    cost_checks: &AuditedValue,
) -> Result<ConstructedEnvelope> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning cost authority document"))?;
    let correct = plan.current.is_some();
    let slot = if correct {
        let slot = shape::text(m, &plan.selected.terms, "correction_source_slot")?;
        let definitions = shape::array(m, program.original_policy_value(), "binding_slots")?;
        let supplier_slot = shape::text(m, &plan.selected.original, "supplier_slot")?;
        let definition = shape::named(m, definitions, "name", supplier_slot)?;
        eq(
            m,
            &definition["correction_source_slot"],
            &plan.selected.terms["correction_source_slot"],
        )?;
        slot
    } else {
        shape::text(m, plan.delayed, "evidence_source_slot")?
    };
    let mode = if correct {
        EconomicMode::CostCorrect
    } else {
        EconomicMode::CostFinalize
    };
    let selected_source = source(m, r, c, program, &plan.selected, slot, mode)?;
    let frame = &c.authorities[0];
    let accepted = r.get(
        m,
        &c.key,
        &frame.accepted_agreement_ref,
        "workflow-accepted-agreement",
        "ledger-workflow-accepted-agreement/1",
    )?;
    for field in ["scope", "customer", "agreement_id", "version"] {
        eq(m, &accepted[field], &plan.selected.binding[field])?;
    }
    eq(
        m,
        &accepted["binding_ref"],
        &json!(plan.selected.binding_ref),
    )?;
    let (terms_ref, terms) = load(
        m,
        r,
        c,
        &accepted["terms_ref"],
        "workflow-acceptance-terms",
        "ledger-workflow-acceptance-terms/1",
    )?;
    for field in ["scope", "customer", "agreement_id", "version"] {
        eq(m, &terms[field], &plan.selected.binding[field])?;
    }
    let bytes = r.canonical(m, &terms_ref)?;
    let digest =
        super::super::records::hash_parts(m, "workflow-acceptance-terms", &[bytes], false)?;
    eq(m, &accepted["terms_digest"], &json!(digest))?;
    for field in [
        "agreement_id",
        "version",
        "customer",
        "source_namespace",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "roles_refs",
        "source_slots",
        "accepted_context",
        "bindings",
        "funding_legs",
        "starts_at",
        "quantity_ceilings",
        "exposure_ceilings",
    ] {
        m.visit("cost.original_acceptance_projection")?;
        eq(
            m,
            &terms["binding_inputs"][field],
            &plan.selected.binding[field],
        )?;
    }
    match (
        terms["binding_inputs"].get("ends_at"),
        plan.selected.binding.get("ends_at"),
    ) {
        (Some(a), Some(b)) => eq(m, a, b)?,
        (None, None) => {}
        _ => return Err(shape::fail("cost original binding interval projection")),
    }
    let activation = r.get(
        m,
        &c.key,
        &frame.activation_ref,
        "workflow-activation",
        "ledger-workflow-activation-transition/1",
    )?;
    eq(
        m,
        &activation["binding_ref"],
        &json!(plan.selected.binding_ref),
    )?;
    eq(m, &activation["action"], &json!("activate"))?;
    let at = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    let effective =
        Timestamp::parse_with_work(shape::text(m, &activation, "effective_at")?, m)?.micros();
    if meter::cmp(m, &effective.into(), &at.into())?.is_gt() {
        return Err(shape::fail("cost original activation not effective"));
    }
    let start =
        Timestamp::parse_with_work(shape::text(m, &plan.selected.binding, "starts_at")?, m)?
            .micros();
    if meter::cmp(m, &at.into(), &start.into())?.is_lt() {
        return Err(shape::fail("cost before original binding interval"));
    }
    if let Some(end) = plan.selected.binding.get("ends_at") {
        let raw = end
            .as_str()
            .ok_or_else(|| shape::fail("cost original binding end"))?;
        let end = Timestamp::parse_with_work(raw, m)?.micros();
        if !meter::cmp(m, &at.into(), &end.into())?.is_lt() {
            return Err(shape::fail("cost original binding interval ended"));
        }
    }
    // The source operation's grant is today's independent namespace; original
    // work identity remains in the actual WorkClaim and is never rewritten.
    eq(
        m,
        &selected_source.current["canonical_economic_namespace"],
        &json!(c.canonical_economic_namespace),
    )?;
    for (value, maximum) in [(causal_checks, 64usize), (cost_checks, 31usize)] {
        let rows = value
            .value()
            .as_array()
            .ok_or_else(|| shape::fail("cost authority closure array"))?;
        if w::compare(m, rows.len(), maximum)?.is_gt() {
            return Err(shape::fail("cost authority closure bound"));
        }
        let mut prior: Option<crate::canonical::audit_arena::AuditedBytes> = None;
        for row in rows {
            m.visit("cost.canonical_closure_check")?;
            if !row.is_object() {
                return Err(shape::fail("cost closure object"));
            }
            let bytes =
                m.canonical_owned_representation(&arena, row, RepresentationKind::Authority)?;
            if let Some(previous) = &prior {
                if !shape::byte_order(m, previous.bytes(), bytes.bytes())?.is_lt() {
                    return Err(shape::fail("cost closure checks not a canonical set"));
                }
            }
            prior = Some(bytes);
        }
    }
    let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-economic-authority/4","finalize":"cost.finalize","correct":"cost.correct","finalize_rights":["read","submit"],"correct_rights":["correct","read"],"verified":[]}"#,RepresentationKind::SourceDefinition)?;
    let actor = arena.string_value(&c.actor, m)?;
    let source = arena.string_value(&c.key.source, m)?;
    let namespace = arena.string_value(&c.canonical_economic_namespace, m)?;
    let mode = arena.string_value(&c.installation_mode, m)?;
    let observed = arena.string_value(&c.accepted_at, m)?;
    let slot = arena.string_value(slot, m)?;
    let enrollment = owning_reference(m, &arena, &frame.enrollment_ref)?;
    let state = owning_reference(m, &arena, &frame.current_state_ref)?;
    let current = owning_reference(m, &arena, &frame.current_grant_ref)?;
    let binding = owning_reference(m, &arena, &plan.selected.binding_ref)?;
    let accepted = owning_reference(m, &arena, &frame.accepted_agreement_ref)?;
    let originals = super::records::reference_array(m, &arena, &frame.original_grant_refs)?;
    let observation = owning_reference(m, &arena, &frame.authority_observation_ref)?;
    let invocation = owning_reference(m, &arena, &plan.selected.original_ref)?;
    let terms = owning_reference(m, &arena, &plan.selected.terms_ref)?;
    let value = arena.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            (
                "operation",
                if correct {
                    &constants.value()["correct"]
                } else {
                    &constants.value()["finalize"]
                },
            ),
            ("actor", actor.value()),
            ("source", source.value()),
            ("scope", &plan.selected.binding["scope"]),
            ("customer", &plan.selected.binding["customer"]),
            ("canonical_economic_namespace", namespace.value()),
            ("installation_mode", mode.value()),
            ("enrollment_ref", enrollment.value()),
            ("current_state_ref", state.value()),
            ("current_grant_ref", current.value()),
            ("original_binding_ref", binding.value()),
            ("original_accepted_agreement_ref", accepted.value()),
            ("original_grant_refs", originals.value()),
            ("source_slot", slot.value()),
            (
                "rights",
                if correct {
                    &constants.value()["correct_rights"]
                } else {
                    &constants.value()["finalize_rights"]
                },
            ),
            ("evidence_refs", &plan.request["evidence_refs"]),
            ("verified_fact_refs", &constants.value()["verified"]),
            ("observed_at", observed.value()),
            ("authority_observation_ref", observation.value()),
            ("causal_closure_checks", causal_checks.value()),
            (
                "cost_authority_ref",
                &plan.selected.original["cost_authority_ref"],
            ),
            ("supplier_terms_ref", terms.value()),
            ("offer_ref", &plan.selected.original["offer_ref"]),
            ("original_invocation_ref", invocation.value()),
            ("cost_closure_checks", cost_checks.value()),
        ],
        m,
    )?;
    body::document_borrowed(m, "workflow-economic-authority", value.value())
}
