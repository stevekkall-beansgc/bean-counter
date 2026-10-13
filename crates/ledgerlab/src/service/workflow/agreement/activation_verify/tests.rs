use super::*;
fn fixtures() -> (Value, Value) {
    (
        serde_json::from_slice(include_bytes!("../testdata/descriptor-independent.json")).unwrap(),
        serde_json::from_slice(include_bytes!("../testdata/semantic-independent.json")).unwrap(),
    )
}
// This assertion operand supplies only the factory's required binding copies;
// full binding/origin eligibility is tested by the integrated coordinator later.
fn operand(envelope: &Value) -> Document {
    let p = &envelope["payload"];
    Document{reference:serde_json::from_value(p["accepted_binding_ref"].clone()).unwrap(),schema:"ledger-workflow-binding/1".into(),
        bytes:records::bytes(&json!({"schema":"ledger-workflow-binding/1",
            "customer":if p["agreement_id"]=="agreement-A" {"customer-A"}else{"customer-B"},"agreement_id":p["agreement_id"],
            "policy_ref":p["policy_ref"],"product_ref":p["product_ref"],"fact_schema_refs":p["fact_schema_refs"],"compiled_plan_ref":p["compiled_plan_ref"]})).unwrap()}
}
#[test]
fn activation_factory_matches_independent_a_b_head_and_transition_bytes() {
    let (descriptors, semantic) = fixtures();
    for op in descriptors["operations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| matches!(o["name"].as_str(), Some("A.activate" | "B.activate")))
    {
        let envelope = &op["request_evidence"]["normalized_request"];
        let req = request::Request::parse_agreement(&records::bytes(envelope).unwrap()).unwrap();
        let binding = operand(envelope);
        let payload = normalize(&req, &binding).unwrap();
        assert_eq!(payload, envelope["payload"]);
        let at = Timestamp::parse(op["receipt"]["accepted_at"].as_str().unwrap()).unwrap();
        let m = TransitionMetadata {
            key: req.key(),
            actor: "fixture-admin",
            at: &at,
            sequence: op["receipt"]["commit_sequence"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            binding: &binding,
            prior: None,
            target: None,
        };
        let prepared = build_records(&req, &payload, &m).unwrap();
        assert_eq!(json!(prepared.results), op["receipt"]["result_refs"]);
        for d in prepared
            .documents
            .iter()
            .filter(|d| d.reference.kind != "workflow-agreement-intent")
        {
            let expected = semantic["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["ref"] == json!(d.reference))
                .unwrap();
            assert_eq!(
                d.bytes,
                expected["canonical_utf8"].as_str().unwrap().as_bytes()
            );
        }
        assert!(matches!(prepared.projection, Projection::Activation(_)));
    }
}
#[test]
fn activation_normalization_preserves_schedule_intent_and_refuses_discarded_fields_or_wrong_copies()
{
    let (descriptors, _) = fixtures();
    let mut e = descriptors["operations"][2]["request_evidence"]["normalized_request"].clone();
    let binding = operand(&e);
    e["payload"]["expected_revision"] = json!("000");
    e["payload"]["effective"] = json!({"kind":"not_before","at":"2026-10-13T00:00:20Z"});
    let req = request::Request::parse_agreement(&records::bytes(&e).unwrap()).unwrap();
    let p = normalize(&req, &binding).unwrap();
    assert_eq!(p["expected_revision"], "0");
    assert_eq!(p["effective"]["at"], "2026-10-13T00:00:20.000000Z");
    let at = Timestamp::parse("2026-10-13T00:00:18Z").unwrap();
    let m = TransitionMetadata {
        key: req.key(),
        actor: "fixture-admin",
        at: &at,
        sequence: 18,
        binding: &binding,
        prior: None,
        target: None,
    };
    let result = build_records(&req, &p, &m).unwrap();
    assert_eq!(
        canonical::parse_bounded(&result.documents[0].bytes, 262144).unwrap()["payload"]
            ["effective"],
        p["effective"]
    );
    for mode in 0..4 {
        let mut bad = e.clone();
        match mode {
            0 => bad["payload"]["effective"]["ignored"] = json!(true),
            1 => bad["payload"]["product_ref"] = bad["payload"]["policy_ref"].clone(),
            2 => {
                bad["payload"]["fact_schema_refs"] = json!([
                    bad["payload"]["fact_schema_refs"][0],
                    bad["payload"]["fact_schema_refs"][0]
                ])
            }
            _ => bad["payload"]["expected_revision"] = json!("9223372036854775808"),
        }
        let bad = request::Request::parse_agreement(&records::bytes(&bad).unwrap()).unwrap();
        assert!(normalize(&bad, &binding).is_err());
    }
    e["payload"]["effective"]["at"] = json!("2026-10-13T00:00:17Z");
    let req = request::Request::parse_agreement(&records::bytes(&e).unwrap()).unwrap();
    let p = normalize(&req, &binding).unwrap();
    let m = TransitionMetadata {
        key: req.key(),
        actor: "fixture-admin",
        at: &at,
        sequence: 18,
        binding: &binding,
        prior: None,
        target: None,
    };
    assert!(build_records(&req, &p, &m).is_err());
}
#[test]
fn revoke_keeps_binding_and_requires_exact_target_and_prior_activation_head() {
    let (descriptors, semantic) = fixtures();
    let e = &descriptors["operations"][4]["request_evidence"]["normalized_request"];
    let req = request::Request::parse_agreement(&records::bytes(e).unwrap()).unwrap();
    let binding = operand(&descriptors["operations"][2]["request_evidence"]["normalized_request"]);
    let p = normalize(&req, &binding).unwrap();
    let prior: RecordRef = serde_json::from_value(
        semantic["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "A.activation-head-v1")
            .unwrap()["ref"]
            .clone(),
    )
    .unwrap();
    let target = checked_ref(&p["transition_ref"], "workflow-activation").unwrap();
    let at = Timestamp::parse(
        descriptors["operations"][4]["receipt"]["accepted_at"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let m = TransitionMetadata {
        key: req.key(),
        actor: "fixture-admin",
        at: &at,
        sequence: 20,
        binding: &binding,
        prior: Some(prior.clone()),
        target: Some(target.clone()),
    };
    let r = build_records(&req, &p, &m).unwrap();
    let a = canonical::parse_bounded(&r.documents[2].bytes, 262144).unwrap();
    assert_eq!(a["action"], "revoke");
    assert_eq!(a["effective_at"], json!(at));
    assert_eq!(a["binding_ref"], json!(binding.reference));
    for m in [
        TransitionMetadata {
            prior: None,
            target: Some(target),
            ..m
        },
        TransitionMetadata {
            prior: Some(prior),
            target: None,
            ..m
        },
    ] {
        assert!(build_records(&req, &p, &m).is_err());
    }
}

#[test]
fn complete_sealed_binding_without_scope_normalizes_exact_activation_copies() {
    let f: Value = serde_json::from_slice(include_bytes!(
        "../testdata/r1-binding-scope-independent.json"
    ))
    .unwrap();
    for op in f["operations"].as_array().unwrap() {
        let label = op["name"].as_str().unwrap().split('.').next().unwrap();
        let vector = f["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == format!("{label}.binding"))
            .unwrap();
        assert!(vector["body"].get("scope").is_none());
        let binding = Document {
            reference: serde_json::from_value(vector["ref"].clone()).unwrap(),
            schema: "ledger-workflow-binding/1".into(),
            bytes: vector["canonical_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec(),
        };
        assert_eq!(canonical_body(&binding).unwrap(), vector["body"]);
        let e = &op["request_evidence"]["normalized_request"];
        let req = request::Request::parse_agreement(&records::bytes(e).unwrap()).unwrap();
        assert_eq!(normalize(&req, &binding).unwrap(), e["payload"]);
        for field in [
            "policy_ref",
            "product_ref",
            "compiled_plan_ref",
            "accepted_binding_ref",
        ] {
            let mut bad = e.clone();
            bad["payload"][field] = json!(RecordRef {
                kind: bad["payload"][field]["kind"].as_str().unwrap().into(),
                id: format!(
                    "{}{}",
                    if field == "compiled_plan_ref" {
                        "wplan_"
                    } else {
                        "wdoc_"
                    },
                    "f".repeat(64)
                ),
                digest: format!("sha256:{}", "f".repeat(64))
            });
            let bad = request::Request::parse_agreement(&records::bytes(&bad).unwrap()).unwrap();
            assert_eq!(normalize(&bad, &binding), Err(reject("INVALID_REFERENCE")));
        }
    }
}
