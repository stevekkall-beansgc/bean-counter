//! Subject-owned opaque fact groups. No generic JSON graph walk, latest head,
//! new evidence or recursively re-executed referenced endpoint is introduced.
use super::super::selected_program::SelectedProgram;
use super::{
    data::{EconomicRecordView, EconomicStateData, ExactRef, OperationKey},
    fact_references::{self, FactReferenceInputs},
    local_locators::LocalOperationLocators,
    record_body::RecordBody,
    records::Records,
    shape,
    working_value::{owning_definition, owning_reference, parse_owned_reference},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    validation_work::{self as w, OriginalAuditSession, RepresentationKind, ValidationWork},
    Result,
};
use serde_json::Value;
use std::mem;

pub struct OperationFactReferenceGroup {
    role: &'static str,
    inputs: FactReferenceInputs,
}
impl OperationFactReferenceGroup {
    pub fn role(&self) -> &str {
        self.role
    }
    pub fn inputs(&self) -> &FactReferenceInputs {
        &self.inputs
    }
}
pub struct OperationFactReferenceInputs {
    groups: Vec<OperationFactReferenceGroup>,
    _ownership: ExternalBufferLease,
}
impl OperationFactReferenceInputs {
    pub fn groups(&self) -> &[OperationFactReferenceGroup] {
        &self.groups
    }
}
type Emit<'a> = dyn FnMut(&'static str, &Value, &Value, bool, &mut CoreMeter) -> Result<()> + 'a;
fn load(
    m: &mut CoreMeter,
    a: &AuditArena,
    r: &mut Records<'_, '_>,
    key: &OperationKey,
    reference: &Value,
    kind: &str,
    schemas: &[&str],
) -> Result<RecordBody> {
    let reference = parse_owned_reference(m, a, reference)?;
    r.get_schemas(m, key, reference.reference(), kind, schemas)
}
fn facts(m: &mut CoreMeter, role: &'static str, value: &Value, emit: &mut Emit<'_>) -> Result<()> {
    emit(
        role,
        shape::field(m, value, "fact_schema_ref")?,
        shape::field(m, value, "facts")?,
        false,
        m,
    )
}
fn work(m: &mut CoreMeter, value: &Value, binding: &Value, emit: &mut Emit<'_>) -> Result<()> {
    let payload = shape::field(m, value, "payload")?;
    let actual_binding = shape::field(m, payload, "binding_ref")?;
    shape::equal(m, actual_binding, binding)?;
    facts(m, "original_work", payload, emit)
}
fn decision(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    selected: (&AuditArena, &mut Records<'_, '_>, &OperationKey),
    reference: &ExactRef,
    binding: &Value,
    emit: &mut Emit<'_>,
) -> Result<()> {
    let (a, r, key) = selected;
    let d = r.get_schemas(
        m,
        key,
        reference,
        "workflow-decision",
        &[
            "ledger-workflow-decision-revision/1",
            "ledger-workflow-decision-revision/2",
            "ledger-workflow-decision-revision/3",
        ],
    )?;
    // State can contain multiple selected roles. Only this original subject's
    // accepted Binding supplies the Program/schema dictionary for its facts.
    let terms = shape::field(m, &d, "original_terms_ref")?;
    if !shape::same(m, terms, binding)? {
        return Ok(());
    }
    let subject_ref = shape::field(m, &d, "subject_ref")?;
    let kind = shape::text(m, subject_ref, "kind")?;
    if shape::same_text(m, kind, "workflow-work")? {
        let value = load(
            m,
            a,
            r,
            key,
            subject_ref,
            "workflow-work",
            &["ledger-workflow-work-record/1"],
        )?;
        work(m, &value, binding, emit)?;
    } else if shape::same_text(m, kind, "workflow-outcome-claim")? {
        let value = load(
            m,
            a,
            r,
            key,
            subject_ref,
            "workflow-outcome-claim",
            &[
                "ledger-workflow-outcome-claim/1",
                "ledger-workflow-outcome-claim/2",
            ],
        )?;
        let payload = shape::field(m, &value, "original_payload")?;
        let actual_binding = shape::field(m, payload, "binding_ref")?;
        shape::equal(m, actual_binding, binding)?;
        facts(m, "original_outcome", payload, emit)?;
    }
    let evaluation_ref = shape::field(m, &d, "evaluation_ref")?;
    let evaluation_schemas: &[&str] = if program.uses_component_lifecycle_recipe() {
        &[
            "ledger-workflow-component-evaluation-proof/1",
            "ledger-workflow-evaluation-proof/1",
        ]
    } else {
        &["ledger-workflow-evaluation-proof/1"]
    };
    let evaluation = load(
        m,
        a,
        r,
        key,
        evaluation_ref,
        "workflow-evaluation",
        evaluation_schemas,
    )?;
    let evaluation_binding = shape::field(m, &evaluation, "binding_ref")?;
    shape::equal(m, evaluation_binding, binding)?;
    let actual = shape::field(m, &evaluation, "facts")?;
    let fields = actual
        .as_object()
        .ok_or_else(|| shape::fail("current fact group object"))?;
    // Explicit none Outcome correction retains exactly empty facts. This DATA
    // skip supplies no none/transition proof; the actual current reader must
    // establish that closed transition before it accepts the evaluation.
    if !w::compare(m, fields.len(), 0usize)?.is_eq() {
        facts(m, "selected_evaluation", &evaluation, emit)?;
    }
    Ok(())
}
fn component_without_new_facts(program: &SelectedProgram, schema: &str) -> bool {
    program.uses_component_lifecycle_recipe()
        && matches!(
            schema,
            "ledger-workflow-cost-observation-attest/1" | "ledger-workflow-link-assert/1"
        )
}
fn walk(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    selected: (&AuditArena, &mut Records<'_, '_>, &OperationKey),
    locators: &LocalOperationLocators,
    payload: &Value,
    state: &EconomicStateData<'_>,
    emit: &mut Emit<'_>,
) -> Result<()> {
    let (a, r, key) = selected;
    let reference = owning_reference(m, a, locators.original_binding_ref())?;
    let binding = reference.value();
    let original = r.get(
        m,
        key,
        locators.original_binding_ref(),
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let context = shape::field(m, &original, "accepted_context")?;
    let expected_context = owning_definition(m, a, program.context_schema_ref())?;
    let context_schema = shape::field(m, context, "fact_schema_ref")?;
    shape::equal(m, context_schema, expected_context.value())?;
    emit(
        "accepted_context",
        shape::field(m, context, "fact_schema_ref")?,
        shape::field(m, context, "facts")?,
        false,
        m,
    )?;
    if let Some(reference) = locators.original_work_ref() {
        let value = r.get(
            m,
            key,
            reference,
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        work(m, &value, binding, emit)?;
    }
    let schema = shape::text(m, payload, "schema")?;
    match schema {
        "ledger-workflow-stage-close/1" if program.uses_stage_lifecycle_recipe() => {
            facts(m, "new_stage", payload, emit)?
        }
        "ledger-workflow-invocation-authorize/1"
        | "ledger-workflow-invocation-close/1"
        | "ledger-workflow-stage-authorize/1"
            if program.uses_stage_lifecycle_recipe() => {}
        "ledger-workflow-outcome/1" | "ledger-workflow-outcome/2" => {
            facts(m, "new_outcome", payload, emit)?
        }
        "ledger-workflow-outcome-correction/1" | "ledger-workflow-outcome-correction/2" => {
            let claim_ref = shape::field(m, payload, "original_claim_ref")?;
            let claim = load(
                m,
                a,
                r,
                key,
                claim_ref,
                "workflow-outcome-claim",
                &[
                    "ledger-workflow-outcome-claim/1",
                    "ledger-workflow-outcome-claim/2",
                ],
            )?;
            let original_payload = shape::field(m, &claim, "original_payload")?;
            facts(m, "original_outcome", original_payload, emit)?;
            let replacement = shape::field(m, payload, "replacement")?;
            if replacement.get("kind").is_none() {
                facts(m, "replacement_outcome", replacement, emit)?;
            }
        }
        "ledger-workflow-fact-attest/1" => {
            let att = shape::field(m, payload, "attestation")?;
            emit(
                "asserted_fields",
                shape::field(m, att, "subject_schema_ref")?,
                shape::field(m, att, "claims")?,
                true,
                m,
            )?;
        }
        schema if component_without_new_facts(program, schema) => {}
        "ledger-workflow-work-reverse/2"
        | "ledger-workflow-quantity-correction/2"
        | "ledger-workflow-outcome-close/2"
        | "ledger-workflow-cost-finalize/2"
        | "ledger-workflow-cost-correct/2"
        | "ledger-workflow-family-close-attest/1"
        | "ledger-workflow-app-observe/1"
        | "ledger-workflow-app-cost-attest/1" => {}
        _ => return Err(shape::fail("lifecycle fact group operation recipe")),
    }
    for head in &state.financial.decision_heads {
        m.visit("fact_group.selected_decision_head")?;
        decision(program, m, (a, r, key), &head.reference, binding, emit)?;
    }
    for head in &state.financial.family_heads {
        m.visit("fact_group.selected_family_head")?;
        let family = r.get_schemas(
            m,
            key,
            &head.reference,
            "workflow-family-state",
            &[
                "ledger-workflow-family-state/1",
                "ledger-workflow-family-state/2",
            ],
        )?;
        if let Some(reference) = family.get("decision_ref") {
            let reference = parse_owned_reference(m, a, reference)?;
            decision(
                program,
                m,
                (a, r, key),
                reference.reference(),
                binding,
                emit,
            )?;
        }
    }
    if let Some(prior) = locators.current_root_predecessor_ref() {
        decision(program, m, (a, r, key), prior, binding, emit)?;
    }
    Ok(())
}
pub(super) fn select(
    locators: &LocalOperationLocators,
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    selected: (&EconomicStateData<'_>, &[EconomicRecordView<'_>]),
    s: &mut OriginalAuditSession,
) -> Result<OperationFactReferenceInputs> {
    s.check_cancelled()?;
    program.require_attempt_owner(s.meter_mut())?;
    if !program.uses_work_lifecycle_recipe() {
        return Err(shape::fail("exact lifecycle fact group recipe required"));
    }
    let a = s.arena().shared_handle();
    let payload = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
    let m = s.meter_mut();
    super::codec::validate_selected_types(m, payload.value())?;
    let (state, inputs) = selected;
    let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
    let mut count = 0usize;
    walk(
        m,
        program,
        (&a, &mut r, key),
        locators,
        payload.value(),
        state,
        &mut |_, _, _, _, m| {
            count = w::usize_add(m, count, 1)?;
            Ok(())
        },
    )?;
    ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[
            &count.into(),
            &mem::size_of::<OperationFactReferenceGroup>().into(),
        ],
    )?;
    let bytes = count
        .checked_mul(mem::size_of::<OperationFactReferenceGroup>())
        .ok_or_else(|| shape::fail("reference fact group metadata"))?;
    let bytes = w::usize_add(m, bytes, mem::size_of::<OperationFactReferenceInputs>())?;
    let ownership = a.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut groups = Vec::new();
    groups
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("reference fact group allocation"))?;
    walk(
        m,
        program,
        (&a, &mut r, key),
        locators,
        payload.value(),
        state,
        &mut |role, schema, values, claims, m| {
            groups.push(OperationFactReferenceGroup {
                role,
                inputs: fact_references::select_group(m, program, schema, values, claims)?,
            });
            Ok(())
        },
    )?;
    Ok(OperationFactReferenceInputs {
        groups,
        _ownership: ownership,
    })
}

