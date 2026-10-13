use super::*;

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
