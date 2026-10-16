use super::super::transition_constructor::CoreMeter;
use super::*;
use crate::canonical;
use serde_json::Value;
#[test]
fn selected_source_derives_entire_plan_without_expected_input() {
    let source: Value = canonical::parse(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json"
    ))
    .unwrap();
    let expected: Value =
        serde_json::from_slice(include_bytes!("fixtures/plan-independent.json")).unwrap();
    let support = SelectedCompilerSupport::builtin().unwrap();
    assert_eq!(
        support.canonical_value(),
        &expected["builtin_descriptor"]["body"]
    );
    let input = SelectedSourceInput {
        documents: source["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
            .collect(),
        root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
    };
    let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
    let program = compile_selected_program(input, &support, &mut meter).unwrap();
    assert_eq!(
        program.compiled_plan().canonical_value(),
        &expected["builtin_plan"]["body"]
    );
    assert_eq!(
        serde_json::to_value(program.compiled_plan().reference()).unwrap(),
        expected["builtin_plan"]["ref"]
    );
    assert_eq!(
        program.compiled_plan().canonical_bytes(),
        canonical::CanonicalBytes::from_value(&expected["builtin_plan"]["body"])
            .unwrap()
            .as_slice()
    );
}

fn modified_input(edit: impl FnOnce(&mut Value), extra: bool) -> SelectedSourceInput {
    let mut source: Value = canonical::parse(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json"
    ))
    .unwrap();
    let documents = source["documents"].as_array_mut().unwrap();
    let policy = documents
        .iter_mut()
        .find(|v| v["schema"] == "ledger-workflow-policy/1")
        .unwrap();
    edit(policy);
    let digest = super::super::document_digest("ledger-workflow-policy/1", policy).unwrap();
    let root = super::super::DefinitionRef::new(
        "workflow-policy",
        &format!("wdoc_{}", &digest[7..]),
        &digest,
    )
    .unwrap();
    if extra {
        let mut unused = documents
            .iter()
            .find(|v| v["schema"] == "ledger-workflow-fact-schema/1")
            .unwrap()
            .clone();
        unused["schema_id"] = serde_json::json!("another:unused");
        documents.push(unused);
    }
    SelectedSourceInput {
        documents: documents
            .iter()
            .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
            .collect(),
        root_policy_ref: root,
    }
}
#[test]
fn selected_rules_use_phase_order_and_refuse_cross_activity_or_duplicate_effects() {
    let support = SelectedCompilerSupport::builtin().unwrap();
    let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
    let program = compile_selected_program(
        modified_input(
            |p| {
                p["rules"].as_array_mut().unwrap().rotate_right(1);
            },
            false,
        ),
        &support,
        &mut meter,
    )
    .unwrap();
    let instructions = program.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .unwrap();
    assert_eq!(instructions[0]["rule"]["phase"], "base");
    assert_eq!(instructions[1]["rule"]["phase"], "base");
    assert_eq!(instructions[2]["rule"]["phase"], "discount");
    for kind in 0..7 {
        let input = modified_input(
            |p| match kind {
                0 => {
                    let mut duplicate = p["rules"][1].clone();
                    duplicate["rule_id"] = serde_json::json!("another:duplicate");
                    p["rules"].as_array_mut().unwrap().push(duplicate);
                }
                1 => p["rules"][2]["activity"] = p["rules"][0]["activity"].clone(),
                2 => p["rules"][1]["phase"] = serde_json::json!("cap"),
                3 => p["families"][0]["code_field"] = serde_json::json!("fixture.measure:occurred"),
                5 => p["rules"][0]["when"]["extra"] = serde_json::json!(true),
                6 => p["families"][0]["codes"][0]["effect"]["extra"] = serde_json::json!(true),
                _ => {}
            },
            kind == 4,
        );
        let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
        assert!(
            compile_selected_program(input, &support, &mut meter).is_err(),
            "negative {kind}"
        );
    }
}
#[test]
fn equivalent_policy_counter_decimal_and_set_spellings_keep_one_normalized_root() {
    let source: Value = canonical::parse(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json"
    ))
    .unwrap();
    let mut input = modified_input(
        |p| {
            p["version"] = serde_json::json!("0001");
            p["rules"][0]["formula"]["amount"] = serde_json::json!("00.4100");
            p["binding_slots"][0]["allowed_activities"]
                .as_array_mut()
                .unwrap()
                .reverse();
            p["families"][0]["codes"].as_array_mut().unwrap().reverse();
        },
        false,
    );
    input.root_policy_ref = serde_json::from_value(source["root_policy_ref"].clone()).unwrap();
    let support = SelectedCompilerSupport::builtin().unwrap();
    let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
    let program = compile_selected_program(input, &support, &mut meter).unwrap();
    let expected: Value =
        serde_json::from_slice(include_bytes!("fixtures/plan-independent.json")).unwrap();
    assert_eq!(
        program.compiled_plan().canonical_value(),
        &expected["builtin_plan"]["body"]
    );
    for kind in 0..2 {
        let input = modified_input(
            |p| {
                if kind == 0 {
                    p["families"][0]["window"]["occurrence_us"] = serde_json::json!("0");
                } else {
                    p["rules"].as_array_mut().unwrap().remove(1);
                }
            },
            false,
        );
        let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
        assert!(compile_selected_program(input, &support, &mut meter).is_err());
    }
}
#[test]
fn policy_decimal_rejects_negative_zero_and_31_digit_normalized_coefficient() {
    for raw in [
        "-0",
        "-0.000",
        "-000.000",
        "1000000000000000000000000000000",
        "0001000000000000000000000000000000.000",
    ] {
        let mut value = raw.to_owned();
        let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
        assert_eq!(
            normalize::decimal(&mut meter, &mut value).unwrap_err().code,
            "INVALID_SCALAR",
            "{raw}"
        );
    }
    for (raw, normalized) in [
        (
            "999999999999999999999999999999",
            "999999999999999999999999999999",
        ),
        (
            "000999999999999999999999999999999.0000",
            "999999999999999999999999999999",
        ),
        ("0.000000000000000001", "0.000000000000000001"),
        ("0000.0000", "0"),
    ] {
        let mut value = raw.to_owned();
        let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
        normalize::decimal(&mut meter, &mut value).unwrap();
        assert_eq!(value, normalized);
    }
}
#[test]
fn one_retail_binding_is_required_and_its_name_does_not_replace_template_role() {
    let support = SelectedCompilerSupport::builtin().unwrap();
    let input = modified_input(
        |p| {
            let mut second = p["binding_slots"][0].clone();
            second["name"] = serde_json::json!("example:second");
            p["binding_slots"].as_array_mut().unwrap().push(second);
        },
        false,
    );
    let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
    assert!(compile_selected_program(input, &support, &mut meter).is_err());
    let input = modified_input(
        |p| {
            p["binding_slots"][0]["name"] = serde_json::json!("example:booking");
            for rule in p["rules"].as_array_mut().unwrap() {
                rule["binding_slot"] = serde_json::json!("example:booking");
            }
            p["families"][0]["selector"]["binding_slot"] = serde_json::json!("example:booking");
        },
        false,
    );
    let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
    let program = compile_selected_program(input, &support, &mut meter).unwrap();
    assert_eq!(
        program.compiled_plan().canonical_value()["component_slots"][0]["role_slot"],
        "fixture.measure:retail"
    );
}

