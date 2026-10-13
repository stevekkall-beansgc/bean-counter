//! Real held SQLite transactions exercise proof lifetime and memo boundaries.
use super::*;
use crate::{
    billing::BillingLedger,
    service::workflow::{local::deadline, LocalWorkflowLedger},
    store::{
        errors::{CommitError, StoreError},
        ports::{AcceptanceStore, AcceptanceTx},
    },
};

async fn installed() -> (
    tempfile::TempDir,
    LocalWorkflowLedger,
    OperationKey,
    JournalOperation,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!("proof-session-test");
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let key = OperationKey {
        logical_store_id: "proof-session-test".into(),
        scope: ["example-company".into(), "local".into()],
        source: "urn:example:work".into(),
        operation_id: "first-proof".into(),
    };
    let content = b"Explicit local test registration statement; no commercial assent.";
    let raw = records::bytes(&json!({"schema":"ledger-operation/1","contract":"workflow/1",
        "kind":"evidence.retain","key":key,"payload":{"schema":"ledger-workflow-evidence-retain/1",
        "purpose":"customer_registration","subject":{"customer":"test-customer"},
        "media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),
        "content_sha256":records::raw_hash(content),"metadata":{"acceptor":"test-customer"}}})).unwrap();
    ledger.prerequisite(&raw).await.unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let original = tx
        .workflow_agreement_lookup(&key.bytes().unwrap())
        .await
        .unwrap()
        .unwrap();
    tx.rollback().await.unwrap();
    (directory, ledger, key, original)
}
async fn cannot_commit(tx: SqliteTx) {
    assert!(matches!(
        tx.commit().await,
        Err(CommitError::RolledBack(StoreError::Integrity(_)))
    ));
}

#[tokio::test]
async fn full_proof_hits_and_append_epoch_reproof_use_exact_original() {
    let (_directory, ledger, key, original) = installed().await;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let ((), counts) = verify::audit_origins(with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            let first = verify::retained(tx, &key, &original).await?;
            assert_eq!(verify::retained(tx, &key, &original).await?, first);
            // Unit test the epoch barrier itself. Actual appends invoke this same
            // barrier before their saved-certificate proof in the public controller.
            after_append(tx, &key)?;
            assert_eq!(verify::retained(tx, &key, &original).await?, first);
            Ok(())
        })
    }))
    .await
    .unwrap();
    assert_eq!(counts.get(&key.bytes().unwrap()), Some(&2));
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn changed_wrapper_or_member_cannot_hit_a_completed_proof() {
    let (_directory, ledger, key, original) = installed().await;
    for member_change in [false, true] {
        let mut changed = original.clone();
        if member_change {
            changed.members.pop();
        } else {
            changed.request_hash.push('0');
        }
        let mut tx = ledger.store.begin(deadline()).await.unwrap();
        let result = with_proofs(&mut tx, &key, |tx| {
            Box::pin(async {
                verify::retained(tx, &key, &original).await?;
                // A caught fingerprint error still invalidates the outer scope.
                assert_eq!(
                    verify::retained(tx, &key, &changed).await,
                    Err(ServiceError::IntegrityFailure)
                );
                Ok(())
            })
        })
        .await;
        assert_eq!(result, Err(ServiceError::IntegrityFailure));
        cannot_commit(tx).await;
    }
    ledger.close().await;
}

#[tokio::test]
async fn completed_alias_does_not_satisfy_later_direct_original_obligation() {
    let (_directory, ledger, key, original) = installed().await;
    let mut body = verify::checked(&original.operation_bytes, 8388608).unwrap()["request_evidence"]
        ["normalized_request"]
        .clone();
    body["key"]["operation_id"] = json!("alias-proof");
    ledger
        .prerequisite(&records::bytes(&body).unwrap())
        .await
        .unwrap();
    let alias_key = OperationKey::parse(&records::bytes(&body["key"]).unwrap()).unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let alias = tx
        .workflow_agreement_lookup(&alias_key.bytes().unwrap())
        .await
        .unwrap()
        .unwrap();
    let result = with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            let value = verify::retained(tx, &alias_key, &alias).await?;
            assert_eq!(value["receipt"]["result_kind"], "semantic_alias");
            assert!(matches!(
                enter(
                    tx,
                    &alias_key,
                    &alias.identity,
                    journal_binding(&alias)?,
                    true
                ),
                Err(ServiceError::IntegrityFailure)
            ));
            Ok(())
        })
    })
    .await;
    assert_eq!(result, Err(ServiceError::IntegrityFailure));
    cannot_commit(tx).await;
    ledger.close().await;
}

