//! After-evaluation AUTH03 explicit-Ref fixed point and N containment. Required
//! implicit locators must still come from the actual per-kind selector. This
//! does not validate a historical origin or replace child authority checks.
use super::super::{
    data::{Construction, DraftContextData, OperationKey},
    emitted_record::ConstructedEnvelope,
    records::Records,
    shape,
    working_value::{owning_reference, parse_owned_reference},
    CoreMeter,
};
use crate::{
    validation_work::{self as w, ValidationWork},
    Result,
};
use serde_json::Value;

use super::super::input_edges::body;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn output_edges_keep_business_facts_and_history_seed_targets_separate() {
        let reference =
            json!({"kind":"workflow-binding","id":"literal-id","digest":"literal-digest"});
        let facts =
            json!({"kind":"a business enum","id":"a business id","digest":"a business digest"});
        let value = json!({"schema":"ledger-workflow-evaluation-proof/1","binding_ref":reference,"facts":facts,"context":{"fact_schema_ref":reference,"values":facts},"steps":[{"basis_refs":[reference]}]});
        let mut m = CoreMeter::new(65536).unwrap();
        let mut count = 0;
        body(
            &value,
            "workflow-evaluation",
            false,
            &mut m,
            &mut |v, _| {
                assert_eq!(v, &reference);
                count += 1;
                Ok(())
            },
            &mut |_, _, _| panic!("unexpected cut"),
        )
        .unwrap();
        assert_eq!(count, 3);
        let claim = json!({"schema":"ledger-workflow-outcome-claim/1","target_ref":reference,"original_payload":{"facts":facts,"binding_ref":reference}});
        let mut count = 0;
        body(
            &claim,
            "workflow-outcome-claim",
            false,
            &mut m,
            &mut |v, _| {
                assert_eq!(v, &reference);
                count += 1;
                Ok(())
            },
            &mut |_, _, _| panic!("unexpected cut"),
        )
        .unwrap();
        assert_eq!(count, 2);
        let financial = json!({"schema":"ledger-workflow-financial-history-observation/3","root_ref":reference,"verifier_ref":reference,"preexisting_input_refs":[reference],"history_edges":[{"owner_ref":reference,"field":"prior_ref","target_ref":facts}]});
        let mut count = 0;
        body(
            &financial,
            "workflow-financial-history-observation",
            false,
            &mut m,
            &mut |v, _| {
                assert_eq!(v, &reference);
                count += 1;
                Ok(())
            },
            &mut |_, _, _| panic!("unexpected cut"),
        )
        .unwrap();
        assert_eq!(count, 2);
        // An actually typed old Ref cannot disappear behind facts handling.
        assert!(body(
            &value,
            "workflow-evaluation",
            false,
            &mut m,
            &mut |_, _| Err(shape::fail("not in actual observed I")),
            &mut |_, _, _| panic!("unexpected cut")
        )
        .is_err());
        let decision =
            json!({"schema":"ledger-workflow-decision-revision/3","prior_ref":reference});
        let mut cuts = 0;
        body(
            &decision,
            "workflow-decision",
            true,
            &mut m,
            &mut |_, _| panic!("old qualified edge must be cut"),
            &mut |field, v, _| {
                assert_eq!(field, "prior_ref");
                assert_eq!(v, &reference);
                cuts += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(cuts, 1);
        let mut refs = 0;
        body(
            &decision,
            "workflow-decision",
            false,
            &mut m,
            &mut |v, _| {
                assert_eq!(v, &reference);
                refs += 1;
                Ok(())
            },
            &mut |_, _, _| panic!("new prior target must stay immediate"),
        )
        .unwrap();
        assert_eq!(refs, 1);
    }

    #[test]
    fn actual_owned_fixedpoint_rejects_missing_body_and_unselected_header_rows() {
        use super::super::super::{body as produce, data::EconomicRecordView};
        use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
        let arena = AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let m = session.meter_mut();
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["scope".into(), "test".into()],
            source: "source".into(),
            operation_id: "operation".into(),
        };
        // Canonical, content-addressed DATA only. These tests assert no source
        // authority/original completion or financial candidate qualification.
        let b = produce::document_borrowed(
            m,
            "workflow-evidence",
            &json!({"schema":"ledger-workflow-retained-evidence/1","text":"leaf"}),
        )
        .unwrap();
        let a = produce::document_borrowed(
            m,
            "workflow-evidence",
            &json!({"schema":"ledger-workflow-retained-evidence/1","dependency_ref":b.reference}),
        )
        .unwrap();
        let unused = produce::document_borrowed(
            m,
            "workflow-evidence",
            &json!({"schema":"ledger-workflow-retained-evidence/1","text":"unused"}),
        )
        .unwrap();
        let output = produce::document_borrowed(
            m,
            "workflow-evidence",
            &json!({"schema":"ledger-workflow-retained-evidence/1","old_ref":a.reference}),
        )
        .unwrap();
        let inputs = [
            EconomicRecordView {
                reference: &a.reference,
                canonical_body: &a.canonical_body,
            },
            EconomicRecordView {
                reference: &b.reference,
                canonical_body: &b.canonical_body,
            },
            EconomicRecordView {
                reference: &unused.reference,
                canonical_body: &unused.canonical_body,
            },
        ];
        let observation =
            json!({"preexisting_input_refs":[a.reference,b.reference],"history_edges":[]});
        let outputs = [output];
        let run = |views: &[EconomicRecordView<'_>], observation: &Value| -> Result<usize> {
            let mut original = OriginalAuditSession::for_resource_profile_v2(&arena)?;
            let edges = super::super::super::input_edges::select_resolved_inputs9_edges(
                &key,
                views[0],
                &mut original,
            )?;
            assert_eq!(edges.direct_refs().collect::<Vec<_>>(), vec![&b.reference]);
            assert_eq!(edges.history_edges().count(), 0);
            drop(edges);
            // Parent DATA selection is separately charged, cannot take the
            // economic table claim, and does not memoize child semantics.
            let mut selected =
                Records::borrowed_parent_app(original.meter_mut(), &arena, &key, views)?;
            let parent_body = selected.provenance(original.meter_mut(), &key, &a.reference)?;
            assert_eq!(selected.consumed().len(), 1);
            drop(selected);
            let parent_work = original.usage().semantic_work;
            let mut child = original.meter_mut().original_economic_meter()?;
            let mut records = Records::borrowed_app(&mut child, &arena, &key, views, &[])?;
            let child_body = records.provenance(&mut child, &key, &a.reference)?;
            assert!(!child_body.is_same_owning_value(&parent_body));
            drop(child_body);
            drop(parent_body);
            let result = check_selected(&mut child, &key, &mut records, &outputs, observation);
            let count = records.consumed().len();
            drop(records);
            if result.is_ok() {
                original.meter_mut().finish_original_economic(child)?;
                assert!(original.usage().semantic_work > parent_work);
            } else {
                original.invalidate();
            }
            result.map(|_| count)
        };
        assert_eq!(run(&inputs, &observation).unwrap(), 2);
        let missing = json!({"preexisting_input_refs":[a.reference],"history_edges":[]});
        assert_eq!(
            run(&inputs, &missing).unwrap_err().detail,
            "old input closure edge absent from observed I"
        );
        let extra = json!({"preexisting_input_refs":[a.reference,b.reference,unused.reference],"history_edges":[]});
        assert_eq!(
            run(&inputs, &extra).unwrap_err().detail,
            "observed I contains an unselected input"
        );
        assert!(run(&inputs[..1], &observation).is_err());
        drop(outputs);
        drop(a);
        drop(b);
        drop(unused);
        drop(session);
        assert_eq!(arena.usage().unwrap().live_bytes, before);
    }
}

