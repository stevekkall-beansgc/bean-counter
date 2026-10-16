//! These test individual DATA validators while the complete entry remains
//! unsupported. Inputs are literal prerequisites, never economic expected bodies.
use super::super::{records::Records, representation::InvocationToken, *};
use super::*;
use crate::canonical::CanonicalBytes;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn fixture(name: &str) -> Value {
    let input: Value =
        serde_json::from_slice(include_bytes!("../fixtures/prerequisite-input-data.json")).unwrap();
    let row = input["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap();
    let raw: Vec<u8> = row["canonical_body_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    serde_json::from_slice(&raw).unwrap()
}
// Rebind the actual mutated body before consumption: negatives reach DATA
// relationships after exact canonical/hash checks, rather than stale Ref errors.
fn consumed(m: &mut CoreMeter, c: &DraftContextData, kind: &str, v: &Value) -> Value {
    let schema = v["schema"].as_str().unwrap();
    let bytes = CanonicalBytes::from_value(v).unwrap().into_vec();
    let frame = CanonicalBytes::from_value(&json!([schema, v])).unwrap();
    let mut h = Sha256::new();
    h.update(b"bean-counter/workflow-document/1\0");
    h.update(frame.as_slice());
    let hex = crate::canonical::hex(&h.finalize());
    let reference = ExactRef {
        kind: kind.into(),
        id: format!("wdoc_{hex}"),
        digest: format!("sha256:{hex}"),
    };
    let mut r = Records::new(
        m,
        &[RecordInput {
            reference: reference.clone(),
            canonical_body: bytes,
        }],
    )
    .unwrap();
    r.get(m, &c.key, &reference, kind, schema).unwrap()
}
#[test]
fn initial_source_ceiling_and_registration_metadata_are_real_data_checks() {
    let (_, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    for change in 0..3 {
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &inputs).unwrap();
        let enrollment = fixture("A.enrollment");
        let state = fixture("A.source_state");
        let mut grant = fixture("A.verified_grant");
        if change == 1 {
            grant["operations"] = json!(["work.accept"]);
        } else if change == 2 {
            grant["source"] = json!("urn:other:source");
        }
        let grant = consumed(&mut m, &c, "workflow-source-grant", &grant);
        let result = source::initial(
            &mut m,
            &mut r,
            &c,
            &o,
            &enrollment,
            &state,
            &grant,
            &json!("customer-A"),
        );
        if change == 0 {
            assert!(result.is_ok(), "{:?}", result);
        } else {
            assert_eq!(result.unwrap_err().detail, "data relation");
        }
    }
}
#[test]
fn rebound_catalog_root_and_retail_payer_refuse_at_relational_validator() {
    let (program, c, _, _) = first_work_tests::test_inputs();
    for change in 0..3 {
        let mut m = CoreMeter::new(65536).unwrap();
        let binding = fixture("A.binding");
        let mut catalog = fixture("A.catalog");
        let inst = fixture("A.instantiation");
        let terms = fixture("A.slot_terms");
        let mut role = fixture("A.role");
        if change == 1 {
            catalog["components"][1]["root_slot"] = json!("A-setup");
        }
        if change == 2 {
            role["payer"] = json!("other-customer");
        }
        let catalog = consumed(&mut m, &c, "workflow-slot-catalog", &catalog);
        let role = consumed(&mut m, &c, "workflow-roles", &role);
        let result = slots::validate(
            &mut m,
            &program,
            &binding,
            &catalog,
            &inst,
            &terms,
            &role,
            &fixture("A.verified_grant"),
        );
        if change == 0 {
            assert!(result.is_ok(), "{:?}", result);
        } else {
            assert_eq!(result.unwrap_err().detail, "data relation");
        }
    }
}
#[test]
fn retained_content_hash_and_exact_acceptor_metadata_refuse_after_rebinding() {
    let (_, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    for change in 0..3 {
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &inputs).unwrap();
        let mut ev = fixture("A.registration_evidence");
        if change == 1 {
            ev["content_sha256"] = json!("0".repeat(64));
        }
        if change == 2 {
            ev["metadata"]["acceptor"] = json!("other-acceptor");
        }
        let ev = consumed(&mut m, &c, "workflow-evidence", &ev);
        let result = evidence::retained(
            &mut m,
            &mut r,
            &c,
            &o,
            &ev,
            "customer_registration",
            &json!({"customer":"customer-A"}),
            &json!({"acceptor":"customer-A"}),
            &fixture("A.customer"),
            "registered_at",
        );
        if change == 0 {
            assert!(result.is_ok(), "{:?}", result);
        } else {
            assert_eq!(result.unwrap_err().detail, "data relation");
        }
    }
}

#[test]
fn future_service_start_does_not_invalidate_prior_exact_assent() {
    let (_, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &inputs).unwrap();
    let mut binding = fixture("A.binding");
    binding["starts_at"] = json!("2026-10-14T00:00:00.000000Z");
    // This focused predicate receives exact assent and evidence together. The whole
    // binding/terms reconstruction remains separately gated and unqualified.
    let mut terms = fixture("A.terms");
    terms["binding_inputs"]["starts_at"] = binding["starts_at"].clone();
    let encoded = CanonicalBytes::from_value(&terms).unwrap();
    let mut h = Sha256::new();
    h.update(b"bean-counter/workflow-acceptance-terms/1\0");
    h.update(encoded.as_slice());
    let digest = format!("sha256:{}", crate::canonical::hex(&h.finalize()));
    let mut assent = fixture("A.assent");
    assent["terms_digest"] = json!(digest);
    let mut evidence = fixture("A.assent_evidence");
    evidence["metadata"]["terms_digest"] = assent["terms_digest"].clone();
    let assent = consumed(&mut m, &c, "workflow-assent", &assent);
    let ev = consumed(&mut m, &c, "workflow-evidence", &evidence);
    evidence::assent(
        &mut m,
        &mut r,
        &c,
        &o,
        &assent,
        &ev,
        "assent",
        &fixture("A.customer"),
        &binding,
    )
    .unwrap();
}
#[test]
fn closed_media_nonempty_base64_and_original_quantity_minimum_refuse() {
    let (program, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    for change in 0..3 {
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &inputs).unwrap();
        let mut ev = fixture("A.registration_evidence");
        match change {
            0 => ev["media_type"] = json!("application/json"),
            1 => ev["content_base64"] = json!(""),
            _ => ev["content_base64"] = json!("YR=="),
        };
        let ev = consumed(&mut m, &c, "workflow-evidence", &ev);
        let error = evidence::retained(
            &mut m,
            &mut r,
            &c,
            &o,
            &ev,
            "customer_registration",
            &json!({"customer":"customer-A"}),
            &json!({"acceptor":"customer-A"}),
            &fixture("A.customer"),
            "registered_at",
        )
        .unwrap_err();
        assert!(
            matches!(
                error.detail.as_str(),
                "evidence media type" | "evidence base64 bound" | "base64 unused bits"
            ),
            "{:?}",
            error
        );
    }
    let mut m = CoreMeter::new(65536).unwrap();
    let mut terms = fixture("A.slot_terms");
    terms["quantity_ceilings"][0]["maximum"] = json!("0");
    let terms = consumed(&mut m, &c, "workflow-slot-terms", &terms);
    let error = slots::validate(
        &mut m,
        &program,
        &fixture("A.binding"),
        &fixture("A.catalog"),
        &fixture("A.instantiation"),
        &terms,
        &fixture("A.role"),
        &fixture("A.verified_grant"),
    )
    .unwrap_err();
    assert_eq!(error.detail, "quantity below original minimum");
}

pub(super) fn document_input(kind: &str, v: &Value) -> RecordInput {
    let schema = v["schema"].as_str().unwrap();
    let bytes = CanonicalBytes::from_value(v).unwrap().into_vec();
    let frame = CanonicalBytes::from_value(&json!([schema, v])).unwrap();
    let mut h = Sha256::new();
    h.update(b"bean-counter/workflow-document/1\0");
    h.update(frame.as_slice());
    let hex = crate::canonical::hex(&h.finalize());
    RecordInput {
        reference: ExactRef {
            kind: kind.into(),
            id: format!("wdoc_{hex}"),
            digest: format!("sha256:{hex}"),
        },
        canonical_body: bytes,
    }
}
#[test]
fn changed_historical_registration_root_is_valid_without_snapshot_ref_equality() {
    let (_, c, _, mut inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    let mut root = fixture("P1.authority");
    root["basis"]["source_grant_revision"] = json!("2");
    let bytes = CanonicalBytes::from_value(&root["basis"]).unwrap();
    let mut h = Sha256::new();
    h.update(b"bean-counter/local-authority-basis/1\0");
    h.update(bytes.as_slice());
    root["basis_digest"] = json!(format!("sha256:{}", crate::canonical::hex(&h.finalize())));
    let root_input = document_input("workflow-authority", &root);
    let mut registration = fixture("A.customer");
    registration["authority_ref"] = json!(root_input.reference);
    inputs.push(root_input);
    let registration_input = document_input("workflow-customer", &registration);
    let mut enrollment = fixture("A.enrollment");
    enrollment["customer_refs"] = json!([registration_input.reference]);
    inputs.push(registration_input);
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &inputs).unwrap();
    // The selected source scope/ceiling/state/grant retain their immutable values.
    // Only the registered customer's later administrative snapshot changes.
    source::initial(
        &mut m,
        &mut r,
        &c,
        &o,
        &enrollment,
        &fixture("A.source_state"),
        &fixture("A.verified_grant"),
        &json!("customer-A"),
    )
    .unwrap();
}
#[test]
fn charged_base64_exact_work_and_refusal_and_closed_source_names() {
    let mut m = CoreMeter::new(65536).unwrap();
    assert_eq!(evidence::decode(&mut m, "YWJjZA==").unwrap(), b"abcd");
    let n = m.usage().semantic_work;
    assert!(m.usage().trace.iter().any(|e| e.kind.starts_with("arith.")));
    let mut exact = CoreMeter::new(n).unwrap();
    assert_eq!(evidence::decode(&mut exact, "YWJjZA==").unwrap(), b"abcd");
    assert_eq!(exact.usage(), m.usage());
    let mut less = CoreMeter::new(n - 1).unwrap();
    assert_eq!(
        evidence::decode(&mut less, "YWJjZA==").unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    let (_, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    for field in ["permissions", "operations", "source_slots"] {
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &inputs).unwrap();
        let mut enrollment = fixture("A.enrollment");
        let mut grant = fixture("A.verified_grant");
        enrollment["ceiling"][field] = json!(["not-an-approved-value"]);
        grant[field] = enrollment["ceiling"][field].clone();
        let grant = consumed(&mut m, &c, "workflow-source-grant", &grant);
        let error = source::initial(
            &mut m,
            &mut r,
            &c,
            &o,
            &enrollment,
            &fixture("A.source_state"),
            &grant,
            &json!("customer-A"),
        )
        .unwrap_err();
        assert!(
            matches!(
                error.detail.as_str(),
                "closed enum" | "qualified name required"
            ),
            "{:?}",
            error
        );
    }
}

#[test]
fn publication_program_context_linkage_consumes_actual_source_inputs() {
    let (program, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    for change in 0..2 {
        let mut m = CoreMeter::new(65536).unwrap();
        let mut r = Records::new(&mut m, &inputs).unwrap();
        let mut b = fixture("A.binding");
        if change == 1 {
            b["accepted_context"]["fact_schema_ref"] =
                fixture("P1.product")["fact_schemas"][0].clone();
        }
        let b = consumed(&mut m, &c, "workflow-binding", &b);
        let result = publication::validate(
            &mut m,
            &mut r,
            &c,
            &o,
            &program,
            &b,
            &fixture("A.accepted"),
            &fixture("A.terms"),
        );
        if change == 0 {
            assert!(result.is_ok(), "{:?}", result);
        } else {
            assert_eq!(result.unwrap_err().detail, "data relation");
        }
    }
}
#[test]
fn source_enrollment_actor_link_survives_snapshot_relaxation() {
    let (_, c, _, mut inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    let mut e = fixture("A.enrollment_evidence");
    e["retained_by"] = json!("other-admin");
    let e = document_input("workflow-evidence", &e);
    let mut enrol = fixture("A.enrollment");
    enrol["evidence_ref"] = json!(e.reference);
    inputs.push(e);
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &inputs).unwrap();
    let enrol = consumed(&mut m, &c, "workflow-source-enrollment", &enrol);
    let error = source::initial(
        &mut m,
        &mut r,
        &c,
        &o,
        &enrol,
        &fixture("A.source_state"),
        &fixture("A.verified_grant"),
        &json!("customer-A"),
    )
    .unwrap_err();
    assert_eq!(error.detail, "data relation");
}

#[test]
fn initial_birth_locates_original_operation_not_an_alias_result_locator() {
    let (program, c, _, inputs) = first_work_tests::test_inputs();
    let o = InvocationToken::new();
    let b = fixture("A.binding");
    let a = fixture("A.accepted");
    let t = fixture("A.activation");
    let h = fixture("A.activate.head");
    let mut m = CoreMeter::new(65536).unwrap();
    let mut r = Records::new(&mut m, &inputs).unwrap();
    let result = transitions::validate(&mut m, &mut r, &c, &o, &program, &b, &a, &t, &h);
    if let Err(error) = &result {
        let usage = m.usage();
        println!("BIRTH_DIAGNOSTIC {}",serde_json::to_string(&json!({"error":{"code":error.code,"detail":error.detail},"maximum":65536,"semantic_work":usage.semantic_work,"raw_request_bytes":usage.raw_request_bytes,"unique_resolved_bytes":usage.unique_resolved_bytes,"unique_resolved_refs":usage.unique_resolved_refs,"new_body_bytes":usage.new_body_bytes,"scanned_bytes":usage.scanned_bytes,"hashed_bytes":usage.hashed_bytes,"body_bytes_read":usage.body_bytes_read,"canonical_bytes_written":usage.canonical_bytes_written,"trace":usage.trace.iter().map(|x|json!({"phase":format!("{:?}",x.phase),"kind":x.kind,"operand_limbs":x.operands,"cost":x.cost})).collect::<Vec<_>>()})).unwrap());
        println!(
            "BIRTH_COMPLETED_REF_INPUTS {}",
            serde_json::to_string(&r.consumed()).unwrap()
        );
    }
    assert!(
        result.is_ok(),
        "{:?} work={} ",
        result,
        m.usage().semantic_work
    );
}