#[tokio::test]
async fn scope_or_transaction_mismatch_cannot_be_caught_then_committed() {
    let (_directory, ledger, key, _) = installed().await;
    let (_other_directory, other, _, _) = installed().await;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let mut wrong = key.clone();
    wrong.scope[1] = "different-scope".into();
    let result: Result<()> = with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            assert_eq!(present(tx, &wrong), Err(ServiceError::IntegrityFailure));
            Ok(())
        })
    })
    .await;
    assert_eq!(result, Err(ServiceError::IntegrityFailure));
    cannot_commit(tx).await;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let mut other_tx = other.store.begin(deadline()).await.unwrap();
    let result: Result<()> = with_proofs(&mut tx, &key, |_tx| {
        Box::pin(async {
            assert_eq!(
                present(&mut other_tx, &key),
                Err(ServiceError::IntegrityFailure)
            );
            Ok(())
        })
    })
    .await;
    assert_eq!(result, Err(ServiceError::IntegrityFailure));
    cannot_commit(tx).await;
    cannot_commit(other_tx).await;
    ledger.close().await;
    other.close().await;
}

#[tokio::test]
async fn in_progress_is_not_proof_and_active_epoch_reset_refuses() {
    let (_directory, ledger, key, original) = installed().await;
    for epoch_reset in [false, true] {
        let mut tx = ledger.store.begin(deadline()).await.unwrap();
        let result = with_proofs(&mut tx, &key, |tx| {
            Box::pin(async {
                let Entry::Started(guard) = enter(
                    tx,
                    &key,
                    &original.identity,
                    journal_binding(&original)?,
                    false,
                )?
                else {
                    panic!("fresh scope has no completion");
                };
                if epoch_reset {
                    assert_eq!(after_append(tx, &key), Err(ServiceError::IntegrityFailure));
                } else {
                    assert_eq!(
                        verify::retained(tx, &key, &original).await,
                        Err(ServiceError::IntegrityFailure)
                    );
                }
                drop(guard);
                Ok(())
            })
        })
        .await;
        assert_eq!(result, Err(ServiceError::IntegrityFailure));
        cannot_commit(tx).await;
    }
    ledger.close().await;
}

#[tokio::test]
async fn cancelled_and_unpolled_scopes_poison_only_their_transaction() {
    let (_directory, ledger, key, original) = installed().await;
    for polled in [false, true] {
        let mut tx = ledger.store.begin(deadline()).await.unwrap();
        let future = with_proofs(&mut tx, &key, |tx| {
            Box::pin(async {
                let Entry::Started(guard) = enter(
                    tx,
                    &key,
                    &original.identity,
                    journal_binding(&original)?,
                    false,
                )?
                else {
                    panic!("fresh scope has no completion");
                };
                std::future::pending::<()>().await;
                guard.finish(true)?;
                Ok(())
            })
        });
        if polled {
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(5), future)
                    .await
                    .is_err()
            );
        } else {
            drop(future);
        }
        assert!(SESSION.try_with(|_| ()).is_err());
        cannot_commit(tx).await;
    }
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            verify::retained(tx, &key, &original).await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    // A new actual transaction has neither a failed latch nor cached proof.
    tx.commit().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn original_retryable_error_survives_partial_proof_invalidation() {
    let (_directory, ledger, key, original) = installed().await;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let result: Result<()> = with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            let Entry::Started(guard) = enter(
                tx,
                &key,
                &original.identity,
                journal_binding(&original)?,
                false,
            )?
            else {
                panic!("fresh scope has no completion");
            };
            drop(guard);
            // Match a database deadline/rollback classification: the partial scope
            // is unusable, while its original caller-visible error is preserved.
            Err(ServiceError::Retryable)
        })
    })
    .await;
    assert_eq!(result, Err(ServiceError::Retryable));
    cannot_commit(tx).await;
    ledger.close().await;
}
