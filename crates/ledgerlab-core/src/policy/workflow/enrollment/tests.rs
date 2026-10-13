use super::*;
fn record(name: &str) -> Value {
    let fixture: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    fixture["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()
        .clone()
}
#[test]
fn exact_enrollments_and_initial_grants_match_reviewed_chain() {
    for customer in ["A", "B"] {
        let r = record(&format!("{customer}.enrollment"));
        let e = SourceEnrollment::parse(&serde_json::to_vec(&r["body"]).unwrap()).unwrap();
        assert_eq!(canonical::hex(e.canonical_bytes()), r["canonical_hex"]);
        assert_eq!(serde_json::to_value(e.reference()).unwrap(), r["ref"]);
        let g = super::super::SourceGrant::parse(
            &serde_json::to_vec(&record(&format!("{customer}.verified_grant"))["body"]).unwrap(),
        )
        .unwrap();
        g.validate_lineage(e.enrollment_id(), e.reference(), e.ceiling(), None)
            .unwrap();
    }
}
#[test]
fn closed_enrollment_refuses_null_unknown_duplicate_and_wrong_families() {
    let body = record("A.enrollment")["body"].clone();
    for (key, value) in [
        ("unknown", json!(true)),
        ("enrolled_by", Value::Null),
        ("evidence_ref", record("A.enrollment")["ref"].clone()),
        (
            "customer_refs",
            json!([record("A.enrollment")["ref"].clone()]),
        ),
    ] {
        let mut bad = body.clone();
        bad[key] = value;
        assert!(SourceEnrollment::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut duplicate = body.clone();
    duplicate["customer_refs"] = json!([body["customer_refs"][0], body["customer_refs"][0]]);
    assert!(SourceEnrollment::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    let raw = serde_json::to_string(&body).unwrap();
    let duplicate = raw.replacen("{", "{\"enrolled\\u005fby\":\"duplicate\",", 1);
    assert!(SourceEnrollment::parse(duplicate.as_bytes()).is_err());
}
#[test]
fn source_alias_enrollment_is_explicitly_unsupported_and_sets_normalize() {
    let mut body = record("A.enrollment")["body"].clone();
    body["ceiling"]["permissions"] = json!(["submit", "read", "correct"]);
    let normalized = SourceEnrollment::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert_eq!(
        canonical::hex(normalized.canonical_bytes()),
        record("A.enrollment")["canonical_hex"]
    );
    body["ceiling"]["canonical_economic_namespace"] = json!("urn:other:namespace");
    assert!(SourceEnrollment::parse(&serde_json::to_vec(&body).unwrap()).is_err());
}
