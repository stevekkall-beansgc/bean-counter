//! Actual local-owner captured journey. Proof text is explicit test input;
//! this exercises the accountability model, never commercial assent or money.
use super::super::*;
use crate::{
    billing::BillingLedger,
    service::workflow::LocalWorkflowLedger,
    store::ports::{AcceptanceStore, AcceptanceTx},
};
use ledgerlab_core::policy::workflow::{AcceptanceTerms, FactSchema};
const STORE: &str = "captured-agreement-path-test";
fn envelope(id: &str, kind: &str, payload: Value) -> Value {
    json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":kind,
        "key":{"logical_store_id":STORE,"scope":["example-company","local"],"source":"urn:example:work","operation_id":id},"payload":payload})
}
fn record(name: &str) -> Value {
    let f:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json")).unwrap();
    f["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap()["body"]
        .clone()
}
fn data_document(kind: &str, body: Value) -> Document {
    records::source_document(kind, body).unwrap()
}
fn result_ref(value: &Value, kind: &str) -> RecordRef {
    serde_json::from_value(
        value["receipt"]["result_refs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == kind)
            .unwrap()
            .clone(),
    )
    .unwrap()
}
async fn retained(ledger: &LocalWorkflowLedger, id: &str, payload: Value) -> Value {
    ledger
        .prerequisite(&records::bytes(&envelope(id, "evidence.retain", payload)).unwrap())
        .await
        .unwrap()
}
async fn proof(
    ledger: &LocalWorkflowLedger,
    id: &str,
    purpose: &str,
    customer: &str,
    agreement: Option<&str>,
    metadata: Value,
) -> RecordRef {
    let content=b"Explicit isolated test proof material supplied by the local operator, not commercial consent.";
    let mut subject = json!({"customer":customer});
    if let Some(agreement) = agreement {
        subject["agreement_id"] = json!(agreement);
    }
    let response=retained(ledger,id,json!({"schema":"ledger-workflow-evidence-retain/1","purpose":purpose,
        "subject":subject,"media_type":"text/plain;charset=utf-8",
        "content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":metadata})).await;
    result_ref(&response, "workflow-evidence")
}
async fn input(
    ledger: &LocalWorkflowLedger,
    label: &str,
    publication_ref: &RecordRef,
) -> (Value, RecordRef) {
    let customer = format!("customer-{label}");
    let agreement = format!("agreement-{label}");
    let registration = proof(
        ledger,
        &format!("{label}-registration-proof"),
        "customer_registration",
        &customer,
        None,
        json!({"acceptor":customer}),
    )
    .await;
    let registered=ledger.prerequisite(&records::bytes(&envelope(&format!("{label}-register"),"authority.change",
        json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0",
            "customer":customer,"acceptor":customer,"registration_evidence_ref":registration}))).unwrap()).await.unwrap();
    let customer_ref = result_ref(&registered, "workflow-customer");
    let enrollment_evidence = proof(
        ledger,
        &format!("{label}-enrollment-proof"),
        "source_enrollment",
        &customer,
        None,
        json!({"source":"urn:example:work","principal_id":"example-operator"}),
    )
    .await;
    let enrollment=ledger.prerequisite(&records::bytes(&envelope(&format!("{label}-enroll"),"authority.change",
        json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0",
            "enrollment_id":format!("test-enrollment-{label}"),"customer_refs":[customer_ref],"evidence_ref":enrollment_evidence,
            "ceiling":{"principal_id":"example-operator","source":"urn:example:work","canonical_economic_namespace":"urn:example:work",
                "customer_scope":[customer],"operations":["work.accept","quantity.correct"],"product_selectors":[{"names":["fixture.report:product"]}],
                "activity_selectors":[{"names":["fixture.report:completed"]}],"link_selectors":[],"source_slots":["fixture.report:worker"],
                "permissions":["read","submit","correct"],"starts_at":"2020-01-01T00:00:00.000000Z","ends_at":"2030-01-01T00:00:00.000000Z"}}))).unwrap()).await.unwrap();
    let grant_ref = result_ref(&enrollment, "workflow-source-grant");
    let enrollment_ref = result_ref(&enrollment, "workflow-source-enrollment");
    let funding = proof(
        ledger,
        &format!("{label}-funding-proof"),
        "funding",
        &customer,
        Some(&agreement),
        json!({"resource_owner":customer,"payer":customer,"mode":"customer_local"}),
    )
    .await;
    // Fixture shapes are assertion input only. All trust roots above were
    // produced through the actual held owner/public prerequisite path.
    let mut role = record(&format!("{label}.role"));
    role["scope"] = json!(["example-company", "local"]);
    let role = data_document("workflow-roles", role);
    let catalog = data_document("workflow-slot-catalog", record(&format!("{label}.catalog")));
    let mut instantiation = record(&format!("{label}.instantiation"));
    instantiation["catalog_ref"] = json!(catalog.reference);
    let instantiation = data_document("workflow-slot-instantiation", instantiation);
    let mut slot = record(&format!("{label}.slot_terms"));
    slot["scope"] = json!(["example-company", "local"]);
    slot["role_ref"] = json!(role.reference);
    slot["source_grant_refs"] = json!([grant_ref]);
    let slot = data_document("workflow-slot-terms", slot);
    let mut terms = record(&format!("{label}.terms"));
    terms["scope"] = json!(["example-company", "local"]);
    terms["publication_ref"] = json!(publication_ref);
    let binding = &mut terms["binding_inputs"];
    binding["source_namespace"] = json!("urn:example:work");
    binding["roles_refs"] = json!([role.reference]);
    binding["slot_catalog_ref"] = json!(catalog.reference);
    binding["slot_instantiation_ref"] = json!(instantiation.reference);
    binding["bindings"][0]["binding_ref"] = json!(slot.reference);
    binding["source_slots"][0]["sources"] = json!(["urn:example:work"]);
    binding["funding_legs"][0]["evidence_refs"] = json!([funding]);
    let expected:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
    let context = FactSchema::parse(
        &records::bytes(
            &expected["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "context")
                .unwrap()["body"],
        )
        .unwrap(),
    )
    .unwrap();
    let typed = AcceptanceTerms::parse(&records::bytes(&terms).unwrap(), &context).unwrap();
    let terms = data_document("workflow-acceptance-terms", typed.canonical_value().clone());
    let consent_at = crate::local::now().unwrap();
    let consent = proof(
        ledger,
        &format!("{label}-assent-proof"),
        "assent",
        &customer,
        Some(&agreement),
        json!({"terms_digest":typed.terms_digest(),"acceptor":customer,"accepted_at":consent_at}),
    )
    .await;
    let attestation=proof(ledger,&format!("{label}-operator-statement"),"operator_attestation",&customer,Some(&agreement),
        json!({"terms_digest":typed.terms_digest(),"acceptor":customer,"assent_evidence_ref":consent,"statement":"operator_verified_exact_terms_acceptance"})).await;
    let assent = data_document(
        "workflow-assent",
        json!({"schema":"ledger-workflow-assent/1","scope":["example-company","local"],
        "customer":customer,"agreement_id":agreement,"terms_digest":typed.terms_digest(),"mode":"real","acceptor":customer,
        "accepted_at":consent_at,"evidence_ref":consent,"operator":"example-operator","operator_attestation_ref":attestation}),
    );
    let mut inputs = typed.canonical_value()["binding_inputs"].clone();
    inputs["assent_refs"] = json!([assent.reference]);
    inputs["authority_refs"] = json!([grant_ref]);
    let documents = vec![role, catalog, instantiation, slot, terms, assent]
        .into_iter()
        .map(|d| canonical::parse(&d.bytes).unwrap())
        .collect::<Vec<_>>();
    (
        envelope(
            &format!("{label}-accept"),
            "agreement.accept",
            json!({"schema":"ledger-workflow-agreement-accept/2",
        "agreement_id":agreement,"expected_revision":"0","publication_ref":publication_ref,
        "slot_catalog_ref":inputs["slot_catalog_ref"],"slot_instantiation_ref":inputs["slot_instantiation_ref"],
        "assent_refs":inputs["assent_refs"],"authority_refs":inputs["authority_refs"],"binding_inputs":inputs,"documents":documents}),
        ),
        enrollment_ref,
    )
}

