use super::*;
use serde_json::json;

// Independent reviewer oracle: template-expected.json template-v1. Literal
// canonical bytes and IDs are supplied independently of production outputs.
const V1: &[u8] = br#"{"components":[{"aliases":["review.measure:work","review.measure:work-old"],"book":"retail","key":"review.measure:unit-root","role_slot":"review.measure:retail","root_key":"review.measure:unit-root"}],"families":[{"aliases":["review.measure:outcome"],"key":"review.measure:success-family","role_slot":"review.measure:retail","target_component_key":"review.measure:unit-root"}],"schema":"ledger-workflow-slot-template/1","template_id":"review.measure:template","version":"1"}"#;
const V1_ID: &str = "wdoc_a2b20cda74e68e98f929eac79b5a51065bb2518ce754dc7fb3d896d93e363b33";
fn initial() -> SlotTemplate {
    SlotTemplate::parse(V1, None).unwrap()
}
fn parse(value: &Value, previous: Option<&SlotTemplate>) -> Result<SlotTemplate> {
    SlotTemplate::parse(&serde_json::to_vec(value).unwrap(), previous)
}
fn successor(prior: &SlotTemplate, version: u64) -> Value {
    let mut value = prior.body.clone();
    value["version"] = json!(version.to_string());
    value["previous"] = json!({"kind":"workflow-slot-template","id":prior.document_id().unwrap(),"digest":prior.content_digest().unwrap()});
    value
}
fn component(key: &str, root: &str) -> Value {
    json!({"key":key,"book":"retail","root_key":root,"role_slot":"review.measure:retail","aliases":["review.measure:work"]})
}
fn error<T: std::fmt::Debug>(result: Result<T>, code: &str) {
    assert_eq!(result.unwrap_err().code, code);
}

#[test]
fn independent_template_bytes_and_identity() {
    let template = initial();
    assert_eq!(template.canonical_bytes().unwrap().as_slice(), V1);
    assert_eq!(template.document_id().unwrap(), V1_ID);
    assert_eq!(template.template_id(), "review.measure:template");
    assert_eq!(template.version(), 1);
    let c = template.component("review.measure:unit-root").unwrap();
    assert_eq!(c.book(), TemplateBook::Retail);
    assert_eq!(c.root_key(), c.key());
    assert_eq!(c.role_slot(), "review.measure:retail");
    assert_eq!(
        template
            .family("review.measure:success-family")
            .unwrap()
            .target_component_key(),
        c.key()
    );
}

#[test]
fn roots_are_direct_present_and_compatible() {
    let mut value = initial().body.clone();
    value["components"][0]["root_key"] = json!("review.measure:absent");
    error(parse(&value, None), "TEMPLATE_MISSING_ROOT");
    let mut value = initial().body.clone();
    value["components"].as_array_mut().unwrap().push(component(
        "review.measure:modifier",
        "review.measure:unit-root",
    ));
    parse(&value, None).unwrap();
    let mut incompatible = value.clone();
    incompatible["components"][1]["book"] = json!("supplier");
    error(parse(&incompatible, None), "TEMPLATE_ROOT_SEMANTICS");
    incompatible = value.clone();
    incompatible["components"][1]["role_slot"] = json!("review.measure:other");
    error(parse(&incompatible, None), "TEMPLATE_ROOT_SEMANTICS");
    value["components"]
        .as_array_mut()
        .unwrap()
        .push(component("review.measure:child", "review.measure:modifier"));
    error(parse(&value, None), "TEMPLATE_ROOT_CHAIN");
    value["components"][0]["root_key"] = json!("review.measure:modifier");
    error(parse(&value, None), "TEMPLATE_ROOT_CHAIN");
}

#[test]
fn families_target_original_roots_with_same_roles_in_any_supported_book() {
    let mut value = initial().body.clone();
    value["families"][0]["target_component_key"] = json!("review.measure:absent");
    error(parse(&value, None), "TEMPLATE_MISSING_ROOT");
    value = initial().body.clone();
    value["components"].as_array_mut().unwrap().push(component(
        "review.measure:modifier",
        "review.measure:unit-root",
    ));
    value["families"][0]["target_component_key"] = json!("review.measure:modifier");
    error(parse(&value, None), "TEMPLATE_FAMILY_TARGET");
    value = initial().body.clone();
    value["families"][0]["role_slot"] = json!("review.measure:other");
    error(parse(&value, None), "TEMPLATE_FAMILY_TARGET");
    for book in ["retail", "supplier", "cost_observation"] {
        value = initial().body.clone();
        value["components"][0]["book"] = json!(book);
        parse(&value, None).unwrap();
    }
}

