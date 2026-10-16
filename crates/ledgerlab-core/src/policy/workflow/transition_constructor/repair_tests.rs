use super::{
    records,
    representation::{InvocationToken, ParsedDraftTimes, ParsedGrantInterval, ParsedRef},
    shape, *,
};
use serde_json::{Value, json};
#[test]
fn private_parsed_references_require_owner_and_preserve_raw_validation() {
    let owner = InvocationToken::new();
    let other = InvocationToken::new();
    let mut m = CoreMeter::new(65536).unwrap();
    let body = json!({"schema":"ledger-workflow-retained-evidence/1","purpose":"literal"});
    let digest =
        records::hash(&mut m, "workflow-document", &json!([body["schema"], body])).unwrap();
    let raw =
        json!({"kind":"workflow-evidence","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
    let parsed = ParsedRef::parse(&mut m, &owner, &raw).unwrap();
    let input = RecordInput {
        reference: parsed.exact().clone(),
        canonical_body: codec::encode(&mut m, &body, 4194304).unwrap(),
    };
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["test".into(), "scope".into()],
        source: "source".into(),
        operation_id: "operation".into(),
    };
    let mut r = records::Records::new(&mut m, &[input]).unwrap();
    assert!(
        r.get_parsed(
            &mut m,
            &other,
            &key,
            &parsed,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1"
        )
        .is_err()
    );
    let a = r
        .get_parsed(
            &mut m,
            &owner,
            &key,
            &parsed,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1",
        )
        .unwrap();
    let b = r
        .get(
            &mut m,
            &key,
            parsed.exact(),
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1",
        )
        .unwrap();
    assert_eq!(a, b);
    assert!(
        r.get_parsed(
            &mut m,
            &owner,
            &key,
            &parsed,
            "workflow-roles",
            "ledger-workflow-retained-evidence/1"
        )
        .is_err()
    );
    for edit in 0..4 {
        let mut bad = raw.clone();
        match edit {
            0 => bad["other"] = json!(true),
            1 => bad["digest"] = json!("sha256:0"),
            2 => bad["id"] = json!("broken"),
            _ => bad["id"] = Value::Null,
        };
        assert!(ParsedRef::parse(&mut m, &owner, &bad).is_err());
    }
    let duplicate =
        br#"{"kind":"workflow-evidence","\u006bind":"workflow-evidence","id":"x","digest":"x"}"#;
    assert!(codec::parse(&mut m, duplicate, 262144).is_err());
}
#[test]
fn private_times_parse_once_but_reject_cross_invocation_and_bad_dates() {
    let a = InvocationToken::new();
    let b = InvocationToken::new();
    let mut m = CoreMeter::new(65536).unwrap();
    let times = ParsedDraftTimes::parse(
        &mut m,
        &a,
        "2026-10-13T01:00:00.000000Z",
        "2026-10-13T01:00:00.000000Z",
    )
    .unwrap();
    assert!(times.get(&mut m, &b).is_err());
    let (received, accepted) = times.get(&mut m, &a).unwrap();
    assert_eq!(received, accepted);
    let g = ParsedGrantInterval::parse(
        &mut m,
        &a,
        &json!({"starts_at":"2026-10-12T00:00:00.000000Z","ends_at":"2027-01-01T00:00:00.000000Z"}),
    )
    .unwrap();
    assert!(g.get(&mut m, &b).is_err());
    g.get(&mut m, &a).unwrap();
    assert!(ParsedGrantInterval::parse(&mut m,&a,&json!({"starts_at":"2026-02-30T00:00:00.000000Z","ends_at":"2027-01-01T00:00:00.000000Z"})).is_err());
    assert!(
        ParsedGrantInterval::parse(
            &mut m,
            &a,
            &json!({"starts_at":"2026-10-12T00:00:00.000000Z"})
        )
        .is_err()
    );
}
#[test]
fn nested_equality_membership_and_subset_are_paid_and_short_circuit() {
    let value =
        json!({"a":[{"kind":"workflow-binding","numbers":[18,true,"text"]}],"b":{"name":"value"}});
    let mut m = CoreMeter::new(65536).unwrap();
    assert!(shape::same(&mut m, &value, &value).unwrap());
    let n = m.usage().semantic_work;
    let mut exact = CoreMeter::new(n).unwrap();
    assert!(shape::same(&mut exact, &value, &value).unwrap());
    assert_eq!(m.usage(), exact.usage());
    let mut less = CoreMeter::new(n - 1).unwrap();
    assert_eq!(
        shape::same(&mut less, &value, &value).unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    for change in 0..4 {
        let mut other = value.clone();
        match change {
            0 => other["a"][0]["numbers"][0] = json!(19),
            1 => {
                other.as_object_mut().unwrap().remove("b");
            }
            2 => other["a"][0]["numbers"] = json!([true, 18, "text"]),
            _ => other["b"] = json!("value"),
        };
        let mut meter = CoreMeter::new(65536).unwrap();
        assert!(!shape::same(&mut meter, &value, &other).unwrap());
    }
    let mut meter = CoreMeter::new(65536).unwrap();
    assert!(shape::member(&mut meter, &[json!(false), value.clone()], &value).unwrap());
    shape::subset(&mut meter, &[value.clone()], &[value]).unwrap();
    assert!(
        meter
            .usage()
            .trace
            .iter()
            .any(|x| x.kind == "compare.descent")
    );
}

#[test]
fn noninitial_inventory_remains_explicitly_unsupported() {
    let (_, c, p, _) = super::first_work_tests::test_inputs();
    let payload: serde_json::Value = serde_json::from_slice(&p).unwrap();
    let mut state = super::DraftStateData::default();
    state.chain_head = Some(c.authorities[0].activation_ref.clone());
    assert_eq!(
        super::inventory::entry::validate(
            &mut CoreMeter::new(65536).unwrap(),
            &c,
            &payload,
            &state
        )
        .unwrap_err()
        .code,
        "UNSUPPORTED_COMPOSITION"
    );
}

#[test]
fn byte_identity_counts_actual_mismatch_prefix_without_character_semantics() {
    for (a, b, bytes) in [
        ("abc", "longer", 0),
        ("abc", "xbc", 2),
        ("abc", "axc", 4),
        ("abc", "abc", 6),
    ] {
        let mut m = CoreMeter::new(65536).unwrap();
        assert_eq!(shape::same_text(&mut m, a, b).unwrap(), a == b);
        assert_eq!(m.usage().scanned_bytes, bytes);
        assert_eq!(
            m.usage()
                .trace
                .iter()
                .filter(|x| x.kind == "compare.string")
                .count(),
            1
        );
    }
    let mut zero = CoreMeter::new(0).unwrap();
    assert_eq!(
        shape::same_text(&mut zero, "a", "b").unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    assert_eq!(zero.usage().scanned_bytes, 0);
}
