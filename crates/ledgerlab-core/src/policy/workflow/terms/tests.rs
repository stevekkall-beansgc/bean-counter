use super::*;
use serde_json::json;
fn record(name: &str) -> Value {
    let f: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json"
    ))
    .unwrap();
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
    assert!(
        AcceptanceTerms::parse(
            &serde_json::to_vec(&record("A.terms")["body"]).unwrap(),
            &facts
        )
        .is_err()
    );
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

#[test]
fn causal_terms_v2_are_closed_sorted_and_do_not_upgrade_v1() {
    let old = record("A.slot_terms")["body"].clone();
    let clause = json!({"root_component_slot":"root-a", "family_slot":"family-a",
        "correction_source_slot":"fixture:corrector", "mode":"exact_inverse_and_permanent_close",
        "standalone_deadline_effect":"does_not_expire_preaccepted_causal_reversal"});
    let mut body = old.clone();
    body["schema"] = json!("ledger-workflow-slot-binding-terms/2");
    body["causal_reversal_clauses"] = json!([clause]);
    let normalized = SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert_eq!(normalized.canonical_value(), &body);
    assert_ne!(
        normalized.reference(),
        SlotBindingTerms::parse(&serde_json::to_vec(&old).unwrap())
            .unwrap()
            .reference()
    );
    let mut variants = vec![];
    let mut v = body.clone();
    v.as_object_mut().unwrap().remove("causal_reversal_clauses");
    variants.push(v);
    let mut v = body.clone();
    v["causal_reversal_clauses"] = json!([]);
    variants.push(v);
    let mut v = body.clone();
    v["causal_reversal_clauses"] = json!([clause, clause]);
    variants.push(v);
    let mut v = body.clone();
    v["causal_reversal_clauses"][0]["mode"] = json!("delegate");
    variants.push(v);
    let mut v = body.clone();
    v["causal_reversal_clauses"][0]["unknown"] = json!(true);
    variants.push(v);
    let mut v = body.clone();
    v["schema"] = old["schema"].clone();
    variants.push(v);
    for invalid in variants {
        assert!(SlotBindingTerms::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    let mut other = clause.clone();
    other["family_slot"] = json!("family-b");
    body["causal_reversal_clauses"] = json!([other, clause]);
    let a = SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    body["causal_reversal_clauses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let b = SlotBindingTerms::parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
}

#[test]
fn original_work_terms_v3_match_frozen_bytes_and_do_not_reinterpret_old_terms() {
    let input:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/exposure-scope-01/fixture-input.json")).unwrap();
    let expected:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/exposure-scope-01/fixture-author-expected.json")).unwrap();
    let body = &input["terms3_body"];
    let parsed = SlotBindingTerms::parse(&serde_json::to_vec(body).unwrap()).unwrap();
    exact(
        parsed.canonical_bytes(),
        parsed.reference(),
        &expected["terms3"],
    );
    let old = SlotBindingTerms::parse(&serde_json::to_vec(&input["terms2_body"]).unwrap()).unwrap();
    exact(
        old.canonical_bytes(),
        old.reference(),
        &expected["old_terms2"],
    );
    assert_ne!(parsed.reference(), old.reference());
    let mut absent = body.clone();
    absent.as_object_mut().unwrap().remove("exposure_scope");
    let mut wrong = body.clone();
    wrong["exposure_scope"] = json!("agreement_lifetime");
    let mut supplier = body.clone();
    supplier["book"] = json!("supplier");
    let mut v2 = body.clone();
    v2["schema"] = json!("ledger-workflow-slot-binding-terms/2");
    let mut v1 = body.clone();
    v1["schema"] = json!("ledger-workflow-slot-binding-terms/1");
    v1.as_object_mut()
        .unwrap()
        .remove("causal_reversal_clauses");
    for invalid in [absent, wrong, supplier, v2, v1] {
        assert!(SlotBindingTerms::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    // Empty shape is legal only in /3. Host policy coverage separately proves
    // no promised family was omitted; this parser creates no authority.
    let mut empty = body.clone();
    empty["causal_reversal_clauses"] = json!([]);
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&empty).unwrap()).is_ok());
    empty
        .as_object_mut()
        .unwrap()
        .remove("causal_reversal_clauses");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&empty).unwrap()).is_err());
}

