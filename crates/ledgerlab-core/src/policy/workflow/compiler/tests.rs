use super::*;

#[test]
fn family_close_descriptor_data_is_exact_and_does_not_widen_legacy_support() {
    let (_, _, legacy) = input();
    let original = legacy.descriptor().canonical_bytes().to_vec();
    let mut body = legacy.descriptor().canonical_value().clone();
    body["schema"] = json!("ledger-workflow-compiler-descriptor/2");
    body["compiler_semantics"] = json!("ledger-policy-compiler/family-close-1");
    body["plan_schema"] = json!("ledger-workflow-compiled-plan/2");
    body["primitive_versions"] = json!([
        "unit/1",
        "outcome_family_binding_dependency/1",
        "family_no_success_close/1"
    ]);
    let bytes = CanonicalBytes::from_value(&body).unwrap();
    let parsed = CompilerDescriptor::parse(bytes.as_slice()).unwrap();
    assert_eq!(parsed.canonical_value()["schema"], body["schema"]);
    assert_ne!(parsed.reference(), legacy.descriptor().reference());
    assert!(normalize_definition(bytes.as_slice(), &[], &legacy).is_err());
    assert_eq!(
        CompilerDescriptor::parse(&original)
            .unwrap()
            .canonical_bytes(),
        original
    );
    for (field, wrong) in [
        ("compiler_semantics", json!("ledger-policy-compiler/1")),
        ("plan_schema", json!("ledger-workflow-compiled-plan/1")),
        ("primitive_versions", json!(["family_no_success_close/1"])),
        (
            "primitive_versions",
            json!([
                "family_no_success_close/1",
                "outcome_family_binding_dependency/1",
                "unit/1",
                "unit/1"
            ]),
        ),
        ("unknown", json!(true)),
    ] {
        let mut bad = body.clone();
        bad[field] = wrong;
        assert!(
            CompilerDescriptor::parse(CanonicalBytes::from_value(&bad).unwrap().as_slice())
                .is_err()
        );
    }
}

