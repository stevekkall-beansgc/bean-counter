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

fn shared_input() -> (SourceEnrollment, Value, Value) {
    let original = record("A.enrollment")["body"].clone();
    let owner = SourceEnrollment::parse(&serde_json::to_vec(&original).unwrap()).unwrap();
    let mut target = original.clone();
    target["enrollment_id"] = json!("enrollment-shared-D");
    target["enrolled_at"] = json!("2026-10-13T00:00:06.000000Z");
    target["ceiling"]["source"] = json!("urn:fixture:shared-D");
    // Deliberately independent D rights, not copied from the owner by parsing.
    target["ceiling"]["permissions"] = json!(["read"]);
    target["ceiling"]["operations"] = json!(["evidence.retain"]);
    let payload = json!({"schema":"ledger-workflow-source-enroll/3","action":"source.enroll","expected_revision":"0","enrollment_id":target["enrollment_id"],"customer_refs":target["customer_refs"],"ceiling":target["ceiling"],"evidence_ref":target["evidence_ref"],"namespace_owner_enrollment_ref":owner.reference()});
    (owner, target, payload)
}

#[test]
fn shared_namespace_reader_preserves_exact_producer_records_and_legacy_refusal() {
    use super::super::{
        transition_constructor::{
            produce_initial_shared_namespace_source_records, ExactRef, OperationKey,
        },
        SourceGrant, SourceState,
    };
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    let (owner, target, payload) = shared_input();
    let raw = serde_json::to_vec(&target).unwrap();
    assert!(SourceEnrollment::parse(&raw).is_err());
    let shared = SourceEnrollment::parse_shared_namespace(&raw, &payload, &owner).unwrap();
    assert_eq!(
        shared.namespace_owner_enrollment_ref(),
        Some(owner.reference())
    );
    assert_eq!(
        shared.canonical_bytes(),
        CanonicalBytes::from_value(&target).unwrap().as_slice()
    );
    assert_eq!(
        shared.ceiling().canonical_value().unwrap()["permissions"],
        json!(["read"])
    );
    assert_ne!(
        shared.ceiling().canonical_bytes(),
        owner.ceiling().canonical_bytes()
    );
    let key = OperationKey {
        logical_store_id: "shared-reader-store".into(),
        scope: owner.scope().clone(),
        source: owner.ceiling().source().into(),
        operation_id: "enroll-shared-D".into(),
    };
    let root: ExactRef = serde_json::from_value(target["authority_ref"].clone()).unwrap();
    let arena = AuditArena::new().unwrap();
    let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let produced = produce_initial_shared_namespace_source_records(
        (&key, &payload),
        &payload["ceiling"],
        target["installation_mode"].as_str().unwrap(),
        target["enrolled_by"].as_str().unwrap(),
        target["enrolled_at"].as_str().unwrap(),
        &root,
        &mut session,
    )
    .unwrap();
    let reconstructed = SourceEnrollment::parse_shared_namespace(
        produced.enrollment().canonical_body(),
        &payload,
        &owner,
    )
    .unwrap();
    assert_eq!(shared.canonical_bytes(), reconstructed.canonical_bytes());
    assert_eq!(
        serde_json::to_value(shared.reference()).unwrap(),
        serde_json::to_value(produced.enrollment().reference()).unwrap()
    );
    let grant = SourceGrant::parse(produced.grant().canonical_body()).unwrap();
    let state = SourceState::parse(produced.state().canonical_body(), &reconstructed).unwrap();
    state
        .validate_lineage(&reconstructed, Some(&grant), None)
        .unwrap();
    assert_eq!(
        grant.ceiling().canonical_bytes(),
        shared.ceiling().canonical_bytes()
    );
    assert!(SourceEnrollment::parse(produced.enrollment().canonical_body()).is_err());
}

#[test]
fn shared_namespace_reader_requires_exact_original_owner_and_request_correspondence() {
    let (owner, target, payload) = shared_input();
    let raw = serde_json::to_vec(&target).unwrap();
    for (field, value) in [
        ("schema", json!("ledger-workflow-source-enroll/2")),
        ("action", json!("source.restrict")),
        ("expected_revision", json!("1")),
        ("enrollment_id", json!("other")),
        (
            "namespace_owner_enrollment_ref",
            record("B.enrollment")["ref"].clone(),
        ),
        (
            "evidence_ref",
            record("B.enrollment")["body"]["evidence_ref"].clone(),
        ),
        (
            "customer_refs",
            record("B.enrollment")["body"]["customer_refs"].clone(),
        ),
        ("unknown", json!(true)),
    ] {
        let mut wrong = payload.clone();
        wrong[field] = value;
        assert!(
            SourceEnrollment::parse_shared_namespace(&raw, &wrong, &owner).is_err(),
            "{field}"
        );
    }
    let mut missing = payload.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("namespace_owner_enrollment_ref");
    assert!(SourceEnrollment::parse_shared_namespace(&raw, &missing, &owner).is_err());
    for (field, value) in [
        ("scope", json!(["other", "sandbox"])),
        ("installation_mode", json!("real_local")),
        ("enrolled_by", json!("other-principal")),
        ("enrolled_at", json!("2026-10-13T00:00:04.000000Z")),
        (
            "customer_refs",
            record("B.enrollment")["body"]["customer_refs"].clone(),
        ),
    ] {
        let mut wrong = target.clone();
        wrong[field] = value;
        let mut claimed = payload.clone();
        claimed["customer_refs"] = wrong["customer_refs"].clone();
        assert!(
            SourceEnrollment::parse_shared_namespace(
                &serde_json::to_vec(&wrong).unwrap(),
                &claimed,
                &owner
            )
            .is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("principal_id", json!("other-principal")),
        ("canonical_economic_namespace", json!("urn:fixture:other")),
        ("permissions", json!(["read", "submit"])),
    ] {
        let mut wrong = target.clone();
        wrong["ceiling"][field] = value;
        // Even a jointly changed request cannot change the owner/principal.
        let mut claimed = payload.clone();
        if field != "permissions" {
            claimed["ceiling"] = wrong["ceiling"].clone();
        }
        assert!(
            SourceEnrollment::parse_shared_namespace(
                &serde_json::to_vec(&wrong).unwrap(),
                &claimed,
                &owner
            )
            .is_err(),
            "{field}"
        );
    }
    let alias = SourceEnrollment::parse_shared_namespace(&raw, &payload, &owner).unwrap();
    let mut transitive = payload.clone();
    transitive["namespace_owner_enrollment_ref"] = serde_json::to_value(alias.reference()).unwrap();
    assert!(SourceEnrollment::parse_shared_namespace(&raw, &transitive, &alias).is_err());
}