#[test]
fn historical_selected_recipe_recomputes_saved_plan_and_refuses_substitution() {
    use crate::{
        canonical::audit_arena::AuditArena,
        validation_work::{OriginalAuditSession, ValidationVisit},
    };
    fn input() -> SelectedSourceInput {
        let source:Value=canonical::parse(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        SelectedSourceInput {
            documents: source["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
        }
    }
    let support = SelectedCompilerSupport::builtin().unwrap();
    let original = compile_selected_program(
        input(),
        &support,
        &mut CoreMeter::for_compile(1_048_576).unwrap(),
    )
    .unwrap();
    let arena = AuditArena::new().unwrap();
    let mut audit = OriginalAuditSession::new(&arena).unwrap();
    let restored = restore_selected_program(
        input(),
        support.canonical_bytes(),
        original.compiled_plan().canonical_bytes(),
        &mut audit,
    )
    .unwrap();
    assert_eq!(
        restored.compiled_plan().reference(),
        original.compiled_plan().reference()
    );
    assert_eq!(restored.root_policy_ref(), original.root_policy_ref());
    assert_eq!(restored.compiler_descriptor_ref(), support.reference());
    assert_eq!(
        restored.compiled_plan().canonical_bytes(),
        original.compiled_plan().canonical_bytes()
    );
    audit.work().visit(ValidationVisit::Constraint).unwrap();
    let mut bad_support = support.canonical_value().clone();
    bad_support["limits_profile"] = serde_json::json!("unregistered-profile/1");
    let wrong = canonical::CanonicalBytes::from_value(&bad_support).unwrap();
    let mut denied = OriginalAuditSession::new(&arena).unwrap();
    let error = restore_selected_program(
        input(),
        wrong.as_slice(),
        original.compiled_plan().canonical_bytes(),
        &mut denied,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "ORIGINAL_VERIFIER_UNAVAILABLE");
    assert!(denied.work().visit(ValidationVisit::Constraint).is_err());
    let mut changed_plan = original.compiled_plan().canonical_value().clone();
    changed_plan["worst_case"]["runtime_work"] = serde_json::json!("65535");
    let changed_plan = canonical::CanonicalBytes::from_value(&changed_plan).unwrap();
    let mut denied = OriginalAuditSession::new(&arena).unwrap();
    let error = restore_selected_program(
        input(),
        support.canonical_bytes(),
        changed_plan.as_slice(),
        &mut denied,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "INTEGRITY_FAILURE");
}
