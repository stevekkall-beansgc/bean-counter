use super::*;
use serde_json::{json, Value};
use types::*;
fn trace(name: &str) -> Value {
    let bytes = match name {
"vectors/independent-host-identity.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/independent-host-identity.json"),
"customer-trace.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json"),
"minimal-trace.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/minimal-trace.json"),
"vectors/grant-retirement.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/grant-retirement.json"),
"vectors/abort-partial.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/abort-partial.json"),
"vectors/writer-epoch.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/writer-epoch.json"),
"vectors/supplement-control.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/supplement-control.json"),
"vectors/authority-delegation.json" => include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/authority-delegation.json"),
_ => panic!("unknown fixture"), };
    serde_json::from_str(bytes).unwrap()
}
#[test]
fn frozen_all_29_command_shapes_roundtrip_exactly() {
    let mut kinds = std::collections::BTreeSet::new();
    for name in [
        "customer-trace.json",
        "vectors/grant-retirement.json",
        "vectors/abort-partial.json",
        "vectors/writer-epoch.json",
        "vectors/supplement-control.json",
        "vectors/authority-delegation.json",
        "vectors/independent-host-identity.json",
    ] {
        let t = trace(name);
        for step in t["commands"].as_array().unwrap() {
            let command = step.get("command").unwrap_or(step);
            let bytes = canonical_bytes(command, COMMAND_BYTES).unwrap();
            let parsed = ParsedCommand::parse(&bytes)
                .unwrap_or_else(|e| panic!("{name} {}: {e}", command["kind"]));
            assert_eq!(
                canonical_bytes(parsed.command(), COMMAND_BYTES).unwrap(),
                bytes
            );
            kinds.insert(command["kind"].as_str().unwrap().to_owned());
        }
    }
    assert_eq!(kinds.len(), 29, "{kinds:?}");
}
#[test]
fn strict_scalar_and_counter_boundaries() {
    let max = Count::new(Count::MAX).unwrap();
    assert!(max.checked_add(Count::new(1).unwrap()).is_err());
    assert!(max.checked_mul(max).is_err());
    for s in ["-1", "01", "-0", "1.0", "1000000000000000000000000000000"] {
        assert!(Count::parse(s).is_err());
    }
    assert!(Id::parse(&"é".repeat(64)).is_ok());
    assert!(Id::parse(&"é".repeat(65)).is_err());
    assert!(Time::parse("2026-02-30T00:00:00.000000Z").is_err());
    assert!(Time::parse("2026-09-22T00:00:00Z").is_err());
    let mut credit =
        resources::CounterCredit::new(Count::new(Count::MAX - 2).unwrap(), Count::new(2).unwrap())
            .unwrap();
    let before = credit.clone();
    assert!(credit.reserve(Count::new(1).unwrap()).is_err());
    assert_eq!(credit, before);
    credit
        .spend(Count::new(2).unwrap(), Count::new(2).unwrap())
        .unwrap();
    assert_eq!(credit.consumed, max);
    assert_eq!(credit.held, Count::ZERO);
}
#[test]
fn malformed_command_and_noncanonical_base64_reject() {
    let mut command = trace("minimal-trace.json")["commands"][0].clone();
    if command.get("command").is_some() {
        command = command["command"].clone();
    }
    command["payload"]["unexpected"] = json!(1);
    assert!(ParsedCommand::parse(&canonical_bytes(&command, COMMAND_BYTES).unwrap()).is_err());
    assert!(crate::canonical::parse(b"{\"x\":1,\"x\":2}").is_err());
    assert_eq!(proofs::decode_base64("e30=", 2).unwrap(), b"{}");
    for b in ["e31=", "e30=AAAA", "e30", "e30=\n"] {
        assert!(proofs::decode_base64(b, 32).is_err());
    }
}
#[test]
fn r3_encoding_has_separate_explicit_8mib_bound() {
    let value = json!({"\u{10000}":1,"\u{e000}":2});
    assert_eq!(
        canonical_bytes(&value, 100).unwrap(),
        crate::canonical::CanonicalBytes::from_value(&value)
            .unwrap()
            .as_slice()
    );
    let v = json!("a".repeat(4 * 1024 * 1024));
    assert!(crate::canonical::CanonicalBytes::from_value(&v).is_err());
    assert_eq!(
        canonical_bytes(&v, SEGMENT_BYTES).unwrap().len(),
        4 * 1024 * 1024 + 2
    );
    assert!(canonical_bytes(&v, COMMAND_BYTES).is_err());
    assert_eq!(MAX_INDEX_PATH_PAGES, 8922);
}

#[test]
fn prefix_and_membership_bind_full_source_and_key() {
    let hash = "0".repeat(64);
    let value = json!({"store":"store","scope":["t","e"],"target":"target","profile":PROFILE,"enrollment":hash,"registration":"reg","host":"g1","ordinal":"1","segment":hash,"root":hash});
    let prefix: commands::ExpectedPrefix = parse_exact(
        &canonical_bytes(&value, COMMAND_BYTES).unwrap(),
        COMMAND_BYTES,
    )
    .unwrap();
    let object_value = json!({"origin":{"store":"store","scope":["t","e"],"registration":"reg","host":"g1","ordinal":"1"},"kind":"GRANT","full_key":"grant","body":"e30=","body_hash":raw_sha256(b"{}"),"bytes":"2"});
    let object: commands::RetainedObject = serde_json::from_value(object_value).unwrap();
    let proof_value = json!({"store":"store","scope":["t","e"],"registration":"reg","host":"g1","ordinal":"1","segment":hash,"root":hash,"fact_kind":"GRANT","full_key":"grant","body_hash":raw_sha256(b"{}"),"bytes":"2","trusted_observation_ref":hash});
    let proof: commands::Proof = serde_json::from_value(proof_value).unwrap();
    proofs::VerifiedObjectBytes::check(object.clone()).unwrap();
    proofs::check_membership_identity(&proof, &object, &prefix).unwrap();
    let mut wrong = prefix.clone();
    wrong.target = Id::parse("different-target").unwrap();
    assert!(prefix.matches(&wrong).is_err());
    wrong = prefix.clone();
    wrong.enrollment = raw_sha256(b"other");
    assert!(prefix.matches(&wrong).is_err());
    let mut wrong_object = object.clone();
    wrong_object.origin.host = Id::parse("g2").unwrap();
    assert!(proofs::check_membership_identity(&proof, &wrong_object, &prefix).is_err());
    let mut wrong_proof = proof.clone();
    wrong_proof.full_key = commands::ProofFullKey::V2(Id::parse("other-key").unwrap());
    assert!(proofs::check_membership_identity(&wrong_proof, &object, &prefix).is_err());
}