async fn successor(
    ledger: &LocalWorkflowLedger,
    a: &Value,
    version: u64,
    context: &FactSchema,
) -> Value {
    let mut inputs = a["payload"]["binding_inputs"].clone();
    inputs["version"] = json!(version.to_string());
    inputs.as_object_mut().unwrap().remove("assent_refs");
    inputs.as_object_mut().unwrap().remove("authority_refs");
    let body = json!({"schema":"ledger-workflow-acceptance-terms/1","scope":["example-company","local"],
        "customer":"customer-A","agreement_id":"agreement-A","version":version.to_string(),
        "publication_ref":a["payload"]["publication_ref"],"binding_inputs":inputs});
    let terms = AcceptanceTerms::parse(&records::bytes(&body).unwrap(), context).unwrap();
    let at = crate::local::now().unwrap();
    let consent = proof(
        ledger,
        &format!("A-v{version}-consent"),
        "assent",
        "customer-A",
        Some("agreement-A"),
        json!({"terms_digest":terms.terms_digest(),"acceptor":"customer-A","accepted_at":at}),
    )
    .await;
    let statement=proof(ledger,&format!("A-v{version}-statement"),"operator_attestation","customer-A",Some("agreement-A"),
        json!({"terms_digest":terms.terms_digest(),"acceptor":"customer-A","assent_evidence_ref":consent,
            "statement":"operator_verified_exact_terms_acceptance"})).await;
    let assent = data_document(
        "workflow-assent",
        json!({"schema":"ledger-workflow-assent/1","scope":["example-company","local"],
        "customer":"customer-A","agreement_id":"agreement-A","terms_digest":terms.terms_digest(),"mode":"real","acceptor":"customer-A",
        "accepted_at":at,"evidence_ref":consent,"operator":"example-operator","operator_attestation_ref":statement}),
    );
    let mut successor = a["payload"].clone();
    successor["expected_revision"] = json!((version - 1).to_string());
    successor["binding_inputs"]["version"] = json!(version.to_string());
    successor["binding_inputs"]["assent_refs"] = json!([assent.reference]);
    successor["assent_refs"] = json!([assent.reference]);
    successor["documents"] = json!([
        terms.canonical_value(),
        canonical::parse(&assent.bytes).unwrap()
    ]);
    envelope(
        &format!("A-accept-v{version}"),
        "agreement.accept",
        successor,
    )
}

