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

#[tokio::test]
async fn chronological_owning_frames_follow_actual_public_prerequisite_history() {
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession,
    };
    let (_directory, ledger, _path) = installation("actual-prefix").await;
    let mut receipts = Vec::new();
    for (operation, content) in [
        ("first", b"Original customer evidence".as_slice()),
        ("second", b"Second original customer evidence".as_slice()),
    ] {
        let mut input = envelope(&key("actual-prefix", operation));
        input["kind"] = json!("evidence.retain");
        input["payload"] = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"prefix-customer"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"prefix-acceptor"}});
        receipts.push(
            ledger
                .prerequisite(&records::bytes(&input).unwrap())
                .await
                .unwrap(),
        );
    }
    let through = receipts[1]["receipt"]["commit_sequence"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let root = key("actual-prefix", "prefix-reader");
    let arena = AuditArena::new().unwrap();
    let baseline = arena.usage().unwrap().live_bytes;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    proof_session::with_proofs(&mut tx, &root, |tx| {
        Box::pin(async {
            let mut after = 0;
            for (operation, receipt) in [("first", &receipts[0]), ("second", &receipts[1])] {
                let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let next = prefix::next(tx, &root, after, through, &mut session)
                    .await?
                    .unwrap();
                assert_eq!(next.key(), &key("actual-prefix", operation));
                after = next.frame.sequence;
                let resolved = next.checked_wrappers(tx, &mut session).await?;
                assert_eq!(
                    resolved.wrappers().class(),
                    wrappers::WireClass::Prerequisite
                );
                assert_eq!(resolved.wrappers().receipt(), &receipt["receipt"]);
                // Exact saved request is read from the original row, not recreated
                // from the receipt or supplied as an expected construction operand.
                assert_eq!(
                    resolved.wrappers().original_request().value()["key"]["operation_id"],
                    operation
                );
                // The connected current/prefix document reader is exercised
                // only with a genuine public prerequisite's retained member.
                // No synthetic financial history or provenance token is made.
                let reference: ledgerlab_core::policy::workflow::transition_constructor::ExactRef =
                    serde_json::from_value(
                        receipt["receipt"]["result_refs"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["kind"] == "workflow-evidence")
                            .unwrap()
                            .clone(),
                    )
                    .unwrap();
                assert!(
                    tx.workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &root.scope,
                        after,
                        &mut session
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
                );
                let foreign = ["foreign-tenant".into(), root.scope[1].clone()];
                assert!(
                    tx.workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &foreign,
                        through + 1,
                        &mut session
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
                );
                let live_before = arena.usage().unwrap().live_bytes;
                let retained = tx
                    .workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &root.scope,
                        after + 1,
                        &mut session,
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .unwrap();
                assert_eq!(
                    retained.schema.bytes(),
                    b"ledger-workflow-retained-evidence/1"
                );
                let body = canonical::parse_bounded(retained.body.bytes(), 4194304).unwrap();
                assert_eq!(
                    body["content_sha256"],
                    resolved.wrappers().original_request().value()["payload"]["content_sha256"]
                );
                assert!(arena.usage().unwrap().live_bytes > live_before);
                drop(retained);
                assert_eq!(arena.usage().unwrap().live_bytes, live_before);
                drop(resolved);
            }
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(
                prefix::next(tx, &root, after, through, &mut session)
                    .await?
                    .is_none()
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    // This test qualifies mechanical chronological acquisition only. It creates
    // neither an AUTH02 CompletedOrigin nor a financial receipt/candidate.
    ledger.close().await;
}
