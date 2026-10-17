use super::*;
use serde_json::json;

// Independent publication expected vector, derived from author input only by
// reviewer stdlib code (publication-expected.json). No production oracle.
const FACTS: &[u8] = br#"{"fields":[{"name":"fixture.report:pages","provenance":{"kind":"caller"},"required":true,"type":{"kind":"integer","max":"100","min":"0","unit":"fixture.report:page"}}],"normalization":"ledger-facts-normalization/1","schema":"ledger-workflow-fact-schema/1","schema_id":"fixture.report:facts","version":"1"}"#;
const PRODUCT: &[u8] = br#"{"activities":[{"fact_schema":{"digest":"sha256:a3327d388ed157f06a9fed0943cc38d86de2e3632b23ea07b2e01112e35214a7","id":"wdoc_a3327d388ed157f06a9fed0943cc38d86de2e3632b23ea07b2e01112e35214a7","kind":"workflow-fact-schema"},"funding_leg":"fixture.report:customer","labels":{"title":"Completed report"},"name":"fixture.report:completed","primitive":"work","source_slot":"fixture.report:worker"}],"evaluator":"ledger-workflow-evaluator/1","fact_schemas":[{"digest":"sha256:a3327d388ed157f06a9fed0943cc38d86de2e3632b23ea07b2e01112e35214a7","id":"wdoc_a3327d388ed157f06a9fed0943cc38d86de2e3632b23ea07b2e01112e35214a7","kind":"workflow-fact-schema"}],"grammar":"ledger-policy-grammar/1","labels":{"title":"Synthetic report"},"limits_profile":"workflow-limits/1","links":[],"product_id":"fixture.report:product","profile":"workflow/1","schema":"ledger-workflow-product/1","units":[{"dimension":"fixture.report:count","labels":{"title":"Page"},"name":"fixture.report:page"}],"version":"1"}"#;
fn facts() -> FactSchema {
    FactSchema::parse(FACTS).unwrap()
}
fn source() -> Value {
    serde_json::from_slice(PRODUCT).unwrap()
}
fn parse(value: &Value) -> Result<Product> {
    Product::parse(&serde_json::to_vec(value).unwrap(), &[facts()])
}
fn error<T: std::fmt::Debug>(result: Result<T>, code: &str) {
    assert_eq!(result.unwrap_err().code, code);
}
fn unit(name: &str, dimension: &str) -> Value {
    json!({"name":name,"dimension":dimension,"labels":{"title":"Unit"}})
}
fn link(name: &str) -> Value {
    json!({"name":name,"child_activity":"fixture.report:completed","predecessor_activities":["fixture.report:completed"],"child_primitive":"work","predecessor_primitive":"work","minimum":"0","maximum":"1","binding_relation":"same_binding","on_missing":"refuse","max_depth":"16"})
}

#[test]
fn independent_product_canonical_bytes_and_identity() {
    let product = parse(&source()).unwrap();
    assert_eq!(product.canonical_bytes().unwrap().as_slice(), PRODUCT);
    assert_eq!(
        product.document_id().unwrap(),
        "wdoc_d0e956daa316b95c98d2553a2d3d308fcbc76edca02e2265cf0933f473eb43cb"
    );
    assert_eq!(product.product_id(), "fixture.report:product");
    assert_eq!(product.version(), 1);
    let activity = product.activity("fixture.report:completed").unwrap();
    assert_eq!(activity.primitive(), Primitive::Work);
    assert_eq!(activity.source_slot(), "fixture.report:worker");
    assert_eq!(activity.funding_leg(), Some("fixture.report:customer"));
    assert_eq!(
        product
            .fact_schema(activity.fact_schema_id())
            .unwrap()
            .canonical_value(),
        facts().canonical_value()
    );
}