#[tokio::test]
#[ignore = "requires reviewed controlled build with actual executable provenance"]
async fn captured_agreement_public_two_customers_accept_original_recovery_and_reopen() {
    assert!(crate::build_identity::embedded_inputs().is_ok());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!(STORE);
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let expected:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
    let mut publication = expected["normalized_request"].clone();
    publication["key"] = envelope("publish-shared", "policy.publish", json!({}))["key"].clone();
    let published = ledger
        .publish(&records::bytes(&publication).unwrap())
        .await
        .unwrap();
    let publication_ref = result_ref(&published, "workflow-publication");
    let (a, enrollment_a) = input(&ledger, "A", &publication_ref).await;
    let a_key =
        super::super::super::request::OperationKey::parse(&records::bytes(&a["key"]).unwrap())
            .unwrap();
    super::execute::inject_committed_reply_loss(a_key.bytes().unwrap());
    assert_eq!(
        ledger.agreement(&records::bytes(&a).unwrap()).await,
        Err(ServiceError::ResponseLost)
    );
    let accepted_a = ledger.status(&a_key.bytes().unwrap()).await.unwrap();
    assert_eq!(accepted_a["status"], "committed");
    let (b, _) = input(&ledger, "B", &publication_ref).await;
    let accepted_b = ledger
        .agreement(&records::bytes(&b).unwrap())
        .await
        .unwrap();
    assert_ne!(
        result_ref(&accepted_a, "workflow-accepted-agreement"),
        result_ref(&accepted_b, "workflow-accepted-agreement")
    );
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    for customer in ["customer-A", "customer-B"] {
        let agreement = customer.replace("customer", "agreement");
        let head = tx
            .workflow_agreement_head(
                STORE,
                &["example-company".into(), "local".into()],
                customer,
                &agreement,
                crate::store::sqlite::workflow::agreement::acceptance::HeadKind::Acceptance,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(head.revision, 1);
        assert!(tx
            .workflow_agreement_head(
                STORE,
                &["example-company".into(), "local".into()],
                customer,
                &agreement,
                crate::store::sqlite::workflow::agreement::acceptance::HeadKind::Activation
            )
            .await
            .unwrap()
            .is_none());
    }
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 17);
    tx.rollback().await.unwrap();
    let context = FactSchema::parse(
        &records::bytes(
            &expected["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "context")
                .unwrap()["body"],
        )
        .unwrap(),
    )
    .unwrap();
    let mut last = a.clone();
    let mut last_receipt = accepted_a.clone();
    for version in 2..=5 {
        last = successor(&ledger, &a, version, &context).await;
        last_receipt = ledger
            .agreement(&records::bytes(&last).unwrap())
            .await
            .unwrap();
    }
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(
        tx.workflow_agreement_head(
            STORE,
            &["example-company".into(), "local".into()],
            "customer-A",
            "agreement-A",
            crate::store::sqlite::workflow::agreement::acceptance::HeadKind::Acceptance
        )
        .await
        .unwrap()
        .unwrap()
        .revision,
        5
    );
    assert_eq!(
        tx.workflow_agreement_head(
            STORE,
            &["example-company".into(), "local".into()],
            "customer-B",
            "agreement-B",
            crate::store::sqlite::workflow::agreement::acceptance::HeadKind::Acceptance
        )
        .await
        .unwrap()
        .unwrap()
        .revision,
        1
    );
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 29);
    tx.rollback().await.unwrap();
    let pretty = serde_json::to_vec_pretty(&a).unwrap();
    assert_eq!(
        ledger.agreement(&pretty).await.unwrap()["receipt"],
        accepted_a["receipt"]
    );
    let revoked = envelope(
        "revoke-A-work-source",
        "authority.change",
        json!({"schema":"ledger-workflow-source-revoke/1","action":"source.revoke",
        "enrollment_ref":enrollment_a,"expected_revision":"1","reason":"Explicit test stop after acceptance"}),
    );
    ledger
        .prerequisite(&records::bytes(&revoked).unwrap())
        .await
        .unwrap();
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let latest_key =
        super::super::super::request::OperationKey::parse(&records::bytes(&last["key"]).unwrap())
            .unwrap();
    let (latest_status, counts) =
        verify::audit_origins(reopened.status(&latest_key.bytes().unwrap()))
            .await
            .unwrap();
    assert_eq!(latest_status["receipt"], last_receipt["receipt"]);
    let accepts: Vec<_> = counts
        .iter()
        .filter(|(identity, _)| {
            let key = super::super::super::request::OperationKey::parse(identity).unwrap();
            key.operation_id == "A-accept" || key.operation_id.starts_with("A-accept-v")
        })
        .collect();
    assert_eq!(accepts.len(), 5);
    for (_, count) in accepts {
        assert_eq!(
            *count, 1,
            "each prior acceptance certificate is verified once"
        );
    }
    assert_eq!(
        reopened
            .agreement(&serde_json::to_vec_pretty(&last).unwrap())
            .await
            .unwrap()["receipt"],
        last_receipt["receipt"]
    );
    assert_eq!(
        reopened.agreement(&pretty).await.unwrap()["receipt"],
        accepted_a["receipt"]
    );
    let key =
        super::super::super::request::OperationKey::parse(&records::bytes(&a["key"]).unwrap())
            .unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["receipt"],
        accepted_a["receipt"]
    );
    let mut tx = reopened
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 30);
    tx.rollback().await.unwrap();
    reopened.close().await;
}
