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
fn policy_scalar_input(support: &SelectedCompilerSupport) -> SelectedSourceInput {
    input(support, |docs| {
        let old_work = reference("workflow-fact-schema", &docs[0]);
        let old_product = reference("workflow-product", &docs[3]);
        docs[0]["fields"][0]["type"] =
            json!({"kind":"decimal","min":"0","max":"120","max_scale":"3","unit":"support:second"});
        docs[0]["fields"].as_array_mut().unwrap().push(json!({"name":"support:approved","required":false,"provenance":{"kind":"caller"},"type":{"kind":"boolean"}}));
        docs[0]["fields"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|v| v["name"].as_str().unwrap().to_owned());
        let new_work = reference("workflow-fact-schema", &docs[0]);
        for doc in docs.iter_mut() {
            replace(doc, &old_work, &new_work);
        }
        docs[3]["units"] = json!([
            {"name":"support:minute","labels":{"title":"Minute"},"dimension":"support:time"},
            {"name":"support:second","labels":{"title":"Second"},"dimension":"support:time","normalization":{"to":"support:minute","factor":{"numerator":"1","denominator":"60"}}}
        ]);
        docs[3]["fact_schemas"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec());
        let new_product = reference("workflow-product", &docs[3]);
        for doc in docs.iter_mut() {
            replace(doc, &old_product, &new_product);
        }
        docs[5]["app_delayed_costs"][0]["usage_unit"] = json!("support:second");
        docs[5]["rules"][0]["formula"] = json!({"kind":"unit","quantity_field":"support:quantity","unit":"support:second","normalize_to":"support:minute","rate":"0.1"});
        docs[5]["rules"][0]["when"] = json!({"op":"all","args":[
            {"op":"ge","field":{"fact":"support:quantity"},"value":"60"},
            {"op":"eq","field":{"binding_fact":"support:service-eligibility"},"value":"support:billable"},
            {"op":"not","arg":{"op":"eq","field":{"fact":"support:approved"},"value":false}}
        ]});
        docs[5]["rules"][0]["when"]["args"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec());
    })
}
fn policy_context_input(support: &SelectedCompilerSupport) -> SelectedSourceInput {
    let mut input = policy_scalar_input(support);
    for bytes in &mut input.documents {
        let mut value: Value = serde_json::from_slice(bytes).unwrap();
        if value["schema"] == "ledger-workflow-policy/4" {
            let args = value["rules"][0]["when"]["args"].as_array_mut().unwrap();
            args.push(json!({"op":"eq","field":{"context":"source"},"value":"urn:example:original-worker"}));
            args.push(
                json!({"op":"eq","field":{"context":"funding_mode"},"value":"customer_local"}),
            );
            value["app_delayed_costs"][0]["resource_funding_leg"] = json!("support:aaa-platform");
            input.root_policy_ref =
                serde_json::from_value(reference("workflow-policy", &value)).unwrap();
        }
        *bytes = canonical::CanonicalBytes::from_value(&value)
            .unwrap()
            .into_vec();
    }
    input
}
#[test]
fn context2_actual_source_plan_reload_and_component_funding_use_original_operands() {
    let (_a, mut compile_session) = session();
    let support =
        SelectedCompilerSupport::local_policy_context_execution_with_session(&mut compile_session)
            .unwrap();
    let p = compile_selected_publication(
        policy_context_input(&support),
        &support,
        &mut compile_session,
    )
    .unwrap();
    assert!(p.uses_policy_context_recipe());
    p.require_local_admission_with_session(&mut compile_session)
        .unwrap();
    let mut plain = super::super::CoreMeter::for_compile(1_048_576).unwrap();
    let data = super::super::super::selected_program::compile_selected_program(
        policy_context_input(&support),
        &support,
        &mut plain,
    )
    .unwrap();
    assert_eq!(
        p.compiled_plan().canonical_bytes(),
        data.compiled_plan().canonical_bytes()
    );
    let context = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":"support:billable"}});
    let facts = json!({"support:quantity":"90","support:approved":true});
    // Both are declared accepted legs: the app's independent platform leg sorts
    // first, while this service component is bound to the customer-local leg.
    let binding = json!({"bindings":[{"slot":"support:retail"}],"funding_legs":[{"name":"support:aaa-platform","mode":"platform"},{"name":"support:customer","mode":"customer_local"}],"accepted_context":context});
    let m = compile_session.meter_mut();
    assert_eq!(
        p.original_component_funding_mode(m, &binding, "support:retail")
            .unwrap(),
        "customer_local"
    );
    let prices = pricing::work_operational(
        &p,
        "support:complete",
        (&facts, &context),
        Some((&binding, "urn:example:original-worker")),
        m,
    )
    .unwrap();
    assert_eq!(prices[0].atoms, 150_000_000_000_000_000);
    assert_eq!(prices[0].predicate_state, "true");
    let changed_source = pricing::work_operational(
        &p,
        "support:complete",
        (&facts, &context),
        Some((&binding, "urn:example:current-corrector")),
        m,
    )
    .unwrap();
    assert_eq!(changed_source[0].predicate_state, "false");
    assert_eq!(changed_source[0].atoms, 0);
    assert!(pricing::work(&p, "support:complete", (&facts, &context), m).is_err());
    let payload = json!({"activity":"support:complete","facts":facts});
    assert!(p
        .root_is_selected_for_work_operational(
            m,
            &payload,
            "support:service",
            (&context, Some((&binding, "urn:example:original-worker")))
        )
        .unwrap());
    let app = pricing::app_final_cost_operational(
        &p,
        "support:complete",
        "support:model-retail",
        (
            (&facts, &context),
            Some((&binding, "urn:example:original-worker")),
        ),
        &2_000_000_000_000_000_000i128.into(),
        m,
    )
    .unwrap();
    assert_eq!(app[0].atoms, 2_000_000_000_000_000_000);
    let service = p.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["rule"]["component_key"] == "support:service")
        .unwrap();
    assert_eq!(service["fact_slots"].as_array().unwrap().len(), 2);
    assert_eq!(service["binding_fact_slots"].as_array().unwrap().len(), 1);
    // Original reload retains new descriptor exactly; older profile does not
    // gain the source/funding capability by loading a familiar Plan8 shape.
    let mut incoming = policy_context_input(&support);
    incoming.documents.reverse();
    let normalized = super::super::super::selected_program::normalize_local_publication_input(
        incoming,
        &p,
        &mut compile_session,
    )
    .unwrap();
    assert_eq!(normalized.root_policy_ref(), p.root_policy_ref());
    let (_a, mut reload) = session();
    let restored = load_local_selected_execution(
        policy_context_input(&support),
        p.compiled_plan().canonical_bytes(),
        p.compiled_plan().reference(),
        &mut reload,
    )
    .unwrap();
    assert!(restored.uses_policy_context_recipe());
    assert_eq!(
        restored.compiled_plan().canonical_bytes(),
        p.compiled_plan().canonical_bytes()
    );
    let (_a, mut old_session) = session();
    let old =
        SelectedCompilerSupport::local_policy_semantics_execution_with_session(&mut old_session)
            .unwrap();
    assert!(
        compile_selected_publication(policy_context_input(&old), &old, &mut old_session).is_err()
    );
}

