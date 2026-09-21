//! Storage primitive evidence. Accepted-plan tests use the coordinator's builder;
//! these raw fixtures do not claim authority or a complete composite acceptance.
use super::*;
use crate::store::{ports::AcceptanceTx, sqlite::tests as original};
use sqlx::AssertSqlSafe;
use std::time::Duration;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
async fn fresh() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), original::installation())
        .await
        .unwrap();
    (dir, store)
}
fn boundaries() -> Boundaries {
    Boundaries { fault: None }
}
fn settlement() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../../contracts/candidates/reservation-settlement-v1/vectors.json"
    ))
    .unwrap()
}
fn query(scope: [String; 2], required: Vec<ScopedRecordRef>) -> OutcomeResolve {
    OutcomeResolve {
        delivery: ScopedDelivery {
            scope: scope.clone(),
            source: "urn:test:store".into(),
            external_id: "query".into(),
        },
        target: "target".into(),
        invocation_id: "invocation".into(),
        family_key: None,
        required,
        locks: vec![OutcomeLock {
            class: OutcomeLockClass::Target,
            key: serde_json::to_vec(&serde_json::json!([scope, "target"])).unwrap(),
            mode: OutcomeLockMode::Read,
        }],
    }
}

#[tokio::test]
async fn frozen_settlement_envelopes_reopen_byte_exact() {
    let mut total = 0;
    for history in settlement() {
        let (dir, store) = fresh().await;
        let mut expected = Vec::new();
        for step in history["steps"].as_array().unwrap() {
            let mut tx = store.begin_outcome(deadline()).await.unwrap();
            for text in step["canonical_utf8"].as_array().unwrap() {
                let bytes = text.as_str().unwrap().as_bytes().to_vec();
                let r = insert_record(tx.conn(), &bytes, &mut boundaries())
                    .await
                    .unwrap();
                expected.push((r, bytes));
                total += 1;
            }
            tx.commit().await.unwrap();
        }
        store.close().await;
        let reopened = SqliteStore::open(dir.path()).await.unwrap();
        let mut tx = reopened.begin_outcome(deadline()).await.unwrap();
        for (r, bytes) in expected {
            assert_eq!(retained(tx.conn(), &r).await.unwrap(), Some(bytes));
        }
        tx.rollback().await.unwrap();
        reopened.close().await;
    }
    assert_eq!(total, 112);
}