#[test]
fn supplier_terms_and_descriptor_are_additive_data_not_legacy_support() {
    let frozen:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/owner-local-cost-01/fixture-author-expected.json")).unwrap();
    let offer = &frozen["records"]["offer"];
    let parsed_offer = SupplierOffer::parse(
        canonical::CanonicalBytes::from_value(&offer["body"])
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    exact(
        parsed_offer.canonical_bytes(),
        parsed_offer.reference(),
        offer,
    );
    let mut duplicate_offer = offer["body"].clone();
    duplicate_offer["source_slots"]
        .as_array_mut()
        .unwrap()
        .push(offer["body"]["source_slots"][0].clone());
    assert!(
        SupplierOffer::parse(
            canonical::CanonicalBytes::from_value(&duplicate_offer)
                .unwrap()
                .as_slice()
        )
        .is_err()
    );
    let row = &frozen["records"]["supplier_terms"];
    let raw = canonical::CanonicalBytes::from_value(&row["body"]).unwrap();
    let parsed = SlotBindingTerms::parse(raw.as_slice()).unwrap();
    exact(parsed.canonical_bytes(), parsed.reference(), row);
    for (field, value) in [
        ("consumption_basis", json!("cumulative_positive_deltas")),
        ("book", json!("retail")),
        ("schema", json!("ledger-workflow-slot-binding-terms/3")),
        (
            "offer_ref",
            json!({"kind":"workflow-policy","id":row["body"]["offer_ref"]["id"],"digest":row["body"]["offer_ref"]["digest"]}),
        ),
        ("unknown", json!(true)),
    ] {
        let mut bad = row["body"].clone();
        bad[field] = value;
        assert!(
            SlotBindingTerms::parse(
                canonical::CanonicalBytes::from_value(&bad)
                    .unwrap()
                    .as_slice()
            )
            .is_err()
        );
    }
    let old = record("A.slot_terms");
    let old_parsed = SlotBindingTerms::parse(&serde_json::to_vec(&old["body"]).unwrap()).unwrap();
    exact(old_parsed.canonical_bytes(), old_parsed.reference(), &old);
    let mut old_with_supplier = old["body"].clone();
    old_with_supplier["consumption_basis"] = json!("owned_obligation_high_water");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&old_with_supplier).unwrap()).is_err());
    let support = super::super::VerifiedCompilerSupport::builtin().unwrap();
    let old_descriptor = support.descriptor().canonical_bytes().to_vec();
    let mut descriptor = support.descriptor().canonical_value().clone();
    descriptor["schema"] = json!("ledger-workflow-compiler-descriptor/3");
    descriptor["compiler_semantics"] = json!("ledger-policy-compiler/owner-local-cost-1");
    descriptor["plan_schema"] = json!("ledger-workflow-compiled-plan/3");
    descriptor["primitive_versions"] = json!([
        "unit/1",
        "family_no_success_close/1",
        "outcome_family_binding_dependency/1",
        "owner_local_supplier_cost/1",
        "owned_obligation_high_water/1"
    ]);
    canonical::sort_set(descriptor["primitive_versions"].as_array_mut().unwrap()).unwrap();
    let raw = canonical::CanonicalBytes::from_value(&descriptor).unwrap();
    let d = super::super::CompilerDescriptor::parse(raw.as_slice()).unwrap();
    assert_eq!(d.canonical_bytes(), raw.as_slice());
    assert!(super::super::normalize_definition(raw.as_slice(), &[], &support).is_err());
    assert_eq!(support.descriptor().canonical_bytes(), old_descriptor);
    descriptor["plan_schema"] = json!("ledger-workflow-compiled-plan/2");
    assert!(
        super::super::CompilerDescriptor::parse(
            canonical::CanonicalBytes::from_value(&descriptor)
                .unwrap()
                .as_slice()
        )
        .is_err()
    );
}