// Independently derived and reviewed PUB-ENC-01 snapshot. Test inputs are the
// sealed source bodies, not production output or author fixture generation.
fn fixture() -> Value {
    serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap()
}
fn vector(name: &str) -> Value {
    fixture()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap()
        .clone()
}
fn input() -> (Vec<Vec<u8>>, DefinitionRef, VerifiedCompilerSupport) {
    let documents: Vec<_> = fixture()["vectors"].as_array().unwrap()[..6]
        .iter()
        .map(|v| {
            v["canonical_body_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect();
    let root = DefinitionRef::parse(
        CanonicalBytes::from_value(&vector("policy")["ref"])
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let descriptor = CompilerDescriptor::parse(
        vector("compiler")["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    (
        documents,
        root,
        VerifiedCompilerSupport::fixture(descriptor).unwrap(),
    )
}
fn plan_digest() -> String {
    vector("unit-plan")["ref"]["digest"]
        .as_str()
        .unwrap()
        .into()
}
fn error<T: std::fmt::Debug>(result: Result<T>, code: &str) {
    assert_eq!(result.unwrap_err().code, code);
}

#[test]
fn selected_compiler_reproduces_every_independent_plan_byte() {
    let (documents, root, support) = input();
    let result = compile_publication(&documents, &root, &plan_digest(), &support).unwrap();
    let expected = vector("unit-plan");
    assert_eq!(
        result.compiled_plan().canonical_bytes(),
        expected["canonical_body_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        serde_json::to_value(result.compiled_plan().reference()).unwrap(),
        expected["ref"]
    );
    assert_eq!(result.source_documents().len(), 6);
    assert_eq!(result.root_policy_ref(), &root);
    assert_eq!(result.descriptor_refs().unwrap().len(), 3);
    for actual in result.source_documents() {
        let expected = fixture()["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["ref"]["id"] == actual.id())
            .unwrap()
            .clone();
        assert_eq!(
            actual.canonical_bytes(),
            expected["canonical_body_utf8"].as_str().unwrap().as_bytes()
        );
    }
}

#[test]
fn lexical_equivalence_normalizes_without_current_heads_or_plan_compilation() {
    let (mut documents, root, support) = input();
    documents.reverse();
    for bytes in &mut documents {
        let mut value = canonical::parse(bytes).unwrap();
        if value["schema"] == "ledger-workflow-policy/1" {
            value["version"] = json!("0001");
            value["rules"][0]["formula"]["rate"] = json!("00.3700");
        }
        *bytes = serde_json::to_vec_pretty(&value).unwrap();
    }
    let normalized = normalize_publication_sources(&documents, &root).unwrap();
    assert_eq!(normalized.root_policy_ref(), &root);
    let actual = compile_publication(&documents, &root, &plan_digest(), &support).unwrap();
    assert_eq!(
        actual.compiled_plan().id(),
        vector("unit-plan")["ref"]["id"].as_str().unwrap()
    );
}

#[test]
fn strict_raw_source_boundary_preserves_lexical_refusals() {
    let (documents, root, _) = input();
    for bad in [
        br#"{"schema":"ledger-workflow-fact-schema/1","\u0073chema":"ledger-workflow-fact-schema/1"}"#.as_slice(),
        b"\xef\xbb\xbf{}",br#"{"schema":"ledger-workflow-policy/1","scale":1e1}"#,
        br#"{"schema":"ledger-workflow-policy/1","scale":9007199254740992}"#,
        br#"{"schema":"ledger-workflow-policy/1","scale":-0}"#,
    ] {
        let mut wrong=documents.clone();wrong[0]=bad.to_vec();assert!(normalize_publication_sources(&wrong,&root).is_err());
    }
    let mut wrong = documents.clone();
    let mut value = canonical::parse(&wrong[0]).unwrap();
    value["unexpected"] = json!(true);
    wrong[0] = serde_json::to_vec(&value).unwrap();
    assert!(normalize_publication_sources(&wrong, &root).is_err());
    let mut wrong = documents.clone();
    let mut value = canonical::parse(&wrong[5]).unwrap();
    value["rules"][0]["formula"]["amount_override"] = json!("1");
    wrong[5] = serde_json::to_vec(&value).unwrap();
    assert!(normalize_publication_sources(&wrong, &root).is_err());
    let mut wrong = documents;
    let mut value = canonical::parse(&wrong[0]).unwrap();
    value["fields"][0]["type"]["unit"] = Value::Null;
    wrong[0] = serde_json::to_vec(&value).unwrap();
    assert!(normalize_publication_sources(&wrong, &root).is_err());
}

#[test]
fn extra_or_missing_definitions_and_private_data_cannot_be_published() {
    let (documents, root, _) = input();
    let mut missing = documents.clone();
    missing.remove(0);
    error(
        normalize_publication_sources(&missing, &root),
        "DEFINITION_MISSING",
    );
    let mut extra = documents.clone();
    let mut value = canonical::parse(&documents[0]).unwrap();
    value["schema_id"] = json!("fixture.report:unused");
    extra.push(serde_json::to_vec(&value).unwrap());
    error(
        normalize_publication_sources(&extra, &root),
        "DEFINITION_EXTRA",
    );
    extra = documents;
    extra.push(br#"{"schema":"ledger-workflow-binding/1","customer":"fake"}"#.to_vec());
    error(
        normalize_publication_sources(&extra, &root),
        "UNSUPPORTED_SCHEMA_PROFILE",
    );
}

#[test]
fn only_separately_verified_support_and_expected_plan_are_accepted() {
    let (documents, root, support) = input();
    error(
        compile_publication(
            &documents,
            &root,
            &format!("sha256:{}", "0".repeat(64)),
            &support,
        ),
        "PLAN_DIGEST_MISMATCH",
    );
    let mut value = support.descriptor().canonical_value().clone();
    value["limits_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    let untrusted = CompilerDescriptor::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    error(
        VerifiedCompilerSupport::fixture(untrusted),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
}

#[test]
fn unsupported_modes_operators_and_ancestry_fail_closed() {
    let (documents, root, _) = input();
    for (field, replacement) in [
        ("mode", json!("closure_share")),
        ("families", json!([{}])),
        ("previous", serde_json::to_value(&root).unwrap()),
    ] {
        let mut wrong = documents.clone();
        let mut policy = canonical::parse(&wrong[5]).unwrap();
        policy[field] = replacement;
        wrong[5] = serde_json::to_vec(&policy).unwrap();
        error(
            normalize_publication_sources(&wrong, &root),
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
        );
    }
    for op in ["false", "eq", "custom"] {
        let mut wrong = documents.clone();
        let mut policy = canonical::parse(&wrong[5]).unwrap();
        policy["rules"][0]["when"]["op"] = json!(op);
        wrong[5] = serde_json::to_vec(&policy).unwrap();
        error(
            normalize_publication_sources(&wrong, &root),
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
        );
    }
}

#[test]
fn duplicate_ingress_is_rejected_before_shared_dependency_resolution() {
    let (documents, root, _) = input();
    for duplicate in &documents {
        let mut wrong = documents.clone();
        wrong.push(duplicate.clone());
        error(
            normalize_publication_sources(&wrong, &root),
            "DEFINITION_DUPLICATE",
        );
        let mut value = canonical::parse(duplicate).unwrap();
        if value["schema"] == "ledger-workflow-policy/1" {
            value["rules"][0]["formula"]["rate"] = json!("00.3700");
        }
        let mut wrong = documents.clone();
        wrong.push(serde_json::to_vec_pretty(&value).unwrap());
        error(
            normalize_publication_sources(&wrong, &root),
            "DEFINITION_DUPLICATE",
        );
    }
}

fn replace_ref(value: &mut Value, old: &Value, new: &Value) {
    if value == old {
        *value = new.clone();
        return;
    }
    match value {
        Value::Object(values) => {
            for child in values.values_mut() {
                replace_ref(child, old, new);
            }
        }
        Value::Array(values) => {
            for child in values {
                replace_ref(child, old, new);
            }
        }
        _ => (),
    }
}
fn changed_input(mut bodies: Vec<Value>) -> (Vec<Vec<u8>>, DefinitionRef) {
    // Construct linked test inputs bottom-up. These hashes are input references,
    // never expected plan bytes or expected economic answers.
    for index in [0usize, 2, 4, 5] {
        let old = vector(
            [
                "facts", "context", "template", "compiler", "product", "policy",
            ][index],
        )["ref"]
            .clone();
        let kind = old["kind"].as_str().unwrap();
        if index == 4 {
            let schemas = [
                FactSchema::parse(CanonicalBytes::from_value(&bodies[0]).unwrap().as_slice())
                    .unwrap(),
                FactSchema::parse(CanonicalBytes::from_value(&bodies[1]).unwrap().as_slice())
                    .unwrap(),
            ];
            bodies[index] = Product::parse(
                CanonicalBytes::from_value(&bodies[index])
                    .unwrap()
                    .as_slice(),
                &schemas,
            )
            .unwrap()
            .canonical_value()
            .clone();
        }
        let new = serde_json::to_value(
            DefinitionDocument::document(kind, bodies[index].clone())
                .unwrap()
                .reference(),
        )
        .unwrap();
        for downstream in bodies.iter_mut().skip(index + 1) {
            replace_ref(downstream, &old, &new);
        }
    }
    let root = DefinitionDocument::document("workflow-policy", bodies[5].clone())
        .unwrap()
        .reference()
        .clone();
    (
        bodies
            .into_iter()
            .map(|value| CanonicalBytes::from_value(&value).unwrap().into_vec())
            .collect(),
        root,
    )
}

#[test]
fn static_exposure_proof_checks_sum_of_simultaneous_retail_roots() {
    let (documents, _, support) = input();
    let mut bodies: Vec<_> = documents
        .iter()
        .map(|b| canonical::parse(b).unwrap())
        .collect();
    bodies[0]["fields"][0]["type"]["max"] = json!("600000000000");
    bodies[5]["rules"][0]["formula"]["rate"] = json!("1");
    let (one, root) = changed_input(bodies.clone());
    // Independent reviewed expectation:6e29 fits one component but the new
    // source changes the plan, so the historical digest cannot match.
    error(
        compile_publication(&one, &root, &plan_digest(), &support),
        "PLAN_DIGEST_MISMATCH",
    );
    let mut component = bodies[2]["components"][0].clone();
    component["key"] = json!("fixture.report:second");
    component["root_key"] = json!("fixture.report:second");
    component["aliases"] = json!(["fixture.report:second"]);
    bodies[2]["components"]
        .as_array_mut()
        .unwrap()
        .push(component);
    let mut rule = bodies[5]["rules"][0].clone();
    rule["rule_id"] = json!("fixture.report:second-charge");
    rule["component_name"] = json!("fixture.report:second");
    rule["component_key"] = json!("fixture.report:second");
    bodies[5]["rules"].as_array_mut().unwrap().push(rule);
    let (two, root) = changed_input(bodies);
    // Each6e29 fits individually;1.2e30 violates MAX_ATOMS before plan acceptance.
    error(
        compile_publication(&two, &root, &plan_digest(), &support),
        "ARITHMETIC_OVERFLOW",
    );
}

#[test]
fn symbolic_role_names_and_optional_activity_funding_are_not_invented_aliases() {
    let (documents, _, support) = input();
    let mut bodies: Vec<_> = documents
        .iter()
        .map(|b| canonical::parse(b).unwrap())
        .collect();
    bodies[2]["components"][0]["role_slot"] = json!("fixture.report:independent-role");
    bodies[4]["activities"][0]
        .as_object_mut()
        .unwrap()
        .remove("funding_leg");
    let (changed, root) = changed_input(bodies.clone());
    // Static source checks succeed; changed descriptor/source bytes need a new
    // plan digest. Actual role/funding resolution is later binding authority.
    error(
        compile_publication(&changed, &root, &plan_digest(), &support),
        "PLAN_DIGEST_MISMATCH",
    );
    bodies[4]["activities"][0]["funding_leg"] = json!("fixture.report:wrong-leg");
    let (changed, root) = changed_input(bodies);
    error(
        compile_publication(&changed, &root, &plan_digest(), &support),
        "PROGRAM_BINDING",
    );
}

#[test]
fn binary_catalog_narrow_subset_rejects_unused_broader_declarations() {
    let (documents, _, support) = input();
    let base: Vec<_> = documents
        .iter()
        .map(|b| canonical::parse(b).unwrap())
        .collect();
    for kind in ["optional", "negative", "outcome", "conversion", "links"] {
        let mut bodies = base.clone();
        match kind {
            "optional"=>bodies[0]["fields"][0]["required"]=json!(false),
            "negative"=>bodies[0]["fields"][0]["type"]["min"]=json!("-1"),
            "outcome"=>bodies[4]["activities"][0]["primitive"]=json!("outcome"),
            "conversion"=>bodies[4]["units"].as_array_mut().unwrap().push(json!({"name":"fixture.report:batch","dimension":"fixture.report:count","labels":{"title":"Batch"},"normalization":{"to":"fixture.report:page","factor":{"numerator":"2","denominator":"1"}}})),
            "links"=>bodies[4]["links"].as_array_mut().unwrap().push(json!({"name":"fixture.report:previous","child_activity":"fixture.report:completed","predecessor_activities":["fixture.report:completed"],"child_primitive":"work","predecessor_primitive":"work","minimum":"0","maximum":"1","binding_relation":"same_binding","on_missing":"refuse","max_depth":"16"})),
            _=>unreachable!(),
        }
        let (changed, root) = changed_input(bodies);
        // Linked source closure and syntax remain valid; only advertised support
        // bounds reject these otherwise parseable constructs before plan hash.
        normalize_publication_sources(&changed, &root).unwrap();
        error(
            compile_publication(&changed, &root, &plan_digest(), &support),
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
        );
    }
}

// Separate independent catalog anchor and source-only plan expectations. These
// inputs retain synthetic authority/build evidence and qualify only pure compile.
fn runtime_fixture() -> Value {
    serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap()
}

#[test]
fn builtin_descriptor_is_derived_from_reviewed_embedded_artifacts() {
    let expected: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/independent-expected.json")).unwrap();
    let support = VerifiedCompilerSupport::builtin().unwrap();
    assert_eq!(
        support.descriptor().canonical_bytes(),
        expected["canonical_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        serde_json::to_value(support.descriptor().reference()).unwrap(),
        expected["ref"]
    );
    assert_eq!(
        support.descriptor().reference().id(),
        "wdoc_90bf960b3ed89569b1bb6e07ae05339a65a3b045b47b85f1449143d29cf902b2"
    );
    let (documents, root, _) = input();
    error(
        compile_publication(&documents, &root, &plan_digest(), &support),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
}

#[test]
fn builtin_compiler_reproduces_independent_real_catalog_plan_bytes() {
    let expected = runtime_fixture();
    let vectors = expected["vectors"].as_array().unwrap();
    let documents: Vec<_> = vectors[..6]
        .iter()
        .map(|v| {
            v["canonical_body_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect();
    let root = DefinitionRef::parse(
        CanonicalBytes::from_value(&vectors[5]["ref"])
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let plan = &vectors[6];
    let support = VerifiedCompilerSupport::builtin().unwrap();
    let result = compile_publication(
        &documents,
        &root,
        plan["ref"]["digest"].as_str().unwrap(),
        &support,
    )
    .unwrap();
    assert_eq!(
        result.compiled_plan().canonical_bytes(),
        plan["canonical_body_utf8"].as_str().unwrap().as_bytes()
    );
    assert_eq!(
        serde_json::to_value(result.compiled_plan().reference()).unwrap(),
        plan["ref"]
    );
    assert_eq!(
        result.compiled_plan().id(),
        "wplan_be51d43d564750c345eb6894f3f1c0e2d38b28b87716de5ae31d526fb2f2aa8b"
    );
    for actual in result.source_documents() {
        let source = vectors[..6]
            .iter()
            .find(|v| v["ref"]["id"] == actual.id())
            .unwrap();
        assert_eq!(
            actual.canonical_bytes(),
            source["canonical_body_utf8"].as_str().unwrap().as_bytes()
        );
    }
    // Changing caller-advertised catalog data never produces new trusted support.
    let mut changed: Vec<_> = documents
        .iter()
        .map(|b| canonical::parse(b).unwrap())
        .collect();
    changed[3]["limits_digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");
    let changed_descriptor =
        CompilerDescriptor::parse(CanonicalBytes::from_value(&changed[3]).unwrap().as_slice())
            .unwrap();
    changed[5]["compiler_ref"] = serde_json::to_value(changed_descriptor.reference()).unwrap();
    let root = DefinitionDocument::document("workflow-policy", changed[5].clone())
        .unwrap()
        .reference()
        .clone();
    let changed: Vec<_> = changed
        .iter()
        .map(|v| CanonicalBytes::from_value(v).unwrap().into_vec())
        .collect();
    normalize_publication_sources(&changed, &root).unwrap();
    error(
        compile_publication(
            &changed,
            &root,
            plan["ref"]["digest"].as_str().unwrap(),
            &support,
        ),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
}

#[test]
fn individual_definition_normalization_matches_independent_sources_without_plan() {
    let fixture = runtime_fixture();
    let vectors = fixture["vectors"].as_array().unwrap();
    let support = VerifiedCompilerSupport::builtin().unwrap();
    let mut definitions: Vec<DefinitionDocument> = Vec::new();
    for source in &vectors[..6] {
        let raw = source["canonical_body_utf8"].as_str().unwrap().as_bytes();
        let dependencies = match source["ref"]["kind"].as_str().unwrap() {
            "workflow-product" => vec![definitions[0].clone()],
            "workflow-policy" => definitions.clone(),
            _ => Vec::new(),
        };
        let normalized = normalize_definition(raw, &dependencies, &support).unwrap();
        assert_eq!(normalized.document().canonical_bytes(), raw);
        assert_eq!(
            serde_json::to_value(normalized.document().reference()).unwrap(),
            source["ref"]
        );
        let descriptor_refs = normalized
            .descriptor_refs_for(normalized.document().reference())
            .unwrap();
        match normalized.document().kind() {
            "workflow-fact-schema" | "workflow-compiler-descriptor" => assert_eq!(
                descriptor_refs,
                vec![normalized.document().reference().clone()]
            ),
            "workflow-slot-template" => assert!(descriptor_refs.is_empty()),
            "workflow-product" => {
                assert_eq!(descriptor_refs, vec![definitions[0].reference().clone()])
            }
            "workflow-policy" => {
                let mut expected = vec![
                    definitions[0].reference().clone(),
                    definitions[1].reference().clone(),
                    definitions[3].reference().clone(),
                ];
                canonical::sort_set(&mut expected).unwrap();
                assert_eq!(descriptor_refs, expected);
                assert_eq!(normalized.direct_dependency_refs().unwrap().len(), 4);
            }
            _ => unreachable!(),
        }
        definitions.push(normalized.document().clone());
    }
}
#[test]
fn individual_source_refuses_missing_extra_duplicate_and_unsupported_dependencies() {
    let fixture = runtime_fixture();
    let vectors = fixture["vectors"].as_array().unwrap();
    let support = VerifiedCompilerSupport::builtin().unwrap();
    let fact = normalize_definition(
        vectors[0]["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
        &[],
        &support,
    )
    .unwrap()
    .document()
    .clone();
    let context = normalize_definition(
        vectors[1]["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
        &[],
        &support,
    )
    .unwrap()
    .document()
    .clone();
    let product = vectors[4]["canonical_body_utf8"]
        .as_str()
        .unwrap()
        .as_bytes();
    assert!(normalize_definition(product, &[], &support).is_err());
    error(
        normalize_definition(product, &[fact.clone(), context], &support),
        "DEFINITION_EXTRA",
    );
    error(
        normalize_definition(product, &[fact.clone(), fact.clone()], &support),
        "DEFINITION_CONFLICT",
    );
    error(
        normalize_definition(
            fact.canonical_bytes(),
            std::slice::from_ref(&fact),
            &support,
        ),
        "DEFINITION_EXTRA",
    );
    error(
        normalize_definition(
            vector("compiler")["canonical_body_utf8"]
                .as_str()
                .unwrap()
                .as_bytes(),
            &[],
            &support,
        ),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
    let raw = vectors[0]["canonical_body_utf8"]
        .as_str()
        .unwrap()
        .replacen("{", "{\"sch\\u0065ma\":\"duplicate\",", 1);
    assert!(normalize_definition(raw.as_bytes(), &[], &support).is_err());
    let mut template: Value = canonical::parse(
        vectors[2]["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    template["previous"] = vectors[2]["ref"].clone();
    template["version"] = json!("2");
    error(
        normalize_definition(
            CanonicalBytes::from_value(&template).unwrap().as_slice(),
            &[],
            &support,
        ),
        "PROGRAM_UNSUPPORTED_PRIMITIVE",
    );
}
#[test]
fn policy_definition_typechecks_quantity_and_binding_references() {
    let (raw, root, support) = input();
    let sources = normalize_publication_sources(&raw, &root).unwrap();
    let dependencies: Vec<_> = sources
        .source_documents()
        .iter()
        .filter(|d| d.reference() != &root)
        .cloned()
        .collect();
    let mut policy: Value = canonical::parse(&raw[5]).unwrap();
    for key in ["quantity_field", "unit"] {
        let mut bad = policy.clone();
        bad["rules"][0]["formula"][key] = json!("fixture.report:missing");
        assert!(
            normalize_definition(
                CanonicalBytes::from_value(&bad).unwrap().as_slice(),
                &dependencies,
                &support
            )
            .is_err()
        );
    }
    policy["rules"][0]["binding_slot"] = json!("fixture.report:missing");
    error(
        normalize_definition(
            CanonicalBytes::from_value(&policy).unwrap().as_slice(),
            &dependencies,
            &support,
        ),
        "PROGRAM_BINDING",
    );
}

#[test]
fn canonical_definition_closure_budget_does_not_charge_root_wire_whitespace() {
    let fixture = runtime_fixture();
    let vectors = fixture["vectors"].as_array().unwrap();
    let support = VerifiedCompilerSupport::builtin().unwrap();
    let mut dependencies = vec![
        normalize_definition(
            vectors[0]["canonical_body_utf8"]
                .as_str()
                .unwrap()
                .as_bytes(),
            &[],
            &support,
        )
        .unwrap()
        .document()
        .clone(),
    ];
    let values: Vec<_> = (0..32)
        .map(|i| format!("{}:v{i:02}{}", "n".repeat(60), "x".repeat(58)))
        .collect();
    for schema in 0..4 {
        let fields:Vec<_>=(0..60).map(|i|json!({"name":format!("fixture.enum:f{i:02}"),"required":true,"type":{"kind":"enum","values":values},"provenance":{"kind":"caller"}})).collect();
        let body = json!({"schema":"ledger-workflow-fact-schema/1","schema_id":format!("fixture.extra:s{schema}"),"version":"1","normalization":"ledger-facts-normalization/1","fields":fields});
        dependencies.push(
            normalize_definition(
                CanonicalBytes::from_value(&body).unwrap().as_slice(),
                &[],
                &support,
            )
            .unwrap()
            .document()
            .clone(),
        );
    }
    let mut product: Value = canonical::parse(
        vectors[4]["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    product["fact_schemas"] = json!(
        dependencies
            .iter()
            .map(DefinitionDocument::reference)
            .collect::<Vec<_>>()
    );
    let raw = CanonicalBytes::from_value(&product).unwrap().into_vec();
    let canonical = normalize_definition(&raw, &dependencies, &support).unwrap();
    let dependency_bytes: usize = dependencies.iter().map(|d| d.canonical_bytes().len()).sum();
    assert!(dependency_bytes + canonical.document().canonical_bytes().len() <= 1048576);
    let mut padded = raw;
    padded.resize(262144, b' ');
    assert!(dependency_bytes + padded.len() > 1048576);
    let padded_result = normalize_definition(&padded, &dependencies, &support).unwrap();
    assert_eq!(
        padded_result.document().canonical_bytes(),
        canonical.document().canonical_bytes()
    );
    assert_eq!(
        padded_result.document().reference(),
        canonical.document().reference()
    );
}

#[test]
fn dependency_discovery_is_closed_typed_and_exact_before_storage_resolution() {
    let fixture = runtime_fixture();
    let vectors = fixture["vectors"].as_array().unwrap();
    for source in &vectors[..6] {
        let raw = source["canonical_body_utf8"].as_str().unwrap().as_bytes();
        let refs = candidate_dependency_refs(raw).unwrap();
        match source["ref"]["kind"].as_str().unwrap() {
            "workflow-product" => assert_eq!(
                serde_json::to_value(refs).unwrap(),
                json!([vectors[0]["ref"]])
            ),
            "workflow-policy" => {
                let mut expected: Vec<DefinitionRef> = [1, 2, 3, 4]
                    .iter()
                    .map(|i| canonical::dto(vectors[*i]["ref"].clone()).unwrap())
                    .collect();
                canonical::sort_set(&mut expected).unwrap();
                assert_eq!(refs, expected);
            }
            _ => assert!(refs.is_empty()),
        }
    }
    let mut product: Value = canonical::parse(
        vectors[4]["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    product["units"][0]["unknown"] = json!(true);
    assert!(
        candidate_dependency_refs(CanonicalBytes::from_value(&product).unwrap().as_slice())
            .is_err()
    );
    product["units"][0]
        .as_object_mut()
        .unwrap()
        .remove("unknown");
    product["fact_schemas"] = json!([vectors[4]["ref"]]);
    assert!(
        candidate_dependency_refs(CanonicalBytes::from_value(&product).unwrap().as_slice())
            .is_err()
    );
    let raw = vectors[4]["canonical_body_utf8"]
        .as_str()
        .unwrap()
        .replacen("{", "{\"fact\\u005fschemas\":[],", 1);
    assert!(candidate_dependency_refs(raw.as_bytes()).is_err());
}
#[test]
fn retained_definition_data_replays_old_descriptor_without_current_support() {
    let (raw, root, _) = input();
    let source = normalize_publication_sources(&raw, &root).unwrap();
    let builtin = VerifiedCompilerSupport::builtin().unwrap();
    let deps: Vec<_> = source
        .source_documents()
        .iter()
        .filter(|d| d.reference() != &root)
        .cloned()
        .collect();
    let historical = normalize_retained_definition(&raw[5], &deps).unwrap();
    assert_eq!(historical.document().canonical_bytes(), raw[5]);
    assert_eq!(historical.document().reference(), &root);
    let descriptor = normalize_retained_definition(&raw[3], &[]).unwrap();
    assert_eq!(
        descriptor.document().reference(),
        source.compiler_descriptor().reference()
    );
    error(
        normalize_definition(&raw[3], &[], &builtin),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
    error(
        normalize_definition(&raw[5], &deps, &builtin),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
    error(
        compile_publication(&raw, &root, &plan_digest(), &builtin),
        "UNSUPPORTED_COMPILER_DESCRIPTOR",
    );
    // Reassembly/typed data construction makes no claim of executable support,
    // original retention or authority. The coordinator still verifies origin.
}

#[test]
fn paid_definition_ref_matches_legacy_and_refuses_at_callback_boundary() {
    use crate::validation_work::tests::CountingWork;
    for (kind, prefix) in [
        ("workflow-policy", "wdoc_"),
        ("workflow-compiled-plan", "wplan_"),
    ] {
        let id = format!("{prefix}{}", "a".repeat(64));
        let digest = format!("sha256:{}", "a".repeat(64));
        let mut full = CountingWork::new(usize::MAX);
        let paid = DefinitionRef::new_with_work(kind, &id, &digest, &mut full).unwrap();
        assert_eq!(paid, DefinitionRef::new(kind, &id, &digest).unwrap());
        assert_eq!(full.written, kind.len() + id.len() + digest.len());
        assert!(full.used > 128);
        let mut exact = CountingWork::new(full.used);
        assert_eq!(
            DefinitionRef::new_with_work(kind, &id, &digest, &mut exact).unwrap(),
            paid
        );
        let mut short = CountingWork::new(full.used - 1);
        assert_eq!(
            DefinitionRef::new_with_work(kind, &id, &digest, &mut short)
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
        assert_eq!(short.written, 0);
        for (bad_kind, bad_id, bad_digest) in [
            ("unsupported", id.as_str(), digest.as_str()),
            (kind, "wrong", digest.as_str()),
            (kind, id.as_str(), "sha256:BAD"),
        ] {
            let mut work = CountingWork::new(usize::MAX);
            assert_eq!(
                DefinitionRef::new_with_work(bad_kind, bad_id, bad_digest, &mut work)
                    .unwrap_err()
                    .code,
                DefinitionRef::new(bad_kind, bad_id, bad_digest)
                    .unwrap_err()
                    .code
            );
        }
    }
}