/// Must execute in the SAME economic child after every result/reservation has
/// been constructed. Current/ref grammar and F's exact ordered set framing
/// have already been checked; actual membership comparisons still execute.
pub(in crate::policy::workflow::transition_constructor) fn check(
    m: &mut CoreMeter,
    c: &DraftContextData,
    t: &mut Construction,
) -> Result<()> {
    let financial = t
        .financial_observation
        .as_ref()
        .ok_or_else(|| shape::fail("actual observation missing during result containment"))?;
    check_selected(
        m,
        &c.key,
        &mut t.records,
        &t.new_envelopes,
        &financial.observation,
    )
}
fn check_selected(
    m: &mut CoreMeter,
    key: &OperationKey,
    records: &mut Records,
    envelopes: &[ConstructedEnvelope],
    observation: &Value,
) -> Result<()> {
    let preexisting = shape::array(m, observation, "preexisting_input_refs")?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning result input containment"))?;
    for reference in records.consumed() {
        m.visit("containment.actual_consumed_operand")?;
        // Records appends ONLY preexisting operands to this private vector;
        // pending source rows have their separate intrinsic count. No caller
        // partition or repeated identity-index lookup establishes this fact.
        let value = owning_reference(m, &arena, reference)?;
        if !shape::member(m, preexisting, value.value())? {
            return Err(shape::fail("consumed old input absent from observed I"));
        }
    }
    let mut emit = |value: &Value, m: &mut CoreMeter| -> Result<()> {
        let reference = parse_owned_reference(m, &arena, value)?;
        for row in envelopes {
            m.visit("containment.actual_new_reference")?;
            if shape::same_ref(m, reference.reference(), &row.reference)? {
                return Ok(());
            }
        }
        if !shape::member(m, preexisting, value)? {
            return Err(shape::fail(
                "constructed result introduces an unaudited old input",
            ));
        }
        // A matching header Ref alone is not an input body. Preserve the exact
        // old record in the SAME owning I partition; complete transitive
        // closure/original semantics remain separately mandatory.
        records.provenance(m, key, reference.reference())?;
        Ok(())
    };
    for row in envelopes {
        m.visit("containment.actual_constructed_result")?;
        let value = row.decoded(m, 4194304)?;
        body(
            &value,
            &row.reference.kind,
            false,
            m,
            &mut emit,
            &mut |_, _, _| Err(shape::fail("new result cannot own an old history cut")),
        )?;
    }
    drop(emit);
    // Complete the actual explicit-Ref fixed point, not the prefetched operand
    // inventory or the supplied header list. Recipe-specific implicit locators
    // still have to be selected independently by the interpreter/parent seam.
    let history = shape::array(m, observation, "history_edges")?;
    let mut derived_history = 0usize;
    let mut position = 0usize;
    while w::compare(m, position, records.consumed().len())?.is_lt() {
        m.visit("containment.actual_old_closure_owner")?;
        let owner_value = owning_reference(m, &arena, &records.consumed()[position])?;
        let owner = parse_owned_reference(m, &arena, owner_value.value())?;
        let value = records.provenance(m, key, owner.reference())?;
        let mut edge = |value: &Value, m: &mut CoreMeter| -> Result<()> {
            let target = parse_owned_reference(m, &arena, value)?;
            for row in envelopes {
                m.visit("containment.old_to_new_exclusion")?;
                if shape::same_ref(m, target.reference(), &row.reference)? {
                    return Err(shape::fail(
                        "preexisting input refers to a new pending result",
                    ));
                }
            }
            if !shape::member(m, preexisting, value)? {
                return Err(shape::fail("old input closure edge absent from observed I"));
            }
            records.provenance(m, key, target.reference())?;
            Ok(())
        };
        let mut cut = |field: &'static str, target: &Value, m: &mut CoreMeter| -> Result<()> {
            let reference = parse_owned_reference(m, &arena, target)?;
            if !shape::same_text(m, &reference.reference().kind, &owner.reference().kind)? {
                return Err(shape::fail("qualified history target kind"));
            }
            let mut count = 0usize;
            for supplied in history {
                m.visit("containment.exact_history_edge")?;
                let actual_owner = shape::field(m, supplied, "owner_ref")?;
                if !shape::same(m, actual_owner, owner_value.value())? {
                    continue;
                }
                let actual_field = shape::text(m, supplied, "field")?;
                if !shape::same_text(m, actual_field, field)? {
                    continue;
                }
                let actual_target = shape::field(m, supplied, "target_ref")?;
                if shape::same(m, actual_target, target)? {
                    count = w::usize_add(m, count, 1)?;
                }
            }
            if !w::compare(m, count, 1usize)?.is_eq() {
                return Err(shape::fail("missing or duplicate actual history cut"));
            }
            derived_history = w::usize_add(m, derived_history, 1)?;
            Ok(())
        };
        // Exact predecessor identity/revision/schema/authority was part of the
        // earlier origin's SAME saved recipe, independently completed before
        // this consuming attempt. This only derives its current immediate cut;
        // it does not mint historical completion from a body or header.
        body(
            &value,
            &owner.reference().kind,
            true,
            m,
            &mut edge,
            &mut cut,
        )?;
        position = w::usize_add(m, position, 1)?;
    }
    if !w::compare(m, derived_history, history.len())?.is_eq() {
        return Err(shape::fail("extra supplied financial history cut"));
    }
    if !w::compare(m, records.consumed().len(), preexisting.len())?.is_eq() {
        return Err(shape::fail("observed I contains an unselected input"));
    }
    Ok(())
}