#[test]
fn policy_scalar_actual_source_plan_reload_pricing_and_exposure_share_typed_semantics() {
    let (_arena, mut s) = session();
    let support =
        SelectedCompilerSupport::local_policy_semantics_execution_with_session(&mut s).unwrap();
    let p = compile_selected_publication(policy_scalar_input(&support), &support, &mut s).unwrap();
    p.require_local_admission_with_session(&mut s).unwrap();
    let plan = p.compiled_plan().canonical_value();
    let service = plan["typed_instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["rule"]["component_key"] == "support:service")
        .unwrap();
    assert_eq!(service["fact_slots"].as_array().unwrap().len(), 2);
    assert_eq!(service["binding_fact_slots"].as_array().unwrap().len(), 1);
    let explanation = plan["explanation_map"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["ordinal"] == service["ordinal"])
        .unwrap();
    assert_eq!(
        explanation["fields"],
        json!([
            "support:approved",
            "support:quantity",
            "support:service-eligibility"
        ])
    );
    let context = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":"support:billable"}});
    for (facts, state, atoms) in [
        (
            json!({"support:quantity":"90","support:approved":true}),
            "true",
            150_000_000_000_000_000i128,
        ),
        (
            json!({"support:quantity":"30","support:approved":true}),
            "false",
            0,
        ),
        (json!({"support:quantity":"90"}), "unknown", 0),
    ] {
        let prices =
            pricing::work(&p, "support:complete", (&facts, &context), s.meter_mut()).unwrap();
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0].predicate_state, state);
        assert_eq!(prices[0].atoms, atoms);
    }
    let facts = json!({"support:quantity":"30","support:approved":false});
    let upper =
        pricing::work_exposure_bound(&p, "support:complete", (&facts, &context), s.meter_mut())
            .unwrap();
    assert_eq!(upper[0].atoms, 50_000_000_000_000_000);
    // The exposure DATA may ignore eligibility; an actual billed value never does.
    assert_eq!(
        pricing::work(&p, "support:complete", (&facts, &context), s.meter_mut()).unwrap()[0].atoms,
        0
    );
    let mut plain = super::super::CoreMeter::for_compile(1_048_576).unwrap();
    let data = super::super::super::selected_program::compile_selected_program(
        policy_scalar_input(&support),
        &support,
        &mut plain,
    )
    .unwrap();
    assert_eq!(
        data.compiled_plan().canonical_bytes(),
        p.compiled_plan().canonical_bytes()
    );
    // Original-intent normalization consumes ACTUAL incoming source bodies,
    // including Product after Policy and noncanonical scalar spelling. It
    // restores the same saved identity, without substituting saved bodies.
    let mut incoming = policy_scalar_input(&support);
    for bytes in &mut incoming.documents {
        let mut value: Value = serde_json::from_slice(bytes).unwrap();
        if value["schema"] == "ledger-workflow-policy/4" {
            for arg in value["rules"][0]["when"]["args"].as_array_mut().unwrap() {
                if arg["op"] == "ge" {
                    arg["value"] = json!("060.000");
                }
            }
        }
        *bytes = canonical::CanonicalBytes::from_value(&value)
            .unwrap()
            .into_vec();
    }
    incoming.documents.reverse();
    let normalized = super::super::super::selected_program::normalize_local_publication_input(
        incoming, &p, &mut s,
    )
    .unwrap();
    assert_eq!(normalized.root_policy_ref(), p.root_policy_ref());
    assert_eq!(
        normalized.source_documents().len(),
        p.source_documents().len()
    );
    for actual in normalized.source_documents() {
        let expected = p
            .source_documents()
            .iter()
            .find(|d| d.reference() == actual.reference())
            .unwrap();
        assert_eq!(actual.canonical_bytes(), expected.canonical_bytes());
    }
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
    let (_old_arena, mut old) = session();
    let old_support =
        SelectedCompilerSupport::local_conditional_execution_with_session(&mut old).unwrap();
    assert!(compile_selected_publication(
        policy_scalar_input(&old_support),
        &old_support,
        &mut old
    )
    .is_err());
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
    let explanation = plan["explanation_map"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["ordinal"] == service["ordinal"])
        .unwrap();
    assert_eq!(
        explanation["fields"],
        json!(["support:quantity", "support:service-eligibility"])
    );
    let facts = service["fact_slots"].as_array().unwrap();
    assert_eq!(facts.len(), 1);
    let fact = plan["fact_slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["index"] == facts[0])
        .unwrap();
    assert_eq!(fact["field"]["name"], "support:quantity");
    // Same source-derived plan from the nonowning DATA compiler; not admission.
    let support =
        SelectedCompilerSupport::local_conditional_execution_with_session(&mut s).unwrap();
    let mut plain = super::super::CoreMeter::for_compile(1_048_576).unwrap();
    let plain_program = super::super::super::selected_program::compile_selected_program(
        input(&support, |_| {}),
        &support,
        &mut plain,
    )
    .unwrap();
    assert_eq!(
        plain_program.compiled_plan().canonical_bytes(),
        p.compiled_plan().canonical_bytes()
    );

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
        let context = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":literal}});
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
        input_selection::head_rows(
            &p,
            (&payload, &context, None),
            &inst,
            m,
            &mut |_, slot, _| {
                if slot.is_some() {
                    family += 1;
                } else {
                    app_heads += 1;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(family, usize::from(expected != 0));
        assert_eq!(app_heads, 1);
        // Actual parent DATA producer repeats the same selection in its paid
        // count and emit passes; this is not authority or financial proof.
        let key = super::super::OperationKey {
            logical_store_id: "conditional-store".into(),
            scope: ["conditional-company".into(), "local".into()],
            source: "urn:example:conditional".into(),
            operation_id: "conditional-work".into(),
        };
        let request = json!({"activity":"support:complete","customer":"customer","chain":"chain","business_operation_id":"service","external_event_id":"delivery"});
        let binding = json!({"agreement_id":"agreement","accepted_context":context});
        let locators = input_selection::derive_work_head_locators(
            &p,
            &key,
            &key.source,
            &request,
            &binding,
            &inst,
            m,
        )
        .unwrap();
        assert_eq!(locators.family_keys().len(), usize::from(expected != 0));
        assert_eq!(locators.app_decision_ids().len(), 1);
    }
    let condition = &p.original_policy_value()["rules"][0]["when"];
    for malformed in [
        json!({"support:service-eligibility":"support:billable"}),
        json!({"fact_schema_ref":p.context_schema_ref()}),
        json!({"fact_schema_ref":p.context_schema_ref(),"facts":{}}),
        json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":true}}),
    ] {
        assert_eq!(
            p.evaluate_work_condition(m, condition, &malformed)
                .unwrap_err()
                .code,
            "INTEGRITY_FAILURE"
        );
    }
    let (_zero_arena, mut z) = session();
    let zero = program(&mut z, |docs| {
        docs[5]["rules"][0]["formula"]["rate"] = json!("0")
    })
    .unwrap();
    let m = z.meter_mut();
    let context = json!({"fact_schema_ref":zero.context_schema_ref(),"facts":{"support:service-eligibility":"support:billable"}});
    let prices = pricing::work(&zero, "support:complete", (&facts, &context), m).unwrap();
    assert!(prices[0].predicate);
    assert_eq!(prices[0].atoms, 0);
    assert!(zero
        .root_is_selected(m, &json!("support:complete"), "support:service", &context)
        .unwrap());
}

#[test]
fn conditional_retained_capacity_uses_selected_targets_and_keeps_independent_app() {
    use super::super::{
        authority, body, capacity, data::*, record_body::RecordBody, records::Records, work_keys,
    };
    // Retained-capacity DATA fixture. Earlier authority/PUB/proof validation is
    // a caller obligation; this exercises the actual financial suffix consumer.
    for (literal, zero_rate) in [
        ("support:included", false),
        ("support:billable", false),
        ("support:billable", true),
    ] {
        let (_compiled_arena, mut s) = session();
        let p = program(&mut s, |docs| {
            if zero_rate {
                docs[5]["rules"][0]["formula"]["rate"] = json!("0");
            }
        })
        .unwrap();
        let (arena, mut consumer) = session();
        let m = consumer.meter_mut();
        let key = OperationKey {
            logical_store_id: "retained-capacity".into(),
            scope: ["company".into(), "local".into()],
            source: "urn:example:work".into(),
            operation_id: "work".into(),
        };
        let c = DraftContextData {
            key,
            actor: "worker".into(),
            canonical_economic_namespace: "urn:example:work".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-23T00:00:00.000000Z".into(),
            accepted_at: "2026-10-23T00:00:00.000000Z".into(),
            authorities: vec![],
            source_observation_inputs: vec![],
        };
        let binding = body::document(m,"workflow-binding",json!({"schema":"ledger-workflow-binding/2","agreement_id":"agreement","accepted_context":{"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":literal}}})).unwrap();
        let binding_value = binding.decoded(m, 4194304).unwrap();
        let facts_ref = p
            .source_documents()
            .iter()
            .find(|d| d.canonical_value()["schema_id"] == "support:work-facts")
            .unwrap()
            .reference();
        let payload = json!({"customer":"customer","chain":"chain","status":"succeeded","activity":"support:complete","product_ref":p.product_ref(),"policy_ref":p.root_policy_ref(),"fact_schema_ref":facts_ref,"facts":{"support:quantity":"3"},"links":[],"binding_ref":binding.reference,"invocation_refs":[],"evidence_refs":[],"business_operation_id":"service","external_event_id":"delivery"});
        let claim_id =
            work_keys::claim(m, &c.key, &c.canonical_economic_namespace, &payload).unwrap();
        let event = work_keys::event(m, &c.key, &payload).unwrap();
        let work = body::record(m,&c.key,"workflow-work",event,json!({"schema":"ledger-workflow-work-record/1","key":c.key,"payload":payload,"claim_id":claim_id,"received_at":c.received_at,"accepted_at":c.accepted_at,"source_namespace":c.canonical_economic_namespace})).unwrap();
        let tuple = json!([
            payload["customer"],
            payload["chain"],
            payload["status"],
            payload["activity"],
            payload["product_ref"],
            payload["policy_ref"],
            payload["fact_schema_ref"],
            payload["facts"],
            payload["links"],
            payload["binding_ref"],
            payload["invocation_refs"],
            payload["evidence_refs"]
        ]);
        let digest = super::super::records::hash(m, "workflow-claim-facts", &tuple).unwrap();
        let claim = body::record(m,&c.key,"workflow-work-claim",claim_id.clone(),json!({"schema":"ledger-workflow-work-claim/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"source_namespace":c.canonical_economic_namespace,"business_operation_id":"service","completion":"completion","work_ref":work.reference,"facts_digest":digest})).unwrap();
        let decision_id = work_keys::decision(m, &claim_id).unwrap();
        let revision_id = body::semantic_id(
            m,
            "wdrev_",
            "workflow-decision-revision",
            &json!([decision_id, "1"]),
        )
        .unwrap();
        let decision = body::record(m,&c.key,"workflow-decision",revision_id,json!({"schema":"ledger-workflow-decision-revision/1","decision_id":decision_id,"revision":"1","kind":"work","transition":"initial","subject_ref":work.reference})).unwrap();
        let terms = authority::Terms {
            binding: binding_value,
            accepted: RecordBody::Legacy(json!({})),
            _activation: RecordBody::Legacy(json!({})),
            catalog: RecordBody::Legacy(
                json!({"components":[{"slot_id":"service","root_slot":"service","book":"retail"},{"slot_id":"app","root_slot":"app","book":"retail"}]}),
            ),
            instantiation: RecordBody::Legacy(
                json!({"component_map":[{"template_key":"support:service","slot_id":"service"},{"template_key":"support:model-retail","slot_id":"app"}]}),
            ),
            slot_terms: RecordBody::Legacy(json!({})),
            slot_terms_ref: binding.reference.clone(),
            role_ref: binding.reference.clone(),
        };
        let rows = [work, claim, decision];
        let views: Vec<_> = rows
            .iter()
            .map(|row| EconomicRecordView {
                reference: &row.reference,
                canonical_body: &row.canonical_body,
            })
            .collect();
        let mut r = Records::borrowed_parent_app(m, &arena, &c.key, &views).unwrap();
        let work_body = rows[0].decoded(m, 4194304).unwrap();
        let result = capacity::original::derive_financial_for_work(
            &arena, m, &mut r, &c, &p, &work_body, &terms,
        );
        if literal == "support:included" {
            let reservation = result.unwrap();
            assert_eq!(reservation.root_decision_id, decision_id);
            assert_eq!(reservation.root_rule_count, 0);
            assert_eq!(reservation.maximum_root_actions, 0);
            assert!(reservation.families.is_empty());
            assert_eq!(reservation.app_costs.len(), 1);
            assert_eq!(reservation.app_costs[0].target_ref, rows[0].reference);
            assert_eq!(reservation.app_costs[0].maximum_inverse_actions, 1);
        } else {
            // True, including exact-zero Unit, still needs its actual Target.
            let error = result.err().unwrap();
            assert_eq!(error.code, "INTEGRITY_FAILURE");
            assert_eq!(error.detail, "missing birth identity");
        }
    }
}

#[test]
fn policy_decimal_slot_bounds_and_current_quantities_use_original_numeric_schema() {
    let (_arena, mut s) = session();
    let support =
        SelectedCompilerSupport::local_policy_context_execution_with_session(&mut s).unwrap();
    let p = compile_selected_publication(policy_context_input(&support), &support, &mut s).unwrap();
    let activity = p.compiled_plan().canonical_value()["activity_table"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["activity"] == "support:complete")
        .unwrap();
    let q = json!({"activity":"support:complete","field":"support:quantity","unit":"support:second","maximum":"120"});
    super::super::inventory::slots::quantity_declaration(s.meter_mut(), &p, activity, &q).unwrap();
    for maximum in ["0", "0.0001", "119.9995", "120"] {
        let mut q = q.clone();
        q["maximum"] = json!(maximum);
        super::super::inventory::slots::quantity_declaration(s.meter_mut(), &p, activity, &q)
            .unwrap();
    }
    for (field, value) in [
        ("maximum", "120.0001"),
        ("maximum", "-0.001"),
        ("unit", "support:minute"),
        ("field", "support:approved"),
    ] {
        let mut bad = q.clone();
        bad[field] = json!(value);
        assert!(
            super::super::inventory::slots::quantity_declaration(s.meter_mut(), &p, activity, &bad)
                .is_err(),
            "{field}={value}"
        );
    }
    let binding = json!({"quantity_ceilings":[{"activity":"support:complete","field":"support:quantity","unit":"support:second","maximum":"90.125"}]});
    let terms = json!({"quantity_ceilings":[{"activity":"support:complete","field":"support:quantity","unit":"support:second","maximum":"90.124"}]});
    for (quantity, valid) in [("90.123", true), ("90.124", true), ("90.125", false)] {
        let facts = json!({"support:quantity":quantity});
        let result = super::super::inventory::entry::quantities_for_program(
            s.meter_mut(),
            &p,
            &binding,
            &terms,
            "support:complete",
            &facts,
        );
        assert_eq!(result.is_ok(), valid, "{quantity}: {result:?}");
    }
    // The old integer reader remains distinct, including its refusal of a
    // fractional actual value under an older recipe.
    assert!(super::super::inventory::entry::quantities(
        s.meter_mut(),
        &binding,
        &terms,
        "support:complete",
        &json!({"support:quantity":"90.123"})
    )
    .is_err());
}

#[test]
fn policy_causal_declarations_do_not_substitute_a_current_operation_role() {
    let (_arena, mut s) = session();
    let support =
        SelectedCompilerSupport::local_policy_context_execution_with_session(&mut s).unwrap();
    let program =
        compile_selected_publication(policy_context_input(&support), &support, &mut s).unwrap();
    let binding = json!({"source_slots":[{"name":"support:assessor"}]});
    let catalog = json!({"families":[{"slot_id":"support:quality","target_component_slot":"support:service"}]});
    let terms = json!({"schema":"ledger-workflow-slot-binding-terms/7","causal_reversal_clauses":[{"root_component_slot":"support:service","family_slot":"support:quality","correction_source_slot":"support:assessor","mode":"exact_inverse_and_permanent_close","standalone_deadline_effect":"does_not_expire_preaccepted_causal_reversal"}]});
    let grant = json!({"customer_scope":["customer-B"],"operations":["evidence.retain"],"permissions":["read","submit"],"source_slots":["support:meter"],"activity_selectors":[{"names":["support:complete"]}],"product_selectors":[{"names":["support:product"]}]});
    super::super::inventory::slots::validate_selected_causal_clauses(
        s.meter_mut(),
        &program,
        &binding,
        &catalog,
        &terms,
        &grant,
    )
    .unwrap();
    // Declaration projection supplies neither current retention permission nor
    // causal correction rights. Actual selected grant coverage remains closed.
    super::super::authority::grant_coverage(
        s.meter_mut(),
        &grant,
        &json!("customer-B"),
        "support:complete",
        "support:product",
        "support:meter",
        ("evidence.retain", &["read", "submit"]),
    )
    .unwrap();
    assert!(super::super::authority::grant_coverage(
        s.meter_mut(),
        &grant,
        &json!("customer-B"),
        "support:complete",
        "support:product",
        "support:assessor",
        ("quantity.correct", &["correct", "read"])
    )
    .is_err());
    assert!(super::super::authority::grant_coverage(
        s.meter_mut(),
        &grant,
        &json!("customer-B"),
        "support:complete",
        "support:product",
        "support:assessor",
        ("evidence.retain", &["read", "submit"]),
    )
    .is_err());
    let mut no_retention = grant.clone();
    no_retention["operations"] = json!([]);
    assert!(super::super::authority::grant_coverage(
        s.meter_mut(),
        &no_retention,
        &json!("customer-B"),
        "support:complete",
        "support:product",
        "support:meter",
        ("evidence.retain", &["read", "submit"]),
    )
    .is_err());
    for (part, value) in [
        ("root_component_slot", "support:other"),
        ("family_slot", "support:other"),
        ("correction_source_slot", "support:other"),
        ("mode", "waive"),
    ] {
        let mut bad = terms.clone();
        bad["causal_reversal_clauses"][0][part] = json!(value);
        assert!(
            super::super::inventory::slots::validate_selected_causal_clauses(
                s.meter_mut(),
                &program,
                &binding,
                &catalog,
                &bad,
                &grant
            )
            .is_err(),
            "{part}"
        );
    }
}

#[test]
fn lifecycle_exact_factory_and_saved_reload_preserve_recipe_identity_under_source_hold() {
    let (_arena, mut s) = session();
    let support =
        SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut s).unwrap();
    let input = policy_context_input(&support);
    let p = compile_selected_publication(input, &support, &mut s).unwrap();
    assert!(p.uses_work_lifecycle_recipe());
    assert!(p.uses_policy_context_recipe());
    assert!(p.execution_registry().is_some());
    let (_arena, mut reloading) = session();
    let restored = load_local_selected_execution(
        policy_context_input(&support),
        p.compiled_plan().canonical_bytes(),
        p.compiled_plan().reference(),
        &mut reloading,
    )
    .unwrap();
    assert!(restored.uses_work_lifecycle_recipe());
    assert_eq!(restored.execution_registry(), p.execution_registry());
    assert_eq!(
        restored.compiled_plan().canonical_bytes(),
        p.compiled_plan().canonical_bytes()
    );
    let (_arena, mut old) = session();
    let old_support =
        SelectedCompilerSupport::local_policy_context_execution_with_session(&mut old).unwrap();
    let original =
        compile_selected_publication(policy_context_input(&old_support), &old_support, &mut old)
            .unwrap();
    assert!(!original.uses_work_lifecycle_recipe());
    assert_ne!(
        original.compiled_plan().canonical_bytes(),
        p.compiled_plan().canonical_bytes()
    );
    // This proves exact recipe dispatch/reload only. Failed/Reference/current
    // lifecycle input and original consumer completion are still SOURCE_HOLD.
}

#[test]
fn lifecycle_failed_work_uses_real_status_zero_steps_and_no_promised_heads() {
    // Production pricing/step/parent selectors; custody/append is host proof.
    let (_arena, mut s) = session();
    let support =
        SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut s).unwrap();
    let mut source = policy_context_input(&support);
    for raw in &mut source.documents {
        let mut value: Value = serde_json::from_slice(raw).unwrap();
        if value["schema"] == "ledger-workflow-policy/4" {
            value["rules"][0]["when"]["args"]
                .as_array_mut()
                .unwrap()
                .push(json!({"op":"eq","field":{"context":"status"},"value":"succeeded"}));
            source.root_policy_ref =
                serde_json::from_value(reference("workflow-policy", &value)).unwrap();
        }
        *raw = canonical::CanonicalBytes::from_value(&value)
            .unwrap()
            .into_vec();
    }
    let p = compile_selected_publication(source, &support, &mut s).unwrap();
    let context = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":"support:billable"}});
    let binding = json!({"agreement_id":"agreement","bindings":[{"slot":"support:retail"}],"funding_legs":[{"name":"support:aaa-platform","mode":"platform"},{"name":"support:customer","mode":"customer_local"}]});
    let inst = json!({"component_map":[{"template_key":"support:service","slot_id":"service"},{"template_key":"support:model-retail","slot_id":"app"}],"family_map":[{"template_key":"support:quality","slot_id":"quality"}]});
    for (status, quantity, atoms, heads) in [
        ("succeeded", "90", 150_000_000_000_000_000i128, 2usize),
        ("succeeded", "30", 0, 1),
        ("succeeded", "0", 0, 1),
        ("failed", "90", 0, 0),
        ("failed", "0", 0, 0),
    ] {
        let subject = json!({"activity":"support:complete","status":status,"facts":{"support:quantity":quantity,"support:approved":true}});
        let prices = pricing::work_subject(
            &p,
            ("support:complete", &subject),
            (&subject["facts"], &context),
            Some((&binding, "urn:example:original-worker")),
            s.meter_mut(),
        )
        .unwrap();
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0].atoms, atoms);
        assert_eq!(prices[0].failed, status == "failed");
        let mut selected = 0usize;
        input_selection::head_rows(
            &p,
            (
                &subject,
                &context,
                Some((&binding, "urn:example:original-worker")),
            ),
            &inst,
            s.meter_mut(),
            &mut |_, _, _| {
                selected += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(selected, heads);
        if status == "failed" {
            assert_eq!(prices[0].rational.numerator.to_string(), "0");
            assert_eq!(prices[0].rational.denominator.to_string(), "1");
            let key = super::super::OperationKey {
                logical_store_id: "store".into(),
                scope: ["org".into(), "local".into()],
                source: "urn:example:original-worker".into(),
                operation_id: "failed".into(),
            };
            let r = |kind: &str, prefix: &str| super::super::ExactRef {
                kind: kind.into(),
                id: format!("{prefix}{}", "a".repeat(64)),
                digest: format!("sha256:{}", "b".repeat(64)),
            };
            let posting = super::super::cost_postings::original_cost_posting_key(
                s.meter_mut(),
                &key,
                super::super::cost_postings::OriginalCostPosting {
                    binding: &binding,
                    original_work_claim: &r("workflow-work-claim", "wclaim_"),
                    component_slot: &json!("service"),
                    root_slot: &json!("service"),
                    book: &json!("retail"),
                    terms_ref: &r("workflow-slot-terms", "wdoc_"),
                    terms: &json!({"funding_leg":"support:customer"}),
                    role_ref: &r("workflow-roles", "wdoc_"),
                    decision_id: "decision",
                    revision: &1.into(),
                    replacement: false,
                    basis_refs: None,
                },
                &prices[0],
            )
            .unwrap();
            assert!(posting.action.is_none());
            assert_eq!(posting.step.value()["disposition"], "failed_work");
            assert_eq!(posting.step.value()["rounded"]["atoms"], "0");
            assert!(posting.step.value().get("action_ref").is_none());
        }
    }
    // A failed-only rule cannot witness reachable completion billing. The old
    // descriptor must not acquire status from the familiar typed AST shape.
    for lifecycle in [true, false] {
        let (_arena, mut s) = session();
        let support = if lifecycle {
            SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut s)
        } else {
            SelectedCompilerSupport::local_policy_context_execution_with_session(&mut s)
        }
        .unwrap();
        let mut source = policy_context_input(&support);
        for raw in &mut source.documents {
            let mut value: Value = serde_json::from_slice(raw).unwrap();
            if value["schema"] == "ledger-workflow-policy/4" {
                value["rules"][0]["when"] =
                    json!({"op":"eq","field":{"context":"status"},"value":"failed"});
                source.root_policy_ref =
                    serde_json::from_value(reference("workflow-policy", &value)).unwrap();
            }
            *raw = canonical::CanonicalBytes::from_value(&value)
                .unwrap()
                .into_vec();
        }
        let error = compile_selected_publication(source, &support, &mut s)
            .err()
            .unwrap();
        assert_eq!(
            error.detail,
            if lifecycle {
                "unreachable monetary rule"
            } else {
                "original predicate context capability"
            }
        );
    }
    // Price eligibility is narrower than the declared nonnegative fact
    // domain. A selected Unit needs positive quantity, including replacement;
    // a positive quantity that prices to zero remains a selected root.
    for zero_only in [true, false] {
        let (_arena, mut s) = session();
        let support =
            SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut s).unwrap();
        let mut source = policy_context_input(&support);
        for raw in &mut source.documents {
            let mut value: Value = serde_json::from_slice(raw).unwrap();
            if value["schema"] == "ledger-workflow-policy/4" {
                value["rules"][0]["when"] = if zero_only {
                    json!({"op":"eq","field":{"fact":"support:quantity"},"value":"0"})
                } else {
                    json!({"op":"true"})
                };
                value["rules"][0]["formula"]["rate"] = json!("0");
                source.root_policy_ref =
                    serde_json::from_value(reference("workflow-policy", &value)).unwrap();
            }
            *raw = canonical::CanonicalBytes::from_value(&value)
                .unwrap()
                .into_vec();
        }
        let compiled = compile_selected_publication(source, &support, &mut s);
        if zero_only {
            assert_eq!(compiled.err().unwrap().detail, "unreachable monetary rule");
            continue;
        }
        let p = compiled.unwrap();
        for (status, q, valid) in [
            ("succeeded", "0", false),
            ("failed", "0", true),
            ("succeeded", "1", true),
        ] {
            let subject = json!({"activity":"support:complete","status":status,"facts":{"support:quantity":q,"support:approved":true}});
            let prices = pricing::work_subject(
                &p,
                ("support:complete", &subject),
                (&subject["facts"], &context),
                Some((&binding, "urn:example:original-worker")),
                s.meter_mut(),
            );
            if !valid {
                assert_eq!(
                    prices.err().unwrap().detail,
                    "successful selected quantity must be positive"
                );
            } else {
                let prices = prices.unwrap();
                assert_eq!(prices[0].atoms, 0);
                assert!(prices[0].predicate);
                let mut heads = 0usize;
                input_selection::head_rows(
                    &p,
                    (
                        &subject,
                        &context,
                        Some((&binding, "urn:example:original-worker")),
                    ),
                    &inst,
                    s.meter_mut(),
                    &mut |_, _, _| {
                        heads += 1;
                        Ok(())
                    },
                )
                .unwrap();
                assert_eq!(heads, if status == "failed" { 0 } else { 2 });
            }
        }
    }
}

