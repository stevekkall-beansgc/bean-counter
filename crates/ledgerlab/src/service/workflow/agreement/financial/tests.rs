//! Disabled-shell checks. No financial acceptance, fake receipt or raw append.
use super::*;
use crate::{billing::BillingLedger, store::errors::CommitError};

async fn installation(store: &str) -> (tempfile::TempDir, LocalWorkflowLedger, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!(store);
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    (dir, ledger, path)
}
fn key(store: &str, operation: &str) -> OperationKey {
    OperationKey {
        logical_store_id: store.into(),
        scope: ["example-company".into(), "local".into()],
        source: "urn:example:work".into(),
        operation_id: operation.into(),
    }
}
fn envelope(key: &OperationKey) -> Value {
    // Deliberately only lexical-envelope data. No complete normalized economic
    // payload or snapshot is claimed by the disabled host path.
    json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"work.accept","key":key,"payload":{"schema":"ledger-workflow-work/1"}})
}

#[test]
fn financial_shell_preserves_closed_lexical_envelope_only() {
    let value = envelope(&key("disabled-store", "never-written"));
    let raw = records::bytes(&value).unwrap();
    assert_eq!(request::Request::parse_financial(&raw).unwrap().raw(), raw);
    let mut value = value.clone();
    value["prepared"] = json!(true);
    assert!(request::Request::parse_financial(&records::bytes(&value).unwrap()).is_err());
    let duplicate = String::from_utf8(raw).unwrap().replacen(
        "\"kind\":\"work.accept\"",
        "\"kind\":\"work.accept\",\"\\u006bind\":\"work.accept\"",
        1,
    );
    assert!(request::Request::parse_financial(duplicate.as_bytes()).is_err());
    value.as_object_mut().unwrap().remove("prepared");
    value["kind"] = json!("agreement.accept");
    assert!(request::Request::parse_financial(&records::bytes(&value).unwrap()).is_err());
}

#[tokio::test]
async fn financial_shell_denies_fresh_with_zero_delta_and_reopens_not_observed() {
    let (_dir, ledger, path) = installation("disabled-financial").await;
    let key = key("disabled-financial", "never-written");
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        ledger
            .financial(&records::bytes(&envelope(&key)).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    assert_eq!(
        ledger.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    let mut wrong = key.clone();
    wrong.scope[0] = "another-tenant".into();
    assert!(
        matches!(ledger.financial(&records::bytes(&envelope(&wrong)).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="AUTHORITY_REQUIRED")
    );
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    assert_eq!(
        reopened
            .financial(&records::bytes(&envelope(&key)).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    reopened.close().await;
}

#[tokio::test]
async fn financial_shell_global_original_lookup_precedes_frozen_fresh_gate() {
    let (_dir, ledger, _path) = installation("disabled-original-order").await;
    let key = key("disabled-original-order", "existing-administrative-key");
    let content = b"Explicit operator test material; no commercial financial authority.";
    let mut proof = envelope(&key);
    proof["kind"] = json!("evidence.retain");
    proof["payload"] = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"test-customer"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"test-acceptor"}});
    let original = ledger
        .prerequisite(&records::bytes(&proof).unwrap())
        .await
        .unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        matches!(ledger.financial(&records::bytes(&envelope(&key)).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    assert_eq!(
        ledger.status(&key.bytes().unwrap()).await.unwrap()["receipt"],
        original["receipt"]
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn financial_shell_management_frame_wrong_transaction_poisons_both_handles() {
    let (_a, first, _path_a) = installation("frame-store").await;
    let (_b, second, _path_b) = installation("frame-store").await;
    let key = key("frame-store", "frame-only");
    let mut a = first.store.begin(deadline()).await.unwrap();
    let frame = proof_session::with_proofs(&mut a, &key, |tx| {
        Box::pin(ManagementObservation::capture(&first, tx, &key))
    })
    .await
    .unwrap();
    let mut b = second.store.begin(deadline()).await.unwrap();
    assert_eq!(
        frame.recheck(&second, &mut b, &key).await.unwrap_err(),
        ServiceError::IntegrityFailure
    );
    assert!(matches!(a.commit().await, Err(CommitError::RolledBack(_))));
    assert!(matches!(b.commit().await, Err(CommitError::RolledBack(_))));
    first.close().await;
    second.close().await;
}
