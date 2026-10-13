use super::*;
use ledgerlab_core::policy::workflow::FactSchema;
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
fn document(r: &Value) -> Document {
    Document {
        reference: serde_json::from_value(r["ref"].clone()).unwrap(),
        schema: r["body"]["schema"].as_str().unwrap().into(),
        bytes: records::bytes(&r["body"]).unwrap(),
    }
}
#[test]
fn exact_a_b_binding_intent_accepted_head_records_match_sealed_encoding() {
    // Mechanical source-only encoding check. It does not create an
    // ObservedPublication/ObservedSource or assert authenticated fixture actors.
    let f:Value=serde_json::from_slice(include_bytes!("../../../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap();
    let vectors = f["vectors"].as_array().unwrap();
    let docs: Vec<_> = vectors.iter().take(6).map(document).collect();
    let sources: Vec<_> = docs.iter().collect();
    let publication = document(vectors.iter().find(|v| v["name"] == "publication").unwrap());
    let plan = document(vectors.iter().find(|v| v["name"] == "unit-plan").unwrap());
    let definitions =
        PublicationInputs::fixture(&canonical::parse(&publication.bytes).unwrap(), &sources)
            .unwrap();
    let context = FactSchema::parse(
        &docs
            .iter()
            .find(|d| {
                d.reference
                    == serde_json::from_value::<RecordRef>(
                        definitions.policy["context_schema_ref"].clone(),
                    )
                    .unwrap()
            })
            .unwrap()
            .bytes,
    )
    .unwrap();
    for customer in ["A", "B"] {
        let operation = record(&format!("{customer}.accept.operation"))["body"].clone();
        let payload = &operation["request_evidence"]["normalized_request"]["payload"];
        let key: OperationKey = serde_json::from_value(operation["key"].clone()).unwrap();
        let at = Timestamp::parse(operation["accepted_at"].as_str().unwrap()).unwrap();
        let sequence = operation["commit_sequence"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let mut input =
            AcceptInput::parse(&records::bytes(payload).unwrap(), &key.scope, &context).unwrap();
        input
            .normalize_private_documents(&definitions.template, &definitions.product, &[])
            .unwrap();
        let authority = document(&record(&format!("{customer}.accept.authority")));
        let prepared = build(
            &input,
            &definitions,
            &publication,
            &plan,
            &authority,
            CommitMetadata {
                key: &key,
                actor: "fixture-admin",
                at: &at,
                sequence,
                prior: None,
            },
        )
        .unwrap();
        assert_eq!(json!(prepared.results), operation["result_refs"]);
        for (kind, name) in [
            ("workflow-binding", "binding"),
            ("workflow-agreement-intent", "accept.intent"),
            ("workflow-accepted-agreement", "accepted"),
            ("workflow-head-transition", "accept.head"),
        ] {
            let expected = record(&format!("{customer}.{name}"));
            let actual = prepared
                .documents
                .iter()
                .find(|d| d.reference.kind == kind)
                .unwrap();
            assert_eq!(json!(actual.reference), expected["ref"]);
            assert_eq!(canonical::hex(&actual.bytes), expected["canonical_hex"]);
        }
        match prepared.projection {
            Projection::Acceptance(p) => {
                assert_eq!(p.head.revision, 1);
                assert_eq!(p.expected_revision, 0);
                assert_eq!(p.customer, input.customer());
            }
            _ => panic!("acceptance projection"),
        }
        assert!(build(
            &input,
            &definitions,
            &publication,
            &plan,
            &authority,
            CommitMetadata {
                key: &key,
                actor: "fixture-admin",
                at: &at,
                sequence,
                prior: Some(authority.reference.clone())
            }
        )
        .is_err());
        assert!(build(
            &input,
            &definitions,
            &publication,
            &plan,
            &authority,
            CommitMetadata {
                key: &key,
                actor: "fixture-admin",
                at: &at,
                sequence: 0,
                prior: None
            }
        )
        .is_err());
    }
}
