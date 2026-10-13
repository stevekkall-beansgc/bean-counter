use super::*;
use serde_json::json;
fn record(name: &str) -> Value {
    let f:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    f["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap()
        .clone()
}
fn source(name: &str) -> Value {
    let f:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap();
    f["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap()["body"]
        .clone()
}
fn context() -> super::super::FactSchema {
    super::super::FactSchema::parse(&serde_json::to_vec(&source("context")).unwrap()).unwrap()
}
fn product() -> Product {
    let facts =
        super::super::FactSchema::parse(&serde_json::to_vec(&source("facts")).unwrap()).unwrap();
    Product::parse(&serde_json::to_vec(&source("product")).unwrap(), &[facts]).unwrap()
}
fn terms(customer: &str) -> AcceptanceTerms {
    AcceptanceTerms::parse(
        &serde_json::to_vec(&record(&format!("{customer}.terms"))["body"]).unwrap(),
        &context(),
    )
    .unwrap()
}
fn exact(bytes: &[u8], reference: &AgreementRef, r: &Value) {
    assert_eq!(canonical::hex(bytes), r["canonical_hex"]);
    assert_eq!(serde_json::to_value(reference).unwrap(), r["ref"]);
}
#[test]
fn six_selected_constraint_terms_and_assent_bodies_match_reviewed_chain() {
    for customer in ["A", "B"] {
        let slot = record(&format!("{customer}.slot_terms"));
        let s = SlotBindingTerms::parse(&serde_json::to_vec(&slot["body"]).unwrap()).unwrap();
        let role = RoleAssignment::parse(
            &serde_json::to_vec(&record(&format!("{customer}.role"))["body"]).unwrap(),
        )
        .unwrap();
        s.validate_role(&role).unwrap();
        s.validate_product(&product()).unwrap();
        exact(s.canonical_bytes(), s.reference(), &slot);
        let t = terms(customer);
        t.validate_product(&product()).unwrap();
        exact(
            t.canonical_bytes(),
            t.reference(),
            &record(&format!("{customer}.terms")),
        );
        let r = record(&format!("{customer}.assent"));
        let a = AssentRecord::parse(&serde_json::to_vec(&r["body"]).unwrap(), &t).unwrap();
        exact(a.canonical_bytes(), a.reference(), &r);
        assert_eq!(
            t.terms_digest(),
            r["body"]["terms_digest"].as_str().unwrap()
        );
        assert_ne!(t.terms_digest(), t.reference().digest());
    }
}
#[test]
fn terms_context_and_copied_identities_cannot_be_substituted() {
    let mut value = record("A.terms")["body"].clone();
    value["binding_inputs"]["accepted_context"]["facts"] = json!({"fixture.report:unexpected":"1"});
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&value).unwrap(), &context()).is_err());
    value = record("A.terms")["body"].clone();
    value["customer"] = json!("customer-B");
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&value).unwrap(), &context()).is_err());
    value = record("A.terms")["body"].clone();
    value["binding_inputs"]["assent_refs"] = record("A.binding")["body"]["assent_refs"].clone();
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&value).unwrap(), &context()).is_err());
    let facts =
        super::super::FactSchema::parse(&serde_json::to_vec(&source("facts")).unwrap()).unwrap();
    assert!(AcceptanceTerms::parse(
        &serde_json::to_vec(&record("A.terms")["body"]).unwrap(),
        &facts
    )
    .is_err());
}
#[test]
fn quantities_money_and_role_ownership_are_data_constraints_only() {
    let base = record("A.slot_terms")["body"].clone();
    let s = SlotBindingTerms::parse(&serde_json::to_vec(&base).unwrap()).unwrap();
    let wrong =
        RoleAssignment::parse(&serde_json::to_vec(&record("B.role")["body"]).unwrap()).unwrap();
    assert!(s.validate_role(&wrong).is_err());
    for (key, value) in [
        ("unit", json!("fixture.report:wrong")),
        ("maximum", json!("100.5")),
        ("field", json!("fixture.report:missing")),
    ] {
        let mut body = base.clone();
        body["quantity_ceilings"][0][key] = value;
        let parsed = SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert!(parsed.validate_product(&product()).is_err());
    }
    let mut body = base.clone();
    body["exposure_ceiling"]["atoms"] = json!("-1");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).is_err());
    body = base.clone();
    body["quantity_ceilings"] = json!([base["quantity_ceilings"][0], base["quantity_ceilings"][0]]);
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).is_err());
    body = base;
    body["quantity_ceilings"][0]["maximum"] = json!("100.00");
    let parsed = SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert_eq!(
        canonical::hex(parsed.canonical_bytes()),
        record("A.slot_terms")["canonical_hex"]
    );
}
#[test]
fn half_open_binding_times_and_future_assent_do_not_manufacture_authority() {
    let mut value = record("A.terms")["body"].clone();
    value["binding_inputs"]["ends_at"] = value["binding_inputs"]["starts_at"].clone();
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&value).unwrap(), &context()).is_err());
    value = record("A.terms")["body"].clone();
    value["binding_inputs"]["starts_at"] = json!("2027-01-01T00:00:00.000000Z");
    let future = AcceptanceTerms::parse(&serde_json::to_vec(&value).unwrap(), &context()).unwrap();
    let mut assent = record("A.assent")["body"].clone();
    assent["terms_digest"] = json!(future.terms_digest());
    AssentRecord::parse(&serde_json::to_vec(&assent).unwrap(), &future).unwrap();
    assert!(AssentRecord::parse(&serde_json::to_vec(&assent).unwrap(), &terms("A")).is_err());
    assent["accepted_at"] = json!("2026-02-30T00:00:00.000000Z");
    assert!(AssentRecord::parse(&serde_json::to_vec(&assent).unwrap(), &future).is_err());
}
#[test]
fn new_data_codecs_reject_null_unknown_nested_fields_and_decoded_duplicate_keys() {
    let body = record("A.terms")["body"].clone();
    let mut bad = body.clone();
    bad["binding_inputs"]["ends_at"] = Value::Null;
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&bad).unwrap(), &context()).is_err());
    bad = body.clone();
    bad["binding_inputs"]["funding_legs"][0]["automatic_permission"] = json!(true);
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&bad).unwrap(), &context()).is_err());
    let raw = serde_json::to_string(&body).unwrap().replacen(
        "{",
        "{\"cust\\u006fmer\":\"duplicate\",",
        1,
    );
    assert!(AcceptanceTerms::parse(raw.as_bytes(), &context()).is_err());
    let mut bad = record("A.assent")["body"].clone();
    bad["mode"] = json!("trusted");
    assert!(AssentRecord::parse(&serde_json::to_vec(&bad).unwrap(), &terms("A")).is_err());
}