/// Two exact fresh Work groups before there is an original Work or nonwork
/// locator: actual submitted facts and the authenticated original Binding's
/// accepted context. This is DATA under the held parent, not Binding authority.
pub fn select_work_fact_reference_inputs(
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    original_binding: EconomicRecordView<'_>,
    s: &mut OriginalAuditSession,
) -> Result<OperationFactReferenceInputs> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        if !program.uses_work_lifecycle_recipe() {
            return Err(shape::fail(
                "exact lifecycle Work fact group recipe required",
            ));
        }
        let a = s.arena().shared_handle();
        let payload = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let m = s.meter_mut();
        super::codec::validate_selected_types(m, payload.value())?;
        let schema = shape::text(m, payload.value(), "schema")?;
        if !shape::same_text(m, schema, "ledger-workflow-work/1")? {
            return Err(shape::fail("fresh Work fact group payload"));
        }
        super::input_selection::work_activity(program, payload.value(), m)?;
        let expected = owning_reference(m, &a, original_binding.reference)?;
        let supplied = shape::field(m, payload.value(), "binding_ref")?;
        shape::equal(m, supplied, expected.value())?;
        let inputs = [original_binding];
        let mut r = Records::borrowed_parent_selected(m, &a, key, &inputs, program)?;
        let binding = r.get(
            m,
            key,
            original_binding.reference,
            "workflow-binding",
            "ledger-workflow-binding/2",
        )?;
        let context = shape::field(m, &binding, "accepted_context")?;
        let expected_context = owning_definition(m, &a, program.context_schema_ref())?;
        let context_schema = shape::field(m, context, "fact_schema_ref")?;
        shape::equal(m, context_schema, expected_context.value())?;
        let bytes = mem::size_of::<OperationFactReferenceInputs>()
            + 2 * mem::size_of::<OperationFactReferenceGroup>();
        let ownership = a.reserve_external_payload(bytes, m)?;
        m.write_bytes(bytes)?;
        let mut groups = Vec::new();
        groups
            .try_reserve_exact(2)
            .map_err(|_| shape::fail("fresh Work fact group allocation"))?;
        for (role, value) in [("new_work", payload.value()), ("accepted_context", context)] {
            let schema = shape::field(m, value, "fact_schema_ref")?;
            let actual = shape::field(m, value, "facts")?;
            groups.push(OperationFactReferenceGroup {
                role,
                inputs: fact_references::select_group(m, program, schema, actual, false)?,
            });
        }
        Ok(OperationFactReferenceInputs {
            groups,
            _ownership: ownership,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::ResourceLimits};
    use serde_json::json;
    #[test]
    fn component_only_observation_and_link_have_no_new_fact_groups() {
        use crate::policy::workflow::selected_program::{
            compile_selected_publication, SelectedCompilerSupport,
        };
        let (legacy, _, _, _) = super::super::first_work_tests::test_inputs();
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut author = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support =
            SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut author)
                .unwrap();
        let input = super::super::pricing::component_tests::component_input(&support, &mut author);
        let mut compile = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let component = compile_selected_publication(input, &support, &mut compile).unwrap();
        for schema in [
            "ledger-workflow-cost-observation-attest/1",
            "ledger-workflow-link-assert/1",
        ] {
            assert!(component_without_new_facts(&component, schema));
            assert!(!component_without_new_facts(&legacy, schema));
        }
        assert!(!component_without_new_facts(
            &component,
            "ledger-workflow-unknown/1"
        ));
    }
    #[test]
    fn named_current_prior_and_none_evaluations_select_only_their_actual_facts() {
        let (program, _, _, _) = super::super::first_work_tests::test_inputs();
        let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let mut producer = CoreMeter::new(65536).unwrap();
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["company".into(), "scope".into()],
            source: "worker".into(),
            operation_id: "original".into(),
        };
        let binding = json!({"kind":"workflow-binding","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
        let schema = json!({"kind":"workflow-fact-schema","id":format!("wdoc_{}","b".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))});
        // Framed DATA records are not original producer/member authority.
        let work = super::super::body::record(&mut producer,&key,"workflow-work",format!("wevent_{}","c".repeat(64)),json!({"schema":"ledger-workflow-work-record/1","payload":{"binding_ref":binding,"fact_schema_ref":schema,"facts":{"example:q":"3"}}})).unwrap();
        let mut evaluations = Vec::new();
        let mut decisions = Vec::new();
        for (i, actual) in [
            json!({"example:q":"3"}),
            json!({}),
            json!({"example:q":"2"}),
        ]
        .into_iter()
        .enumerate()
        {
            let evaluation = super::super::body::document(&mut producer,"workflow-evaluation",json!({"schema":"ledger-workflow-evaluation-proof/1","binding_ref":binding,"fact_schema_ref":schema,"facts":actual})).unwrap();
            let id = super::super::body::semantic_id(
                &mut producer,
                "wdrev_",
                "workflow-decision-revision",
                &json!(["decision", (i + 1).to_string()]),
            )
            .unwrap();
            let decision = super::super::body::record(&mut producer,&key,"workflow-decision",id,json!({"schema":"ledger-workflow-decision-revision/1","original_terms_ref":binding,"subject_ref":work.reference,"evaluation_ref":evaluation.reference})).unwrap();
            evaluations.push(evaluation);
            decisions.push(decision);
        }
        let views = std::iter::once(&work)
            .chain(evaluations.iter())
            .chain(decisions.iter())
            .map(|v| EconomicRecordView {
                reference: &v.reference,
                canonical_body: &v.canonical_body,
            })
            .collect::<Vec<_>>();
        let mut r = Records::borrowed_parent_app(s.meter_mut(), &a, &key, &views).unwrap();
        for (i, d) in decisions.iter().enumerate() {
            let mut seen = Vec::new();
            decision(
                &program,
                s.meter_mut(),
                (&a, &mut r, &key),
                &d.reference,
                &binding,
                &mut |role, _, facts, claims, _| {
                    assert!(!claims);
                    seen.push((role, facts.clone()));
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(seen[0], ("original_work", json!({"example:q":"3"})));
            if i == 1 {
                assert_eq!(seen.len(), 1);
            } else {
                assert_eq!(
                    seen[1],
                    (
                        "selected_evaluation",
                        json!({"example:q":if i==0 {"3"} else {"2"}})
                    )
                );
            }
        }
    }
}
