//! Framing witnesses only; no constructor, origin auditor or calibration runs.
use super::{super::*, *};
use crate::canonical::CanonicalBytes;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const READY: &str = "sha256:e023e7bf17aeed815f30b6fdce35fc0bf68cc9efcc0612a019b0b109494ac169";
const CAUSAL: &str = "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54";
fn current(name: &str) -> Value {
    let input: Value =
        serde_json::from_slice(include_bytes!("../fixtures/prerequisite-input-data.json")).unwrap();
    let row = input["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap();
    let bytes = row["canonical_body_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    serde_json::from_slice(&bytes).unwrap()
}
fn legacy() -> Value {
    // Preserved reviewed AGR03 acceptance/2 body, not a rewritten acceptance/3.
    let fixture:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    fixture["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["body"])
        .find(|b| {
            b["schema"] == "ledger-workflow-operation-record/1" && b["kind"] == "agreement.accept"
        })
        .unwrap()
        .clone()
}
fn hash(domain: &str, version: u8, v: &Value) -> String {
    let mut h = Sha256::new();
    h.update(format!("bean-counter/{domain}/{version}\0").as_bytes());
    h.update(CanonicalBytes::from_value(v).unwrap().as_slice());
    format!("sha256:{}", crate::canonical::hex(&h.finalize()))
}
fn base64(raw: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for part in raw.chunks(3) {
        let n = ((part[0] as u32) << 16)
            | ((part.get(1).copied().unwrap_or(0) as u32) << 8)
            | (part.get(2).copied().unwrap_or(0) as u32);
        out.push(A[((n >> 18) & 63) as usize] as char);
        out.push(A[((n >> 12) & 63) as usize] as char);
        out.push(if part.len() > 1 {
            A[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if part.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
fn rebound(op: &mut Value) -> super::super::super::RecordInput {
    // Rebind the available descriptor/evidence hashes and exact operation body.
    // Full saved locator/raw request/manifest/receipt authentication is the host
    // obligation; these tests intentionally exercise the supplied DATA framing.
    let e = &mut op["request_evidence"];
    let mut descriptor = e["descriptor_resolution"].clone();
    descriptor.as_object_mut().unwrap().remove("closure_digest");
    e["descriptor_resolution"]["closure_digest"] =
        json!(hash("descriptor-closure", 1, &descriptor));
    e["schema_bundle_digest"] = json!(hash("schema-bundle", 1, &descriptor["descriptors"]));
    e["request_hash"] = json!(hash(
        "operation-request",
        2,
        &json!([
            e["descriptor_resolution"]["closure_digest"],
            e["normalized_request"]
        ])
    ));
    let raw = CanonicalBytes::from_value(&e["normalized_request"]).unwrap();
    e["original_request_bytes_base64"] = json!(base64(raw.as_slice()));
    e["original_request_bytes_sha256"] =
        json!(crate::canonical::hex(&Sha256::digest(raw.as_slice())));
    let body = CanonicalBytes::from_value(op).unwrap().into_vec();
    let id = hash("workflow-operation", 1, &op["key"]);
    let digest = hash(
        "workflow-record",
        1,
        &json!(["workflow-operation", op["key"]["scope"], op]),
    );
    super::super::super::RecordInput {
        reference: super::super::super::ExactRef {
            kind: "workflow-operation".into(),
            id: format!("wop_{}", &id[7..]),
            digest,
        },
        canonical_body: body,
    }
}
fn framing(op: &Value) -> crate::Result<()> {
    let kind = op["kind"].as_str().unwrap();
    let schema = op["request_evidence"]["normalized_request"]["payload"]["schema"]
        .as_str()
        .unwrap();
    validate(
        &mut CoreMeter::new(65536).unwrap(),
        &op["request_evidence"],
        kind,
        schema,
    )
}
#[test]
fn saved_registry_positive_witnesses_preserve_legacy_v2_v3_and_activation() {
    let witnesses = [
        (legacy(), "ledger-workflow-agreement-accept/2", READY),
        (
            current("A.accept.operation"),
            "ledger-workflow-agreement-accept/3",
            CAUSAL,
        ),
        (
            current("A.activate.operation"),
            "ledger-workflow-activation/1",
            READY,
        ),
    ];
    for (op, schema, registry) in witnesses {
        assert_eq!(
            op["request_evidence"]["normalized_request"]["payload"]["schema"],
            schema
        );
        assert_eq!(
            op["request_evidence"]["descriptor_resolution"]["registry_digest"],
            registry
        );
        framing(&op).unwrap();
    }
}
#[test]
fn saved_registry_rehashed_cross_version_kind_norm_and_unknown_tuples_refuse() {
    for original in [
        legacy(),
        current("A.accept.operation"),
        current("A.activate.operation"),
    ] {
        for changed in 0..6 {
            let mut op = original.clone();
            let expected = op["request_evidence"]["descriptor_resolution"]["registry_digest"]
                .as_str()
                .unwrap()
                .to_owned();
            match changed {
                0 => {
                    op["request_evidence"]["descriptor_resolution"]["registry_digest"] =
                        json!(if expected == READY { CAUSAL } else { READY })
                }
                1 => {
                    op["request_evidence"]["descriptor_resolution"]["registry_digest"] =
                        json!(format!("sha256:{}", "0".repeat(64)))
                }
                2 => {
                    let other = if op["kind"] == "agreement.accept" {
                        "agreement.activate"
                    } else {
                        "agreement.accept"
                    };
                    op["kind"] = json!(other);
                    op["request_evidence"]["normalized_request"]["kind"] = json!(other);
                    op["request_evidence"]["descriptor_resolution"]["operation_kind"] =
                        json!(other);
                }
                3 => {
                    op["request_evidence"]["normalized_request"]["payload"]["schema"] =
                        json!("ledger-workflow-agreement-accept/99");
                    op["request_evidence"]["descriptor_resolution"]["payload_schema"] =
                        json!("ledger-workflow-agreement-accept/99");
                }
                4 => {
                    op["request_evidence"]["descriptor_resolution"]["normalization_profile"] =
                        json!("ledger-operation-normalization/1")
                }
                _ => {
                    op["request_evidence"]["descriptor_resolution"]["operation_kind"] =
                        json!("agreement.revoke")
                }
            }
            let input = rebound(&mut op);
            // Exact operation Ref/body is valid after the mutation; the framing
            // refusal is not a stale outer digest/ID or unconsumed body failure.
            let mut m = CoreMeter::new(65536).unwrap();
            let mut records =
                super::super::super::records::Records::new(&mut m, &[input.clone()]).unwrap();
            let key = serde_json::from_value(op["key"].clone()).unwrap();
            records
                .get(
                    &mut m,
                    &key,
                    &input.reference,
                    "workflow-operation",
                    "ledger-workflow-operation-record/1",
                )
                .unwrap();
            let error = framing(&op).unwrap_err();
            assert!(
                matches!(
                    error.code,
                    "INTEGRITY_FAILURE" | "UNSUPPORTED_SCHEMA_PROFILE"
                ),
                "{error:?}"
            );
        }
    }
}
#[test]
fn saved_operation_and_admit_authority_forbid_wrong_framing_and_economic_closure() {
    let (_, mut context, _, _) = super::super::super::first_work_tests::test_inputs();
    for change in 0..3 {
        let mut op = current("A.activate.operation");
        match change {
            0 => op["closure_profile"] = json!("absent"),
            1 => op["schema"] = json!("ledger-workflow-economic-operation-record/4"),
            _ => {
                op["request_evidence"]["normalization_profile"] =
                    json!("ledger-operation-normalization/1")
            }
        };
        let input = rebound(&mut op);
        context.key = serde_json::from_value(op["key"].clone()).unwrap();
        let mut m = CoreMeter::new(65536).unwrap();
        let mut records = super::super::super::records::Records::new(&mut m, &[input]).unwrap();
        assert!(transitions::operation(
            &mut m,
            &mut records,
            &context,
            &op["key"],
            "agreement.activate"
        )
        .is_err());
    }
    for change in 0..2 {
        let mut op = current("A.activate.operation");
        if change == 0 {
            op["authority_snapshot"]["schema"] = json!("ledger-workflow-agreement-authority/3");
        } else {
            op["authority_snapshot"]["closure_profile"] = json!("absent");
        }
        rebound(&mut op);
        let error = transitions::admission(
            &mut CoreMeter::new(65536).unwrap(),
            &op["authority_snapshot"],
            &op["key"],
            &current("A.accept.operation")["request_evidence"]["normalized_request"]["payload"]
                ["binding_inputs"],
            &op["accepted_at"],
            &json!("fixture-admin"),
            "agreement.activate",
        )
        .unwrap_err();
        assert_eq!(
            error.detail,
            if change == 0 {
                "data relation"
            } else {
                "unknown field"
            }
        );
    }
}