#[test]
fn scoped_supplier_consent_data_preserves_old_offer_and_requires_exact_new_versions() {
    let frozen: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/supplier-scoped-consent-01/fixture-author-expected.json")).unwrap();
    let offer = &frozen["records"]["offer"];
    let bytes = canonical::CanonicalBytes::from_value(&offer["body"]).unwrap();
    let parsed = SupplierOffer::parse(bytes.as_slice()).unwrap();
    exact(parsed.canonical_bytes(), parsed.reference(), offer);
    for (key, value) in [
        ("schema", json!("ledger-workflow-offer/2")),
        ("policy_ref", source("policy")["product_ref"].clone()),
        ("commitment_ref", Value::Null),
    ] {
        let mut bad = offer["body"].clone();
        bad[key] = value;
        assert!(
            SupplierOffer::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{key}"
        );
    }
    let terms = &frozen["records"]["supplier_terms"];
    let parsed = SlotBindingTerms::parse(&serde_json::to_vec(&terms["body"]).unwrap()).unwrap();
    exact(parsed.canonical_bytes(), parsed.reference(), terms);
    for (key, value) in [
        ("schema", json!("ledger-workflow-slot-binding-terms/4")),
        (
            "commitment_ref",
            json!({"kind":"workflow-policy","id":terms["body"]["commitment_ref"]["id"],"digest":terms["body"]["commitment_ref"]["digest"]}),
        ),
        ("consumption_basis", json!("cumulative_positive_deltas")),
    ] {
        let mut bad = terms["body"].clone();
        bad[key] = value;
        assert!(
            SlotBindingTerms::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut missing = terms["body"].clone();
    missing.as_object_mut().unwrap().remove("commitment_ref");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    // An old offer keeps its policy-bound byte recipe; a new field never
    // silently changes the scope of that already accepted assertion.
    let old: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/owner-local-cost-01/fixture-author-expected.json")).unwrap();
    let old_offer = &old["records"]["offer"];
    let parsed = SupplierOffer::parse(&serde_json::to_vec(&old_offer["body"]).unwrap()).unwrap();
    exact(parsed.canonical_bytes(), parsed.reference(), old_offer);
    let mut bad = old_offer["body"].clone();
    bad["commitment_ref"] = offer["body"]["commitment_ref"].clone();
    assert!(SupplierOffer::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    let support = super::super::VerifiedCompilerSupport::builtin().unwrap();
    let mut descriptor = support.descriptor().canonical_value().clone();
    descriptor["schema"] = json!("ledger-workflow-compiler-descriptor/4");
    descriptor["compiler_semantics"] = json!("ledger-policy-compiler/supplier-scoped-consent-1");
    descriptor["plan_schema"] = json!("ledger-workflow-compiled-plan/4");
    descriptor["primitive_versions"] = json!([
        "unit/1",
        "family_no_success_close/1",
        "outcome_family_binding_dependency/1",
        "owner_local_supplier_cost/1",
        "owned_obligation_high_water/1",
        "supplier-semantic-projection/1"
    ]);
    canonical::sort_set(descriptor["primitive_versions"].as_array_mut().unwrap()).unwrap();
    let raw = canonical::CanonicalBytes::from_value(&descriptor).unwrap();
    let parsed = super::super::CompilerDescriptor::parse(raw.as_slice()).unwrap();
    assert_eq!(parsed.canonical_bytes(), raw.as_slice());
    assert!(super::super::normalize_definition(raw.as_slice(), &[], &support).is_err());
    for (field, wrong) in [
        (
            "compiler_semantics",
            "ledger-policy-compiler/owner-local-cost-1",
        ),
        ("plan_schema", "ledger-workflow-compiled-plan/3"),
    ] {
        let mut bad = descriptor.clone();
        bad[field] = json!(wrong);
        assert!(
            super::super::CompilerDescriptor::parse(&serde_json::to_vec(&bad).unwrap()).is_err()
        );
    }
    descriptor["primitive_versions"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v != "supplier-semantic-projection/1");
    assert!(
        super::super::CompilerDescriptor::parse(&serde_json::to_vec(&descriptor).unwrap()).is_err()
    );
}

#[test]
fn app_terms_data_requires_new_consent_recipe_and_never_widens_old_terms() {
    let frozen:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/app-owned-cost-01/fixture-expected.json")).unwrap();
    let slot = &frozen["records"]["app_terms"];
    let parsed = SlotBindingTerms::parse(&serde_json::to_vec(&slot["body"]).unwrap()).unwrap();
    exact(parsed.canonical_bytes(), parsed.reference(), slot);
    for (field, value) in [
        ("schema", json!("ledger-workflow-slot-binding-terms/3")),
        ("exposure_scope", json!("binding_lifetime")),
        ("book", json!("supplier")),
        ("offer_ref", record("A.slot_terms")["ref"].clone()),
    ] {
        let mut bad = slot["body"].clone();
        bad[field] = value;
        assert!(
            SlotBindingTerms::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("cost_basis", json!("supplier_final_cost")),
        ("allocation", json!("implicit_split")),
        (
            "maximum_final_amount",
            json!({"currency":"USD","scale":18,"atoms":"-1"}),
        ),
    ] {
        let mut bad = slot["body"].clone();
        bad["app_cost_terms"][0][field] = value;
        assert!(
            SlotBindingTerms::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{field}"
        );
    }
    let mut missing = slot["body"].clone();
    missing.as_object_mut().unwrap().remove("app_cost_terms");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    // Only the additive acceptance schema may nominate the two app kinds.
    let mut body = record("A.terms")["body"].clone();
    body["schema"] = json!("ledger-workflow-acceptance-terms/2");
    body["binding_inputs"]["source_slots"][0]["evidence_kinds"] =
        json!(["app_usage", "app_final_cost"]);
    let app = AcceptanceTerms::parse(&serde_json::to_vec(&body).unwrap(), &context()).unwrap();
    body["schema"] = json!("ledger-workflow-acceptance-terms/1");
    assert!(AcceptanceTerms::parse(&serde_json::to_vec(&body).unwrap(), &context()).is_err());
    assert_ne!(app.terms_digest(), terms("A").terms_digest());
    let legacy = super::super::VerifiedCompilerSupport::builtin().unwrap();
    let mut descriptor = legacy.descriptor().canonical_value().clone();
    descriptor["schema"] = json!("ledger-workflow-compiler-descriptor/5");
    descriptor["compiler_semantics"] = json!("ledger-policy-compiler/app-owned-cost-1");
    descriptor["plan_schema"] = json!("ledger-workflow-compiled-plan/5");
    let raw = canonical::CanonicalBytes::from_value(&descriptor).unwrap();
    let parsed = super::super::CompilerDescriptor::parse(raw.as_slice()).unwrap();
    assert_eq!(parsed.canonical_bytes(), raw.as_slice());
    assert!(super::super::normalize_definition(raw.as_slice(), &[], &legacy).is_err());
    descriptor["plan_schema"] = json!("ledger-workflow-compiled-plan/4");
    assert!(
        super::super::CompilerDescriptor::parse(&serde_json::to_vec(&descriptor).unwrap()).is_err()
    );
}

#[test]
fn app_terms7_keeps_explicit_anchor_and_optional_exact_causal_clause_separate_from_terms6() {
    let frozen:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/app-cost-causal-01/fixture-expected.json")).unwrap();
    let row = &frozen["records"]["app_terms"];
    let parsed = SlotBindingTerms::parse(&serde_json::to_vec(&row["body"]).unwrap()).unwrap();
    exact(parsed.canonical_bytes(), parsed.reference(), row);
    for (field, value) in [
        ("correction_anchor", json!("latest_cost_accepted_at")),
        (
            "causal_reversal",
            json!({"root_relation":"outcome_family","mode":"exact_inverse_and_permanent_close","standalone_deadline_effect":"does_not_expire_preaccepted_causal_reversal"}),
        ),
    ] {
        let mut bad = row["body"].clone();
        bad["app_cost_terms"][0][field] = value;
        assert!(SlotBindingTerms::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut missing = row["body"].clone();
    missing["app_cost_terms"][0]
        .as_object_mut()
        .unwrap()
        .remove("correction_anchor");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    let mut absent = row["body"].clone();
    absent["app_cost_terms"][0]
        .as_object_mut()
        .unwrap()
        .remove("causal_reversal");
    let parsed = SlotBindingTerms::parse(&serde_json::to_vec(&absent).unwrap()).unwrap();
    assert!(
        parsed.canonical_value()["app_cost_terms"][0]
            .get("causal_reversal")
            .is_none()
    );
    let mut old = row["body"].clone();
    old["schema"] = json!("ledger-workflow-slot-binding-terms/6");
    assert!(SlotBindingTerms::parse(&serde_json::to_vec(&old).unwrap()).is_err());
}
