use ledgerlab_core::{
    canonical::{self, outcome},
    policy::chaining::retained,
};
use serde_json::Value;
#[test]
fn frozen_economic_and_settlement_envelopes_reconstruct_exactly() {
    let mut count = 0;
    for (name, raw) in [
        (
            "aggregate-limit-rejection.json",
            include_str!("../../../contracts/candidates/v2/goldens/aggregate-limit-rejection.json"),
        ),
        (
            "cap-incompatibility.json",
            include_str!("../../../contracts/candidates/v2/goldens/cap-incompatibility.json"),
        ),
        (
            "correction-reinstatement.json",
            include_str!("../../../contracts/candidates/v2/goldens/correction-reinstatement.json"),
        ),
        (
            "correction-replacement.json",
            include_str!("../../../contracts/candidates/v2/goldens/correction-replacement.json"),
        ),
        (
            "correction-timing-rejection.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/correction-timing-rejection.json"
            ),
        ),
        (
            "cross-binding-corrections.json",
            include_str!("../../../contracts/candidates/v2/goldens/cross-binding-corrections.json"),
        ),
        (
            "decision-time-evidence.json",
            include_str!("../../../contracts/candidates/v2/goldens/decision-time-evidence.json"),
        ),
        (
            "duplicate-version-retry.json",
            include_str!("../../../contracts/candidates/v2/goldens/duplicate-version-retry.json"),
        ),
        (
            "equal-observation-times.json",
            include_str!("../../../contracts/candidates/v2/goldens/equal-observation-times.json"),
        ),
        (
            "fixed-success-fee.json",
            include_str!("../../../contracts/candidates/v2/goldens/fixed-success-fee.json"),
        ),
        (
            "full-reversal-reinstatement.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/full-reversal-reinstatement.json"
            ),
        ),
        (
            "inclusive-correction-deadlines.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/inclusive-correction-deadlines.json"
            ),
        ),
        (
            "inclusive-ordinary-deadlines.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/inclusive-ordinary-deadlines.json"
            ),
        ),
        (
            "lossless-event-and-outcome-terms.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/lossless-event-and-outcome-terms.json"
            ),
        ),
        (
            "percentage-rebate.json",
            include_str!("../../../contracts/candidates/v2/goldens/percentage-rebate.json"),
        ),
        (
            "predeclared-unclaimed-family.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/predeclared-unclaimed-family.json"
            ),
        ),
        (
            "retail-net-after-booking-discount.json",
            include_str!(
                "../../../contracts/candidates/v2/goldens/retail-net-after-booking-discount.json"
            ),
        ),
        (
            "rounded-zero.json",
            include_str!("../../../contracts/candidates/v2/goldens/rounded-zero.json"),
        ),
        (
            "signed-rounding.json",
            include_str!("../../../contracts/candidates/v2/goldens/signed-rounding.json"),
        ),
        (
            "supplier-separation.json",
            include_str!("../../../contracts/candidates/v2/goldens/supplier-separation.json"),
        ),
        (
            "timing-rejection.json",
            include_str!("../../../contracts/candidates/v2/goldens/timing-rejection.json"),
        ),
        (
            "two-rule-families.json",
            include_str!("../../../contracts/candidates/v2/goldens/two-rule-families.json"),
        ),
        (
            "zero-adjustment.json",
            include_str!("../../../contracts/candidates/v2/goldens/zero-adjustment.json"),
        ),
        (
            "zero-net-base.json",
            include_str!("../../../contracts/candidates/v2/goldens/zero-net-base.json"),
        ),
    ] {
        let h: Value = serde_json::from_str(raw).unwrap();
        for r in h["seed"].as_array().unwrap().iter().chain(
            h["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|d| d["records"].as_array().unwrap()),
        ) {
            let bytes = outcome::bytes(r).unwrap();
            assert_eq!(
                outcome::decode(&bytes).unwrap_or_else(|e| panic!("{} {}: {e}", name, r["kind"])),
                *r
            );
            count += 1;
        }
    }
    assert_eq!(count, 1450);
    let v: Value = serde_json::from_str(include_str!(
        "../../../contracts/candidates/reservation-settlement-v1/vectors.json"
    ))
    .unwrap();
    let mut count = 0;
    for h in v.as_array().unwrap() {
        for s in h["steps"].as_array().unwrap() {
            for r in s["records"].as_array().unwrap() {
                assert_eq!(outcome::decode(&outcome::bytes(r).unwrap()).unwrap(), *r);
                count += 1;
            }
        }
    }
    assert_eq!(count, 112);
}
#[test]
fn retained_original_evaluations_replay_every_byte_and_reject_forged_outputs() {
    let v: Value = serde_json::from_str(include_str!(
        "../../../contracts/candidates/v2/original-evaluations.json"
    ))
    .unwrap();
    let evaluations = v["evaluations"].as_object().unwrap();
    assert_eq!(evaluations.len(), 23);
    for (name, v) in evaluations {
        let bytes = outcome::bytes(v).unwrap();
        let decoded =
            retained::decode_evaluation(&bytes, &[]).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            canonical::CanonicalBytes::from_value(&decoded)
                .unwrap()
                .as_slice(),
            bytes
        );
        let mut bad = v.clone();
        bad["event"]["event_id"] = serde_json::json!("ev_forged");
        assert!(retained::decode_evaluation(&outcome::bytes(&bad).unwrap(), &[]).is_err());
        let mut bad = v.clone();
        bad["explanations"] = serde_json::json!([]);
        assert!(retained::decode_evaluation(&outcome::bytes(&bad).unwrap(), &[]).is_err());
    }
}
