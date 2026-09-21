use ledgerlab_core::canonical::{self, CanonicalBytes, Domain};
use ledgerlab_core::domain::{
    self, AcceptanceContext, Document, ResolvedInput, Revision, Scope, Timestamp,
};
use ledgerlab_core::policy;
use serde_json::{json, Value};

const INPUT: &[u8] = include_bytes!("../../../fixtures/canonical/valid/first-slice-input.json");
const NORMAL: &[u8] = include_bytes!("../../../fixtures/canonical/expected/first-slice.json");
const SEEDS: &str = include_str!("../../../fixtures/journals/first-slice/seed-documents.jsonl");
const JOURNAL: &[u8] =
    include_bytes!("../../../fixtures/journals/first-slice/accepted-records.jsonl");
fn scope() -> Scope {
    Scope::new("demo", "sandbox").unwrap()
}
fn context() -> AcceptanceContext {
    AcceptanceContext {
        principal_id: "demo-app".into(),
        grant_revision: Revision::new(1).unwrap(),
        grant_active: true,
        binding_active: true,
        chain_revision: Revision::new(0).unwrap(),
        chain_event_count: Revision::new(0).unwrap(),
        received_at: Timestamp::parse("2026-09-20T14:00:00.000000Z").unwrap(),
    }
}
fn seed_values() -> Vec<Value> {
    SEEDS
        .lines()
        .map(|l| canonical::parse(l.as_bytes()).unwrap())
        .collect()
}
fn documents() -> Vec<Document> {
    seed_values()
        .into_iter()
        .map(|v| Document::parse(&serde_json::to_vec(&v["body"]).unwrap()).unwrap())
        .collect()
}
fn input(bytes: &[u8]) -> ResolvedInput {
    ResolvedInput::new(
        domain::normalize(bytes, scope(), "urn:demo:app")
            .unwrap()
            .resolve(None)
            .unwrap(),
        documents(),
        context(),
    )
    .unwrap()
}

