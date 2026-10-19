//! Authority DATA emitted from the actual original/current source intersection.
//! Host completed origin, platform consent/currentness and full capacity remain
//! prerequisites of financial preparation; matching this body grants none.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    authority::EconomicMode, body, data::*, emitted_record::ConstructedEnvelope, shape,
    source_authority::SourceSelection, working_value::owning_reference, CoreMeter,
};
use super::{original::OriginalApp, plan::AppPlan};
use crate::{
    canonical::audit_arena::AuditedValue,
    validation_work::{self as w, RepresentationKind, ValidationWork},
    Result,
};
pub(super) fn grant_refs(m: &mut CoreMeter, items: &[ExactRef]) -> Result<AuditedValue> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app grants"))?;
    let n = items.len();
    ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&n.into(), &std::mem::size_of::<&ExactRef>().into()],
    )?;
    let size = n
        .checked_mul(std::mem::size_of::<&ExactRef>())
        .ok_or_else(|| shape::fail("app authority grant metadata"))?;
    let _metadata = arena.reserve_external_payload(size, m)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(n)
        .map_err(|_| shape::fail("app authority grant allocation"))?;
    for reference in items {
        m.visit("app.authority_original_grant")?;
        refs.push(reference);
    }
    super::retention::refs(m, &arena, &refs)
}
pub(super) fn value(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: &OriginalApp,
    source_selection: &SourceSelection,
    mode: EconomicMode,
    observed: Option<&ExactRef>,
    final_refs: &[&ExactRef],
    revision: Option<&ExactRef>,
) -> Result<AuditedValue> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app authority"))?;
    let (operation, rights) = match mode {
        EconomicMode::CostFinalize => ("cost.finalize", br#"["read","submit"]"#.as_slice()),
        EconomicMode::CostCorrect => ("cost.correct", br#"["correct","read"]"#.as_slice()),
        _ => return Err(shape::fail("app monetary authority operation")),
    };
    let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-economic-authority/6","new_schema":"ledger-workflow-economic-authority/7","empty":[]}"#,RepresentationKind::SourceDefinition)?;
    let rights =
        m.parse_owned_representation(&arena, rights, RepresentationKind::SourceDefinition)?;
    let operation = arena.string_value(operation, m)?;
    let actor = arena.string_value(&c.actor, m)?;
    let source = arena.string_value(&c.key.source, m)?;
    let namespace = arena.string_value(&c.canonical_economic_namespace, m)?;
    let installation = arena.string_value(&c.installation_mode, m)?;
    let at = arena.string_value(&c.accepted_at, m)?;
    let frame = &c.authorities[0];
    let slot = arena.string_value(&source_selection.source_slot, m)?;
    let enrollment = owning_reference(m, &arena, &frame.enrollment_ref)?;
    let state = owning_reference(m, &arena, &frame.current_state_ref)?;
    let grant = owning_reference(m, &arena, &frame.current_grant_ref)?;
    let binding = owning_reference(m, &arena, &original.binding_ref)?;
    let accepted = owning_reference(m, &arena, &frame.accepted_agreement_ref)?;
    let originals = grant_refs(m, &source_selection.original_grant_refs)?;
    let app_observations = match observed {
        Some(reference) => super::retention::refs(m, &arena, &[reference])?,
        None => super::retention::refs(m, &arena, &[])?,
    };
    let final_evidence = super::retention::refs(m, &arena, final_refs)?;
    let revision = match revision {
        Some(reference) => super::retention::refs(m, &arena, &[reference])?,
        None => super::retention::refs(m, &arena, &[])?,
    };
    let value = arena.object_from_fields(
        &[
            (
                "schema",
                if program.uses_app_cost_causal_recipe() {
                    &constants.value()["new_schema"]
                } else {
                    &constants.value()["schema"]
                },
            ),
            ("operation", operation.value()),
            ("actor", actor.value()),
            ("source", source.value()),
            ("scope", &original.work["key"]["scope"]),
            ("customer", &original.terms.binding["customer"]),
            ("canonical_economic_namespace", namespace.value()),
            ("installation_mode", installation.value()),
            ("enrollment_ref", enrollment.value()),
            ("current_state_ref", state.value()),
            ("current_grant_ref", grant.value()),
            ("original_binding_ref", binding.value()),
            ("original_accepted_agreement_ref", accepted.value()),
            ("original_grant_refs", originals.value()),
            ("source_slot", slot.value()),
            ("rights", rights.value()),
            ("evidence_refs", &constants.value()["empty"]),
            ("verified_fact_refs", &constants.value()["empty"]),
            ("observed_at", at.value()),
            ("causal_closure_checks", &constants.value()["empty"]),
            ("app_observation_refs", app_observations.value()),
            ("app_final_evidence_refs", final_evidence.value()),
            ("app_cost_revision_refs", revision.value()),
            ("app_cost_closure_checks", &constants.value()["empty"]),
        ],
        m,
    )?;
    let value = if program.uses_app_cost_causal_recipe() {
        super::super::working_value::WorkingValue::Owned(value)
            .with_fields(
                m,
                &[(
                    "app_pending_cost_closure_checks",
                    &constants.value()["empty"],
                )],
            )?
            .into_owned()?
    } else {
        value
    };
    let value = super::authority::attach_source_observation(m, program, frame, value)?;
    Ok(value)
}
pub(super) fn document(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: &OriginalApp,
    source_selection: &SourceSelection,
    mode: EconomicMode,
    observed: &ExactRef,
    final_refs: &[&ExactRef],
    revision: Option<&ExactRef>,
) -> Result<ConstructedEnvelope> {
    let value = value(
        m,
        c,
        program,
        original,
        source_selection,
        mode,
        Some(observed),
        final_refs,
        revision,
    )?;
    body::document_borrowed(m, "workflow-economic-authority", value.value())
}
pub(super) fn for_plan(
    m: &mut CoreMeter,
    r: &mut super::super::records::Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &AppPlan,
) -> Result<(ConstructedEnvelope, SourceSelection)> {
    let correct = plan.current.is_some();
    let slot = shape::text(
        m,
        plan.original.declaration,
        if correct {
            "correction_source_slot"
        } else {
            "cost_source_slot"
        },
    )?;
    let mode = if correct {
        EconomicMode::CostCorrect
    } else {
        EconomicMode::CostFinalize
    };
    let source = plan.original.source(m, r, c, program, slot, mode)?;
    let (revision, old) = match &plan.current {
        Some(old) => (
            Some(&old.revision_ref),
            Some(&old.revision["current_evidence_ref"]),
        ),
        None => (None, None),
    };
    let _ = old;
    let authority = if let Some(old) = &plan.old_cost {
        document(
            m,
            c,
            program,
            &plan.original,
            &source,
            mode,
            &plan.observation.pair.attestation_ref,
            &[
                &old.pair.attestation_ref,
                &plan.final_cost.pair.attestation_ref,
            ],
            revision,
        )?
    } else {
        document(
            m,
            c,
            program,
            &plan.original,
            &source,
            mode,
            &plan.observation.pair.attestation_ref,
            &[&plan.final_cost.pair.attestation_ref],
            revision,
        )?
    };
    Ok((authority, source))
}

/// Shared exact authority version boundary. Local execution has no audit
/// observation operand; experimental authority retains its mandatory real Ref.
pub(in crate::policy::workflow::transition_constructor) fn attach_source_observation(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    frame: &AuthorityContextData,
    value: AuditedValue,
) -> Result<AuditedValue> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning authority recipe"))?;
    if program.uses_local_execution_recipe() {
        if frame.authority_observation_ref.is_some() {
            return Err(shape::fail(
                "local authority cannot carry an experimental observation",
            ));
        }
        let schema = arena.string_value("ledger-workflow-economic-authority/8", m)?;
        super::super::working_value::WorkingValue::Owned(value)
            .with_fields(m, &[("schema", schema.value())])?
            .into_owned()
    } else {
        let observed = owning_reference(m, &arena, frame.required_observation_ref()?)?;
        super::super::working_value::WorkingValue::Owned(value)
            .with_fields(m, &[("authority_observation_ref", observed.value())])?
            .into_owned()
    }
}
