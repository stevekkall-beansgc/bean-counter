use super::*;
fn fixture() -> Value {
    serde_json::from_slice(include_bytes!("../../../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap()
}
#[test]
fn priced_rule_coverage_requires_original_role_activity_source_and_funding() {
    // Syntax/relationship assertions only, not an ObservedSource factory or
    // evidence/customer/source authentication test.
    let publication:Value=serde_json::from_slice(include_bytes!("../../../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap();
    let vectors = publication["vectors"].as_array().unwrap();
    let source_docs: Vec<_> = vectors
        .iter()
        .take(6)
        .map(|v| Document {
            reference: serde_json::from_value(v["ref"].clone()).unwrap(),
            schema: v["body"]["schema"].as_str().unwrap().into(),
            bytes: records::bytes(&v["body"]).unwrap(),
        })
        .collect();
    let refs: Vec<_> = source_docs.iter().collect();
    let mut definitions = PublicationInputs::fixture(
        &vectors.iter().find(|v| v["name"] == "publication").unwrap()["body"],
        &refs,
    )
    .unwrap();
    let f = fixture();
    let op = &f["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "A.accept.operation")
        .unwrap()["body"];
    let payload = &op["request_evidence"]["normalized_request"]["payload"];
    let mut input = AcceptInput::parse(
        &records::bytes(payload).unwrap(),
        &["fixture".into(), "sandbox".into()],
        &definitions.context,
    )
    .unwrap();
    let docs = input
        .normalize_private_documents(&definitions.template, &definitions.product, &[])
        .unwrap();
    let binding = input.value().unwrap()["binding_inputs"].clone();
    coverage(&binding, &definitions, &docs).unwrap();
    let mut expanded_product = definitions.product.canonical_value().clone();
    let mut other = expanded_product["activities"][0].clone();
    other["name"] = json!("fixture.report:other");
    expanded_product["activities"]
        .as_array_mut()
        .unwrap()
        .push(other);
    let facts: Vec<_> = source_docs
        .iter()
        .filter(|d| d.reference.kind == "workflow-fact-schema")
        .map(|d| ledgerlab_core::policy::workflow::FactSchema::parse(&d.bytes).unwrap())
        .collect();
    definitions.product = ledgerlab_core::policy::workflow::Product::parse(
        &records::bytes(&expanded_product).unwrap(),
        &facts,
    )
    .unwrap();
    let broadened: Vec<_> = docs
        .iter()
        .map(|d| {
            let mut body = canonical::parse(&d.bytes).unwrap();
            if d.reference.kind == "workflow-slot-terms" {
                body["allowed_activities"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("fixture.report:other"));
                ledgerlab_core::policy::workflow::SlotBindingTerms::parse(
                    &records::bytes(&body).unwrap(),
                )
                .unwrap()
                .validate_product(&definitions.product)
                .unwrap();
            }
            Document {
                reference: d.reference.clone(),
                schema: d.schema.clone(),
                bytes: records::bytes(&body).unwrap(),
            }
        })
        .collect();
    assert!(coverage(&binding, &definitions, &broadened).is_err());
    for field in ["bindings", "source_slots", "funding_legs"] {
        let mut missing = binding.clone();
        missing[field] = json!([]);
        assert!(coverage(&missing, &definitions, &docs).is_err(), "{field}");
    }
    for (field, value) in [
        ("role_slot", json!("fixture.report:wrong")),
        ("allowed_activities", json!([])),
        ("correction_source_slot", json!("fixture.report:wrong")),
    ] {
        let changed: Vec<_> = docs
            .iter()
            .map(|d| {
                let mut body = canonical::parse(&d.bytes).unwrap();
                if d.reference.kind == "workflow-slot-terms" {
                    body[field] = value.clone();
                }
                Document {
                    reference: d.reference.clone(),
                    schema: d.schema.clone(),
                    bytes: records::bytes(&body).unwrap(),
                }
            })
            .collect();
        // Deliberately changed assertion data. The real entry rejects such
        // ref/body mismatches during private normalization before this check.
        assert!(
            coverage(&binding, &definitions, &changed).is_err(),
            "{field}"
        );
    }
    assert!(selected(
        &json!([{"namespace":"fixture.report:"}]),
        "fixture.report:completed"
    ));
    assert!(!selected(
        &json!([{"names":["fixture.report:other"]}]),
        "fixture.report:completed"
    ));
}