#[test]
fn semantic_keys_and_per_key_aliases_are_unique() {
    let mut value = initial().body.clone();
    let mut duplicate = value["components"][0].clone();
    duplicate["aliases"] = json!(["review.measure:other"]);
    value["components"].as_array_mut().unwrap().push(duplicate);
    error(parse(&value, None), "TEMPLATE_DUPLICATE_KEY");
    value = initial().body.clone();
    let mut duplicate = value["families"][0].clone();
    duplicate["aliases"] = json!(["review.measure:other"]);
    value["families"].as_array_mut().unwrap().push(duplicate);
    error(parse(&value, None), "TEMPLATE_DUPLICATE_KEY");
    value = initial().body.clone();
    value["components"][0]["aliases"] = json!(["review.measure:work", "review.measure:work"]);
    assert!(parse(&value, None).is_err());
}

#[test]
fn alias_overlap_cannot_select_another_stable_key() {
    let mut value = initial().body.clone();
    value["components"].as_array_mut().unwrap().push(component(
        "review.measure:other-root",
        "review.measure:other-root",
    ));
    // Component and family key namespaces are separate.
    value["families"][0]["key"] = json!("review.measure:other-root");
    value["families"][0]["aliases"] = json!(["review.measure:work", "review.measure:other-root"]);
    let template = parse(&value, None).unwrap();
    assert_eq!(
        template
            .component_named("review.measure:unit-root", "review.measure:work")
            .unwrap()
            .key(),
        "review.measure:unit-root"
    );
    assert_eq!(
        template
            .component_named("review.measure:other-root", "review.measure:work")
            .unwrap()
            .key(),
        "review.measure:other-root"
    );
    assert_eq!(
        template
            .family_named("review.measure:other-root", "review.measure:work")
            .unwrap()
            .key(),
        "review.measure:other-root"
    );
    error(
        template.component_named("review.measure:work", "review.measure:work"),
        "TEMPLATE_UNKNOWN_KEY",
    );
    error(
        template.component_named("review.measure:unit-root", "review.measure:unlisted"),
        "TEMPLATE_UNKNOWN_ALIAS",
    );
}

#[test]
fn normalization_preserves_semantics_under_array_permutation() {
    let mut value = initial().body.clone();
    value["components"].as_array_mut().unwrap().push(component(
        "review.measure:modifier",
        "review.measure:unit-root",
    ));
    let original = parse(&value, None).unwrap();
    value["version"] = json!("0001");
    value["components"].as_array_mut().unwrap().reverse();
    value["components"][1]["aliases"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        parse(&value, None).unwrap().canonical_bytes().unwrap(),
        original.canonical_bytes().unwrap()
    );
}