#[test]
fn eighteen_reviewed_quantity_vectors_check_exact_original_type_and_unit() {
    let fixture: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/quantity-ceilings-fixture-input.json")).unwrap();
    // Build typed operands from the literal descriptor input. Expected results
    // are the independently reviewed literal vectors, never product output.
    let fields = fixture["activity_fields"]["fixture:a"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, def)| {
            let mut ty = def.clone();
            ty.as_object_mut().unwrap().remove("required");
            if let Some(scale) = ty.get_mut("max_scale") {
                *scale = json!(scale.as_u64().unwrap().to_string());
            }
            json!({"name":name,"required":def["required"],"type":ty,"provenance":{"kind":"caller"}})
        })
        .collect::<Vec<_>>();
    let schema = super::super::FactSchema::parse(&serde_json::to_vec(&json!({"schema":"ledger-workflow-fact-schema/1","schema_id":"fixture:quantities","version":"1","normalization":"ledger-facts-normalization/1","fields":fields})).unwrap()).unwrap();
    let r = json!({"kind":"workflow-fact-schema","id":schema.document_id().unwrap(),"digest":schema.content_digest().unwrap()});
    let mut p = source("product");
    p["units"] = json!([
        {"name":"fixture:item","labels":{"title":"Quantity fixture"},"dimension":"fixture:count","normalization":{"to":"fixture:terminal","factor":{"numerator":"1","denominator":"1"}}},
        {"name":"fixture:other","labels":{"title":"Quantity fixture"},"dimension":"fixture:count"},
        {"name":"fixture:terminal","labels":{"title":"Quantity fixture"},"dimension":"fixture:count"}
    ]);
    p["activities"] = json!(["fixture:a","fixture:b"].iter().map(|name| json!({"name":name,"labels":{"title":"Quantity fixture"},"primitive":"work","fact_schema":r,"source_slot":"fixture:worker"})).collect::<Vec<_>>());
    p["fact_schemas"] = json!([r]);
    let p = Product::parse(&serde_json::to_vec(&p).unwrap(), &[schema]).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 18);
    for case in fixture["cases"].as_array().unwrap() {
        let mut entries: Vec<QuantityCeiling> =
            serde_json::from_value(case["entries"].clone()).unwrap();
        let result =
            quantities(&mut entries).and_then(|()| validate_product_quantities(&entries, &p));
        if let Some(error) = case["error"].as_str() {
            assert_eq!(result.unwrap_err().code, error, "{}", case["name"]);
        } else {
            result.unwrap();
            assert_eq!(
                serde_json::to_value(entries).unwrap(),
                case["normalized"],
                "{}",
                case["name"]
            );
        }
    }
}