#[test]
fn all_sixty_frozen_canonical_hash_and_identity_vectors() {
    let v = canonical::parse_bounded(
        include_bytes!("../../../fixtures/journals/first-slice/vectors.json"),
        4 * 1024 * 1024,
    )
    .unwrap();
    let vectors = v["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 60);
    for v in vectors {
        let name = v["name"].as_str().unwrap();
        let domain = Domain::parse(v["domain"].as_str().unwrap()).unwrap();
        let bytes = CanonicalBytes::from_value(&v["value"]).unwrap();
        assert_eq!(
            bytes.as_slice(),
            v["canonical_utf8"].as_str().unwrap().as_bytes(),
            "{name}"
        );
        assert_eq!(
            canonical::hex(bytes.as_slice()),
            v["canonical_hex"],
            "{name}"
        );
        assert_eq!(
            canonical::hex(&canonical::hash_input(domain, &v["value"]).unwrap()),
            v["hash_input_hex"],
            "{name}"
        );
        assert_eq!(
            canonical::hash(domain, &v["value"]).unwrap(),
            v["sha256"],
            "{name}"
        );
        if let Some(id) = v.get("id") {
            assert_eq!(
                canonical::identity(domain, &v["value"]).unwrap(),
                id.as_str().unwrap(),
                "{name}"
            );
        }
    }
}
#[test]
fn entire_generated_journal_equals_frozen_bytes() {
    let input = input(INPUT);
    let plan = policy::evaluate(&input).unwrap();
    assert_eq!(input.event().bytes().as_slice(), NORMAL);
    assert_eq!(plan.records().len(), 25);
    let expected: Vec<Value> = std::str::from_utf8(JOURNAL)
        .unwrap()
        .lines()
        .map(|l| canonical::parse(l.as_bytes()).unwrap())
        .collect();
    for (record, expected) in plan.records().iter().zip(expected) {
        assert_eq!(
            serde_json::to_value(record).unwrap(),
            expected,
            "record {} {}",
            record.kind(),
            record.id()
        );
    }
    assert_eq!(plan.journal_bytes().unwrap(), JOURNAL);
    assert_eq!(
        plan.actions()
            .iter()
            .map(|a| a.amount().atoms())
            .collect::<Vec<_>>(),
        [100, -20]
    );
    assert_eq!(plan.intentions()[0].amount().atoms(), 80);
    assert_eq!(
        plan.records()
            .iter()
            .find(|r| r.kind() == "decision-manifest")
            .unwrap()
            .body()["members"]
            .as_array()
            .unwrap()
            .len(),
        29
    );
    assert_eq!(
        plan.receipt().bytes().unwrap().as_slice(),
        include_bytes!("../../../fixtures/journals/first-slice/receipt.json")
    );
    assert_eq!(
        CanonicalBytes::from_value(plan.claim_facts())
            .unwrap()
            .as_slice(),
        include_bytes!("../../../fixtures/journals/first-slice/claim-facts.json")
    );
    for (actual, expected) in plan.effect_facts().iter().zip([
        include_bytes!("../../../fixtures/journals/first-slice/effect-facts-1.json").as_slice(),
        include_bytes!("../../../fixtures/journals/first-slice/effect-facts-2.json").as_slice(),
    ]) {
        assert_eq!(
            CanonicalBytes::from_value(actual).unwrap().as_slice(),
            expected
        );
    }
    for (actual, expected) in plan.explanations().iter().zip([
        include_bytes!("../../../fixtures/journals/first-slice/explanation-0.json").as_slice(),
        include_bytes!("../../../fixtures/journals/first-slice/explanation-1.json").as_slice(),
    ]) {
        assert_eq!(
            CanonicalBytes::from_value(actual).unwrap().as_slice(),
            expected
        );
    }
    domain::verify(&input, JOURNAL).unwrap();
}
#[test]
fn input_documents_reproduce_frozen_envelopes() {
    for seed in seed_values() {
        let doc = Document::parse(&serde_json::to_vec(&seed["body"]).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(doc.envelope(&scope())).unwrap(), seed);
    }
}
#[test]
fn equivalent_decimal_request_is_identical_at_every_boundary() {
    let a = input(INPUT);
    let b = input(include_bytes!(
        "../../../fixtures/canonical/valid/decimal-equivalent.json"
    ));
    assert_eq!(
        a.event().candidate().ingress_bytes(),
        b.event().candidate().ingress_bytes()
    );
    assert_eq!(
        a.event().candidate().ingress_hash(),
        b.event().candidate().ingress_hash()
    );
    assert_eq!(
        policy::evaluate(&a).unwrap().journal_bytes().unwrap(),
        policy::evaluate(&b).unwrap().journal_bytes().unwrap()
    );
}
#[test]
fn identity_and_semantic_duplicate_projections_are_distinct() {
    let first = input(INPUT);
    let mut other = canonical::parse(INPUT).unwrap();
    other["id"] = json!("renamed-delivery");
    let second = input(&serde_json::to_vec(&other).unwrap());
    assert_ne!(first.event().id(), second.event().id());
    assert_eq!(
        first.event().completion_claim_id().unwrap(),
        second.event().completion_claim_id().unwrap()
    );
    assert_eq!(
        first.event().completion_facts(&[]).unwrap(),
        second.event().completion_facts(&[]).unwrap()
    );
    other["quantity"] = json!("2");
    let changed = domain::normalize(
        &serde_json::to_vec(&other).unwrap(),
        scope(),
        "urn:demo:app",
    )
    .unwrap()
    .resolve(None)
    .unwrap();
    assert_ne!(
        first.event().completion_facts(&[]).unwrap(),
        changed.completion_facts(&[]).unwrap()
    );
    other["quantity"] = json!("1");
    other["extensions"] = json!({"note":"diagnostic"});
    let extension = domain::normalize(
        &serde_json::to_vec(&other).unwrap(),
        scope(),
        "urn:demo:app",
    )
    .unwrap()
    .resolve(None)
    .unwrap();
    assert_eq!(
        first.event().completion_facts(&[]).unwrap(),
        extension.completion_facts(&[]).unwrap()
    );
    assert_ne!(first.event().content_hash(), extension.content_hash());
}
#[test]
fn frozen_invalid_lexical_json_is_rejected() {
    for bytes in [
        include_bytes!("../../../fixtures/canonical/invalid/bom.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/duplicate-key.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/exponent-token.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/fraction-token.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/invalid-utf8.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/lone-surrogate.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/negative-zero.json").as_slice(),
        include_bytes!("../../../fixtures/canonical/invalid/unsafe-integer.json").as_slice(),
    ] {
        assert!(canonical::parse(bytes).is_err(), "{bytes:?}");
    }
    for raw in [
        r#"{"a":1,"\u0061":2}"#,
        r#"[01]"#,
        r#"[+1]"#,
        r#"[1,]"#,
        r#"{"a":1,}"#,
        r#""\udc00""#,
    ] {
        assert!(canonical::parse(raw.as_bytes()).is_err(), "{raw}");
    }
}
#[test]
fn unicode_order_escape_equivalence_and_no_normalization() {
    let input = canonical::parse(include_bytes!(
        "../../../fixtures/canonical/valid/unicode-order.json"
    ))
    .unwrap();
    assert_eq!(
        CanonicalBytes::from_value(&input).unwrap().as_slice(),
        include_bytes!("../../../fixtures/canonical/expected/unicode-order.json")
    );
    let unicode =
        canonical::parse(r#"{"\ue000":1,"\ud83d\ude00":2,"é":3,"e\u0301":4}"#.as_bytes()).unwrap();
    assert_eq!(
        std::str::from_utf8(CanonicalBytes::from_value(&unicode).unwrap().as_slice()).unwrap(),
        "{\"é\":4,\"é\":3,\"😀\":2,\"\u{e000}\":1}"
    );
    assert_eq!(canonical::parse(br#""\ud83d\ude00""#).unwrap(), json!("😀"));
    assert_ne!(
        canonical::digest(Domain::EventContent, &json!("é")).unwrap(),
        canonical::digest(Domain::EventContent, &json!("é")).unwrap()
    );
}
#[test]
fn strict_event_shape_bounds_and_variant_validation() {
    let original = canonical::parse(INPUT).unwrap();
    let cases = canonical::parse(include_bytes!("../../../fixtures/canonical/cases.json")).unwrap();
    for case in cases["schema_invalid"].as_array().unwrap() {
        assert!(domain::normalize(
            &serde_json::to_vec(&case["event"]).unwrap(),
            scope(),
            "urn:demo:app"
        )
        .is_err());
    }
    for (field, value) in [
        ("quantity", json!("0")),
        ("quantity", json!("-0")),
        ("quantity", json!("1e2")),
        ("quantity", Value::Null),
        ("quantity", json!(1)),
        ("id", json!("é".repeat(65))),
        ("id", json!("bad\n")),
        ("source", json!("not-a-uri")),
        ("unit", json!("CALL")),
        ("corrects", json!("external-id")),
        ("claim_id", json!("wrong-variant")),
        ("extensions", json!({"x":[]})),
        ("extensions", json!({"x":null})),
        (
            "links",
            json!([{"relation":"generated_from","from":{"source":"urn:demo:app","id":"generation-1"}}]),
        ),
        (
            "links",
            json!([{"relation":"generated_from","from":{"source":"urn:demo:app","id":"earlier","price":"2"}}]),
        ),
    ] {
        let mut bad = original.clone();
        bad[field] = value;
        assert!(
            domain::normalize(&serde_json::to_vec(&bad).unwrap(), scope(), "urn:demo:app").is_err(),
            "{bad}"
        );
    }
}
#[test]
fn set_normalization_rejects_semantic_duplicate_and_sorts() {
    let mut a = canonical::parse(INPUT).unwrap();
    let link = json!({"relation":"generated_from","from":{"source":"urn:demo:app","id":"earlier"}});
    let mut explicit = link.clone();
    explicit["schema"] = json!("ledger-link/1");
    a["links"] = json!([link, explicit]);
    assert!(domain::normalize(&serde_json::to_vec(&a).unwrap(), scope(), "urn:demo:app").is_err());
    let ids = [
        format!("doc_{}", "a".repeat(64)),
        format!("doc_{}", "b".repeat(64)),
    ];
    a["links"] = json!([]);
    a["evidence"] = json!([ids[1], ids[0]]);
    let b = domain::normalize(&serde_json::to_vec(&a).unwrap(), scope(), "urn:demo:app").unwrap();
    a["evidence"] = json!([ids[0], ids[1]]);
    assert_eq!(
        b.ingress_bytes(),
        domain::normalize(&serde_json::to_vec(&a).unwrap(), scope(), "urn:demo:app")
            .unwrap()
            .ingress_bytes()
    );
    a["evidence"] = json!([ids[0], ids[0]]);
    assert!(domain::normalize(&serde_json::to_vec(&a).unwrap(), scope(), "urn:demo:app").is_err());
}
#[test]
fn omitted_source_and_chain_are_retained_in_original_ingress() {
    let mut raw = canonical::parse(INPUT).unwrap();
    raw.as_object_mut().unwrap().remove("source");
    raw.as_object_mut().unwrap().remove("chain");
    let candidate =
        domain::normalize(&serde_json::to_vec(&raw).unwrap(), scope(), "urn:demo:app").unwrap();
    let ingress = canonical::parse(candidate.ingress_bytes().as_slice()).unwrap();
    assert!(ingress.get("source").is_none());
    assert!(ingress.get("chain").is_none());
    let resolved = candidate.resolve(None).unwrap();
    assert!(resolved.chain().starts_with("auto-"));
    assert_eq!(resolved.chain().len(), 69);
    assert_eq!(resolved.source(), "urn:demo:app");
    let again = domain::normalize(&serde_json::to_vec(&raw).unwrap(), scope(), "urn:demo:app")
        .unwrap()
        .resolve(None)
        .unwrap();
    assert_eq!(resolved.bytes(), again.bytes());
    assert_ne!(resolved.candidate().ingress_hash(), resolved.content_hash());
}
#[test]
fn timestamp_normalizes_offsets_and_checks_real_calendar() {
    let a = Timestamp::parse("2026-09-20T10:00:00-04:00").unwrap();
    assert_eq!(a.as_str(), "2026-09-20T14:00:00.000000Z");
    assert_eq!(
        Timestamp::parse("1970-01-01T00:00:00Z").unwrap().micros(),
        0
    );
    assert_eq!(
        Timestamp::parse("1969-12-31T23:59:59.999999Z")
            .unwrap()
            .micros(),
        -1
    );
    for s in [
        "2026-02-29T00:00:00Z",
        "1900-02-29T00:00:00Z",
        "2026-01-01T00:00:60Z",
        "2026-01-01T00:00:00.1234567Z",
        "0001-01-01T00:00:00+00:01",
        "9999-12-31T23:59:59-00:01",
        "2026-01-01T00:00:00",
    ] {
        assert!(Timestamp::parse(s).is_err(), "{s}");
    }
    assert!(Timestamp::parse("2000-02-29T00:00:00Z").is_ok());
}
#[test]
fn failed_work_has_one_explanation_no_economic_outputs() {
    let mut raw = canonical::parse(INPUT).unwrap();
    raw["status"] = json!("failed");
    raw["quantity"] = json!("0");
    raw["id"] = json!("failed-1");
    raw["operation_id"] = json!("failed-1");
    let plan = policy::evaluate(&input(&serde_json::to_vec(&raw).unwrap())).unwrap();
    assert!(plan.actions().is_empty());
    assert!(plan.intentions().is_empty());
    assert_eq!(plan.explanations().len(), 1);
    assert_eq!(plan.explanations()[0].code(), "FAILED_WORK");
    assert!(!plan.receipt().id().is_empty());
}
#[test]
fn resolved_authority_and_binding_must_agree() {
    let event = domain::normalize(INPUT, scope(), "urn:demo:app")
        .unwrap()
        .resolve(None)
        .unwrap();
    for change in 0..5 {
        let mut c = context();
        match change {
            0 => c.grant_active = false,
            1 => c.binding_active = false,
            2 => c.principal_id = "someone-else".into(),
            3 => c.received_at = Timestamp::parse("2026-09-19T14:00:00Z").unwrap(),
            _ => c.chain_revision = Revision::new(i64::MAX as u64).unwrap(),
        };
        assert!(ResolvedInput::new(event.clone(), documents(), c).is_err());
    }
    let mut docs = documents();
    docs[0] = docs[1].clone();
    assert!(ResolvedInput::new(event, docs, context()).is_err());
}
#[test]
fn replay_rejects_modified_rehashed_and_incomplete_journal() {
    let resolved = input(INPUT);
    let mut records: Vec<Value> = std::str::from_utf8(JOURNAL)
        .unwrap()
        .lines()
        .map(|l| canonical::parse(l.as_bytes()).unwrap())
        .collect();
    let record = records.iter_mut().find(|r| r["kind"] == "action").unwrap();
    record["body"]["amount"]["atoms"] = json!("101");
    record["content_hash"] = json!(canonical::digest(
        Domain::RecordContent,
        &json!(["action", 1, record["body"]])
    )
    .unwrap());
    let mut altered = Vec::new();
    for r in records {
        altered.extend(CanonicalBytes::from_value(&r).unwrap().as_slice());
        altered.push(b'\n');
    }
    assert!(domain::verify(&resolved, &altered).is_err());
    assert!(domain::verify(&resolved, &JOURNAL[..JOURNAL.len() - 1]).is_err());
}
#[test]
fn strict_nesting_and_byte_limits() {
    let okay = format!("{}0{}", "[".repeat(32), "]".repeat(32));
    assert!(canonical::parse(okay.as_bytes()).is_ok());
    let bad = format!("[{okay}]");
    assert!(canonical::parse(bad.as_bytes()).is_err());
    assert!(canonical::parse(&vec![b' '; 262145]).is_err());
    for token in ["9007199254740991", "-9007199254740991"] {
        assert!(canonical::parse(token.as_bytes()).is_ok());
    }
}

fn with_policy(mut change: impl FnMut(&mut Value)) -> ResolvedInput {
    let mut values = seed_values();
    let policy = values
        .iter_mut()
        .find(|d| d["document_type"] == "policy")
        .unwrap();
    change(&mut policy["body"]);
    let policy_id = Document::parse(&serde_json::to_vec(&policy["body"]).unwrap())
        .unwrap()
        .id()
        .to_string();
    let binding = values
        .iter_mut()
        .find(|d| d["document_type"] == "binding")
        .unwrap();
    binding["body"]["policy"] = json!(policy_id);
    let docs = values
        .iter()
        .map(|v| Document::parse(&serde_json::to_vec(&v["body"]).unwrap()).unwrap())
        .collect();
    ResolvedInput::new(
        domain::normalize(INPUT, scope(), "urn:demo:app")
            .unwrap()
            .resolve(None)
            .unwrap(),
        docs,
        context(),
    )
    .unwrap()
}
#[test]
fn evaluator_uses_typed_inputs_not_fixture_constants() {
    let changed = with_policy(|p| {
        p["rules"][0]["amount"]["fixed"] = json!("2.00");
    });
    let plan = policy::evaluate(&changed).unwrap();
    assert_eq!(
        plan.actions()
            .iter()
            .map(|a| a.amount().atoms())
            .collect::<Vec<_>>(),
        [200, -40]
    );
    assert_eq!(plan.intentions()[0].amount().atoms(), 160);
    let original = policy::evaluate(&input(INPUT)).unwrap();
    assert_eq!(plan.actions()[0].id(), original.actions()[0].id());
    assert_ne!(plan.effect_facts(), original.effect_facts());
    assert_ne!(
        plan.receipt().decision_hash(),
        original.receipt().decision_hash()
    );
}
#[test]
fn rule_rename_changes_provenance_but_not_stable_effect_facts() {
    let changed = with_policy(|p| {
        p["rules"][0]["id"] = json!("renamed-base");
    });
    let plan = policy::evaluate(&changed).unwrap();
    let original = policy::evaluate(&input(INPUT)).unwrap();
    assert_eq!(plan.effect_facts(), original.effect_facts());
    assert_eq!(plan.actions()[0].id(), original.actions()[0].id());
    assert_ne!(
        plan.receipt().decision_hash(),
        original.receipt().decision_hash()
    );
}
#[test]
fn policy_rejects_unknown_executable_shapes_and_bad_bases() {
    let source = seed_values()
        .into_iter()
        .find(|d| d["document_type"] == "policy")
        .unwrap()["body"]
        .clone();
    for case in 0..9 {
        let mut p = source.clone();
        match case {
            0 => p["rules"][0]["amount"]["extra"] = json!("1"),
            1 => p["rules"][1]["amount"]["basis"] = json!("self.missing.base"),
            2 => p["rules"][1]["amount"]["percent"] = json!("100.01"),
            3 => p["rules"][0]["amount"]["fixed"] = json!("0.001"),
            4 => p["rules"][0]["component"] = p["rules"][1]["component"].clone(),
            5 => p["rules"][1]["when"][0] = json!({"field":"binding.priority","eq":"TRUE"}),
            6 => p["rules"][0]["op"] = json!("cap"),
            7 => p["rules"][1]["when"][0] = json!({"field":"status","eq":"failed"}),
            _ => p["rules"][0]["amount"]["fixed"] = Value::Null,
        }
        assert!(
            Document::parse(&serde_json::to_vec(&p).unwrap()).is_err(),
            "{p}"
        );
    }
}
#[test]
fn discount_failure_rejects_whole_evaluation() {
    let invalid = with_policy(|p| {
        p["rules"][1]["amount"]["percent"] = json!("60");
        let mut second = p["rules"][1].clone();
        second["id"] = json!("second-discount");
        second["component"] = json!("generation.second-discount");
        p["rules"].as_array_mut().unwrap().push(second);
    });
    assert_eq!(
        policy::evaluate(&invalid).unwrap_err().code,
        "DISCOUNT_EXCEEDS_BASIS"
    );
    let overflow = with_policy(|p| {
        p["rules"][0]["amount"]["fixed"] = json!("9999999999999999999999999999.99");
        let mut second = p["rules"][0].clone();
        second["id"] = json!("second-base");
        second["component"] = json!("second.base");
        second["amount"]["fixed"] = json!("0.01");
        p["rules"].as_array_mut().unwrap().insert(1, second);
        p["rules"][2]["amount"]["percent"] = json!("0");
    });
    assert_eq!(
        policy::evaluate(&overflow).unwrap_err().code,
        "ARITHMETIC_OVERFLOW"
    );
}
#[test]
fn zero_and_skipped_rules_emit_explanations_without_zero_actions() {
    let skipped = with_policy(|p| {
        p["rules"][1]["when"][0]["eq"] = json!("standard");
    });
    let plan = policy::evaluate(&skipped).unwrap();
    assert_eq!(plan.actions().len(), 1);
    assert_eq!(plan.intentions()[0].amount().atoms(), 100);
    assert_eq!(plan.explanations()[1].code(), "PREDICATE_FALSE");
    let zero = with_policy(|p| {
        p["rules"][0]["amount"]["fixed"] = json!("0");
    });
    let plan = policy::evaluate(&zero).unwrap();
    assert!(plan.actions().is_empty());
    assert!(plan.intentions().is_empty());
    assert!(plan
        .explanations()
        .iter()
        .all(|e| e.code() == "ZERO_ROUNDED"));
}
