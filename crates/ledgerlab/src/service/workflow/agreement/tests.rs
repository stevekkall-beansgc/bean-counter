//! Actual local-owner prerequisite path. Test proof material is explicitly supplied;
//! no synthetic dictionary is used by a production authority/evidence factory.
use super::*;
use crate::{
    billing::BillingLedger,
    service::workflow::LocalWorkflowLedger,
    store::ports::{AcceptanceStore, AcceptanceTx},
};
async fn installation() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!("prerequisite-path-test");
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    (dir, path)
}
fn envelope(id: &str, kind: &str, payload: Value) -> Value {
    json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":kind,"key":{"logical_store_id":"prerequisite-path-test","scope":["example-company","local"],"source":"urn:example:work","operation_id":id},"payload":payload})
}
fn proof_payload() -> Value {
    let content=b"Explicit test proof material supplied through the local operator route; no commercial acceptance.";
    json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"test-customer-A"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"test-acceptor-A"}})
}
#[tokio::test]
async fn agreement_actual_owner_evidence_registration_original_alias_conflict_reopen() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let proof = envelope("explicit-proof", "evidence.retain", proof_payload());
    let raw = records::bytes(&proof).unwrap();
    let committed = ledger.prerequisite(&raw).await.unwrap();
    assert_eq!(committed["receipt"]["commit_sequence"], "1");
    assert_eq!(committed["receipt"]["result_kind"], "administrative");
    let proof_ref: RecordRef =
        serde_json::from_value(committed["receipt"]["result_refs"][0].clone()).unwrap();
    let key = super::super::request::OperationKey::parse(&records::bytes(&proof["key"]).unwrap())
        .unwrap();
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let retained = loaded(&mut tx, &key, &proof_ref).await.unwrap();
    let body = canonical_body(&retained).unwrap();
    assert_eq!(body["retained_by"], "example-operator");
    assert_eq!(body["installation_mode"], "real_local");
    assert_eq!(body["content_base64"], proof["payload"]["content_base64"]);
    tx.rollback().await.unwrap();
    let pretty = serde_json::to_vec_pretty(&proof).unwrap();
    let retry = ledger.prerequisite(&pretty).await.unwrap();
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], committed["receipt"]);
    assert_eq!(retry["request_evidence"], committed["request_evidence"]);
    let mut alias = proof.clone();
    alias["key"]["operation_id"] = json!("proof-new-key");
    let aliased = ledger
        .prerequisite(&records::bytes(&alias).unwrap())
        .await
        .unwrap();
    assert_eq!(aliased["duplicate_kind"], "semantic");
    assert_eq!(
        aliased["receipt"]["result_refs"],
        committed["receipt"]["result_refs"]
    );
    let registration = envelope(
        "explicit-register",
        "authority.change",
        json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0","customer":"test-customer-A","acceptor":"test-acceptor-A","registration_evidence_ref":proof_ref}),
    );
    let registered = ledger
        .prerequisite(&records::bytes(&registration).unwrap())
        .await
        .unwrap();
    assert_eq!(registered["receipt"]["commit_sequence"], "3");
    let mut alias = registration.clone();
    alias["key"]["operation_id"] = json!("register-new-key");
    let aliased = ledger
        .prerequisite(&records::bytes(&alias).unwrap())
        .await
        .unwrap();
    assert_eq!(aliased["duplicate_kind"], "semantic");
    assert_eq!(
        aliased["receipt"]["result_refs"],
        registered["receipt"]["result_refs"]
    );
    let mut different = registration.clone();
    different["payload"]["acceptor"] = json!("other-acceptor");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&different).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    different["key"]["operation_id"] = json!("refuse-changed-registration");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&different).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="SEMANTIC_CONFLICT")
    );
    let mut crosskind = proof.clone();
    crosskind["kind"] = json!("authority.change");
    crosskind["payload"] = registration["payload"].clone();
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&crosskind).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(meter.operations, 4);
    tx.rollback().await.unwrap();
    let status = ledger.status(&key.bytes().unwrap()).await.unwrap();
    assert_eq!(status["receipt"], committed["receipt"]);
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap(),
        status
    );
    assert_eq!(
        reopened.prerequisite(&pretty).await.unwrap()["receipt"],
        committed["receipt"]
    );
    let mut tx = reopened
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(
        tx.workflow_test_meter().await.unwrap().payload_bytes,
        meter.payload_bytes
    );
    tx.rollback().await.unwrap();
    reopened.close().await;
}
#[tokio::test]
async fn agreement_explicit_attestation_requires_prior_exact_subject_terms_and_actor() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let digest = format!("sha256:{}", "7".repeat(64));
    let content = b"Explicit test assent material only, never commercial activation.";
    let assent = envelope(
        "assent-material",
        "evidence.retain",
        json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"assent","subject":{"customer":"test-customer-A","agreement_id":"test-agreement-A"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"terms_digest":digest,"acceptor":"test-acceptor-A","accepted_at":"2026-10-13T00:00:00.000000Z"}}),
    );
    let saved = ledger
        .prerequisite(&records::bytes(&assent).unwrap())
        .await
        .unwrap();
    let r = &saved["receipt"]["result_refs"][0];
    let attestation = envelope(
        "explicit-attestation",
        "evidence.retain",
        json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"operator_attestation","subject":{"customer":"test-customer-A","agreement_id":"test-agreement-A"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(b"Explicit test operator statement."),"content_sha256":records::raw_hash(b"Explicit test operator statement."),"metadata":{"terms_digest":digest,"acceptor":"test-acceptor-A","assent_evidence_ref":r,"statement":"operator_verified_exact_terms_acceptance"}}),
    );
    ledger
        .prerequisite(&records::bytes(&attestation).unwrap())
        .await
        .unwrap();
    let mut bad = attestation.clone();
    bad["key"]["operation_id"] = json!("wrong-terms");
    bad["payload"]["metadata"]["terms_digest"] = json!(format!("sha256:{}", "9".repeat(64)));
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&bad).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="ASSENT_TERMS_MISMATCH")
    );
    let mut invented = attestation;
    invented["key"]["operation_id"] = json!("missing-explicit-statement");
    invented["payload"]["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("statement");
    assert!(
        ledger
            .prerequisite(&records::bytes(&invented).unwrap())
            .await
            .is_err()
    );
    ledger.close().await;
}
