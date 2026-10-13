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
// Acceptance results expose AcceptedAgreement, whose fully verified retained
// body names Binding. Read real committed data; never invent a returned Ref.
async fn accepted_binding_ref(ledger: &LocalWorkflowLedger, value: &Value) -> (RecordRef, Value) {
    let reference = result_ref(value, "workflow-accepted-agreement");
    let key = super::super::super::request::OperationKey::parse(
        &records::bytes(&envelope("read-accepted-binding", "agreement.accept", json!({}))["key"])
            .unwrap(),
    )
    .unwrap();
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    ledger
        .admin
        .read(&ledger.store, &mut tx, &key)
        .await
        .unwrap();
    let reference = proof_session::with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            verify::document_origin(tx, &key, &reference).await?;
            let document = loaded(tx, &key, &reference).await?;
            let body = canonical::parse_bounded(&document.bytes, 262144)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let binding: RecordRef = serde_json::from_value(body["binding_ref"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            require(binding.kind == "workflow-binding", "INTEGRITY_FAILURE")?;
            let binding_doc = loaded(tx, &key, &binding).await?;
            let binding_body = canonical_body(&binding_doc)?;
            Ok((binding, binding_body))
        })
    })
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    reference
}
fn diagnostic<'a, T: 'a>(
    label: &'a str,
    future: impl std::future::Future<Output = Result<T>> + 'a,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + 'a>> {
    diagnostic_expected(label, future, &[], &[])
}
fn diagnostic_expected<'a, T: 'a>(
    label: &'a str,
    future: impl std::future::Future<Output = Result<T>> + 'a,
    expected_origins: &'a [(Vec<u8>, usize)],
    expected_publications: &'a [(Vec<u8>, usize)],
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + 'a>> {
    // Keep the audit wrappers off the large captured journey future's stack.
    // The inner await order, task-local counts and returned error are unchanged.
    let future = Box::pin(future);
    Box::pin(async move {
        eprintln!("ACCEPT-DIAG {label} before audited call");
        let begun = std::time::Instant::now();
        let (result, origins, work) = verify::audit_work(future).await;
        let mut named: Vec<_> = origins
            .iter()
            .map(|(identity, count)| {
                (
                    super::super::super::request::OperationKey::parse(identity)
                        .unwrap()
                        .operation_id,
                    *count,
                )
            })
            .collect();
        named.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let publications: Vec<_> = work
            .publications
            .iter()
            .map(|(identity, count)| {
                (
                    super::super::super::request::OperationKey::parse(identity)
                        .unwrap()
                        .operation_id,
                    *count,
                )
            })
            .collect();
        eprintln!("ACCEPT-DIAG {label} elapsed_ms={} result={:?} origin_total={} origin_counts={:?} publication_total={} publication_counts={:?} document_origin_total={} selected_loaded_helper_total={}",
            begun.elapsed().as_millis(), result.as_ref().map(|_| "ok"), origins.values().sum::<usize>(), named,
            work.publications.values().sum::<usize>(), publications, work.document_origins.values().sum::<usize>(), work.helper_document_loads.values().sum::<usize>());
        if result.is_ok() {
            for (identity, expected) in expected_origins {
                assert_eq!(
                    origins.get(identity),
                    Some(expected),
                    "required actual origin proof count"
                );
            }
            for (identity, expected) in expected_publications {
                assert_eq!(
                    work.publications.get(identity),
                    Some(expected),
                    "required actual publication proof count"
                );
            }
        }
        result
    })
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
    if label == "B" {
        binding["ends_at"] = json!("2028-12-31T00:00:00.000000Z");
    }
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
async fn test_root_choice_must_be_in_actual_scoped_latest_candidate_set() {
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
    let evidence = proof(
        &ledger,
        "first-root-proof",
        "customer_registration",
        "customer-A",
        None,
        json!({"acceptor":"customer-A"}),
    )
    .await;
    // An actual retained evidence Ref is not an eligible owner-root candidate.
    // The hook must refuse rather than falling back to an arbitrary valid root.
    *ledger.preferred_root.lock().unwrap() = Some(evidence);
    let content = b"Explicit isolated alternate registration proof";
    let request = envelope(
        "not-eligible-root",
        "evidence.retain",
        json!({
        "schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration",
        "subject":{"customer":"customer-B"},"media_type":"text/plain;charset=utf-8",
        "content_base64":super::super::super::publication::base64(content),
        "content_sha256":records::raw_hash(content),"metadata":{"acceptor":"customer-B"}}),
    );
    assert_eq!(
        ledger
            .prerequisite(&records::bytes(&request).unwrap())
            .await,
        Err(ServiceError::IntegrityFailure)
    );
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 1);
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
#[ignore = "requires reviewed controlled build with actual executable provenance"]
async fn captured_r1_agreement_lifecycle_two_customers_original_alias_recovery_and_reopen() {
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
        diagnostic(
            "A1 agreement",
            ledger.agreement(&records::bytes(&a).unwrap())
        )
        .await,
        Err(ServiceError::ResponseLost)
    );
    let accepted_a = diagnostic("A1 status", ledger.status(&a_key.bytes().unwrap()))
        .await
        .unwrap();
    assert_eq!(accepted_a["status"], "committed");
    // Force the legitimate A1-origin tie/path observed in capture008.
    // The Ref comes from actual committed bytes, never a fixture or seeded row.
    let mut root_tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    ledger
        .admin
        .read(&ledger.store, &mut root_tx, &a_key)
        .await
        .unwrap();
    let a_operation = root_tx
        .workflow_agreement_lookup(&a_key.bytes().unwrap())
        .await
        .unwrap()
        .unwrap();
    let a_body = canonical::parse_bounded(&a_operation.operation_bytes, 8388608).unwrap();
    let a_root: RecordRef =
        serde_json::from_value(a_body["authority_snapshot"]["local_admin_ref"].clone()).unwrap();
    assert!(root_tx
        .workflow_test_latest_root_candidates(STORE, &a_key.scope)
        .await
        .unwrap()
        .iter()
        .any(|d| d.reference == a_root));
    let birth = root_tx
        .workflow_agreement_document_origin(STORE, &a_key.scope, &a_root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        birth,
        (
            a_key.bytes().unwrap(),
            "agreement.accept".into(),
            a_operation.sequence
        )
    );
    eprintln!(
        "ACCEPT-DIAG preferred eligible A1 root={:?} original_sequence={}",
        a_root, a_operation.sequence
    );
    root_tx.rollback().await.unwrap();
    *ledger.preferred_root.lock().unwrap() = Some(a_root.clone());
    let (b, enrollment_b) = input(&ledger, "B", &publication_ref).await;
    let mut metadata_tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    for id in [
        "B-registration-proof",
        "B-register",
        "B-enrollment-proof",
        "B-enroll",
        "B-funding-proof",
        "B-assent-proof",
        "B-operator-statement",
    ] {
        let key = super::super::super::request::OperationKey::parse(
            &records::bytes(&envelope(id, "evidence.retain", json!({}))["key"]).unwrap(),
        )
        .unwrap();
        let operation = metadata_tx
            .workflow_agreement_lookup(&key.bytes().unwrap())
            .await
            .unwrap()
            .unwrap();
        let body = canonical::parse_bounded(&operation.operation_bytes, 8388608).unwrap();
        assert_eq!(
            body["authority_snapshot"]["authority_refs"],
            json!([a_root])
        );
        for value in body["result_refs"].as_array().unwrap() {
            let reference: RecordRef = serde_json::from_value(value.clone()).unwrap();
            if matches!(
                reference.kind.as_str(),
                "workflow-evidence" | "workflow-customer" | "workflow-source-enrollment"
            ) {
                let document = metadata_tx
                    .workflow_document(&reference, STORE, &a_key.scope[0], &a_key.scope[1], None)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    canonical::parse(&document.bytes).unwrap()["authority_ref"],
                    json!(a_root)
                );
            }
        }
    }
    metadata_tx.rollback().await.unwrap();
    let published_key = super::super::super::request::OperationKey::parse(
        &records::bytes(&publication["key"]).unwrap(),
    )
    .unwrap();
    let accepted_b = diagnostic_expected(
        "B1 agreement",
        ledger.agreement(&records::bytes(&b).unwrap()),
        &[(a_key.bytes().unwrap(), 2)],
        &[(published_key.bytes().unwrap(), 2)],
    )
    .await
    .unwrap();
    let b_key =
        super::super::super::request::OperationKey::parse(&records::bytes(&b["key"]).unwrap())
            .unwrap();
    assert_eq!(
        diagnostic_expected(
            "B1 status",
            ledger.status(&b_key.bytes().unwrap()),
            &[(a_key.bytes().unwrap(), 1)],
            &[(published_key.bytes().unwrap(), 1)]
        )
        .await
        .unwrap()["receipt"],
        accepted_b["receipt"]
    );
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
        eprintln!("ACCEPT-DIAG successor{version} before prerequisite proofs");
        last = successor(&ledger, &a, version, &context).await;
        eprintln!("ACCEPT-DIAG successor{version} before agreement");
        last_receipt = diagnostic(
            &format!("successor{version}"),
            ledger.agreement(&records::bytes(&last).unwrap()),
        )
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
    // Actual narrowed grant2 stays compatible with the complete bound projection.
    let mut source_tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let enrolled = loaded(&mut source_tx, &a_key, &enrollment_a).await.unwrap();
    let mut ceiling = canonical_body(&enrolled).unwrap()["ceiling"].clone();
    ceiling["starts_at"] = json!("2021-01-01T00:00:00.000000Z");
    source_tx.rollback().await.unwrap();
    let narrowed = envelope(
        "A-narrow",
        "authority.change",
        json!({"schema":"ledger-workflow-source-restrict/1","action":"source.restrict","enrollment_ref":enrollment_a,"expected_revision":"1","replacement_ceiling":ceiling}),
    );
    ledger
        .prerequisite(&records::bytes(&narrowed).unwrap())
        .await
        .unwrap();
    let (a_binding, a_binding_body) = accepted_binding_ref(&ledger, &last_receipt).await;
    let (b_binding, b_binding_body) = accepted_binding_ref(&ledger, &accepted_b).await;
    fn activate_input(
        id: &str,
        binding_ref: &RecordRef,
        binding_payload: &Value,
        expected: &str,
        effective: Value,
    ) -> Value {
        envelope(
            id,
            "agreement.activate",
            json!({"schema":"ledger-workflow-activation/1","agreement_id":binding_payload["agreement_id"],"expected_revision":expected,"accepted_binding_ref":binding_ref,"policy_ref":binding_payload["policy_ref"],"product_ref":binding_payload["product_ref"],"fact_schema_refs":binding_payload["fact_schema_refs"],"compiled_plan_ref":binding_payload["compiled_plan_ref"],"effective":effective,"evidence_refs":[]}),
        )
    }
    let activate_a = activate_input(
        "A-activate",
        &a_binding,
        &a_binding_body,
        "0",
        json!({"kind":"at_commit"}),
    );
    let activate_a_key = super::super::super::request::OperationKey::parse(
        &records::bytes(&activate_a["key"]).unwrap(),
    )
    .unwrap();
    super::execute::inject_committed_reply_loss(activate_a_key.bytes().unwrap());
    assert_eq!(
        diagnostic(
            "A activate narrowed lost reply",
            ledger.agreement(&records::bytes(&activate_a).unwrap())
        )
        .await,
        Err(ServiceError::ResponseLost)
    );
    let active_a = ledger
        .status(&activate_a_key.bytes().unwrap())
        .await
        .unwrap();
    let activate_b = activate_input(
        "B-activate",
        &b_binding,
        &b_binding_body,
        "0",
        json!({"kind":"at_commit"}),
    );
    let active_b = diagnostic(
        "B activate",
        ledger.agreement(&records::bytes(&activate_b).unwrap()),
    )
    .await
    .unwrap();
    let pending_input = activate_input(
        "A-pending",
        &a_binding,
        &a_binding_body,
        "1",
        json!({"kind":"not_before","at":"2029-01-01T00:00:00.000000Z"}),
    );
    let pending = ledger
        .agreement(&records::bytes(&pending_input).unwrap())
        .await
        .unwrap();
    let mut replace_input = activate_input(
        "A-replace",
        &a_binding,
        &a_binding_body,
        "2",
        json!({"kind":"not_before","at":"2029-02-01T00:00:00.000000Z"}),
    );
    replace_input["payload"]["schema"] = json!("ledger-workflow-activation-replace/1");
    replace_input["payload"]["replaces_transition_ref"] =
        json!(result_ref(&pending, "workflow-activation"));
    let replaced = ledger
        .agreement(&records::bytes(&replace_input).unwrap())
        .await
        .unwrap();
    let target = result_ref(&replaced, "workflow-activation");
    let stop_proof = proof(&ledger,"A-stop-proof","admission_stop","customer-A",Some("agreement-A"),json!({"binding_ref":a_binding,"target_transition_ref":target,"reason":"Explicit pending cancellation and immediate stop"})).await;
    let stop_input = envelope(
        "A-stop",
        "agreement.revoke",
        json!({"schema":"ledger-workflow-agreement-revoke/1","agreement_id":"agreement-A","expected_revision":"3","accepted_binding_ref":a_binding,"transition_ref":target,"reason":"Explicit pending cancellation and immediate stop","evidence_refs":[stop_proof]}),
    );
    let mut bad_stop = stop_input.clone();
    bad_stop["key"]["operation_id"] = json!("A-stop-wrong-active-target");
    bad_stop["payload"]["transition_ref"] = json!(result_ref(&active_a, "workflow-activation"));
    let bad_target_proof=proof(&ledger,"A-stop-active-proof","admission_stop","customer-A",Some("agreement-A"),json!({"binding_ref":a_binding,"target_transition_ref":result_ref(&active_a,"workflow-activation"),"reason":"Explicit pending cancellation and immediate stop"})).await;
    bad_stop["payload"]["evidence_refs"] = json!([bad_target_proof]);
    assert_eq!(
        ledger.agreement(&records::bytes(&bad_stop).unwrap()).await,
        Err(reject("ACTIVATION_SCHEDULE_CONFLICT"))
    );
    let stopped = ledger
        .agreement(&records::bytes(&stop_input).unwrap())
        .await
        .unwrap();
    let pretty = serde_json::to_vec_pretty(&a).unwrap();
    assert_eq!(
        diagnostic("A1 original retry", ledger.agreement(&pretty))
            .await
            .unwrap()["receipt"],
        accepted_a["receipt"]
    );
    let revoked = envelope(
        "revoke-A-work-source",
        "authority.change",
        json!({"schema":"ledger-workflow-source-revoke/1","action":"source.revoke",
        "enrollment_ref":enrollment_a,"expected_revision":"2","reason":"Explicit test stop after acceptance"}),
    );
    ledger
        .prerequisite(&records::bytes(&revoked).unwrap())
        .await
        .unwrap();
    for (id, original, receipt) in [
        ("A-accept-alias-after-revoke", &last, &last_receipt),
        ("A-activate-alias-after-revoke", &activate_a, &active_a),
        ("A-stop-alias-after-revoke", &stop_input, &stopped),
    ] {
        let mut alias = original.clone();
        alias["key"]["operation_id"] = json!(id);
        let result = ledger
            .agreement(&records::bytes(&alias).unwrap())
            .await
            .unwrap();
        assert_eq!(result["receipt"]["result_kind"], "semantic_alias");
        assert_eq!(
            result["receipt"]["result_refs"],
            receipt["receipt"]["result_refs"]
        );
    }
    let mut expiry_tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let b_enrolled = loaded(&mut expiry_tx, &a_key, &enrollment_b).await.unwrap();
    let mut b_ceiling = canonical_body(&b_enrolled).unwrap()["ceiling"].clone();
    b_ceiling["ends_at"] = json!("2026-10-13T00:00:00.000000Z");
    expiry_tx.rollback().await.unwrap();
    let expire_b = envelope(
        "B-expire-source",
        "authority.change",
        json!({"schema":"ledger-workflow-source-restrict/1","action":"source.restrict","enrollment_ref":enrollment_b,"expected_revision":"1","replacement_ceiling":b_ceiling}),
    );
    ledger
        .prerequisite(&records::bytes(&expire_b).unwrap())
        .await
        .unwrap();
    let mut b_alias = activate_b.clone();
    b_alias["key"]["operation_id"] = json!("B-activate-alias-after-expiry");
    let b_aliased = ledger
        .agreement(&records::bytes(&b_alias).unwrap())
        .await
        .unwrap();
    assert_eq!(b_aliased["receipt"]["result_kind"], "semantic_alias");
    assert_eq!(
        b_aliased["receipt"]["result_refs"],
        active_b["receipt"]["result_refs"]
    );
    let b_alias_key = super::super::super::request::OperationKey::parse(
        &records::bytes(&b_alias["key"]).unwrap(),
    )
    .unwrap();
    let mut alias_tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let b_alias_op = alias_tx
        .workflow_agreement_lookup(&b_alias_key.bytes().unwrap())
        .await
        .unwrap()
        .unwrap();
    let b_alias_body = canonical::parse_bounded(&b_alias_op.operation_bytes, 8388608).unwrap();
    assert_eq!(
        b_alias_body["authority_snapshot"]["source_observation"]["status"],
        "active"
    );
    assert_eq!(
        b_alias_body["authority_snapshot"]["source_observation"]["grant_time_status"],
        "expired"
    );
    alias_tx.rollback().await.unwrap();
    let mut denied = activate_a.clone();
    denied["key"]["operation_id"] = json!("A-fresh-after-source-revoke");
    denied["payload"]["expected_revision"] = json!("4");
    assert_eq!(
        ledger.agreement(&records::bytes(&denied).unwrap()).await,
        Err(reject("AUTHORITY_REQUIRED"))
    );
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
        diagnostic(
            "A5 reopened original retry",
            reopened.agreement(&serde_json::to_vec_pretty(&last).unwrap())
        )
        .await
        .unwrap()["receipt"],
        last_receipt["receipt"]
    );
    assert_eq!(
        diagnostic("A1 reopened original retry", reopened.agreement(&pretty))
            .await
            .unwrap()["receipt"],
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
    assert_eq!(
        tx.workflow_agreement_head(
            STORE,
            &a_key.scope,
            "customer-A",
            "agreement-A",
            crate::store::sqlite::workflow::agreement::acceptance::HeadKind::Activation
        )
        .await
        .unwrap()
        .unwrap()
        .revision,
        4
    );
    assert_eq!(
        tx.workflow_agreement_head(
            STORE,
            &a_key.scope,
            "customer-B",
            "agreement-B",
            crate::store::sqlite::workflow::agreement::acceptance::HeadKind::Activation
        )
        .await
        .unwrap()
        .unwrap()
        .revision,
        1
    );
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 43);
    let next_sequence = tx.workflow_sequence().await.unwrap();
    let a_history = activation_verify::schedule::history_before(
        &mut tx,
        &a_key,
        "customer-A",
        "agreement-A",
        next_sequence,
    )
    .await
    .unwrap();
    let b_history = activation_verify::schedule::history_before(
        &mut tx,
        &a_key,
        "customer-B",
        "agreement-B",
        next_sequence,
    )
    .await
    .unwrap();
    let now = crate::local::now().unwrap();
    assert_eq!(
        activation_verify::schedule::selected_binding(&a_history, &now).unwrap(),
        None
    );
    assert_eq!(
        activation_verify::schedule::selected_binding(&b_history, &now).unwrap(),
        Some(b_binding.clone())
    );
    assert_eq!(
        activation_verify::schedule::selected_binding(
            &b_history,
            &Timestamp::parse("2029-03-01T00:00:00.000000Z").unwrap()
        )
        .unwrap(),
        None,
        "finite B expiry never resumes an older binding"
    );
    assert_eq!(
        activation_verify::schedule::selected_binding(
            &a_history,
            &Timestamp::parse("2029-03-01T00:00:00.000000Z").unwrap()
        )
        .unwrap(),
        None
    );
    tx.rollback().await.unwrap();
    for (request, response) in [
        (&activate_a, &active_a),
        (&activate_b, &active_b),
        (&stop_input, &stopped),
    ] {
        assert_eq!(
            reopened
                .agreement(&serde_json::to_vec_pretty(request).unwrap())
                .await
                .unwrap()["receipt"],
            response["receipt"]
        );
    }
    reopened.close().await;
}