#[test]
fn immutable_reference_closure_is_exact_and_never_fetched() {
    error(Product::parse(PRODUCT, &[]), "DEFINITION_MISSING");
    let mut wrong = source();
    wrong["fact_schemas"][0]["kind"] = json!("workflow-policy");
    error(parse(&wrong), "DEFINITION_KIND");
    wrong = source();
    wrong["fact_schemas"][0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    error(parse(&wrong), "DEFINITION_DIGEST_MISMATCH");
    wrong = source();
    wrong["activities"][0]["fact_schema"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    error(parse(&wrong), "DEFINITION_DIGEST_MISMATCH");
    wrong = source();
    wrong["activities"][0]["fact_schema"]["id"] = json!("https://untrusted.example/schema");
    error(parse(&wrong), "DEFINITION_MISSING");
    wrong = source();
    let reference = wrong["fact_schemas"][0].clone();
    wrong["fact_schemas"]
        .as_array_mut()
        .unwrap()
        .push(reference);
    assert!(parse(&wrong).is_err());
}

#[test]
fn units_use_exact_positive_one_hop_normalization() {
    let mut value = source();
    let mut normalized = unit("fixture.report:batch", "fixture.report:count");
    normalized["normalization"] =
        json!({"to":"fixture.report:page","factor":{"numerator":"3","denominator":"2"}});
    value["units"].as_array_mut().unwrap().push(normalized);
    let product = parse(&value).unwrap();
    let (to, factor) = product
        .unit("fixture.report:batch")
        .unwrap()
        .normalization()
        .unwrap();
    assert_eq!(to, "fixture.report:page");
    assert_eq!(
        serde_json::to_value(factor).unwrap(),
        json!({"numerator":"3","denominator":"2"})
    );
    for numerator in ["0", "-1"] {
        let mut wrong = value.clone();
        wrong["units"][1]["normalization"]["factor"]["numerator"] = json!(numerator);
        assert!(parse(&wrong).is_err());
    }
    let mut wrong = value.clone();
    wrong["units"][1]["normalization"]["factor"] = json!({"numerator":"6","denominator":"4"});
    assert!(parse(&wrong).is_err());
    wrong = value.clone();
    wrong["units"][1]["dimension"] = json!("fixture.report:storage");
    error(parse(&wrong), "UNIT_NORMALIZATION");
    wrong = value.clone();
    wrong["units"][1]["normalization"]["to"] = json!("fixture.report:absent");
    error(parse(&wrong), "UNIT_MISSING");
    wrong = value.clone();
    wrong["units"][0]["normalization"] =
        json!({"to":"fixture.report:batch","factor":{"numerator":"1","denominator":"1"}});
    error(parse(&wrong), "UNIT_NORMALIZATION");
    wrong = source();
    wrong["units"][0]["name"] = json!("fixture.report:undeclared");
    error(parse(&wrong), "UNIT_MISSING");
}

#[test]
fn definition_type_edges_allow_same_activity_and_cycles() {
    let mut value = source();
    value["links"] = json!([link("fixture.report:previous")]);
    let product = parse(&value).unwrap();
    let definition = product.link("fixture.report:previous").unwrap();
    assert_eq!(definition.maximum(), 1);
    assert_eq!(definition.minimum(), 0);
    assert_eq!(definition.maximum_depth(), 16);
    assert_eq!(definition.on_missing(), MissingDependency::Refuse);
    product
        .validate_selector_path(
            "fixture.report:completed",
            &["fixture.report:previous".to_owned()],
            "fixture.report:completed",
        )
        .unwrap();
    let mut second = value["activities"][0].clone();
    second["name"] = json!("fixture.report:continued");
    value["activities"].as_array_mut().unwrap().push(second);
    value["links"][0]["predecessor_activities"] = json!(["fixture.report:continued"]);
    let mut back = link("fixture.report:back");
    back["child_activity"] = json!("fixture.report:continued");
    value["links"].as_array_mut().unwrap().push(back);
    parse(&value).unwrap();
    // Actual event graphs and pricing dependencies are separate and must reject
    // concrete cycles later; this parser does not claim they were resolved.
}

#[test]
fn link_primitives_cardinality_and_supplier_requirements_refuse() {
    let mut value = source();
    value["links"] = json!([link("fixture.report:previous")]);
    for (field, replacement, code) in [
        (
            "child_activity",
            json!("fixture.report:absent"),
            "LINK_ACTIVITY_MISSING",
        ),
        ("child_primitive", json!("outcome"), "LINK_PRIMITIVE"),
        ("predecessor_primitive", json!("outcome"), "LINK_PRIMITIVE"),
        ("minimum", json!("2"), "LINK_LIMIT"),
        ("maximum", json!("9"), "LINK_LIMIT"),
        ("max_depth", json!("17"), "LINK_LIMIT"),
        (
            "supplier_slot",
            json!("fixture.report:supplier"),
            "LINK_SUPPLIER_SLOT",
        ),
    ] {
        let mut wrong = value.clone();
        wrong["links"][0][field] = replacement;
        error(parse(&wrong), code);
    }
    value["links"][0]["binding_relation"] = json!("declared_supplier");
    error(parse(&value), "LINK_SUPPLIER_SLOT");
    value["links"][0]["supplier_slot"] = json!("fixture.report:supplier");
    parse(&value).unwrap();
    value["links"][0]["predecessor_activities"] =
        json!(["fixture.report:completed", "fixture.report:completed"]);
    assert!(parse(&value).is_err());
}

#[test]
fn selector_paths_have_exact_named_hops_no_fanout_or_fallback() {
    let mut value = source();
    value["links"] = json!([link("fixture.report:previous")]);
    let product = parse(&value).unwrap();
    for path in [
        vec![],
        vec!["fixture.report:missing".into()],
        vec!["fixture.report:previous".into(); 5],
    ] {
        error(
            product.validate_selector_path(
                "fixture.report:completed",
                &path,
                "fixture.report:completed",
            ),
            "SELECTOR_PATH",
        );
    }
    value["links"][0]["maximum"] = json!("2");
    let product = parse(&value).unwrap();
    error(
        product.validate_selector_path(
            "fixture.report:completed",
            &["fixture.report:previous".into()],
            "fixture.report:completed",
        ),
        "SELECTOR_PATH",
    );
}

#[test]
fn named_collections_sort_and_semantic_duplicates_refuse() {
    let mut value = source();
    value["units"]
        .as_array_mut()
        .unwrap()
        .push(unit("fixture.report:aaa", "fixture.report:count"));
    value["links"] = json!([link("fixture.report:z"), link("fixture.report:a")]);
    let expected = parse(&value).unwrap().canonical_bytes().unwrap();
    value["version"] = json!("0001");
    value["units"].as_array_mut().unwrap().reverse();
    value["links"].as_array_mut().unwrap().reverse();
    assert_eq!(parse(&value).unwrap().canonical_bytes().unwrap(), expected);
    for collection in ["units", "activities", "links"] {
        let mut wrong = value.clone();
        let mut duplicate = wrong[collection][0].clone();
        if collection != "links" {
            duplicate["labels"]["title"] = json!("Changed label");
        }
        wrong[collection].as_array_mut().unwrap().push(duplicate);
        error(parse(&wrong), "PRODUCT_DUPLICATE_KEY");
    }
}

#[test]
fn product_bounds_closed_shapes_and_unicode_label_bytes() {
    let mut value = source();
    value["labels"]["title"] = json!("é".repeat(64));
    parse(&value).unwrap();
    value["labels"]["title"] = json!("é".repeat(65));
    assert!(parse(&value).is_err());
    for field in ["customer", "scope", "provider_url"] {
        let mut wrong = source();
        wrong[field] = json!("untrusted");
        assert!(parse(&wrong).is_err());
    }
    for field in ["profile", "evaluator", "grammar", "limits_profile"] {
        let mut wrong = source();
        wrong[field] = json!("unknown/1");
        error(parse(&wrong), "UNSUPPORTED_SCHEMA_PROFILE");
    }
    let mut wrong = source();
    wrong["activities"][0]["funding_leg"] = Value::Null;
    assert!(parse(&wrong).is_err());
    value = source();
    value["activities"] = json!(
        (0..16)
            .map(|i| {
                let mut activity = source()["activities"][0].clone();
                activity["name"] = json!(format!("fixture.report:a{i}"));
                activity
            })
            .collect::<Vec<_>>()
    );
    parse(&value).unwrap();
    let mut overflow = source()["activities"][0].clone();
    overflow["name"] = json!("fixture.report:overflow");
    value["activities"].as_array_mut().unwrap().push(overflow);
    error(parse(&value), "PRODUCT_LIMIT");
    value = source();
    value["units"] = json!(
        (0..16)
            .map(|i| if i == 0 {
                source()["units"][0].clone()
            } else {
                unit(&format!("fixture.report:u{i}"), "fixture.report:count")
            })
            .collect::<Vec<_>>()
    );
    parse(&value).unwrap();
    value["units"]
        .as_array_mut()
        .unwrap()
        .push(unit("fixture.report:overflow", "fixture.report:count"));
    error(parse(&value), "PRODUCT_LIMIT");
    value = source();
    value["links"] = json!(
        (0..32)
            .map(|i| link(&format!("fixture.report:l{i}")))
            .collect::<Vec<_>>()
    );
    parse(&value).unwrap();
    value["links"]
        .as_array_mut()
        .unwrap()
        .push(link("fixture.report:overflow"));
    error(parse(&value), "PRODUCT_LIMIT");
}

#[test]
fn binding_context_is_required_caller_only_and_can_be_explicitly_empty() {
    let empty=FactSchema::parse(br#"{"schema":"ledger-workflow-fact-schema/1","schema_id":"fixture.report:empty-context","version":"1","normalization":"ledger-facts-normalization/1","fields":[]}"#).unwrap();
    assert_eq!(
        empty.normalize_context(b"{}").unwrap().canonical_value(),
        &json!({})
    );
    error(
        empty.normalize_context(br#"{"fixture.report:undeclared":true}"#),
        "FACT_UNKNOWN",
    );
    let mut declaration = facts().canonical_value().clone();
    declaration["fields"][0]["required"] = json!(false);
    let optional = FactSchema::parse(&serde_json::to_vec(&declaration).unwrap()).unwrap();
    error(optional.normalize_context(b"{}"), "CONTEXT_SCHEMA");
    declaration = facts().canonical_value().clone();
    declaration["fields"][0]["provenance"] = json!({"kind":"attested","source_slot":"fixture.report:worker","evidence_kind":"meter_quantity"});
    let attested = FactSchema::parse(&serde_json::to_vec(&declaration).unwrap()).unwrap();
    error(
        attested.normalize_context(br#"{"fixture.report:pages":"1"}"#),
        "CONTEXT_SCHEMA",
    );
    facts()
        .normalize_context(br#"{"fixture.report:pages":"001"}"#)
        .unwrap();
}

#[test]
fn independent_unit_order_and_exact_ratio_vector() {
    // product-expected.json supplemental independent reviewer vector; unit
    // labels intentionally put whole-object JCS order opposite to name order.
    let mut value = source();
    value["units"][0]["labels"]["title"] = json!("A label that sorts before Z");
    let mut other = unit("fixture.report:aaa", "fixture.report:count");
    other["labels"]["title"] = json!("Z label that sorts after A");
    other["normalization"] =
        json!({"to":"fixture.report:page","factor":{"numerator":"3","denominator":"2"}});
    value["units"].as_array_mut().unwrap().push(other);
    let product = parse(&value).unwrap();
    let expected_units = br#"[{"dimension":"fixture.report:count","labels":{"title":"Z label that sorts after A"},"name":"fixture.report:aaa","normalization":{"factor":{"denominator":"2","numerator":"3"},"to":"fixture.report:page"}},{"dimension":"fixture.report:count","labels":{"title":"A label that sorts before Z"},"name":"fixture.report:page"}]"#;
    assert_eq!(
        CanonicalBytes::from_value(&product.canonical_value()["units"])
            .unwrap()
            .as_slice(),
        expected_units
    );
    assert_eq!(
        product.document_id().unwrap(),
        "wdoc_842e3a576b559024d8b86d813ae7303354f15bace2e0512009d66cd667ac41ce"
    );
}

#[test]
fn schema_registry_keys_and_sixteen_schema_boundary() {
    let mut value = source();
    let mut schemas = vec![facts()];
    for i in 1..16 {
        let mut definition = facts().canonical_value().clone();
        definition["schema_id"] = json!(format!("fixture.report:schema{i}"));
        let schema = FactSchema::parse(&serde_json::to_vec(&definition).unwrap()).unwrap();
        value["fact_schemas"].as_array_mut().unwrap().push(json!({"kind":"workflow-fact-schema","id":schema.document_id().unwrap(),"digest":schema.content_digest().unwrap()}));
        schemas.push(schema);
    }
    Product::parse(&serde_json::to_vec(&value).unwrap(), &schemas).unwrap();
    let mut seventeen = value.clone();
    let repeated = value["fact_schemas"][0].clone();
    seventeen["fact_schemas"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    error(
        Product::parse(&serde_json::to_vec(&seventeen).unwrap(), &schemas),
        "PRODUCT_LIMIT",
    );
    let mut conflict = facts().canonical_value().clone();
    conflict["fields"][0]["type"]["max"] = json!("99");
    let conflict = FactSchema::parse(&serde_json::to_vec(&conflict).unwrap()).unwrap();
    let mut value = source();
    let conflict_ref = json!({"kind":"workflow-fact-schema","id":conflict.document_id().unwrap(),"digest":conflict.content_digest().unwrap()});
    value["fact_schemas"]
        .as_array_mut()
        .unwrap()
        .push(conflict_ref.clone());
    error(
        Product::parse(
            &serde_json::to_vec(&value).unwrap(),
            &[facts(), conflict.clone()],
        ),
        "DEFINITION_CONFLICT",
    );
    value = source();
    value["activities"][0]["fact_schema"] = conflict_ref;
    error(
        Product::parse(&serde_json::to_vec(&value).unwrap(), &[facts(), conflict]),
        "DEFINITION_MISSING",
    );
}

#[test]
fn selector_declaration_feasibility_does_not_prove_every_actual_endpoint() {
    let mut value = source();
    for name in ["fixture.report:A", "fixture.report:B"] {
        let mut activity = source()["activities"][0].clone();
        activity["name"] = json!(name);
        value["activities"].as_array_mut().unwrap().push(activity);
    }
    let mut first = link("fixture.report:first");
    first["predecessor_activities"] = json!(["fixture.report:A", "fixture.report:B"]);
    let mut second = link("fixture.report:second");
    second["child_activity"] = json!("fixture.report:A");
    value["links"] = json!([first, second]);
    let product = parse(&value).unwrap();
    product
        .validate_selector_path(
            "fixture.report:completed",
            &[
                "fixture.report:first".into(),
                "fixture.report:second".into(),
            ],
            "fixture.report:completed",
        )
        .unwrap();
    // A concrete first-hop B endpoint cannot use the second slot. Runtime must
    // perform this endpoint check rather than treating feasibility as totality.
    error(
        product.validate_selector_path(
            "fixture.report:B",
            &["fixture.report:second".into()],
            "fixture.report:completed",
        ),
        "SELECTOR_PATH",
    );
}

#[test]
fn executing_terminal_product_matches_independent_bytes_and_exact_budget() {
    use crate::validation_work::tests::CountingWork;
    let mut work = CountingWork::new(usize::MAX);
    let product = Product::parse_with_work(PRODUCT, &[facts()], &mut work).unwrap();
    assert_eq!(
        product
            .canonical_bytes_with_work(&mut work)
            .unwrap()
            .as_slice(),
        PRODUCT
    );
    assert_eq!(
        product.document_id_with_work(&mut work).unwrap(),
        "wdoc_d0e956daa316b95c98d2553a2d3d308fcbc76edca02e2265cf0933f473eb43cb"
    );
    let mut parse_work = CountingWork::new(usize::MAX);
    Product::parse_with_work(PRODUCT, &[facts()], &mut parse_work).unwrap();
    let mut exact = CountingWork::new(parse_work.used);
    Product::parse_with_work(PRODUCT, &[facts()], &mut exact).unwrap();
    let mut short = CountingWork::new(parse_work.used - 1);
    error(
        Product::parse_with_work(PRODUCT, &[facts()], &mut short),
        "TEST_EXHAUSTED",
    );
    let mut decoded_work = CountingWork::new(usize::MAX);
    let decoded = Product::from_value_with_work(
        canonical::parse(PRODUCT).unwrap(),
        &[facts()],
        &mut decoded_work,
    )
    .unwrap();
    assert_eq!(decoded.body, product.body);
    assert_eq!(decoded_work.used, parse_work.used);
    assert_eq!(decoded_work.scanned + PRODUCT.len(), parse_work.scanned);
    let mut wrong = source();
    wrong["activities"][0]["fact_schema"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    let mut enough = CountingWork::new(usize::MAX);
    error(
        Product::from_value_with_work(wrong, &[facts()], &mut enough),
        "DEFINITION_DIGEST_MISMATCH",
    );
    let mut exhausted = CountingWork::new(0);
    error(
        Product::parse_with_work(PRODUCT, &[], &mut exhausted),
        "TEST_EXHAUSTED",
    );
}

#[test]
fn executing_link_bounds_and_lookup_refusals_are_fallible() {
    use crate::validation_work::tests::CountingWork;
    let mut value = source();
    value["links"] = json!([link("fixture.report:z"), link("fixture.report:a")]);
    let mut work = CountingWork::new(usize::MAX);
    let product = Product::from_value_with_work(value.clone(), &[facts()], &mut work).unwrap();
    assert_eq!(product.body["links"][0]["name"], "fixture.report:a");
    assert!(work.visits.contains(&Visit::SortMove));
    let mut short = CountingWork::new(work.used - 1);
    error(
        Product::from_value_with_work(value.clone(), &[facts()], &mut short),
        "TEST_EXHAUSTED",
    );
    value["links"][0]["maximum"] = json!("9");
    let mut enough = CountingWork::new(usize::MAX);
    error(
        Product::from_value_with_work(value, &[facts()], &mut enough),
        "LINK_LIMIT",
    );
}

#[test]
fn executing_key_and_selector_helpers_do_not_hide_membership() {
    use crate::validation_work::tests::CountingWork;
    let mut value = source();
    value["links"] = json!([link("fixture.report:previous")]);
    let product = parse(&value).unwrap();
    let mut key_work = CountingWork::new(2);
    assert!(
        product
            .activity_with_work("fixture.report:completed", &mut key_work)
            .unwrap()
            .is_some()
    );
    assert_eq!(key_work.visits, [Visit::RefLookup, Visit::MemberCompare]);
    let mut short_key = CountingWork::new(1);
    error(
        product.unit_with_work("fixture.report:page", &mut short_key),
        "TEST_EXHAUSTED",
    );
    let path = vec!["fixture.report:previous".to_string()];
    let mut probe = CountingWork::new(usize::MAX);
    product
        .validate_selector_path_with_work(
            "fixture.report:completed",
            &path,
            "fixture.report:completed",
            &mut probe,
        )
        .unwrap();
    assert_eq!(
        probe.visits.iter().filter(|v| **v == Visit::Edge).count(),
        1
    );
    let mut exact = CountingWork::new(probe.used);
    product
        .validate_selector_path_with_work(
            "fixture.report:completed",
            &path,
            "fixture.report:completed",
            &mut exact,
        )
        .unwrap();
    let mut short = CountingWork::new(probe.used - 1);
    error(
        product.validate_selector_path_with_work(
            "fixture.report:completed",
            &path,
            "fixture.report:completed",
            &mut short,
        ),
        "TEST_EXHAUSTED",
    );
}

#[test]
fn complete_source_product_preserves_links_schema_clones_and_terminal_refusal() {
    let schemas = [facts()];
    let mut raw = source();
    raw["version"] = json!("0001");
    raw["labels"]["description"] = json!("Unicode ℓ and an escaped \" label");
    raw["links"] = json!([link("fixture.report:prior")]);
    raw["links"][0]["maximum"] = json!("0001");
    let bound =
        Product::normalization_payload_bound_with_work(&raw, &schemas, &mut NoopValidationWork)
            .unwrap();
    assert!(
        bound
            > schemas[0]
                .clone_payload_bound_with_work(&mut NoopValidationWork)
                .unwrap()
    );
    let old =
        Product::from_value_with_work(raw.clone(), &schemas, &mut NoopValidationWork).unwrap();
    let complete =
        Product::from_value_complete_with_work(raw.clone(), &schemas, &mut NoopValidationWork)
            .unwrap();
    assert_eq!(
        old.canonical_bytes().unwrap(),
        complete.canonical_bytes().unwrap()
    );
    assert_eq!(
        old.fact_schema(schemas[0].document_id().unwrap().as_str())
            .unwrap()
            .canonical_value(),
        complete
            .fact_schema(schemas[0].document_id().unwrap().as_str())
            .unwrap()
            .canonical_value()
    );
    assert_eq!(
        old.link("fixture.report:prior").unwrap().maximum(),
        complete.link("fixture.report:prior").unwrap().maximum()
    );
    for selector in [0, 1] {
        let mut invalid = raw.clone();
        if selector == 0 {
            invalid["fact_schemas"][0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        } else {
            let duplicate = invalid["fact_schemas"][0].clone();
            invalid["fact_schemas"]
                .as_array_mut()
                .unwrap()
                .push(duplicate);
        }
        let old = Product::from_value_with_work(invalid.clone(), &schemas, &mut NoopValidationWork)
            .unwrap_err();
        let new =
            Product::from_value_complete_with_work(invalid, &schemas, &mut NoopValidationWork)
                .unwrap_err();
        assert_eq!(old.code, new.code);
    }
    // Present null and malformed ratio input are refused BEFORE ratio serde,
    // exactly as the selected compiler's preexisting unit-terminal check.
    for normalization in [
        Value::Null,
        json!({"to":"fixture.report:page","factor":{"numerator":[],"denominator":"0"}}),
    ] {
        let mut invalid = raw.clone();
        invalid["units"][0]["normalization"] = normalization;
        assert_eq!(
            Product::from_value_complete_with_work(invalid, &schemas, &mut NoopValidationWork)
                .unwrap_err()
                .code,
            "UNSUPPORTED_SCHEMA_PROFILE"
        );
    }
}
