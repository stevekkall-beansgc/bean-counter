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
