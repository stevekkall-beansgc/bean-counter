//! Exact deadline endpoint interpreter, independently of host wall-clock wait.
use super::super::record_body::RecordBody;
use super::*;
use serde_json::json;
#[test]
fn boundary_deadline_close_requires_strictly_after_original_inclusive_endpoint() {
    let mut m = CoreMeter::new(65536).unwrap();
    let work_ref = ExactRef {
        kind: "workflow-work".into(),
        id: format!("wevent_{}", "1".repeat(64)),
        digest: format!("sha256:{}", "2".repeat(64)),
    };
    let family = json!({"window":{"anchor":"work_accepted_at","anchor_path":[],"start_offset_us":"0","occurrence_us":"1000000","receipt_grace_us":"1000000","acceptance_grace_us":"1000000","correction_us":"1000000","activation_deadline_us":"0"}});
    let work_at = "2026-10-19T00:00:00.000000Z";
    let window =
        super::super::targets::frozen_window(&mut m, &family["window"], &work_ref, work_at)
            .unwrap()
            .unwrap();
    let empty = RecordBody::Legacy(json!({}));
    let selected = FamilyContext {
        target_ref: work_ref.clone(),
        target: empty.clone(),
        work_ref: work_ref.clone(),
        work: RecordBody::Legacy(json!({"accepted_at":work_at})),
        claim_ref: work_ref.clone(),
        _claim: empty.clone(),
        decision_ref: work_ref.clone(),
        decision: empty.clone(),
        family_head_ref: work_ref.clone(),
        family_head: RecordBody::Legacy(
            json!({"state":"open","window":&*window,"anchor_work_ref":work_ref}),
        ),
        family: &family,
    };
    for at in ["2026-10-19T00:00:02.999999Z", "2026-10-19T00:00:03.000000Z"] {
        assert_eq!(
            deadline_time(&mut m, &selected, at).unwrap_err().code,
            "DEADLINE_NOT_EXPIRED"
        );
    }
    deadline_time(&mut m, &selected, "2026-10-19T00:00:03.000001Z").unwrap();
    // A stored later endpoint cannot slide the original work-based clock.
    let mut changed = selected;
    changed.family_head.legacy_mut()["window"]["accepted_by"] =
        json!("2026-10-19T00:00:04.000000Z");
    assert_eq!(
        deadline_time(&mut m, &changed, "2026-10-19T00:00:03.000001Z")
            .unwrap_err()
            .code,
        "INTEGRITY_FAILURE"
    );
}
