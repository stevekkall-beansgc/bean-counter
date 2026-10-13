//! Actual trusted-entry/migration tests. No synthetic authority snapshot is used.
use super::*;
use crate::billing::BillingLedger;

async fn installation(store_id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!(store_id);
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    (dir, path)
}
fn key(store_id: &str) -> request::OperationKey {
    request::OperationKey {
        logical_store_id: store_id.into(),
        scope: ["example-company".into(), "local".into()],
        source: "urn:example:work".into(),
        operation_id: "never-committed".into(),
    }
}

#[tokio::test]
async fn trusted_local_entry_explicit_upgrade_reopen_and_wrong_store_read() {
    let (_dir, path) = installation("owner-test-store").await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    assert_eq!(
        ledger
            .status(&key("owner-test-store").bytes().unwrap())
            .await
            .unwrap()["status"],
        "not_observed"
    );
    assert!(
        matches!(ledger.status(&key("other-store").bytes().unwrap()).await,Err(ServiceError::Rejection(code)) if code=="AUTHORITY_REQUIRED")
    );
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened
            .status(&key("owner-test-store").bytes().unwrap())
            .await
            .unwrap()["status"],
        "not_observed"
    );
    reopened.close().await;
}

#[tokio::test]
async fn same_logical_id_does_not_transfer_capability_to_another_held_owner() {
    // Logical ID uniqueness remains the trusted operator's obligation. This
    // intentionally colliding configuration proves only exact instance binding.
    let (_a, a) = installation("operator-selected-id").await;
    let (_b, b) = installation("operator-selected-id").await;
    let first = LocalWorkflowLedger::open_local(&a).await.unwrap();
    let second = LocalWorkflowLedger::open_local(&b).await.unwrap();
    let mut tx = second.store.begin(deadline()).await.unwrap();
    assert!(
        matches!(first.admin.read(&second.store,&mut tx,&key("operator-selected-id")).await,Err(ServiceError::Rejection(code)) if code=="AUTHORITY_REQUIRED")
    );
    tx.rollback().await.unwrap();
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn owner_read_survives_source_rights_revocation_and_frozen_admission() {
    let (_dir, path) = installation("owner-read-test").await;
    let billing = BillingLedger::open(&path).await.unwrap();
    billing.permissions("customer-1","urn:example:work",br#"{"schema":"ledger-billing-permissions/2","customer":"customer-1","source":"urn:example:work","change_id":"remove-all-source-rights","expected_revision":"1","permissions":[],"reason":"synthetic explicit source revocation"}"#).await.unwrap();
    billing.close().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        ledger
            .status(&key("owner-read-test").bytes().unwrap())
            .await
            .unwrap()["status"],
        "not_observed"
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert!(
        matches!(ledger.admin.fresh(&ledger.store,&mut tx,&key("owner-read-test")).await,Err(ServiceError::Rejection(code)) if code=="ADMISSION_CLOSED")
    );
    tx.rollback().await.unwrap();
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened
            .status(&key("owner-read-test").bytes().unwrap())
            .await
            .unwrap()["status"],
        "not_observed"
    );
    reopened.close().await;
}

fn publication_request(store_id: &str) -> Value {
    let expected:Value=serde_json::from_str(include_str!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
    let mut request = expected["normalized_request"].clone();
    request["key"] = json!(key(store_id));
    request["key"]["operation_id"] = json!("real-publish-001");
    request
}

#[tokio::test]
async fn unqualified_build_cannot_fresh_publish_or_allocate_retention() {
    if crate::build_identity::embedded_inputs().is_ok() {
        return;
    }
    let (_dir, path) = installation("unqualified-build-test").await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let request = publication_request("unqualified-build-test");
    assert_eq!(
        ledger.publish(&records::bytes(&request).unwrap()).await,
        Err(ServiceError::Unavailable)
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 0);
    assert_eq!(tx.workflow_test_meter().await.unwrap().payload_bytes, 0);
    tx.rollback().await.unwrap();
    ledger.close().await;
}

/// Run only from the immutable captured source build; no synthetic provenance.
#[tokio::test]
#[ignore = "requires trusted captured build inputs; ordinary Cargo cannot publish"]
async fn captured_workflow_publication_reply_loss_retry_alias_read_and_reopen() {
    assert!(
        crate::build_identity::embedded_inputs().is_ok(),
        "run the captured build, not an ordinary test binary"
    );
    let (_dir, path) = installation("captured-publication-test").await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let request = publication_request("captured-publication-test");
    let raw = records::bytes(&request).unwrap();
    let operation_key =
        request::OperationKey::parse(&records::bytes(&request["key"]).unwrap()).unwrap();
    ledger
        .lose_reply
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(ledger.publish(&raw).await, Err(ServiceError::ResponseLost));
    let original = ledger
        .status(&operation_key.bytes().unwrap())
        .await
        .unwrap();
    assert_eq!(original["status"], "committed");
    assert_eq!(original["receipt"]["commit_sequence"], "1");
    assert_eq!(
        original["request_evidence"]["original_request_bytes_sha256"],
        records::raw_hash(&raw)
    );
    let retried = ledger
        .publish_with(&raw, || {
            panic!("original retry must not invoke compiler support or current build")
        })
        .await
        .unwrap();
    assert_eq!(retried["duplicate_kind"], "identity");
    assert_eq!(retried["receipt"], original["receipt"]);
    assert_eq!(retried["request_evidence"], original["request_evidence"]);
    let pretty = serde_json::to_vec_pretty(&request).unwrap();
    assert_ne!(records::raw_hash(&pretty), records::raw_hash(&raw));
    let equivalent = ledger
        .publish_with(&pretty, || {
            panic!("lexical retry invokes no current compiler")
        })
        .await
        .unwrap();
    assert_eq!(equivalent["receipt"], original["receipt"]);
    assert_eq!(equivalent["request_evidence"], original["request_evidence"]);
    let mut changed = request.clone();
    changed["payload"]["expected_plan_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    assert!(
        matches!(ledger.publish(&records::bytes(&changed).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    let mut alias_request = request.clone();
    alias_request["key"]["operation_id"] = json!("real-publish-002");
    let alias = ledger
        .publish(&records::bytes(&alias_request).unwrap())
        .await
        .unwrap();
    assert_eq!(alias["duplicate_kind"], "semantic");
    assert_eq!(alias["receipt"]["commit_sequence"], "2");
    assert_eq!(
        alias["receipt"]["result_refs"],
        original["receipt"]["result_refs"]
    );
    let pub_ref = original["receipt"]["result_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "workflow-publication")
        .unwrap();
    let read_request =
        json!({"schema":"ledger-read/1","kind":"policy.document","payload":{"ref":pub_ref}});
    let doc = ledger
        .read(&records::bytes(&read_request).unwrap())
        .await
        .unwrap();
    assert_eq!(doc["record"]["id"], pub_ref["id"]);
    assert_eq!(doc["canonical_digest"], pub_ref["digest"]);
    let body_bytes = ledgerlab_core::adjudication::proofs::decode_base64(
        doc["canonical_bytes_base64"].as_str().unwrap(),
        4 * 1024 * 1024,
    )
    .unwrap();
    assert_eq!(body_bytes, records::bytes(&doc["record"]["body"]).unwrap());
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(meter.operations, 2);
    let build_ref = tx
        .workflow_publication_build_ref(
            pub_ref["id"].as_str().unwrap(),
            &operation_key.logical_store_id,
            &operation_key.scope[0],
            &operation_key.scope[1],
        )
        .await
        .unwrap()
        .unwrap();
    let build = tx
        .workflow_document(
            &build_ref,
            &operation_key.logical_store_id,
            &operation_key.scope[0],
            &operation_key.scope[1],
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let build_body = ledgerlab_core::canonical::parse_bounded(&build.bytes, 65536).unwrap();
    assert_eq!(
        build_body["build_inputs"],
        crate::build_identity::embedded_inputs().unwrap()
    );
    assert_eq!(
        build_body["executable"]["sha256"],
        doc["record"]["body"]["provenance"]["binary_sha256"]
    );
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        ledger
            .publish_with(&raw, || panic!("frozen admission preserves original"))
            .await
            .unwrap()["receipt"],
        original["receipt"]
    );
    let mut new_request = request.clone();
    new_request["key"]["operation_id"] = json!("refused-new");
    assert!(
        matches!(ledger.publish(&records::bytes(&new_request).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="ADMISSION_CLOSED")
    );
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened
            .status(&operation_key.bytes().unwrap())
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        reopened
            .publish_with(&pretty, || panic!(
                "reopen retry requires no current compiler"
            ))
            .await
            .unwrap()["receipt"],
        original["receipt"]
    );
    assert_eq!(
        reopened
            .read(&records::bytes(&read_request).unwrap())
            .await
            .unwrap(),
        doc
    );
    let mut tx = reopened.store.begin(deadline()).await.unwrap();
    assert_eq!(
        tx.workflow_test_meter().await.unwrap().payload_bytes,
        meter.payload_bytes
    );
    tx.rollback().await.unwrap();
    reopened.close().await;
}
