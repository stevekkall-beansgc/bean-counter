use super::*;
use serde_json::json;
fn input() -> Value {
    let source: Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-ready-fixture-input.json")).unwrap();
    source["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["payload"]["action"] == "source.enroll")
        .unwrap()["payload"]["ceiling"]
        .clone()
}
fn parse(value: &Value) -> SourceCeiling {
    SourceCeiling::parse(&serde_json::to_vec(value).unwrap()).unwrap()
}
#[test]
fn selected_source_ceiling_roundtrips_and_time_is_half_open() {
    let value = input();
    let c = parse(&value);
    assert!(c.is_subset_of(&c));
    assert_eq!(
        parse(&c.canonical_value().unwrap()).canonical_bytes(),
        c.canonical_bytes()
    );
    assert!(c.contains_time(&Timestamp::parse(value["starts_at"].as_str().unwrap()).unwrap()));
    assert!(!c.contains_time(&Timestamp::parse(value["ends_at"].as_str().unwrap()).unwrap()));
}
#[test]
fn every_identity_set_and_time_dimension_must_narrow() {
    let value = input();
    let prior = parse(&value);
    for key in ["principal_id", "source", "canonical_economic_namespace"] {
        let mut changed = value.clone();
        changed[key] = json!("different");
        assert!(!parse(&changed).is_subset_of(&prior));
    }
    for (key, item) in [
        ("customer_scope", "other-customer"),
        ("operations", "backup.create"),
        ("source_slots", "other:slot"),
        ("permissions", "operate"),
    ] {
        let mut changed = value.clone();
        changed[key].as_array_mut().unwrap().push(json!(item));
        assert!(!parse(&changed).is_subset_of(&prior), "{key}");
    }
    let mut changed = value.clone();
    changed["starts_at"] = json!("2020-01-01T00:00:00Z");
    assert!(!parse(&changed).is_subset_of(&prior));
    let mut changed = value;
    changed["ends_at"] = json!("2099-01-01T00:00:00Z");
    assert!(!parse(&changed).is_subset_of(&prior));
}
#[test]
fn namespace_can_narrow_to_names_but_names_cannot_expand_to_namespace() {
    let mut value = input();
    value["product_selectors"] = json!([{"namespace":"a:"}]);
    value["activity_selectors"] = json!([]);
    value["link_selectors"] = json!([]);
    let namespace = parse(&value);
    value["product_selectors"] = json!([{"names":["a:one","a:two"]}]);
    let names = parse(&value);
    assert!(names.is_subset_of(&namespace));
    assert!(!namespace.is_subset_of(&names));
    value["product_selectors"] = json!([{"names":["ab:one"]}]);
    assert!(!parse(&value).is_subset_of(&namespace));
    value["product_selectors"] = json!([]);
    assert!(parse(&value).is_subset_of(&namespace));
}
#[test]
fn strict_raw_closed_sets_and_intervals_refuse_ambiguity() {
    let value = input();
    let raw = serde_json::to_string(&value).unwrap();
    let duplicate = raw.replacen('{', "{\"source\":\"duplicate\",", 1);
    assert!(SourceCeiling::parse(duplicate.as_bytes()).is_err());
    for (key, v) in [
        ("unknown", json!(true)),
        ("ends_at", Value::Null),
        ("permissions", json!(["read", "read"])),
        ("operations", json!(["invented.operation"])),
        (
            "product_selectors",
            json!([{"namespace":"a:","names":["a:one"]}]),
        ),
    ] {
        let mut changed = value.clone();
        changed[key] = v;
        assert!(
            SourceCeiling::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut changed = value;
    changed["ends_at"] = changed["starts_at"].clone();
    assert!(SourceCeiling::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
}
