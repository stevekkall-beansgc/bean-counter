use super::*;
use serde_json::json;
fn expected() -> Value {
    serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-structural-independent-expected.json")).unwrap()
}
fn vector(name: &str) -> Value {
    expected()["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()
        .clone()
}
fn bytes(v: &Value) -> Vec<u8> {
    CanonicalBytes::from_value(v).unwrap().into_vec()
}
fn template() -> SlotTemplate {
    let v:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
    let t = v["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "template")
        .unwrap();
    SlotTemplate::parse(t["canonical_body_utf8"].as_str().unwrap().as_bytes(), None).unwrap()
}
fn error<T: std::fmt::Debug>(r: Result<T>, code: &str) {
    assert_eq!(r.unwrap_err().code, code);
}
#[test]
fn six_independent_customer_document_bytes_and_ids_match() {
    let t = template();
    for customer in ["A", "B"] {
        let r = vector(&format!("{customer}.role"));
        let actual = RoleAssignment::parse(&bytes(&r["body"])).unwrap();
        assert_eq!(
            actual.canonical_bytes(),
            r["canonical_utf8"].as_str().unwrap().as_bytes()
        );
        assert_eq!(actual.document_id(), r["ref"]["id"]);
        assert_eq!(actual.content_digest(), r["ref"]["digest"]);
        let c = vector(&format!("{customer}.catalog"));
        let actual = SlotCatalog::parse(&bytes(&c["body"])).unwrap();
        assert_eq!(
            actual.canonical_bytes(),
            c["canonical_utf8"].as_str().unwrap().as_bytes()
        );
        assert_eq!(actual.document_id(), c["ref"]["id"]);
        assert_eq!(actual.content_digest(), c["ref"]["digest"]);
        let i = vector(&format!("{customer}.instantiation"));
        let actual = SlotInstantiation::parse(&bytes(&i["body"]), &t, &actual).unwrap();
        assert_eq!(
            actual.canonical_bytes(),
            i["canonical_utf8"].as_str().unwrap().as_bytes()
        );
        assert_eq!(actual.document_id(), i["ref"]["id"]);
        assert_eq!(actual.content_digest(), i["ref"]["digest"]);
    }
    assert_ne!(vector("A.catalog")["ref"], vector("B.catalog")["ref"]);
    assert_eq!(
        vector("A.instantiation")["body"]["template_ref"],
        vector("B.instantiation")["body"]["template_ref"]
    );
}
#[test]
fn unsupported_history_delegation_and_binding_terms_refuse() {
    let mut c = vector("A.catalog")["body"].clone();
    c["revision"] = json!("2");
    error(SlotCatalog::parse(&bytes(&c)), "UNSUPPORTED_SCHEMA_PROFILE");
    c["revision"] = json!("1");
    c["previous"] = vector("A.catalog")["ref"].clone();
    error(SlotCatalog::parse(&bytes(&c)), "UNSUPPORTED_SCHEMA_PROFILE");
    let t = template();
    let c = SlotCatalog::parse(&bytes(&vector("A.catalog")["body"])).unwrap();
    let mut i = vector("A.instantiation")["body"].clone();
    i["revision"] = json!("2");
    error(
        SlotInstantiation::parse(&bytes(&i), &t, &c),
        "UNSUPPORTED_SCHEMA_PROFILE",
    );
    let mut r = vector("A.role")["body"].clone();
    r["payer_delegation_ref"] = vector("A.role")["ref"].clone();
    error(
        RoleAssignment::parse(&bytes(&r)),
        "UNSUPPORTED_SCHEMA_PROFILE",
    );
    let mut r = vector("A.role")["body"].clone();
    r["assent_refs"] = json!([]);
    assert!(RoleAssignment::parse(&bytes(&r)).is_err());
}
#[test]
fn raw_duplicate_unknown_null_and_private_terms_refuse() {
    let original = vector("A.catalog")["canonical_utf8"]
        .as_str()
        .unwrap()
        .to_owned();
    let duplicate = original.replacen("{", "{\"customer\":\"customer-A\",", 1);
    assert!(SlotCatalog::parse(duplicate.as_bytes()).is_err());
    let mut c = vector("A.catalog")["body"].clone();
    c["components"][0]["rate"] = json!("0.37");
    assert!(SlotCatalog::parse(&bytes(&c)).is_err());
    let mut c = vector("A.catalog")["body"].clone();
    c["previous"] = Value::Null;
    assert!(SlotCatalog::parse(&bytes(&c)).is_err());
}
#[test]
fn mapping_ownership_complete_injective_and_original_root_checks_refuse() {
    let t = template();
    let cat = SlotCatalog::parse(&bytes(&vector("A.catalog")["body"])).unwrap();
    for mutation in ["owner", "ref", "missing", "unknown", "dangling"] {
        let mut v = vector("A.instantiation")["body"].clone();
        match mutation {
            "owner" => v["customer"] = json!("customer-B"),
            "ref" => v["catalog_ref"] = vector("B.catalog")["ref"].clone(),
            "missing" => v["component_map"] = json!([]),
            "unknown" => v["component_map"][0]["template_key"] = json!("fixture.report:unknown"),
            "dangling" => v["component_map"][0]["slot_id"] = json!("absent"),
            _ => unreachable!(),
        }
        assert!(
            SlotInstantiation::parse(&bytes(&v), &t, &cat).is_err(),
            "{mutation}"
        );
    }
    let mut v = vector("A.catalog")["body"].clone();
    let mut second = v["components"][0].clone();
    second["book"] = json!("supplier");
    v["components"].as_array_mut().unwrap().push(second);
    error(SlotCatalog::parse(&bytes(&v)), "SLOT_DUPLICATE_ID");
    let mut v = vector("A.catalog")["body"].clone();
    v["components"][0]["root_slot"] = json!("absent");
    error(SlotCatalog::parse(&bytes(&v)), "SLOT_MISSING_ROOT");
}

#[test]
fn exact_catalog_limits_and_cross_kind_namespaces_are_structural_only() {
    let mut body = vector("A.catalog")["body"].clone();
    body["components"] = json!((0..128).map(|n| json!({"slot_id":format!("slot-{n}"),"book":"retail","root_slot":format!("slot-{n}"),"role_slot":"fixture.report:retail"})).collect::<Vec<_>>());
    body["families"] = json!((0..64).map(|n| json!({"slot_id":format!("slot-{n}"),"target_component_slot":"slot-0","role_slot":"fixture.report:retail"})).collect::<Vec<_>>());
    // Explicit component/family fields select distinct namespaces, even when
    // their opaque slot identifiers coincide. This creates no economic claim.
    SlotCatalog::parse(&bytes(&body)).unwrap();
    let mut over = body.clone();
    over["components"].as_array_mut().unwrap().push(json!({"slot_id":"over","book":"retail","root_slot":"over","role_slot":"fixture.report:retail"}));
    error(SlotCatalog::parse(&bytes(&over)), "SLOT_LIMIT");
    body["families"].as_array_mut().unwrap().push(json!({"slot_id":"over","target_component_slot":"slot-0","role_slot":"fixture.report:retail"}));
    error(SlotCatalog::parse(&bytes(&body)), "SLOT_LIMIT");
}

#[test]
fn catalog_root_chains_and_mapping_id_reuse_refuse() {
    let mut body = vector("A.catalog")["body"].clone();
    body["components"] = json!([
        {"slot_id":"a","book":"retail","root_slot":"b","role_slot":"fixture.report:retail"},
        {"slot_id":"b","book":"retail","root_slot":"c","role_slot":"fixture.report:retail"},
        {"slot_id":"c","book":"retail","root_slot":"c","role_slot":"fixture.report:retail"}
    ]);
    error(SlotCatalog::parse(&bytes(&body)), "SLOT_SEMANTICS_CHANGED");
    let t = template();
    let cat = SlotCatalog::parse(&bytes(&vector("A.catalog")["body"])).unwrap();
    let mut instantiation = vector("A.instantiation")["body"].clone();
    instantiation["component_map"]
        .as_array_mut()
        .unwrap()
        .push(json!({"template_key":"fixture.report:second","slot_id":"A-base"}));
    error(
        SlotInstantiation::parse(&bytes(&instantiation), &t, &cat),
        "SLOT_MAPPING_DUPLICATE",
    );
}

#[test]
fn owning_role_normalizer_preserves_legacy_bytes_grammar_and_lease_lifetime() {
    use crate::{
        canonical::audit_arena::AuditArena,
        validation_work::{OriginalAuditSession, ValidationVisit},
    };
    let original = vector("A.role")["body"].clone();
    let mut c1 = original.clone();
    c1["provider"] = json!("opaque\u{0085}provider");
    let mut escaped = original.clone();
    escaped["provider"] = json!("quote\"\\😀");
    let mut control = original.clone();
    control["provider"] = json!("bad\u{007f}provider");
    let mut delegated = original.clone();
    delegated["payer_delegation_ref"] = vector("A.role")["ref"].clone();
    let mut extra = original.clone();
    extra["unknown"] = json!("no");
    for body in [original, c1, escaped, control, delegated, extra] {
        let raw = serde_json::to_vec_pretty(&body).unwrap();
        let expected = RoleAssignment::parse(&raw);
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let actual = RoleAssignment::normalize_private_with_session(&raw, &mut session);
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => {
                assert_eq!(actual.bytes(), expected.canonical_bytes());
                assert!(arena.usage().unwrap().live_bytes >= baseline + actual.bytes().len());
                drop(actual);
                assert_eq!(arena.usage().unwrap().live_bytes, baseline);
            }
            (Err(_), Err(_)) => {
                assert!(
                    session
                        .semantic_work()
                        .visit(ValidationVisit::Constraint)
                        .is_err()
                );
                assert_eq!(arena.usage().unwrap().live_bytes, baseline);
            }
            _ => panic!("role normalization changed legacy acceptance"),
        }
    }
}

#[test]
fn owning_structural_transforms_match_original_sort_and_counter_normalization() {
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    let template = template();
    for customer in ["A", "B"] {
        let mut catalog = vector(&format!("{customer}.catalog"))["body"].clone();
        catalog["revision"] = json!("0001");
        catalog["components"].as_array_mut().unwrap().reverse();
        catalog["families"].as_array_mut().unwrap().reverse();
        let raw = serde_json::to_vec_pretty(&catalog).unwrap();
        let expected = SlotCatalog::parse(&raw).unwrap();
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let normalized = SlotCatalog::normalize_private_with_session(&raw, &mut session).unwrap();
        assert_eq!(normalized.bytes(), expected.canonical_bytes());
        drop(normalized);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let mut inst = vector(&format!("{customer}.instantiation"))["body"].clone();
        inst["revision"] = json!("01");
        inst["component_map"].as_array_mut().unwrap().reverse();
        inst["family_map"].as_array_mut().unwrap().reverse();
        let raw = serde_json::to_vec_pretty(&inst).unwrap();
        let expected = SlotInstantiation::parse(&raw, &template, &expected).unwrap();
        let normalized =
            SlotInstantiation::normalize_private_with_session(&raw, &mut session).unwrap();
        assert_eq!(normalized.bytes(), expected.canonical_bytes());
        drop(normalized);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