#[test]
fn lifecycle_input_edges_own_fixed_birth_and_named_revision_locators() {
    use super::super::{body, select_lifecycle_input_edges, EconomicRecordView, OperationKey};
    let (_arena, mut s) = session();
    let support =
        SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut s).unwrap();
    let p = compile_selected_publication(policy_context_input(&support), &support, &mut s).unwrap();
    // Compilation consumes its own original phase. Selection runs in a fresh
    // parent original frame under the same owning arena, before its child.
    let mut s = OriginalAuditSession::for_resource_profile_v2(&_arena).unwrap();
    let mut producer = super::super::CoreMeter::new(65536).unwrap();
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["company".into(), "scope".into()],
        source: "urn:example:worker".into(),
        operation_id: "fixed-identities".into(),
    };
    let claim = format!("wclaim_{}", "a".repeat(64));
    let decision = format!("wdecision_{}", "b".repeat(64));
    // Canonical framing DATA, not original receipt/member or relation proof.
    let work = body::record(
        &mut producer,
        &key,
        "workflow-work",
        format!("wevent_{}", "c".repeat(64)),
        json!({"schema":"ledger-workflow-work-record/1","claim_id":claim,"payload":{"facts":{}}}),
    )
    .unwrap();
    let selected = select_lifecycle_input_edges(
        &p,
        &key,
        EconomicRecordView {
            reference: &work.reference,
            canonical_body: &work.canonical_body,
        },
        &mut s,
    )
    .unwrap();
    drop(work);
    assert_eq!(selected.record_identities().len(), 1);
    let birth = &selected.record_identities()[0];
    assert_eq!(
        (birth.kind(), birth.id(), birth.schemas(), birth.role()),
        (
            "workflow-work-claim",
            claim.as_str(),
            &["ledger-workflow-work-claim/1"][..],
            "work_birth_claim"
        )
    );
    for posting in ["original", "inverse"] {
        let inverse_ref = json!({"kind":"workflow-action","id":format!("waction_{}","d".repeat(64)),"digest":format!("sha256:{}","e".repeat(64))});
        let mut value = json!({"schema":"ledger-workflow-action/1","decision_id":decision,"revision":"2","posting_role":posting});
        if posting == "inverse" {
            value["inverse_of"] = inverse_ref.clone();
        }
        let action = body::record(
            &mut producer,
            &key,
            "workflow-action",
            format!("waction_{}", "f".repeat(64)),
            value,
        )
        .unwrap();
        let selected = select_lifecycle_input_edges(
            &p,
            &key,
            EconomicRecordView {
                reference: &action.reference,
                canonical_body: &action.canonical_body,
            },
            &mut s,
        )
        .unwrap();
        drop(action);
        let expected = body::semantic_id(
            &mut producer,
            "wdrev_",
            "workflow-decision-revision",
            &json!([decision, "2"]),
        )
        .unwrap();
        let revision = &selected.record_identities()[0];
        assert_eq!(
            (revision.kind(), revision.id(), revision.role()),
            (
                "workflow-decision",
                expected.as_str(),
                "action_named_decision_revision"
            )
        );
        assert_eq!(revision.schemas().len(), 3);
        let refs = selected.direct_refs().collect::<Vec<_>>();
        assert_eq!(refs.len(), usize::from(posting == "inverse"));
        if posting == "inverse" {
            assert_eq!(serde_json::to_value(refs[0]).unwrap(), inverse_ref);
        }
    }
    let invalid = body::record(
        &mut producer,
        &key,
        "workflow-action",
        format!("waction_{}", "f".repeat(64)),
        json!({"schema":"ledger-workflow-action/1","decision_id":decision,"revision":"0"}),
    )
    .unwrap();
    assert!(select_lifecycle_input_edges(
        &p,
        &key,
        EconomicRecordView {
            reference: &invalid.reference,
            canonical_body: &invalid.canonical_body
        },
        &mut s
    )
    .is_err());
}