#[test]
fn continuity_pins_original_semantics_and_exact_previous_document() {
    let prior = initial();
    let original_bytes = prior.canonical_bytes().unwrap();
    let mut value = successor(&prior, 2);
    value["components"][0]["aliases"] = json!(["review.measure:work-renamed"]);
    value["components"]
        .as_array_mut()
        .unwrap()
        .push(component("review.measure:future", "review.measure:future"));
    let next = parse(&value, Some(&prior)).unwrap();
    assert_eq!(next.version(), 2);
    assert_eq!(prior.canonical_bytes().unwrap(), original_bytes);
    assert_ne!(next.document_id().unwrap(), prior.document_id().unwrap());
    let mut changed = value.clone();
    changed["components"][0]["book"] = json!("supplier");
    error(parse(&changed, Some(&prior)), "TEMPLATE_CONTINUITY");
    changed = value.clone();
    changed["components"][0]["root_key"] = json!("review.measure:future");
    error(parse(&changed, Some(&prior)), "TEMPLATE_FAMILY_TARGET");
    changed = value.clone();
    changed["families"] = json!([]);
    error(parse(&changed, Some(&prior)), "TEMPLATE_CONTINUITY");
    changed = value.clone();
    changed["families"][0]["target_component_key"] = json!("review.measure:future");
    error(parse(&changed, Some(&prior)), "TEMPLATE_CONTINUITY");
    changed = value.clone();
    changed["components"].as_array_mut().unwrap().remove(0);
    changed["families"] = json!([]);
    error(parse(&changed, Some(&prior)), "TEMPLATE_CONTINUITY");
    changed = value.clone();
    changed["previous"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    error(parse(&changed, Some(&prior)), "TEMPLATE_PREVIOUS_MISMATCH");
    error(parse(&value, None), "TEMPLATE_DEPENDENCY_MISSING");
}

#[test]
fn ancestry_versions_are_strictly_increasing_and_initial_is_unlinked() {
    let prior = initial();
    parse(&successor(&prior, 3), Some(&prior)).unwrap();
    error(
        parse(&successor(&prior, 1), Some(&prior)),
        "TEMPLATE_VERSION",
    );
    let mut value = successor(&prior, 2);
    value["template_id"] = json!("review.measure:other");
    error(parse(&value, Some(&prior)), "TEMPLATE_VERSION");
    value = successor(&prior, 2);
    value["previous"]["kind"] = json!("workflow-policy");
    error(parse(&value, Some(&prior)), "TEMPLATE_PREVIOUS_MISMATCH");
    let second = parse(&successor(&prior, 2), Some(&prior)).unwrap();
    error(
        parse(&successor(&prior, 2), Some(&second)),
        "TEMPLATE_PREVIOUS_MISMATCH",
    );
    value = prior.body.clone();
    value["version"] = json!("0");
    error(parse(&value, None), "TEMPLATE_VERSION");
    value = prior.body.clone();
    value["version"] = json!("2");
    error(parse(&value, Some(&prior)), "TEMPLATE_DEPENDENCY_MISSING");
}

#[test]
fn private_binding_data_unknowns_null_and_lexical_ambiguity_refuse() {
    for (path, value) in [("customer", json!("customer-A")), ("previous", Value::Null)] {
        let mut wrong = initial().body.clone();
        wrong[path] = value;
        assert!(parse(&wrong, None).is_err());
    }
    for field in ["scope", "agreement_id", "slot_id", "extra"] {
        let mut wrong = initial().body.clone();
        wrong["components"][0][field] = json!("untrusted");
        assert!(parse(&wrong, None).is_err());
    }
    let wrong = br#"{"schema":"ledger-workflow-slot-template/1","schema":"ledger-workflow-slot-template/1"}"#;
    error(SlotTemplate::parse(wrong, None), "JSON_DUPLICATE_KEY");
    let mut wrong = initial().body.clone();
    wrong["components"][0]["book"] = json!("arbitrary-book");
    assert!(parse(&wrong, None).is_err());
    wrong = initial().body.clone();
    wrong["components"][0]["key"] = json!("unqualified");
    assert!(parse(&wrong, None).is_err());
}

#[test]
fn exact_template_collection_and_alias_limits() {
    let mut value = initial().body.clone();
    value["families"] = json!([]);
    value["components"] = json!(
        (0..128)
            .map(|i| component(
                &format!("review.measure:c{i}"),
                &format!("review.measure:c{i}")
            ))
            .collect::<Vec<_>>()
    );
    parse(&value, None).unwrap();
    value["components"].as_array_mut().unwrap().push(component(
        "review.measure:overflow",
        "review.measure:overflow",
    ));
    error(parse(&value, None), "TEMPLATE_LIMIT");
    value = initial().body.clone();
    value["components"][0]["aliases"] = json!(
        (0..64)
            .map(|i| format!("review.measure:a{i}"))
            .collect::<Vec<_>>()
    );
    parse(&value, None).unwrap();
    value["components"][0]["aliases"]
        .as_array_mut()
        .unwrap()
        .push(json!("review.measure:overflow"));
    error(parse(&value, None), "TEMPLATE_LIMIT");
    value = initial().body.clone();
    value["components"] = json!([]);
    error(parse(&value, None), "TEMPLATE_LIMIT");
    value = initial().body.clone();
    let family = value["families"][0].clone();
    value["families"] = json!(
        (0..64)
            .map(|i| {
                let mut next = family.clone();
                next["key"] = json!(format!("review.measure:f{i}"));
                next
            })
            .collect::<Vec<_>>()
    );
    parse(&value, None).unwrap();
    let mut overflow = family;
    overflow["key"] = json!("review.measure:overflow");
    value["families"].as_array_mut().unwrap().push(overflow);
    error(parse(&value, None), "TEMPLATE_LIMIT");
}

#[test]
fn independent_ordering_and_successor_vectors() {
    let prior = initial();
    let vectors: &[(&[u8], &str, bool)] = &[
        (br#"{"components":[{"aliases":["review.measure:work","review.measure:work-old","review.measure:work-renamed"],"book":"retail","key":"review.measure:unit-root","role_slot":"review.measure:retail","root_key":"review.measure:unit-root"}],"families":[{"aliases":["review.measure:outcome"],"key":"review.measure:success-family","role_slot":"review.measure:retail","target_component_key":"review.measure:unit-root"}],"previous":{"digest":"sha256:a2b20cda74e68e98f929eac79b5a51065bb2518ce754dc7fb3d896d93e363b33","id":"wdoc_a2b20cda74e68e98f929eac79b5a51065bb2518ce754dc7fb3d896d93e363b33","kind":"workflow-slot-template"},"schema":"ledger-workflow-slot-template/1","template_id":"review.measure:template","version":"2"}"#, "wdoc_8c0c802b371f6d0608bee7f0c485597c58964a382278097efaf725b43e571125", true),
        (br#"{"components":[{"aliases":["review.order:z-alias"],"book":"retail","key":"review.order:a","role_slot":"review.order:retail","root_key":"review.order:a"},{"aliases":["review.order:shared"],"book":"retail","key":"review.order:m","role_slot":"review.order:retail","root_key":"review.order:a"},{"aliases":["review.order:a-alias","review.order:shared"],"book":"supplier","key":"review.order:z","role_slot":"review.order:supplier","root_key":"review.order:z"}],"families":[{"aliases":["review.order:shared"],"key":"review.order:a","role_slot":"review.order:retail","target_component_key":"review.order:a"}],"schema":"ledger-workflow-slot-template/1","template_id":"review.order:template","version":"1"}"#, "wdoc_33f6f637f4547280bdd80b7daac66aad45b6fa7f9072bbffca40c55222507d28", false),
        (br#"{"components":[{"aliases":[],"book":"retail","key":"review.measure:unit-root","role_slot":"review.measure:retail","root_key":"review.measure:unit-root"}],"families":[{"aliases":["review.measure:outcome"],"key":"review.measure:success-family","role_slot":"review.measure:retail","target_component_key":"review.measure:unit-root"}],"previous":{"digest":"sha256:a2b20cda74e68e98f929eac79b5a51065bb2518ce754dc7fb3d896d93e363b33","id":"wdoc_a2b20cda74e68e98f929eac79b5a51065bb2518ce754dc7fb3d896d93e363b33","kind":"workflow-slot-template"},"schema":"ledger-workflow-slot-template/1","template_id":"review.measure:template","version":"3"}"#, "wdoc_f3f3fa65597897bb60a6ee6f56cc7ef3a3d1e1d9aebaf9c4932f80b06493f7c7", true),
    ];
    for (expected_bytes, expected_id, successor) in vectors {
        let previous = successor.then_some(&prior);
        let mut shuffled: Value = serde_json::from_slice(expected_bytes).unwrap();
        shuffled["components"].as_array_mut().unwrap().reverse();
        for component in shuffled["components"].as_array_mut().unwrap() {
            component["aliases"].as_array_mut().unwrap().reverse();
        }
        let actual = parse(&shuffled, previous).unwrap();
        assert_eq!(
            actual.canonical_bytes().unwrap().as_slice(),
            *expected_bytes
        );
        assert_eq!(actual.document_id().unwrap(), *expected_id);
    }
}

#[test]
fn executing_initial_template_matches_sealed_bytes_and_exact_budget() {
    use crate::validation_work::tests::CountingWork;
    let mut work = CountingWork::new(usize::MAX);
    let template = SlotTemplate::parse_with_work(V1, None, &mut work).unwrap();
    let parse_used = work.used;
    assert_eq!(
        template
            .canonical_bytes_with_work(&mut work)
            .unwrap()
            .as_slice(),
        V1
    );
    assert_eq!(template.document_id_with_work(&mut work).unwrap(), V1_ID);
    let mut exact = CountingWork::new(parse_used);
    SlotTemplate::parse_with_work(V1, None, &mut exact).unwrap();
    let mut short = CountingWork::new(parse_used - 1);
    error(
        SlotTemplate::parse_with_work(V1, None, &mut short),
        "TEST_EXHAUSTED",
    );
    let mut decoded_work = CountingWork::new(usize::MAX);
    let decoded =
        SlotTemplate::from_value_with_work(canonical::parse(V1).unwrap(), None, &mut decoded_work)
            .unwrap();
    assert_eq!(decoded.body, template.body);
    assert_eq!(decoded_work.used, parse_used);
    let mut dangling = template.body.clone();
    dangling["components"][0]["root_key"] = json!("review.measure:absent");
    let mut enough = CountingWork::new(usize::MAX);
    error(
        SlotTemplate::from_value_with_work(dangling, None, &mut enough),
        "TEMPLATE_MISSING_ROOT",
    );
    assert!(enough.visits.contains(&Visit::RefLookup));
}

#[test]
fn executing_named_lookup_charges_each_real_candidate_and_stops() {
    use crate::validation_work::tests::CountingWork;
    let template = initial();
    let mut exact = CountingWork::new(4);
    assert_eq!(
        template
            .component_named_with_work(
                "review.measure:unit-root",
                "review.measure:work-old",
                &mut exact
            )
            .unwrap()
            .key(),
        "review.measure:unit-root"
    );
    assert_eq!(
        exact.visits,
        [
            Visit::RefLookup,
            Visit::MemberCompare,
            Visit::MemberCompare,
            Visit::MemberCompare
        ]
    );
    let mut short = CountingWork::new(3);
    error(
        template.component_named_with_work(
            "review.measure:unit-root",
            "review.measure:work-old",
            &mut short,
        ),
        "TEST_EXHAUSTED",
    );
    let mut missing = CountingWork::new(2);
    assert!(
        template
            .component_with_work("review.measure:absent", &mut missing)
            .unwrap()
            .is_none()
    );
    let mut family = CountingWork::new(3);
    template
        .family_named_with_work(
            "review.measure:success-family",
            "review.measure:outcome",
            &mut family,
        )
        .unwrap();
    assert_eq!(family.used, 3);
}

#[test]
fn complete_source_template_preserves_successor_identity_and_continuity() {
    let prior = initial();
    let mut raw = successor(&prior, 2);
    raw["version"] = json!("0002");
    raw["components"][0]["aliases"] = json!(["review.measure:work-old", "review.measure:work"]);
    let bound = SlotTemplate::normalization_payload_bound_with_work(
        &raw,
        Some(&prior),
        &mut NoopValidationWork,
    )
    .unwrap();
    assert!(bound > 0);
    let old =
        SlotTemplate::from_value_with_work(raw.clone(), Some(&prior), &mut NoopValidationWork)
            .unwrap();
    let complete = SlotTemplate::from_value_complete_with_work(
        raw.clone(),
        Some(&prior),
        &mut NoopValidationWork,
    )
    .unwrap();
    assert_eq!(
        old.canonical_bytes().unwrap(),
        complete.canonical_bytes().unwrap()
    );
    assert_eq!(
        old.component("review.measure:unit-root"),
        complete.component("review.measure:unit-root")
    );
    for selector in [0, 1, 2] {
        let mut invalid = raw.clone();
        match selector {
            0 => invalid["previous"]["id"] = json!("wdoc_wrong"),
            1 => invalid["components"][0]["role_slot"] = json!("review.measure:other"),
            _ => {
                invalid["families"][0]["aliases"] =
                    json!(["review.measure:outcome", "review.measure:outcome"])
            }
        }
        let old = SlotTemplate::from_value_with_work(
            invalid.clone(),
            Some(&prior),
            &mut NoopValidationWork,
        )
        .unwrap_err();
        let new = SlotTemplate::from_value_complete_with_work(
            invalid,
            Some(&prior),
            &mut NoopValidationWork,
        )
        .unwrap_err();
        assert_eq!(old.code, new.code);
    }
}
