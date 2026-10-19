//! Actual standalone outcome clock interpreter with frozen original Work DATA.
//! This is timing-only coverage, not substituted original authority replay.
use super::*;
fn context(at: &str) -> DraftContextData {
    DraftContextData {
        key: OperationKey {
            logical_store_id: "store".into(),
            scope: ["test".into(), "local".into()],
            source: "source".into(),
            operation_id: "correction".into(),
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
#[test]
fn standalone_outcome_correction_keeps_original_horizon_through_later_revisions() {
    let mut m = CoreMeter::new(65536).unwrap();
    let work_ref = ExactRef {
        kind: "workflow-work".into(),
        id: format!("wevent_{}", "1".repeat(64)),
        digest: format!("sha256:{}", "2".repeat(64)),
    };
    let declaration = json!({"anchor":"work_accepted_at","anchor_path":[],"start_offset_us":"0","occurrence_us":"1000000","receipt_grace_us":"1000000","acceptance_grace_us":"1000000","correction_us":"1000000","activation_deadline_us":"0"});
    let window = targets::frozen_window(
        &mut m,
        &declaration,
        &work_ref,
        "2026-10-19T00:00:00.000000Z",
    )
    .unwrap()
    .unwrap();
    assert_eq!(window["correction_until"], "2026-10-19T00:00:04.000000Z");
    // Later retained revisions near the endpoint do not restart the horizon.
    let old = json!({"revision":"3","accepted_at":"2026-10-19T00:00:03.000000Z"});
    let family = json!({"revision":"4","accepted_at":"2026-10-19T00:00:03.500000Z","window":&*window,"anchor_work_ref":work_ref});
    for at in ["2026-10-19T00:00:03.999999Z", "2026-10-19T00:00:04.000000Z"] {
        let owner = InvocationToken::new();
        original_correction_clock(
            &mut m,
            &context(at),
            &window,
            &work_ref,
            &old,
            &family,
            &owner,
        )
        .unwrap();
    }
    let owner = InvocationToken::new();
    assert_eq!(
        original_correction_clock(
            &mut m,
            &context("2026-10-19T00:00:04.000001Z"),
            &window,
            &work_ref,
            &old,
            &family,
            &owner
        )
        .err()
        .unwrap()
        .code,
        "CORRECTION_WINDOW_CLOSED"
    );
    let mut slid = family.clone();
    slid["window"]["correction_until"] = json!("2026-10-19T00:00:05.000000Z");
    assert_eq!(
        original_correction_clock(
            &mut m,
            &context("2026-10-19T00:00:04.500000Z"),
            &window,
            &work_ref,
            &old,
            &slid,
            &owner
        )
        .err()
        .unwrap()
        .code,
        "INTEGRITY_FAILURE"
    );
    let mut other_work = family.clone();
    other_work["anchor_work_ref"]["id"] = json!(format!("wevent_{}", "3".repeat(64)));
    assert_eq!(
        original_correction_clock(
            &mut m,
            &context("2026-10-19T00:00:04.000000Z"),
            &window,
            &work_ref,
            &old,
            &other_work,
            &owner
        )
        .err()
        .unwrap()
        .code,
        "INTEGRITY_FAILURE"
    );
    // A valid deadline does not permit moving behind either retained revision.
    assert_eq!(
        original_correction_clock(
            &mut m,
            &context("2026-10-19T00:00:03.499999Z"),
            &window,
            &work_ref,
            &old,
            &family,
            &owner
        )
        .err()
        .unwrap()
        .code,
        "CLOCK_REGRESSION"
    );
    let later_old = json!({"revision":"5","accepted_at":"2026-10-19T00:00:03.750000Z"});
    assert_eq!(
        original_correction_clock(
            &mut m,
            &context("2026-10-19T00:00:03.700000Z"),
            &window,
            &work_ref,
            &later_old,
            &family,
            &owner
        )
        .err()
        .unwrap()
        .code,
        "CLOCK_REGRESSION"
    );
    let mut reversed = context("2026-10-19T00:00:04.000000Z");
    reversed.received_at = "2026-10-19T00:00:04.000001Z".into();
    assert_eq!(
        original_correction_clock(&mut m, &reversed, &window, &work_ref, &old, &family, &owner)
            .err()
            .unwrap()
            .code,
        "CORRECTION_WINDOW_CLOSED"
    );
}
