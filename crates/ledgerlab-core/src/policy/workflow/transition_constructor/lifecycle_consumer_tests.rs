//! Actual finite source plus direct consuming-kernel checks. Constructed records
//! here are DATA, not a host receipt/custody or a financial execution proof.
use super::*;
use crate::{
    canonical::{self, audit_arena::AuditArena},
    policy::workflow::selected_program::{
        compile_selected_publication, SelectedCompilerSupport, SelectedProgram, SelectedSourceInput,
    },
    validation_work::{OriginalAuditSession, ResourceLimits},
};
use serde_json::{json, Value};
fn source(support: &SelectedCompilerSupport) -> SelectedSourceInput {
    let v: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../examples/workflow-work-lifecycle/source.json"
    ))
    .unwrap();
    let mut documents: Vec<Vec<u8>> = v["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
        .collect();
    documents.push(support.canonical_bytes().to_vec());
    SelectedSourceInput {
        documents,
        root_policy_ref: serde_json::from_value(v["root_policy_ref"].clone()).unwrap(),
    }
}
fn session(a: &AuditArena) -> OriginalAuditSession {
    OriginalAuditSession::for_resource_profile_v2(a).unwrap()
}
pub(super) fn program(a: &AuditArena) -> SelectedProgram {
    let mut s = session(a);
    let support =
        SelectedCompilerSupport::local_work_lifecycle_execution_with_session(&mut s).unwrap();
    let p = compile_selected_publication(source(&support), &support, &mut s).unwrap();
    p.require_local_admission_with_session(&mut s).unwrap();
    p
}
#[test]
fn finite_lifecycle_actual_source_structural_admission_and_reference_birth_consumer() {
    let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
    let p = program(&a);
    assert_eq!(
        p.original_policy_value()["rules"].as_array().unwrap().len(),
        3
    );
    let mut producer_session = session(&a);
    let producer = producer_session.meter_mut();
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["example-company".into(), "local".into()],
        source: "urn:example:worker".into(),
        operation_id: "fixed-birth".into(),
    };
    let at = "2026-10-26T00:00:00.000000Z";
    let business: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../examples/workflow-work-lifecycle/business.json"
    ))
    .unwrap();
    let ceiling = business["worker_ceiling"].clone();
    let enrollment=body::document(producer,"workflow-source-enrollment",json!({"schema":"ledger-workflow-source-enrollment/1","enrollment_id":"worker-enrollment","scope":key.scope,"ceiling":ceiling})).unwrap();
    let mut grant = ceiling.clone();
    grant["schema"] = json!("ledger-workflow-source-grant/1");
    grant["grant_id"] = json!("worker-enrollment");
    grant["revision"] = json!("1");
    grant["original_ceiling_ref"] = json!(enrollment.reference);
    let grant = body::document(producer, "workflow-source-grant", grant).unwrap();
    let state_id = body::semantic_id(
        producer,
        "wsource_",
        "workflow-source-state",
        &json!([key.logical_store_id, key.scope, "worker-enrollment", "1"]),
    )
    .unwrap();
    let state=body::record(producer,&key,"workflow-source-state",state_id,json!({"schema":"ledger-workflow-source-state/1","enrollment_ref":enrollment.reference,"revision":"1","status":"active","current_grant_ref":grant.reference,"operation_key":key,"actor":"example-operator","accepted_at":at})).unwrap();
    let binding=body::document(producer,"workflow-binding",json!({"schema":"ledger-workflow-binding/2","customer":"customer-B","source_namespace":key.source,"authority_refs":[grant.reference],"source_slots":[{"name":"support:worker","sources":[key.source]}],"accepted_context":{"fact_schema_ref":p.context_schema_ref(),"facts":{"support:eligibility":"support:eligible"}}})).unwrap();
    let activity = &p.compiled_plan().canonical_value()["activity_table"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["activity"] == "support:fixed")
        .unwrap();
    let payload = json!({"schema":"ledger-workflow-work/1","external_event_id":"fixed-event","business_operation_id":"fixed-operation","customer":"customer-B","chain":"one-chain","activity":"support:fixed","product_ref":p.product_ref(),"policy_ref":p.root_policy_ref(),"fact_schema_ref":activity["fact_schema_ref"],"binding_ref":binding.reference,"status":"succeeded","facts":{"support:quantity":"3"},"fact_proofs":[],"links":[],"evidence_refs":[],"invocation_refs":[]});
    let event = work_keys::event(producer, &key, &payload).unwrap();
    let claim_id = work_keys::claim(producer, &key, &key.source, &payload).unwrap();
    let work=body::record(producer,&key,"workflow-work",event,json!({"schema":"ledger-workflow-work-record/1","key":key,"payload":payload,"claim_id":claim_id,"received_at":at,"accepted_at":at,"source_namespace":key.source})).unwrap();
    let digest = records::hash(
        producer,
        "workflow-claim-facts",
        &json!([
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
        ]),
    )
    .unwrap();
    let claim=body::record(producer,&key,"workflow-work-claim",claim_id.clone(),json!({"schema":"ledger-workflow-work-claim/1","logical_store_id":key.logical_store_id,"scope":key.scope,"source_namespace":key.source,"business_operation_id":"fixed-operation","completion":"completion","work_ref":work.reference,"facts_digest":digest})).unwrap();
    let auth=body::document(producer,"workflow-economic-authority",json!({"schema":"ledger-workflow-economic-authority/8","operation":"work.accept","scope":key.scope,"customer":"customer-B","original_binding_ref":binding.reference,"source":key.source,"canonical_economic_namespace":key.source,"observed_at":at,"source_slot":"support:worker","enrollment_ref":enrollment.reference,"current_state_ref":state.reference,"current_grant_ref":grant.reference,"original_grant_refs":[grant.reference],"actor":"example-operator"})).unwrap();
    let binding_value: Value = serde_json::from_slice(&binding.canonical_body).unwrap();
    let eval=body::document(producer,"workflow-evaluation",json!({"schema":"ledger-workflow-evaluation-proof/1","subject_ref":work.reference,"policy_ref":p.root_policy_ref(),"compiled_plan_ref":p.compiled_plan().reference(),"compiler_ref":p.compiled_plan().canonical_value()["compiler_ref"],"binding_ref":binding.reference,"fact_schema_ref":payload["fact_schema_ref"],"facts":payload["facts"],"context":binding_value["accepted_context"],"steps":[]})).unwrap();
    let permanent = work_keys::decision(producer, &claim_id).unwrap();
    let revision = body::semantic_id(
        producer,
        "wdrev_",
        "workflow-decision-revision",
        &json!([permanent, "1"]),
    )
    .unwrap();
    let decision=body::record(producer,&key,"workflow-decision",revision,json!({"schema":"ledger-workflow-decision-revision/1","decision_id":permanent,"revision":"1","subject_ref":work.reference,"kind":"work","transition":"initial","status":"active","original_terms_ref":binding.reference,"authority_ref":auth.reference,"evaluation_ref":eval.reference,"actions_appended":[],"active_action_refs":[],"dependency_refs":[],"operation_key":key,"accepted_at":at})).unwrap();
    let inputs = [
        &enrollment,
        &grant,
        &state,
        &binding,
        &work,
        &claim,
        &auth,
        &eval,
        &decision,
    ];
    let views: Vec<_> = inputs
        .iter()
        .map(|r| EconomicRecordView {
            reference: &r.reference,
            canonical_body: &r.canonical_body,
        })
        .collect();
    // Use the quantity schema's actual optional Reference field.
    let facts = json!({"support:quantity":"0.1","support:reference":work.reference});
    let mut s = session(&a);
    let arena = s.arena().shared_handle();
    let m = s.meter_mut();
    let mut r = records::Records::borrowed_parent_app(m, &arena, &key, &views).unwrap();
    let br = json!(binding.reference);
    fact_relations::fields(
        m,
        &mut r,
        &key,
        &p,
        (&br, &binding_value, at),
        (&payload["fact_schema_ref"], &facts),
    )
    .unwrap();
    fact_relations::facts(m, &mut r, &key, &p, &payload, &facts, at).unwrap();
    // The same authentic Work under a different original Binding is refused.
    let mut different = binding_value.clone();
    different["agreement_id"] = json!("different-genuine-binding");
    let different = body::document(producer, "workflow-binding", different).unwrap();
    let wrong = json!(different.reference);
    let different_body: Value = serde_json::from_slice(&different.canonical_body).unwrap();
    assert!(fact_relations::fields(
        m,
        &mut r,
        &key,
        &p,
        (&wrong, &different_body, at),
        (&payload["fact_schema_ref"], &facts)
    )
    .is_err());
    drop(r);
    let missing: Vec<_> = views
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 5)
        .map(|(_, v)| *v)
        .collect();
    let mut r = records::Records::borrowed_parent_app(m, &arena, &key, &missing).unwrap();
    assert!(fact_relations::fields(
        m,
        &mut r,
        &key,
        &p,
        (&br, &binding_value, at),
        (&payload["fact_schema_ref"], &facts)
    )
    .is_err());
    // A normal acceptance context with no present Reference remains valid.
    drop(r);
    drop(s);
    let mut s = session(&a);
    validate_lifecycle_context_references(&p, &key, views[3], &views, at, &mut s).unwrap();
}