#[test]
fn terms_enforce_raw_and_quantity_count_bounds_and_normalize_counters() {
    let base = record("A.slot_terms")["body"].clone();
    let mut body = base.clone();
    body["quantity_ceilings"] = json!((0..64).map(|i| json!({"activity":"fixture.report:completed","field":format!("fixture.report:q{i}"),"unit":"fixture.report:page","maximum":"0"})).collect::<Vec<_>>());
    SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    let mut extra = body["quantity_ceilings"][0].clone();
    extra["field"] = json!("fixture.report:extra");
    body["quantity_ceilings"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).is_err());
    let mut raw = serde_json::to_vec(&base).unwrap();
    raw.resize(262144, b' ');
    SlotBindingTerms::parse(&raw).unwrap();
    raw.push(b' ');
    assert!(SlotBindingTerms::parse(&raw).is_err());
    let mut body = record("A.terms")["body"].clone();
    body["version"] = json!("0001");
    body["binding_inputs"]["version"] = json!("01");
    let t = AcceptanceTerms::parse(&serde_json::to_vec(&body).unwrap(), &context()).unwrap();
    assert_eq!(
        canonical::hex(t.canonical_bytes()),
        record("A.terms")["canonical_hex"]
    );
    let mut second = body["binding_inputs"]["exposure_ceilings"][0].clone();
    second["maximum"]["atoms"] = json!("1");
    body["binding_inputs"]["exposure_ceilings"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert_eq!(
        AcceptanceTerms::parse(&serde_json::to_vec(&body).unwrap(), &context())
            .unwrap_err()
            .code,
        "INVALID_SHAPE"
    );
}

#[test]
fn eight_reviewed_exposure_cases_keep_exact_bounded_amounts_and_jcs_order() {
    fn entry(slot: &str, atoms: &str) -> Value {
        json!({"binding_slot":slot,"maximum":{"currency":"USD","scale":18,"atoms":atoms}})
    }
    let a = entry("fixture:a", "10");
    let b = entry("fixture:b", "10");
    let cases = vec![
        (json!([]), Ok(json!([]))),
        (json!([b, a]), Ok(json!([a, b]))),
        (json!([a, a]), Err("INVALID_SHAPE")),
        (json!([a, entry("fixture:a", "11")]), Err("INVALID_SHAPE")),
        (json!([entry("fixture:a", "-1")]), Err("INVALID_SCALAR")),
        (json!([entry("fixture:a", "01")]), Err("INVALID_SCALAR")),
        (
            json!([entry("fixture:a", &"9".repeat(30))]),
            Ok(json!([entry("fixture:a", &"9".repeat(30))])),
        ),
        (
            json!([entry("fixture:a", &format!("1{}", "0".repeat(30)))]),
            Err("INVALID_SCALAR"),
        ),
    ];
    assert_eq!(cases.len(), 8);
    for (raw, expected) in cases {
        let mut entries: Vec<Exposure> = serde_json::from_value(raw).unwrap();
        let result = exposures(&mut entries);
        match expected {
            Ok(normalized) => {
                result.unwrap();
                assert_eq!(serde_json::to_value(entries).unwrap(), normalized);
            }
            Err(error) => assert_eq!(result.unwrap_err().code, error),
        }
    }
}