#[test]
fn lifecycle_fact_groups_select_real_wrappers_and_field_subset() {
    use super::super::{
        body, select_local_operation_locators, select_work_fact_reference_inputs, DraftStateData,
        EconomicCapture, EconomicRecordView, EconomicStateData, OperationKey,
    };
    let (arena, mut compile) = session();
    let support =
        SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut compile).unwrap();
    let p = compile_selected_publication(policy_context_input(&support), &support, &mut compile)
        .unwrap();
    let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let mut producer = super::super::CoreMeter::new(65536).unwrap();
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["company".into(), "scope".into()],
        source: "urn:example:worker".into(),
        operation_id: "fact-groups".into(),
    };
    let context = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{"support:service-eligibility":"support:billable"}});
    let binding = body::document(
        &mut producer,
        "workflow-binding",
        json!({"schema":"ledger-workflow-binding/2","accepted_context":context}),
    )
    .unwrap();
    let product = p
        .source_documents()
        .iter()
        .find(|d| d.reference().kind() == "workflow-product")
        .unwrap();
    let activity = product.canonical_value()["activities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "support:complete")
        .unwrap();
    let payload = json!({"schema":"ledger-workflow-work/1","activity":"support:complete","product_ref":product.reference(),"binding_ref":binding.reference,"fact_schema_ref":activity["fact_schema"],"facts":{"support:quantity":"90","support:approved":true}});
    let raw = canonical::CanonicalBytes::from_value(&payload).unwrap();
    let view = EconomicRecordView {
        reference: &binding.reference,
        canonical_body: &binding.canonical_body,
    };
    let selected =
        select_work_fact_reference_inputs(&p, &key, raw.as_slice(), view, &mut s).unwrap();
    assert_eq!(
        selected
            .groups()
            .iter()
            .map(|g| g.role())
            .collect::<Vec<_>>(),
        vec!["new_work", "accepted_context"]
    );
    assert!(selected
        .groups()
        .iter()
        .all(|g| g.inputs().entries().is_empty()));
    // A field assertion exists before its future Work; selecting it must not
    // require a fabricated complete Work fact map or an original Work body.
    let attestation = json!({"schema":"ledger-workflow-fact-attest/1","attestation":{"source":key.source,"attestation_id":"claim-subset","subject":{"kind":"work","binding_ref":binding.reference},"subject_schema_ref":activity["fact_schema"],"claims":[{"field":"support:quantity","value":"90"}]}});
    let raw = canonical::CanonicalBytes::from_value(&attestation).unwrap();
    let capture = EconomicCapture {
        key: &key,
        actor: "operator",
        canonical_economic_namespace: &key.source,
        installation_mode: "local",
        received_at: "2026-10-26T00:00:00.000000Z",
        accepted_at: "2026-10-26T00:00:00.000000Z",
        sources: &[],
    };
    let locators =
        select_local_operation_locators(&p, &capture, raw.as_slice(), &[view], &mut s).unwrap();
    let state = DraftStateData::default();
    let selected_state = EconomicStateData {
        retained_assertion: None,
        financial: &state,
        capacity_heads: &[],
        supplier: None,
        app: None,
        current_party_authorizations: &[],
        observation: None,
        audit: None,
        initial_source_correspondences: &[],
    };
    let selected = locators
        .select_fact_reference_inputs(&p, &key, raw.as_slice(), (&selected_state, &[view]), &mut s)
        .unwrap();
    assert_eq!(
        selected
            .groups()
            .iter()
            .map(|g| g.role())
            .collect::<Vec<_>>(),
        vec!["accepted_context", "asserted_fields"]
    );
}
