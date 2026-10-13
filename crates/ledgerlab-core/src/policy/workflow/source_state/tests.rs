use super::*;
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
fn enrollment(customer: &str) -> SourceEnrollment {
    SourceEnrollment::parse(
        &serde_json::to_vec(&record(&format!("{customer}.enrollment"))["body"]).unwrap(),
    )
    .unwrap()
}
fn grant(customer: &str) -> SourceGrant {
    SourceGrant::parse(
        &serde_json::to_vec(&record(&format!("{customer}.verified_grant"))["body"]).unwrap(),
    )
    .unwrap()
}
fn parse(v: &Value, e: &SourceEnrollment) -> SourceState {
    SourceState::parse(&serde_json::to_vec(v).unwrap(), e).unwrap()
}
#[test]
fn exact_initial_and_revoked_state_bytes_and_semantic_id_match_reviewed_chain() {
    for customer in ["A", "B"] {
        let e = enrollment(customer);
        let g = grant(customer);
        let r = record(&format!("{customer}.initial_source_state"));
        let s = parse(&r["body"], &e);
        s.validate_lineage(&e, Some(&g), None).unwrap();
        assert_eq!(canonical::hex(s.canonical_bytes()), r["canonical_hex"]);
        assert_eq!(serde_json::to_value(s.reference()).unwrap(), r["ref"]);
    }
    let e = enrollment("A");
    let g = grant("A");
    let first = parse(&record("A.initial_source_state")["body"], &e);
    let r = record("A.revoked_source_state");
    let s = parse(&r["body"], &e);
    s.validate_lineage(&e, None, Some((&first, &g))).unwrap();
    assert_eq!(canonical::hex(s.canonical_bytes()), r["canonical_hex"]);
    assert_eq!(serde_json::to_value(s.reference()).unwrap(), r["ref"]);
}
#[test]
fn lineage_refuses_substitution_stale_state_and_reenabling() {
    let e = enrollment("A");
    let g = grant("A");
    let first = parse(&record("A.initial_source_state")["body"], &e);
    assert!(first.validate_lineage(&e, Some(&grant("B")), None).is_err());
    let mut revoked = record("A.revoked_source_state")["body"].clone();
    revoked["prior_state_ref"] = record("B.initial_source_state")["ref"].clone();
    assert!(parse(&revoked, &e)
        .validate_lineage(&e, None, Some((&first, &g)))
        .is_err());
    revoked = record("A.revoked_source_state")["body"].clone();
    let stopped = parse(&revoked, &e);
    let mut restored = record("A.initial_source_state")["body"].clone();
    restored["revision"] = json!("3");
    restored["prior_state_ref"] = serde_json::to_value(stopped.reference()).unwrap();
    assert!(parse(&restored, &e)
        .validate_lineage(&e, Some(&g), Some((&stopped, &g)))
        .is_err());
    revoked["operation_key"]["source"] = json!("urn:changed:admin");
    parse(&revoked, &e)
        .validate_lineage(&e, None, Some((&first, &g)))
        .unwrap();
}
#[test]
fn restriction_pins_new_grant_and_cannot_restore_removed_rights() {
    let e = enrollment("A");
    let g = grant("A");
    let first = parse(&record("A.initial_source_state")["body"], &e);
    let mut grant_body = g.canonical_value().unwrap();
    grant_body["revision"] = json!("2");
    grant_body["permissions"] = json!(["read"]);
    let narrowed = SourceGrant::parse(&serde_json::to_vec(&grant_body).unwrap()).unwrap();
    let mut body = first.canonical_value().unwrap();
    body["revision"] = json!("2");
    body["prior_state_ref"] = serde_json::to_value(first.reference()).unwrap();
    body["current_grant_ref"] = json!({"kind":"workflow-source-grant","id":narrowed.document_id(),"digest":narrowed.content_digest()});
    let second = parse(&body, &e);
    second
        .validate_lineage(&e, Some(&narrowed), Some((&first, &g)))
        .unwrap();
    assert!(second
        .validate_lineage(&e, Some(&g), Some((&first, &g)))
        .is_err());
    grant_body = g.canonical_value().unwrap();
    grant_body["revision"] = json!("3");
    let restored = SourceGrant::parse(&serde_json::to_vec(&grant_body).unwrap()).unwrap();
    body["revision"] = json!("3");
    body["prior_state_ref"] = serde_json::to_value(second.reference()).unwrap();
    body["current_grant_ref"] = json!({"kind":"workflow-source-grant","id":restored.document_id(),"digest":restored.content_digest()});
    assert!(parse(&body, &e)
        .validate_lineage(&e, Some(&restored), Some((&second, &narrowed)))
        .is_err());
}
#[test]
fn closed_variants_refuse_null_duplicate_cross_scope_and_wrong_prior_shape() {
    let e = enrollment("A");
    let base = record("A.initial_source_state")["body"].clone();
    for (key, value) in [
        ("unknown", json!(true)),
        ("actor", Value::Null),
        ("status", json!("revoked")),
        ("revision", json!("2")),
        (
            "prior_state_ref",
            record("A.initial_source_state")["ref"].clone(),
        ),
        ("enrollment_ref", record("B.enrollment")["ref"].clone()),
    ] {
        let mut body = base.clone();
        body[key] = value;
        assert!(SourceState::parse(&serde_json::to_vec(&body).unwrap(), &e).is_err());
    }
    let mut body = base.clone();
    body["operation_key"]["scope"] = json!(["other", "sandbox"]);
    assert!(SourceState::parse(&serde_json::to_vec(&body).unwrap(), &e).is_err());
    let raw =
        serde_json::to_string(&base)
            .unwrap()
            .replacen("{", "{\"act\\u006fr\":\"duplicate\",", 1);
    assert!(SourceState::parse(raw.as_bytes(), &e).is_err());
}
