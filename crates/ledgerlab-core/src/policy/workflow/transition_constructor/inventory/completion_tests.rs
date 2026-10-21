//! Focused declarations only. No calibration or admission entry is executed.
use super::super::{shape, CoreMeter};
use super::*;
use serde_json::{json, Value};
fn literal(name: &str) -> Value {
    let inputs: Value =
        serde_json::from_slice(include_bytes!("../fixtures/prerequisite-input-data.json")).unwrap();
    let row = inputs["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap();
    let hex = row["canonical_body_hex"].as_str().unwrap();
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    serde_json::from_slice(&bytes).unwrap()
}
#[test]
fn closed_binding_projection_checks_optional_end_and_rehashed_declared_fields() {
    let original = literal("A.binding");
    let signed = literal("A.terms");
    binding::validate(&mut CoreMeter::new(65536).unwrap(), &original, &signed).unwrap();
    for change in 0..4 {
        let mut b = original.clone();
        let mut t = signed.clone();
        match change {
            0 => t["binding_inputs"]["extra"] = json!(true),
            1 => b["ends_at"] = json!("2026-11-01T00:00:00.000000Z"),
            2 => {
                b["funding_legs"][0]["mode"] = json!("platform");
                t["binding_inputs"]["funding_legs"] = b["funding_legs"].clone();
            }
            _ => {
                b["source_slots"][0]["evidence_kinds"] = json!(["fabricated"]);
                t["binding_inputs"]["source_slots"] = b["source_slots"].clone();
            }
        }
        // Canonical body/Ref is honestly rebound before the declaration check;
        // these failures concern typed relations, not stale-hash integrity.
        super::tests::document_input("workflow-binding", &b);
        super::tests::document_input("workflow-acceptance-terms", &t);
        assert!(binding::validate(&mut CoreMeter::new(65536).unwrap(), &b, &t).is_err());
    }
}
#[test]
fn saved_request_closed_framing_rejects_rebound_descriptor_shapes() {
    let op = literal("A.activate.operation");
    let e = &op["request_evidence"];
    request::validate(
        &mut CoreMeter::new(65536).unwrap(),
        e,
        "agreement.activate",
        "ledger-workflow-activation/1",
    )
    .unwrap();
    for change in 0..4 {
        let mut bad = e.clone();
        match change {
            0 => bad["descriptor_resolution"]["anchors"][0]["extra"] = json!(true),
            1 => bad["original_request_bytes_sha256"] = json!("z".repeat(64)),
            2 => bad["descriptor_resolution"]["payload_schema"] = json!("wrong"),
            _ => bad["descriptor_resolution"]["anchors"][0]["role"] = json!("caller-trusted"),
        }
        let mut rebound = op.clone();
        rebound["request_evidence"] = bad.clone();
        let mut m = CoreMeter::new(65536).unwrap();
        super::super::records::hash(
            &mut m,
            "workflow-record",
            &json!(["workflow-operation", ["fixture", "sandbox"], rebound]),
        )
        .unwrap();
        assert!(request::validate(
            &mut CoreMeter::new(65536).unwrap(),
            &bad,
            "agreement.activate",
            "ledger-workflow-activation/1"
        )
        .is_err());
    }
}
#[test]
fn canonical_order_has_examined_byte_accounting_and_paid_recursive_numeric_limits() {
    let mut m = CoreMeter::new(65536).unwrap();
    assert!(shape::byte_order(&mut m, b"az", b"ba").unwrap().is_lt());
    assert_eq!(m.usage().scanned_bytes, 2);
    assert!(shape::byte_order(&mut m, b"a", b"aa").unwrap().is_lt());
    assert_eq!(m.usage().scanned_bytes, 4);
    let raw = br#"{\"a\":[true,\"x\"]}"#;
    let raw = std::str::from_utf8(raw).unwrap().replace("\\\"", "\"");
    let mut full = CoreMeter::new(65536).unwrap();
    super::super::codec::parse(&mut full, raw.as_bytes(), 262144).unwrap();
    let n = full.usage().semantic_work;
    let mut exact = CoreMeter::new(n).unwrap();
    super::super::codec::parse(&mut exact, raw.as_bytes(), 262144).unwrap();
    assert_eq!(full.usage(), exact.usage());
    assert!(super::super::codec::parse(
        &mut CoreMeter::new(n - 1).unwrap(),
        raw.as_bytes(),
        262144
    )
    .is_err());
}

fn literal_input(name: &str) -> super::super::RecordInput {
    let v: Value =
        serde_json::from_slice(include_bytes!("../fixtures/prerequisite-input-data.json")).unwrap();
    let r = v["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap();
    super::super::RecordInput {
        reference: serde_json::from_value(r["reference"].clone()).unwrap(),
        canonical_body: r["canonical_body_hex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    }
}
fn rebound_operation(v: &Value, c: &super::super::DraftContextData) -> super::super::RecordInput {
    use crate::canonical::CanonicalBytes;
    use sha2::{Digest, Sha256};
    let raw = CanonicalBytes::from_value(v).unwrap().into_vec();
    let frame = CanonicalBytes::from_value(&json!(["workflow-operation", c.key.scope, v])).unwrap();
    let mut h = Sha256::new();
    h.update(b"bean-counter/workflow-record/1\0");
    h.update(frame.as_slice());
    let digest = format!("sha256:{}", crate::canonical::hex(&h.finalize()));
    let key = CanonicalBytes::from_value(&v["key"]).unwrap();
    let mut h = Sha256::new();
    h.update(b"bean-counter/workflow-operation/1\0");
    h.update(key.as_slice());
    super::super::RecordInput {
        reference: super::super::ExactRef {
            kind: "workflow-operation".into(),
            id: format!("wop_{}", crate::canonical::hex(&h.finalize())),
            digest,
        },
        canonical_body: raw,
    }
}
#[test]
fn original_birth_key_missing_or_rebound_wrong_kind_and_actor_are_refused() {
    use super::super::{records::Records, representation::InvocationToken};
    let (_, c, _, _) = super::super::first_work_tests::test_inputs();
    let op = literal("A.activate.operation");
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &[]).unwrap();
    assert_eq!(
        transitions::operation(&mut m, &mut r, &c, &op["key"], "agreement.activate")
            .unwrap_err()
            .detail,
        "missing birth identity"
    );
    // An honestly rebound operation under another key is not the original locator,
    // even with the exact original results. No alias substitution or trusted flag.
    let mut alias = op.clone();
    alias["key"]["operation_id"] = json!("another-operation");
    let input = rebound_operation(&alias, &c);
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &[input]).unwrap();
    assert_eq!(
        transitions::operation(&mut m, &mut r, &c, &op["key"], "agreement.activate")
            .unwrap_err()
            .detail,
        "missing birth identity"
    );
    let mut wrong = op.clone();
    wrong["kind"] = json!("agreement.accept");
    let input = rebound_operation(&wrong, &c);
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &[input]).unwrap();
    assert_eq!(
        transitions::operation(&mut m, &mut r, &c, &op["key"], "agreement.activate")
            .unwrap_err()
            .detail,
        "data relation"
    );
    let mut auth = op["authority_snapshot"].clone();
    auth["actor"] = json!("other-admin");
    super::tests::document_input("workflow-authority", &auth);
    assert_eq!(
        transitions::admission(
            &mut CoreMeter::new(65536).unwrap(),
            &auth,
            &op["key"],
            &literal("A.binding"),
            &op["accepted_at"],
            &json!("fixture-admin"),
            "agreement.activate"
        )
        .unwrap_err()
        .detail,
        "data relation"
    );
    let _ = InvocationToken::new();
    let mut head = literal("A.activate.head");
    head["new_revision"] = json!("2");
    super::tests::document_input("workflow-head-transition", &head);
    assert!(transitions::initial(
        &mut CoreMeter::new(65536).unwrap(),
        &head,
        "agreement_activation"
    )
    .is_err());
}
#[test]
fn observation_supported_metadata_and_context_refuse_after_rebinding() {
    let (_, c, _, _) = super::super::first_work_tests::test_inputs();
    let ob_input = c
        .source_observation_inputs
        .iter()
        .find(|i| i.reference.kind == "workflow-authority-prefix-observation")
        .unwrap();
    let vf_input = c
        .source_observation_inputs
        .iter()
        .find(|i| i.reference.kind == "workflow-authority-audit-verifier")
        .unwrap();
    let ob: Value = serde_json::from_slice(&ob_input.canonical_body).unwrap();
    let vf: Value = serde_json::from_slice(&vf_input.canonical_body).unwrap();
    let state = literal("A.source_state");
    let root = literal("P1.authority");
    observation::validate(
        &mut CoreMeter::new(65536).unwrap(),
        &c,
        &state,
        &ob,
        &vf,
        &root,
    )
    .unwrap();
    for change in 0..5 {
        let mut bad = ob.clone();
        let mut verifier = vf.clone();
        match change {
            0 => bad["observed_at"] = json!("2026-10-14T00:00:00.000000Z"),
            1 => bad["origin_count"] = json!("0"),
            2 => bad["current_state_ref"] = json!(literal_input("B.source_state").reference),
            3 => verifier["artifacts"][0]["sha256"] = json!("0".repeat(64)),
            _ => verifier["origin_recipes"][0]["normalization_profile"] = json!("unsupported"),
        };
        super::tests::document_input("workflow-authority-prefix-observation", &bad);
        super::tests::document_input("workflow-authority-audit-verifier", &verifier);
        assert!(observation::validate(
            &mut CoreMeter::new(65536).unwrap(),
            &c,
            &state,
            &bad,
            &verifier,
            &root
        )
        .is_err());
    }
}
#[test]
fn required_unit_declarations_and_current_quantity_exposure_are_paid_and_bounded() {
    let (program, _, _, _) = super::super::first_work_tests::test_inputs();
    let b = literal("A.binding");
    let mut t = literal("A.slot_terms");
    t["quantity_ceilings"] = json!([]);
    super::tests::document_input("workflow-slot-terms", &t);
    assert_eq!(
        slots::validate(
            &mut CoreMeter::new(65536).unwrap(),
            &program,
            &b,
            &literal("A.catalog"),
            &literal("A.instantiation"),
            &t,
            (&literal("A.role"), &literal("A.verified_grant"))
        )
        .unwrap_err()
        .detail,
        "missing original unit quantity ceiling"
    );
    let mut t = literal("A.slot_terms");
    let mut b = b.clone();
    b["quantity_ceilings"][0]["maximum"] = json!("1.5");
    t["quantity_ceilings"] = b["quantity_ceilings"].clone();
    let facts = json!({"fixture.measure:quantity":"1"});
    let mut full = CoreMeter::new(65536).unwrap();
    entry::quantities(&mut full, &b, &t, "fixture.measure:measured", &facts).unwrap();
    let n = full.usage().semantic_work;
    let mut exact = CoreMeter::new(n).unwrap();
    entry::quantities(&mut exact, &b, &t, "fixture.measure:measured", &facts).unwrap();
    assert_eq!(full.usage(), exact.usage());
    assert_eq!(
        entry::quantities(
            &mut CoreMeter::new(n - 1).unwrap(),
            &b,
            &t,
            "fixture.measure:measured",
            &facts
        )
        .unwrap_err()
        .code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    assert_eq!(
        entry::quantities(
            &mut CoreMeter::new(65536).unwrap(),
            &b,
            &t,
            "fixture.measure:measured",
            &json!({"fixture.measure:quantity":"2"})
        )
        .unwrap_err()
        .detail,
        "accepted quantity ceiling"
    );
    assert_eq!(
        entry::exposure(
            &mut CoreMeter::new(65536).unwrap(),
            &b,
            &t,
            &100000000000000000001i128.into()
        )
        .unwrap_err()
        .detail,
        "accepted exposure ceiling"
    );
    assert_eq!(
        entry::exposure(&mut CoreMeter::new(65536).unwrap(), &b, &t, &(-1).into())
            .unwrap_err()
            .detail,
        "aggregate discount exceeds original charge"
    );
}

#[test]
fn extra_selected_declarations_and_alien_initial_grant_are_refused_after_rebinding() {
    let b = literal("A.binding");
    binding::selected_declarations(&mut CoreMeter::new(65536).unwrap(), &b).unwrap();
    for field in [
        "roles_refs",
        "funding_legs",
        "assent_refs",
        "authority_refs",
    ] {
        let mut bad = b.clone();
        let mut extra = bad[field][0].clone();
        match field {
            "roles_refs" => extra = json!(literal_input("B.role").reference),
            "assent_refs" => extra = json!(literal_input("B.assent").reference),
            "authority_refs" => extra = json!(literal_input("B.verified_grant").reference),
            _ => extra["name"] = json!("fixture.measure:alternate"),
        };
        bad[field].as_array_mut().unwrap().push(extra);
        bad[field].as_array_mut().unwrap().sort_by_key(|v| {
            crate::canonical::CanonicalBytes::from_value(v)
                .unwrap()
                .into_vec()
        });
        super::tests::document_input("workflow-binding", &bad);
        assert_eq!(
            binding::selected_declarations(&mut CoreMeter::new(65536).unwrap(), &bad)
                .unwrap_err()
                .code,
            "UNSUPPORTED_COMPOSITION"
        );
    }
    let (_, c, _, _) = super::super::first_work_tests::test_inputs();
    let owner = super::super::representation::InvocationToken::new();
    for field in ["grant_id", "revision"] {
        let mut grant = literal("A.verified_grant");
        grant[field] = if field == "revision" {
            json!("2")
        } else {
            json!("alien-enrollment")
        };
        let input = super::tests::document_input("workflow-source-grant", &grant);
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r =
            super::super::records::Records::new(&mut m, std::slice::from_ref(&input)).unwrap();
        let consumed = r
            .get(
                &mut m,
                &c.key,
                &input.reference,
                "workflow-source-grant",
                "ledger-workflow-source-grant/1",
            )
            .unwrap();
        assert_eq!(
            source::initial(
                &mut m,
                &mut r,
                &c,
                &owner,
                &literal("A.enrollment"),
                &literal("A.source_state"),
                (&consumed, &json!("customer-A"))
            )
            .unwrap_err()
            .detail,
            "data relation"
        );
    }
}
