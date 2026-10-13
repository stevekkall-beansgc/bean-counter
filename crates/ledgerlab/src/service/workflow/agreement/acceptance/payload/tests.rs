use super::*;
fn fixture() -> Value {
    serde_json::from_slice(include_bytes!("../../../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap()
}
fn record(name: &str) -> Value {
    fixture()["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap()
        .clone()
}
fn payload(customer: &str) -> Value {
    record(&format!("{customer}.accept.operation"))["body"]["request_evidence"]
        ["normalized_request"]["payload"]
        .clone()
}
fn source(name: &str) -> Value {
    let f:Value=serde_json::from_slice(include_bytes!("../../../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap();
    f["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()["body"]
        .clone()
}
fn context() -> FactSchema {
    FactSchema::parse(&records::bytes(&source("context")).unwrap()).unwrap()
}
fn product() -> Product {
    let facts = FactSchema::parse(&records::bytes(&source("facts")).unwrap()).unwrap();
    Product::parse(&records::bytes(&source("product")).unwrap(), &[facts]).unwrap()
}
fn template() -> SlotTemplate {
    SlotTemplate::parse(&records::bytes(&source("template")).unwrap(), None).unwrap()
}
fn prepare(body: &Value) -> AcceptInput {
    AcceptInput::parse(
        &records::bytes(body).unwrap(),
        &["fixture".into(), "sandbox".into()],
        &context(),
    )
    .unwrap()
}
#[test]
fn both_complete_private_document_sets_match_reviewed_normalized_requests_and_terms() {
    for customer in ["A", "B"] {
        let original = payload(customer);
        let mut p = prepare(&original);
        let docs = p
            .normalize_private_documents(&template(), &product(), &[])
            .unwrap();
        assert_eq!(docs.len(), 6);
        assert_eq!(p.private_refs().unwrap().len(), 6);
        assert_eq!(p.value().unwrap(), original);
        assert_eq!(p.expected_revision(), "0");
        assert_eq!(
            p.agreement_id(),
            if customer == "A" {
                "agreement-A"
            } else {
                "agreement-B"
            }
        );
        assert_eq!(
            canonical::hex(p.terms().canonical_bytes()),
            record(&format!("{customer}.terms"))["canonical_hex"]
        );
        assert_eq!(
            json!(p.terms().reference()),
            record(&format!("{customer}.terms"))["ref"]
        );
    }
}
#[test]
fn transport_presence_changes_request_but_not_fully_resolved_semantic_intent() {
    let body = payload("A");
    let mut inline = prepare(&body);
    let retained = inline
        .normalize_private_documents(&template(), &product(), &[])
        .unwrap();
    let mut without = body.clone();
    without["documents"] = json!([]);
    let mut loaded = prepare(&without);
    loaded
        .normalize_private_documents(&template(), &product(), &retained)
        .unwrap();
    assert_ne!(inline.value().unwrap(), loaded.value().unwrap());
    assert_eq!(
        inline.semantic_intent().unwrap(),
        loaded.semantic_intent().unwrap()
    );
    let mut changed = body;
    changed["expected_revision"] = json!("1");
    changed["binding_inputs"]["version"] = json!("2");
    assert_ne!(
        inline.semantic_intent().unwrap(),
        prepare(&changed).semantic_intent().unwrap()
    );
}
#[test]
fn coherent_terms_assent_version_must_equal_new_acceptance_revision() {
    for (version, expected, valid) in [
        ("7", "0", false),
        ("7", "1", false),
        ("1", "1", false),
        ("2", "1", true),
    ] {
        let mut body = payload("A");
        body["expected_revision"] = json!(expected);
        body["binding_inputs"]["version"] = json!(version);
        let docs = body["documents"].as_array_mut().unwrap();
        let term = docs
            .iter_mut()
            .find(|d| d["schema"] == "ledger-workflow-acceptance-terms/1")
            .unwrap();
        term["version"] = json!(version);
        term["binding_inputs"]["version"] = json!(version);
        let terms = AcceptanceTerms::parse(&records::bytes(term).unwrap(), &context()).unwrap();
        let assent = docs
            .iter_mut()
            .find(|d| d["schema"] == "ledger-workflow-assent/1")
            .unwrap();
        assent["terms_digest"] = json!(terms.terms_digest());
        let assent = AssentRecord::parse(&records::bytes(assent).unwrap(), &terms).unwrap();
        let reference =
            records::source_document("workflow-assent", assent.canonical_value().clone())
                .unwrap()
                .reference;
        body["assent_refs"] = json!([reference]);
        body["binding_inputs"]["assent_refs"] = json!([reference]);
        // Terms and assent assertions are internally consistent; the altered
        // fixture evidence is not being asserted authenticated by this test.
        let parsed = AcceptInput::parse(
            &records::bytes(&body).unwrap(),
            &["fixture".into(), "sandbox".into()],
            &context(),
        );
        if valid {
            parsed
                .unwrap()
                .normalize_private_documents(&template(), &product(), &[])
                .unwrap();
        } else {
            assert!(parsed.is_err(), "version{version}/expected{expected}");
        }
    }
}
#[test]
fn tentative_documents_cannot_substitute_customer_grants_or_accepted_state() {
    let base = payload("A");
    for extra in [
        record("A.verified_grant")["body"].clone(),
        record("A.binding")["body"].clone(),
        record("A.accepted")["body"].clone(),
    ] {
        let mut body = base.clone();
        body["documents"].as_array_mut().unwrap().push(extra);
        assert!(prepare(&body)
            .normalize_private_documents(&template(), &product(), &[])
            .is_err());
    }
    let mut body = base.clone();
    body["documents"] = payload("B")["documents"].clone();
    assert!(prepare(&body)
        .normalize_private_documents(&template(), &product(), &[])
        .is_err());
    let mut body = base.clone();
    body["documents"].as_array_mut().unwrap().pop();
    assert!(prepare(&body)
        .normalize_private_documents(&template(), &product(), &[])
        .is_err());
    let mut body = base;
    let duplicate = body["documents"][1].clone();
    body["documents"].as_array_mut().unwrap().push(duplicate);
    assert!(prepare(&body)
        .normalize_private_documents(&template(), &product(), &[])
        .is_err());
}
#[test]
fn raw_lexical_and_exact_redundant_copy_constraints_survive_outer_normalization() {
    let base = payload("A");
    for field in [
        "assent_refs",
        "authority_refs",
        "slot_catalog_ref",
        "slot_instantiation_ref",
    ] {
        let mut body = base.clone();
        body[field] = payload("B")[field].clone();
        assert!(AcceptInput::parse(
            &records::bytes(&body).unwrap(),
            &["fixture".into(), "sandbox".into()],
            &context()
        )
        .is_err());
    }
    let mut body = base.clone();
    body["binding_inputs"]["accepted_by"] = json!("caller");
    assert!(AcceptInput::parse(
        &records::bytes(&body).unwrap(),
        &["fixture".into(), "sandbox".into()],
        &context()
    )
    .is_err());
    let raw = String::from_utf8(records::bytes(&base).unwrap())
        .unwrap()
        .replacen("{", "{\"agr\\u0065ement_id\":\"duplicate\",", 1);
    assert!(AcceptInput::parse(
        raw.as_bytes(),
        &["fixture".into(), "sandbox".into()],
        &context()
    )
    .is_err());
    body = base;
    body["documents"][0]["operator_attestation_ref"] = Value::Null;
    assert!(AcceptInput::parse(
        &records::bytes(&body).unwrap(),
        &["fixture".into(), "sandbox".into()],
        &context()
    )
    .is_err());
}

#[test]
fn normalization_rejects_equivalent_duplicate_documents_and_preserves_lexical_forms() {
    let original = payload("A");
    let mut equivalent = original.clone();
    equivalent["expected_revision"] = json!("0000");
    for doc in equivalent["documents"].as_array_mut().unwrap() {
        if doc["schema"] == "ledger-workflow-slot-binding-terms/1" {
            doc["quantity_ceilings"][0]["maximum"] = json!("100.00");
        }
        if doc["schema"] == "ledger-workflow-acceptance-terms/1" {
            doc["binding_inputs"]["quantity_ceilings"][0]["maximum"] = json!("100.00");
        }
    }
    let mut normalized = prepare(&equivalent);
    normalized
        .normalize_private_documents(&template(), &product(), &[])
        .unwrap();
    assert_eq!(normalized.value().unwrap(), original);
    let duplicate = original["documents"][1].clone();
    equivalent["documents"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(prepare(&equivalent)
        .normalize_private_documents(&template(), &product(), &[])
        .is_err());
    let mut raw = records::bytes(&original).unwrap();
    raw.resize(262144, b' ');
    AcceptInput::parse(&raw, &["fixture".into(), "sandbox".into()], &context()).unwrap();
    raw.push(b' ');
    assert!(AcceptInput::parse(&raw, &["fixture".into(), "sandbox".into()], &context()).is_err());
}
