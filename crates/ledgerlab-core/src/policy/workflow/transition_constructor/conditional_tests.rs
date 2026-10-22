//! Pure-core conditional DATA/producer regressions; public lifecycle proof is separate.
use super::super::super::selected_program::{
    compile_selected_publication, load_local_selected_execution, SelectedCompilerSupport,
    SelectedProgram, SelectedSourceInput,
};
use super::super::{input_selection, pricing};
use crate::{
    canonical::{self, audit_arena::AuditArena},
    policy::workflow::{document_digest, DefinitionRef},
    validation_work::{OriginalAuditSession, ResourceLimits},
    Result,
};
use serde_json::{json, Value};

fn reference(kind: &str, value: &Value) -> Value {
    let digest = document_digest(value["schema"].as_str().unwrap(), value).unwrap();
    json!({"kind":kind,"id":format!("wdoc_{}",&digest[7..]),"digest":digest})
}
fn replace(value: &mut Value, old: &Value, new: &Value) {
    if value == old {
        *value = new.clone();
        return;
    }
    match value {
        Value::Array(a) => {
            for v in a {
                replace(v, old, new);
            }
        }
        Value::Object(a) => {
            for v in a.values_mut() {
                replace(v, old, new);
            }
        }
        _ => {}
    }
}
fn input(
    support: &SelectedCompilerSupport,
    edit: impl FnOnce(&mut Vec<Value>),
) -> SelectedSourceInput {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../selected_program/fixtures/local-first-policy-input.json"
    ))
    .unwrap();
    let mut docs = fixture["documents"].as_array().unwrap().clone();
    let old_work = reference("workflow-fact-schema", &docs[0]);
    let old_context = reference("workflow-fact-schema", &docs[2]);
    let old_product = reference("workflow-product", &docs[3]);
    docs[0]["fields"][0]["provenance"] =
        json!({"kind":"attested","source_slot":"support:worker","evidence_kind":"meter_quantity"});
    docs[2]["fields"] = json!([{"name":"support:service-eligibility","required":true,"provenance":{"kind":"caller"},"type":{"kind":"enum","values":["support:billable","support:included"]}}]);
    let new_work = reference("workflow-fact-schema", &docs[0]);
    let new_context = reference("workflow-fact-schema", &docs[2]);
    for doc in &mut docs {
        replace(doc, &old_work, &new_work);
        replace(doc, &old_context, &new_context);
    }
    docs[3]["fact_schemas"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec());
    let new_product = reference("workflow-product", &docs[3]);
    for doc in &mut docs {
        replace(doc, &old_product, &new_product);
    }
    docs[5]["compiler_ref"] = json!(support.reference());
    docs[5]["rules"][0]["formula"] = json!({"kind":"unit","quantity_field":"support:quantity","unit":"support:request","rate":"1"});
    docs[5]["rules"][0]["when"] = json!({"op":"eq","field":{"binding_fact":"support:service-eligibility"},"value":"support:billable"});
    edit(&mut docs);
    let root: DefinitionRef =
        serde_json::from_value(reference("workflow-policy", &docs[5])).unwrap();
    docs.push(support.canonical_value().clone());
    SelectedSourceInput {
        documents: docs
            .iter()
            .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
            .collect(),
        root_policy_ref: root,
    }
}
fn program(
    session: &mut OriginalAuditSession,
    edit: impl FnOnce(&mut Vec<Value>),
) -> Result<SelectedProgram> {
    let support = SelectedCompilerSupport::local_conditional_execution_with_session(session)?;
    compile_selected_publication(input(&support, edit), &support, session)
}
fn session() -> (AuditArena, OriginalAuditSession) {
    let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    (arena, session)
}
#[test]
fn conditional_binding_slots_and_exact_original_reload_keep_support_identity() {
    let (_arena, mut s) = session();
    let p = program(&mut s, |_| {}).unwrap();
    p.require_local_admission_with_session(&mut s).unwrap();
    let plan = p.compiled_plan().canonical_value();
    let service = plan["typed_instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["rule"]["component_key"] == "support:service")
        .unwrap();
    let indices = service["binding_fact_slots"].as_array().unwrap();
    assert_eq!(indices.len(), 1);
    let slot = plan["fact_slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["index"] == indices[0])
        .unwrap();
    assert_eq!(slot["schema_ref"], json!(p.context_schema_ref()));
    assert_eq!(slot["field"]["name"], "support:service-eligibility");
    let app = plan["typed_instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["rule"]["component_key"] == "support:model-retail")
        .unwrap();
    assert_eq!(app["binding_fact_slots"], json!([]));
    let (_read_arena, mut read) = session();
    let restored = load_local_selected_execution(
        SelectedSourceInput {
            documents: p
                .source_documents()
                .iter()
                .map(|d| d.canonical_bytes().to_vec())
                .collect(),
            root_policy_ref: p.root_policy_ref().clone(),
        },
        p.compiled_plan().canonical_bytes(),
        p.compiled_plan().reference(),
        &mut read,
    )
    .unwrap();
    assert_eq!(
        restored.compiled_plan().canonical_bytes(),
        p.compiled_plan().canonical_bytes()
    );
    assert_eq!(
        restored.compiler_descriptor_ref(),
        p.compiler_descriptor_ref()
    );
    let (_old_arena, mut old) = session();
    let old_support = SelectedCompilerSupport::local_execution_with_session(&mut old).unwrap();
    assert!(
        compile_selected_publication(input(&old_support, |_| {}), &old_support, &mut old).is_err()
    );
}
#[test]
fn conditional_source_refuses_unknown_operand_literal_and_coupled_app() {
    for which in ["operand", "literal", "app"] {
        let (_arena, mut s) = session();
        let result = program(&mut s, |docs| match which {
            "operand" => {
                docs[5]["rules"][0]["when"]["field"]["binding_fact"] = json!("support:missing")
            }
            "literal" => docs[5]["rules"][0]["when"]["value"] = json!("support:undeclared"),
            "app" => docs[5]["rules"][1]["when"] = docs[5]["rules"][0]["when"].clone(),
            _ => unreachable!(),
        });
        assert!(result.is_err());
    }
}
#[test]
fn conditional_actual_pricing_distinguishes_nonmatch_from_matched_zero_and_preserves_app() {
    let (_arena, mut s) = session();
    let p = program(&mut s, |_| {}).unwrap();
    let m = s.meter_mut();
    let facts = json!({"support:quantity":"3"});
    for (literal, expected) in [
        ("support:billable", 3_000_000_000_000_000_000i128),
        ("support:included", 0),
    ] {
        let context = json!({"support:service-eligibility":literal});
        let prices = pricing::work(&p, "support:complete", (&facts, &context), m).unwrap();
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0].atoms, expected);
        assert_eq!(prices[0].predicate, expected != 0);
        assert_eq!(
            p.root_is_selected(m, &json!("support:complete"), "support:service", &context)
                .unwrap(),
            expected != 0
        );
        let app = pricing::app_final_cost(
            &p,
            "support:complete",
            "support:model-retail",
            (&facts, &context),
            &2_000_000_000_000_000_000i128.into(),
            m,
        )
        .unwrap();
        assert_eq!(app.len(), 1);
        assert!(app[0].predicate);
        assert_eq!(app[0].atoms, 2_000_000_000_000_000_000);
        let inst = json!({"component_map":[{"template_key":"support:service","slot_id":"service"},{"template_key":"support:model-retail","slot_id":"app"}],"family_map":[{"template_key":"support:quality","slot_id":"quality"}]});
        let payload = json!({"activity":"support:complete"});
        let mut family = 0;
        let mut app_heads = 0;
        input_selection::head_rows(&p, (&payload, &context), &inst, m, &mut |_, slot, _| {
            if slot.is_some() {
                family += 1;
            } else {
                app_heads += 1;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(family, usize::from(expected != 0));
        assert_eq!(app_heads, 1);
    }
    let (_zero_arena, mut z) = session();
    let zero = program(&mut z, |docs| {
        docs[5]["rules"][0]["formula"]["rate"] = json!("0")
    })
    .unwrap();
    let m = z.meter_mut();
    let context = json!({"support:service-eligibility":"support:billable"});
    let prices = pricing::work(&zero, "support:complete", (&facts, &context), m).unwrap();
    assert!(prices[0].predicate);
    assert_eq!(prices[0].atoms, 0);
    assert!(zero
        .root_is_selected(m, &json!("support:complete"), "support:service", &context)
        .unwrap());
}
