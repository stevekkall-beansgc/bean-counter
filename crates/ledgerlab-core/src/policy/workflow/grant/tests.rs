use super::*;
use serde_json::json;
fn record(name: &str) -> Value {
    let fixture:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    fixture["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()
        .clone()
}
fn parse(v: &Value) -> SourceGrant {
    SourceGrant::parse(&serde_json::to_vec(v).unwrap()).unwrap()
}
fn reference(v: &Value) -> AgreementRef {
    AgreementRef::parse(&serde_json::to_vec(v).unwrap()).unwrap()
}
#[test]
fn exact_selected_initial_grants_bind_original_enrollment_and_bytes() {
    for customer in ["A", "B"] {
        let r = record(&format!("{customer}.verified_grant"));
        let g = parse(&r["body"]);
        let e = record(&format!("{customer}.enrollment"));
        let ceiling =
            SourceCeiling::parse(&serde_json::to_vec(&e["body"]["ceiling"]).unwrap()).unwrap();
        g.validate_lineage(
            e["body"]["enrollment_id"].as_str().unwrap(),
            &reference(&e["ref"]),
            &ceiling,
            None,
        )
        .unwrap();
        assert_eq!(canonical::hex(g.canonical_bytes()), r["canonical_hex"]);
        assert_eq!(g.document_id(), r["ref"]["id"].as_str().unwrap());
        assert_eq!(g.content_digest(), r["ref"]["digest"].as_str().unwrap());
    }
}
#[test]
fn current_narrowing_cannot_restore_original_rights_or_skip_revision() {
    let r = record("A.verified_grant");
    let original = parse(&r["body"]);
    let e = record("A.enrollment");
    let er = reference(&e["ref"]);
    let mut narrowed = r["body"].clone();
    narrowed["revision"] = json!("2");
    narrowed["permissions"] = json!(["read"]);
    let prior = parse(&narrowed);
    prior
        .validate_lineage(
            original.grant_id(),
            &er,
            original.ceiling(),
            Some(&original),
        )
        .unwrap();
    let mut restored = r["body"].clone();
    restored["revision"] = json!("3");
    assert!(parse(&restored)
        .validate_lineage(original.grant_id(), &er, original.ceiling(), Some(&prior))
        .is_err());
    narrowed["revision"] = json!("4");
    assert!(parse(&narrowed)
        .validate_lineage(original.grant_id(), &er, original.ceiling(), Some(&prior))
        .is_err());
    narrowed["revision"] = json!("3");
    parse(&narrowed)
        .validate_lineage(original.grant_id(), &er, original.ceiling(), Some(&prior))
        .unwrap();
}
#[test]
fn initial_exact_ceiling_and_original_reference_cannot_be_substituted() {
    let mut value = record("A.verified_grant")["body"].clone();
    let original = parse(&value);
    let er = reference(&record("A.enrollment")["ref"]);
    assert!(original
        .validate_lineage(
            original.grant_id(),
            &reference(&record("B.enrollment")["ref"]),
            original.ceiling(),
            None
        )
        .is_err());
    value["permissions"] = json!(["read"]);
    assert!(parse(&value)
        .validate_lineage(original.grant_id(), &er, original.ceiling(), None)
        .is_err());
    value["unexpected"] = json!(true);
    assert!(SourceGrant::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn checked_reference_remains_closed_family_specific_locator() {
    let fixture:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    for r in fixture["records"].as_array().unwrap() {
        reference(&r["ref"]);
    }
    let v = record("A.enrollment")["ref"].clone();
    for key in ["kind", "id", "digest"] {
        let mut wrong = v.clone();
        wrong[key] = json!("unknown");
        assert!(AgreementRef::parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
    }
    let mut wrong = v;
    wrong["administrator"] = json!(true);
    assert!(AgreementRef::parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
}
