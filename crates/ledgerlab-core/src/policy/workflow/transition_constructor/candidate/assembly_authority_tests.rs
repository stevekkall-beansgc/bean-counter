//! Selection-only regression with actual constructed/hash-bound output shapes;
//! no permission or full original financial replay is replaced by this fixture.
use super::super::super::{body, emitted_record::ConstructedEnvelope};
use super::*;
use crate::{
    canonical::audit_arena::AuditArena,
    validation_work::{OriginalAuditSession, ResourceLimits},
};
use serde_json::json;
fn reference(tag: &str) -> ExactRef {
    ExactRef {
        kind: "workflow-work".into(),
        id: format!("wevent_{}", tag.repeat(64)),
        digest: format!("sha256:{}", tag.repeat(64)),
    }
}
fn context() -> DraftContextData {
    DraftContextData {
        key: OperationKey {
            logical_store_id: "store".into(),
            scope: ["test".into(), "local".into()],
            source: "source".into(),
            operation_id: "reverse".into(),
        },
        actor: "actor".into(),
        canonical_economic_namespace: "namespace".into(),
        installation_mode: "isolated_synthetic".into(),
        received_at: "2026-10-19T00:00:00.000000Z".into(),
        accepted_at: "2026-10-19T00:00:00.000000Z".into(),
        authorities: vec![],
        source_observation_inputs: vec![],
    }
}
fn authority(m: &mut CoreMeter, slot: &str, operation: &str) -> ConstructedEnvelope {
    body::document(m,"workflow-economic-authority",json!({"schema":"ledger-workflow-economic-authority/8","operation":operation,"source_slot":slot})).unwrap()
}
fn decision(
    m: &mut CoreMeter,
    c: &DraftContextData,
    work: &ExactRef,
    auth: &ExactRef,
    transition: &str,
) -> ConstructedEnvelope {
    body::record(m,&c.key,"workflow-decision",format!("wdrev_{}","3".repeat(64)),json!({"schema":"ledger-workflow-decision-revision/1","kind":"work","transition":transition,"subject_ref":work,"authority_ref":auth,"operation_key":c.key})).unwrap()
}
fn check_root_authority(operation: &str, transition: &str, schema: &str) {
    let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
    let m = s.meter_mut();
    let c = context();
    let work = reference("1");
    let root = authority(m, "root:corrector", operation);
    let dependent = authority(m, "family:corrector", operation);
    let output = decision(m, &c, &work, &root.reference, transition);
    let request = json!({"schema":schema,"work_ref":work});
    let expected = local_root_authority(
        m,
        &c,
        &request,
        &[dependent.clone(), root.clone(), output.clone()],
    )
    .unwrap();
    assert_eq!(expected, 1);
    let reordered = local_root_authority(
        m,
        &c,
        &request,
        &[output.clone(), dependent.clone(), root.clone()],
    )
    .unwrap();
    assert_eq!(reordered, 2);
    // Missing or repeated exact root authority cannot silently select a
    // dependent authority sharing its schema and operation.
    assert!(local_root_authority(m, &c, &request, &[dependent.clone(), output.clone()]).is_err());
    assert!(local_root_authority(
        m,
        &c,
        &request,
        &[root.clone(), root.clone(), output.clone()]
    )
    .is_err());
    // Duplicate SAME root decisions are ambiguous, never first-result wins.
    assert!(
        local_root_authority(m, &c, &request, &[root.clone(), output.clone(), output]).is_err()
    );
    let wrong_transition = decision(m, &c, &work, &root.reference, "dependent_reverse");
    assert!(local_root_authority(m, &c, &request, &[root.clone(), wrong_transition]).is_err());
    let wrong_operation = authority(m, "root:corrector", "outcome.correct");
    let wrong_recipe = decision(m, &c, &work, &wrong_operation.reference, transition);
    assert!(local_root_authority(m, &c, &request, &[wrong_operation, wrong_recipe]).is_err());
    let wrong = decision(m, &c, &reference("2"), &dependent.reference, transition);
    assert!(local_root_authority(m, &c, &request, &[root, dependent, wrong]).is_err());
    let mut changed = context();
    changed.key.operation_id = "different".into();
    let auth = authority(m, "root:corrector", operation);
    let wrong_key = decision(m, &changed, &work, &auth.reference, transition);
    assert!(local_root_authority(m, &c, &request, &[auth, wrong_key]).is_err());
}

#[test]
fn local_reversal_root_authority_is_selected_by_its_work_decision_not_result_order() {
    check_root_authority(
        "work.reverse",
        "work_reverse",
        "ledger-workflow-work-reverse/2",
    );
}
#[test]
fn local_quantity_root_authority_is_selected_by_its_work_decision_not_result_order() {
    check_root_authority(
        "quantity.correct",
        "quantity_correct",
        "ledger-workflow-quantity-correction/2",
    );
}