#[tokio::test]
async fn frozen_economic_originals_corrections_and_zero_records_reopen() {
    for name in [
        "correction-replacement",
        "zero-adjustment",
        "supplier-separation",
        "full-reversal-reinstatement",
        "predeclared-unclaimed-family",
        "decision-time-evidence",
    ] {
        let bytes = std::fs::read(format!(
            "{}/../../contracts/candidates/v2/goldens/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let h: Value = serde_json::from_slice(&bytes).unwrap();
        let mut records = h["seed"].as_array().unwrap().clone();
        for d in h["decisions"].as_array().unwrap() {
            records.extend(d["records"].as_array().unwrap().clone());
        }
        let (dir, store) = fresh().await;
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        let mut expected = Vec::new();
        for r in records {
            // Golden envelope keys are ASCII; independent contract audits certify
            // these serialized bytes. No production evaluator supplies expectations.
            let bytes = serde_json::to_vec(&r).unwrap();
            let reference = insert_record(tx.conn(), &bytes, &mut boundaries())
                .await
                .unwrap();
            expected.push((reference, bytes));
        }
        tx.commit().await.unwrap();
        store.close().await;
        let reopened = SqliteStore::open(dir.path()).await.unwrap();
        let mut tx = reopened.begin_outcome(deadline()).await.unwrap();
        for (r, b) in expected {
            assert_eq!(retained(tx.conn(), &r).await.unwrap(), Some(b));
        }
        tx.rollback().await.unwrap();
        reopened.close().await;
    }
}

#[tokio::test]
async fn immutable_identity_hash_and_scoped_reference_constraints() {
    let (dir, store) = fresh().await;
    let bytes = settlement()[0]["steps"][0]["canonical_utf8"][0]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    let r = insert_record(tx.conn(), &bytes, &mut boundaries())
        .await
        .unwrap();
    assert_eq!(
        insert_record(tx.conn(), &bytes, &mut boundaries())
            .await
            .unwrap(),
        r
    );
    let mut altered = json(&bytes).unwrap();
    altered["content_hash"] = Value::String(format!("sha256:{}", "0".repeat(64)));
    assert!(
        insert_record(tx.conn(), &canonical(&altered).unwrap(), &mut boundaries())
            .await
            .is_err()
    );
    for sql in [
        "UPDATE outcome_records SET canonical_bytes=x'7b7d'",
        "DELETE FROM outcome_records",
    ] {
        assert!(sqlx::query(AssertSqlSafe(sql))
            .execute(tx.conn())
            .await
            .is_err());
    }
    // Scope is part of reference identity; a cross-scope link fails at commit.
    sqlx::query(
        "INSERT INTO outcome_members VALUES ('other','sandbox','target','invocation',?,?,?)",
    )
    .bind(&r.kind)
    .bind(&r.id)
    .bind(&r.content_hash)
    .execute(tx.conn())
    .await
    .unwrap();
    assert!(matches!(
        tx.commit().await,
        Err(crate::store::errors::CommitError::OutcomeUnknown)
    ));
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    let mut recovered = reopened.begin_outcome(deadline()).await.unwrap();
    assert!(retained(recovered.conn(), &r).await.unwrap().is_none());
    recovered.rollback().await.unwrap();
    reopened.close().await;
}

#[tokio::test]
async fn resolve_missing_more_locks_and_complete_bytes() {
    let (_dir, store) = fresh().await;
    let bytes = settlement()[0]["steps"][0]["canonical_utf8"][0]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    let r = record_ref(&bytes).unwrap();
    let q = query(r.scope.clone(), vec![r.clone()]);
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    assert!(matches!(
        tx.resolve_outcome(&q).await.unwrap(),
        OutcomeResolution::MoreLocks(_)
    ));
    tx.lock_scopes(&q.locks).await.unwrap();
    match tx.resolve_outcome(&q).await.unwrap() {
        OutcomeResolution::Missing(refs) => assert_eq!(refs, vec![r.clone()]),
        other => panic!("{other:?}"),
    }
    insert_record(tx.conn(), &bytes, &mut boundaries())
        .await
        .unwrap();
    match tx.resolve_outcome(&q).await.unwrap() {
        OutcomeResolution::Complete(s) => {
            assert_eq!(s.records, vec![bytes]);
            assert!(s.anchors.is_empty());
            assert_eq!(s.heads[0].revision, None);
            assert_eq!(s.heads[0].value, None);
        }
        other => panic!("{other:?}"),
    }
    tx.rollback().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn bounded_history_refuses_before_returning_partial_records() {
    let (_dir, store) = fresh().await;
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    let q = query(["synthetic".into(), "sandbox".into()], vec![]);
    tx.lock_scopes(&q.locks).await.unwrap();
    // Deliberately malformed physical fixture proves the SQL size preflight runs
    // before materializing an oversized record set or returning truncated rows.
    for n in 0..3 {
        let id = format!("\"large-{n}\"").into_bytes();
        let hash = format!("sha256:{}", "0".repeat(64));
        sqlx::query("INSERT INTO outcome_records VALUES ('synthetic','sandbox','evidence',?,?,zeroblob(4194304))")
            .bind(&id).bind(&hash).execute(tx.conn()).await.unwrap();
        sqlx::query("INSERT INTO outcome_members VALUES ('synthetic','sandbox','target','invocation','evidence',?,?)")
            .bind(&id).bind(&hash).execute(tx.conn()).await.unwrap();
    }
    assert!(tx.resolve_outcome(&q).await.is_err());
    assert!(tx.failed);
    tx.rollback().await.unwrap();
    store.close().await;
}

async fn control_index(
    c: &mut SqliteConnection,
    r: &ScopedRecordRef,
    source: &str,
    label: &str,
) -> Result<(), sqlx::Error> {
    // Deliberate physical fixture only, not an accepted coordinator decision.
    sqlx::query("INSERT INTO outcome_deliveries (tenant,environment,source,external_id,canonical_source,canonical_external_id,command,ingress,ingress_hash,settlement_kind,settlement_id,settlement_hash) VALUES (?,?,?,?,?,?,x'7b7d',x'7b7d',?,'reservation-receipt',?,?)")
        .bind(&r.scope[0]).bind(&r.scope[1]).bind(source).bind(label).bind(source).bind(label).bind(&r.content_hash).bind(&r.id).bind(&r.content_hash).execute(c).await?;
    Ok(())
}
#[tokio::test]
async fn shared_v1_and_control_delivery_namespace_is_bidirectional() {
    let (_dir, store) = fresh().await;
    let bytes = settlement()[0]["steps"][0]["canonical_utf8"][1]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    let r = insert_record(tx.conn(), &bytes, &mut boundaries())
        .await
        .unwrap();
    control_index(tx.conn(), &r, "urn:test:control", "one")
        .await
        .unwrap();
    let scope = crate::store::records::Scope {
        tenant: r.scope[0].clone(),
        environment: r.scope[1].clone(),
    };
    assert!(matches!(
        super::super::read::identity(tx.conn(), &scope, "urn:test:control", "one").await,
        Err(StoreError::DeliveryConflict)
    ));
    let error = sqlx::query(
        "INSERT INTO delivery_keys VALUES (?,?,?,'one',?,'missing','original',0,x'7b7d',?,1)",
    )
    .bind(&r.scope[0])
    .bind(&r.scope[1])
    .bind("urn:test:control")
    .bind(&r.content_hash)
    .bind(&r.content_hash)
    .execute(tx.conn())
    .await
    .unwrap_err();
    assert!(error.to_string().contains("already retained by outcome"));
    sqlx::query(
        "INSERT INTO delivery_keys VALUES (?,?,?,'two',?,'missing','original',0,x'7b7d',?,1)",
    )
    .bind(&r.scope[0])
    .bind(&r.scope[1])
    .bind("urn:test:control")
    .bind(&r.content_hash)
    .bind(&r.content_hash)
    .execute(tx.conn())
    .await
    .unwrap();
    let error = control_index(tx.conn(), &r, "urn:test:control", "two")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("already retained by v1"));
    tx.rollback().await.unwrap();
    store.close().await;
}
#[tokio::test]
async fn incomplete_composite_is_integrity_failure_not_absence() {
    let (_dir, store) = fresh().await;
    let bytes = settlement()[0]["steps"][0]["canonical_utf8"][1]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    let r = record_ref(&bytes).unwrap();
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    control_index(tx.conn(), &r, "urn:test:control", "missing")
        .await
        .unwrap();
    let key = ScopedDelivery {
        scope: r.scope,
        source: "urn:test:control".into(),
        external_id: "missing".into(),
    };
    assert!(matches!(
        tx.lookup_outcome_delivery(&key).await,
        Err(StoreError::Integrity(_))
    ));
    assert!(tx.failed);
    tx.rollback().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn every_head_class_checks_revision_value_and_locked_absence() {
    let (_dir, store) = fresh().await;
    let classes = [
        OutcomeLockClass::Admission,
        OutcomeLockClass::Authority,
        OutcomeLockClass::Binding,
        OutcomeLockClass::Reservation,
        OutcomeLockClass::Target,
        OutcomeLockClass::Claim,
        OutcomeLockClass::BindingAggregate,
        OutcomeLockClass::InvocationConsumption,
        OutcomeLockClass::BaseReversal,
    ];
    for kind in classes {
        let lock = OutcomeLock {
            class: kind,
            key: b"[[\"demo\",\"sandbox\"],\"guard\"]".to_vec(),
            mode: OutcomeLockMode::Write,
        };
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        tx.lock_scopes(std::slice::from_ref(&lock)).await.unwrap();
        let absent = head(tx.conn(), &lock).await.unwrap();
        assert_observed(tx.conn(), std::slice::from_ref(&absent))
            .await
            .unwrap();
        sqlx::query("INSERT INTO outcome_heads VALUES (?,?, '0', x'7b7d')")
            .bind(class(kind))
            .bind(&lock.key)
            .execute(tx.conn())
            .await
            .unwrap();
        assert!(matches!(
            assert_observed(tx.conn(), &[absent]).await,
            Err(StoreError::ExpectedCurrent)
        ));
        let original = head(tx.conn(), &lock).await.unwrap();
        tx.commit().await.unwrap();
        let mut writer = store.begin_outcome(deadline()).await.unwrap();
        sqlx::query("UPDATE outcome_heads SET value=x'5b5d' WHERE class=? AND key=?")
            .bind(class(kind))
            .bind(&lock.key)
            .execute(writer.conn())
            .await
            .unwrap();
        writer.commit().await.unwrap();
        let mut stale = store.begin_outcome(deadline()).await.unwrap();
        assert!(matches!(
            assert_observed(stale.conn(), std::slice::from_ref(&original)).await,
            Err(StoreError::ExpectedCurrent)
        ));
        let current = head(stale.conn(), &lock).await.unwrap();
        assert_eq!(
            current.revision, original.revision,
            "same-revision byte changes must still conflict"
        );
        assert_observed(stale.conn(), &[current]).await.unwrap();
        stale.rollback().await.unwrap();
    }
    store.close().await;
}

#[tokio::test]
async fn cancelled_outcome_reads_poison_handle_and_reopen_rolls_back() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    for lookup_cut in [false, true] {
        let (dir, store) = fresh().await;
        let bytes = settlement()[0]["steps"][0]["canonical_utf8"][0]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec();
        let r = record_ref(&bytes).unwrap();
        let q = query(r.scope.clone(), vec![r.clone()]);
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        tx.lock_scopes(&q.locks).await.unwrap();
        insert_record(tx.conn(), &bytes, &mut boundaries())
            .await
            .unwrap();
        if lookup_cut {
            let mut f = Box::pin(tx.lookup_outcome_delivery(&q.delivery));
            assert!(matches!(
                f.as_mut().poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
        } else {
            let mut f = Box::pin(tx.resolve_outcome(&q));
            assert!(matches!(
                f.as_mut().poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
        }
        assert!(tx.failed);
        assert!(tx.commit().await.is_err());
        store.close().await;
        let reopened = SqliteStore::open(dir.path()).await.unwrap();
        let mut tx = reopened.begin_outcome(deadline()).await.unwrap();
        assert!(retained(tx.conn(), &r).await.unwrap().is_none());
        tx.rollback().await.unwrap();
        reopened.close().await;
    }
}

#[tokio::test]
async fn legacy_delivery_is_permanent_conflict_without_fabricated_companion() {
    let (_dir, store) = fresh().await;
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    for op in original::seed().into_iter().chain(original::schedule()) {
        tx.write(&op).await.unwrap();
    }
    tx.commit().await.unwrap();
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    let key = ScopedDelivery {
        scope: ["demo".into(), "sandbox".into()],
        source: "urn:demo:app".into(),
        external_id: "generation-1".into(),
    };
    let error = tx.lookup_outcome_delivery(&key).await.unwrap_err();
    assert!(matches!(error, StoreError::DeliveryConflict));
    assert!(!error.retryable_after_rollback());
    tx.rollback().await.unwrap();
    store.close().await;
}
