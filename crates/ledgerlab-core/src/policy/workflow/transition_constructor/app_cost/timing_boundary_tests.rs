//! Deterministic calls to the actual paid timing interpreter. Minimal timing
//! DATA here is not a substituted original authority/financial replay fixture.
use super::super::super::{authority::Terms, working_value::WorkingValue};
use super::super::{
    original::OriginalApp,
    plan::{AppPlan, CurrentApp},
    timing,
};
use super::*;
use crate::{
    canonical::audit_arena::AuditArena,
    policy::workflow::{
        document_digest,
        selected_program::{
            compile_selected_publication, SelectedCompilerSupport, SelectedSourceInput,
        },
        DefinitionRef,
    },
    validation_work::{OriginalAuditSession, ResourceLimits},
};
use serde_json::json;
fn reference() -> ExactRef {
    ExactRef {
        kind: "workflow-work".into(),
        id: format!("wevent_{}", "1".repeat(64)),
        digest: format!("sha256:{}", "2".repeat(64)),
    }
}
fn record(v: Value) -> RecordBody {
    RecordBody::Legacy(v)
}
fn context(at: &str) -> DraftContextData {
    DraftContextData {
        key: OperationKey {
            logical_store_id: "store".into(),
            scope: ["test".into(), "local".into()],
            source: "source".into(),
            operation_id: "timing".into(),
        },
        actor: "actor".into(),
        canonical_economic_namespace: "namespace".into(),
        installation_mode: "isolated_synthetic".into(),
        received_at: at.into(),
        accepted_at: at.into(),
        authorities: vec![],
        source_observation_inputs: vec![],
    }
}
fn original<'a>(a: &AuditArena, m: &mut CoreMeter, declaration: &'a Value) -> OriginalApp<'a> {
    let empty = record(json!({}));
    let r = reference();
    OriginalApp {
        work_ref: r.clone(),
        work: record(json!({"accepted_at":"2026-10-19T00:00:00.000000Z"})),
        claim_ref: r.clone(),
        claim: empty.clone(),
        binding_ref: r.clone(),
        terms: Terms {
            binding: empty.clone(),
            accepted: empty.clone(),
            activation: empty.clone(),
            catalog: empty.clone(),
            instantiation: empty.clone(),
            slot_terms: empty.clone(),
            slot_terms_ref: r.clone(),
            role_ref: r.clone(),
        },
        declaration,
        instruction: declaration,
        component_slot: "app".into(),
        original_decision_ref: r,
        _ownership: a.reserve_external_payload(1024, m).unwrap(),
    }
}
fn current(prior: &str) -> CurrentApp {
    let r = reference();
    let empty = record(json!({}));
    CurrentApp {
        claim_ref: r.clone(),
        claim: empty.clone(),
        revision_ref: r.clone(),
        revision: empty.clone(),
        decision_ref: r.clone(),
        decision: record(json!({"accepted_at":prior})),
        consumption_ref: r.clone(),
        consumption: empty,
        initial_decision_ref: r,
        initial_decision: record(json!({"accepted_at":"2026-10-19T00:00:10.000000Z"})),
    }
}
fn pair(a: &AuditArena, m: &mut CoreMeter) -> EvidencePair {
    let r = reference();
    EvidencePair {
        attestation_ref: r.clone(),
        attestation: record(json!({})),
        verification_ref: r,
        verification: record(json!({})),
        _ownership: a.reserve_external_payload(1024, m).unwrap(),
    }
}
#[test]
fn boundary_app_initial_t0_does_not_slide_and_causal_expiry_is_distinct() {
    let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
    let support = SelectedCompilerSupport::local_execution_with_session(&mut s).unwrap();
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../selected_program/fixtures/local-first-policy-input.json"
    ))
    .unwrap();
    let mut docs = fixture["documents"].as_array().unwrap().clone();
    let policy = docs
        .iter_mut()
        .find(|v| v["schema"] == "ledger-workflow-policy/4")
        .unwrap();
    policy["compiler_ref"] = json!(support.reference());
    let digest = document_digest("ledger-workflow-policy/4", policy).unwrap();
    let root = DefinitionRef::new(
        "workflow-policy",
        &format!("wdoc_{}", &digest[7..]),
        &digest,
    )
    .unwrap();
    docs.push(support.canonical_value().clone());
    let program = compile_selected_publication(
        SelectedSourceInput {
            documents: docs
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: root,
        },
        &support,
        &mut s,
    )
    .unwrap();
    let mut m = CoreMeter::new(65536).unwrap();
    let declaration = json!({"finalize_within_us":"20000000","correction_within_us":"10000000","correction_anchor":"initial_cost_accepted_at","causal_reversal":{"root_relation":"original_work_delayed_cost_dependency","mode":"exact_inverse_and_permanent_close","standalone_deadline_effect":"does_not_expire_preaccepted_causal_reversal"}});
    let observation = AppObservation {
        pair: pair(&a, &mut m),
        quantity: arithmetic::Ratio {
            numerator: 1.into(),
            denominator: 1.into(),
        },
    };
    let final_cost = AppFinalAmount {
        pair: pair(&a, &mut m),
        eligible: 0.into(),
    };
    let plan = AppPlan {
        request: WorkingValue::Legacy(json!({})),
        original: original(&a, &mut m, &declaration),
        observation,
        final_cost,
        old_cost: None,
        current: Some(current("2026-10-19T00:00:15.000000Z")),
        dependency_ref: reference(),
        dependency: record(json!({})),
        decision_id: "decision".into(),
        revision: 3.into(),
        _ownership: a.reserve_external_payload(1024, &mut m).unwrap(),
    };
    // A later revision at 15s cannot extend original T0=10s + C=10s.
    for at in ["2026-10-19T00:00:19.999999Z", "2026-10-19T00:00:20.000000Z"] {
        let clock = timing::standalone(&mut m, &context(at), &program, &plan).unwrap();
        assert_eq!(
            clock.standalone_correction_until,
            Some(
                Timestamp::parse("2026-10-19T00:00:20.000000Z")
                    .unwrap()
                    .micros()
            )
        );
    }
    let late = context("2026-10-19T00:00:20.000001Z");
    assert_eq!(
        timing::standalone(&mut m, &late, &program, &plan)
            .err()
            .unwrap()
            .code,
        "WINDOW_EXPIRED"
    );
    let (_, was_late) = timing::causal(
        &mut m,
        &late,
        &plan.original,
        plan.current.as_ref().unwrap(),
    )
    .unwrap();
    assert!(was_late);
    let mut without = declaration.clone();
    without.as_object_mut().unwrap().remove("causal_reversal");
    let no_clause = original(&a, &mut m, &without);
    assert!(timing::causal(&mut m, &late, &no_clause, plan.current.as_ref().unwrap()).is_err());
    let regression = context("2026-10-19T00:00:14.999999Z");
    assert_eq!(
        timing::standalone(&mut m, &regression, &program, &plan)
            .err()
            .unwrap()
            .detail,
        "app correction trusted clock regression"
    );
    assert_eq!(
        timing::causal(
            &mut m,
            &regression,
            &plan.original,
            plan.current.as_ref().unwrap()
        )
        .err()
        .unwrap()
        .detail,
        "causal cost clock regression"
    );
    // The causal timing seam alone does not establish the parent root's rights:
    // the real root executor separately validates that still-current window.
}
