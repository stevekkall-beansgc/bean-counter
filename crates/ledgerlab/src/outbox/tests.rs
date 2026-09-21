use super::{
    fake::{MemoryDestination, Mode, Outcome},
    *,
};
use crate::{
    store::{
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::{tests as seed, SqliteStore},
    },
    AcceptCommand, AcceptResult, Backend, Ledger, PrincipalContext,
};
use ledgerlab_core::domain::{Scope, Timestamp};
use std::time::Duration;
use tokio::time::Instant;
const NOW: i64 = 1_789_920_000_000_000;
pub(crate) fn command(id: Option<&str>) -> AcceptCommand {
    let oracle = ledgerlab_testkit::FixtureOracle::workspace().unwrap();
    let mut value: Value = serde_json::from_slice(oracle.input("input").unwrap()).unwrap();
    if let Some(id) = id {
        value["id"] = json!(id);
        value["operation_id"] = json!(id);
    }
    AcceptCommand {
        bytes: serde_json::to_vec(&value).unwrap(),
        principal: PrincipalContext {
            scope: Scope::new("demo", "sandbox").unwrap(),
            principal_id: "demo-app".into(),
            source: "urn:demo:app".into(),
            authority_head: "demo-source-grant-v1".into(),
            can_submit: true,
            can_read: true,
        },
        received_at: Timestamp::parse("2026-09-20T14:00:00.000000Z").unwrap(),
        binding_selector: "demo-retail-selector".into(),
    }
}
async fn sqlite() -> (tempfile::TempDir, Ledger) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), seed::installation())
        .await
        .unwrap();
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    for op in seed::seed() {
        tx.write(&op).await.unwrap();
    }
    tx.commit().await.unwrap();
    (
        dir,
        Ledger {
            store: Backend::Sqlite(store),
        },
    )
}
async fn start(ledger: &Ledger, fake: &MemoryDestination, now: i64) -> Lease {
    let out = ledger.outbox(fake);
    let report = out.reconcile(now, Mode::Normal).await.unwrap();
    assert!(report.unresolved.is_empty());
    assert!(report.orphan_keys.is_empty());
    out.resume(&report.digest, now).await.unwrap();
    out.acquire("worker-a", now).await.unwrap()
}
pub(crate) async fn scenario(ledger: &Ledger, case: usize) {
    let fake = MemoryDestination::new();
    let accepted = ledger.accept(command(None)).await.unwrap();
    let receipt = match accepted {
        AcceptResult::Accepted { receipt } | AcceptResult::Duplicate { receipt, .. } => receipt,
        _ => panic!("accepted fixture"),
    };
    let out = ledger.outbox(&fake);
    assert_eq!(out.deliveries().await.unwrap()[0].state, State::Held);
    assert!(fake.receipts("store-demo-slice").is_empty());
    assert!(matches!(out.acquire("held", NOW).await, Err(Error::Held)));
    let lease = start(ledger, &fake, NOW).await;
    match case {
        0 => {
            assert_eq!(
                out.dispatch_one(&lease, NOW, Mode::LoseResponse)
                    .await
                    .unwrap(),
                Some(State::Unknown)
            );
            assert_eq!(fake.receipts("store-demo-slice").len(), 1);
            assert!(out.claim(&lease, NOW + 1).await.unwrap().is_none());
            out.hold(false, NOW + 1).await.unwrap();
            let unresolved = out.reconcile(NOW + 1, Mode::UnknownLookup).await.unwrap();
            assert_eq!(unresolved.unresolved.len(), 1);
            assert!(out.resume(&unresolved.digest, NOW + 1).await.is_err());
            let report = out.reconcile(NOW + 1, Mode::Normal).await.unwrap();
            assert!(report.unresolved.is_empty());
            let saved = match &ledger.store {
                Backend::Sqlite(s) => stored_report(s).await,
                Backend::Postgres(s) => stored_report(s).await,
            };
            assert_eq!(saved["observations"][0]["state"], "delivered");
            assert!(saved["observations"][0]["remote_id"]
                .as_str()
                .unwrap()
                .starts_with("fake:in_"));
            out.resume(&report.digest, NOW + 1).await.unwrap();
            let l = out.acquire("replacement", NOW + 1).await.unwrap();
            assert!(out
                .dispatch_one(&l, NOW + 1, Mode::Normal)
                .await
                .unwrap()
                .is_none());
            assert_eq!(out.deliveries().await.unwrap()[0].attempts, 1);
            let r = &fake.receipts("store-demo-slice")[0];
            let payload: Value = serde_json::from_slice(&r.request.payload).unwrap();
            assert_eq!(payload["amount"]["atoms"], "80");
            assert_eq!(
                r.request.key,
                out.deliveries().await.unwrap()[0].intention_id
            );
        }
        1 | 2 => {
            assert!(matches!(out.acquire("other", NOW).await, Err(Error::Owned)));
            let a = out.claim(&lease, NOW).await.unwrap().unwrap();
            let response = if case == 1 {
                out.send(&a, NOW, Mode::Normal).await.unwrap()
            } else {
                Outcome::Unknown
            };
            let new = out.acquire("replacement", NOW + 15_000_000).await.unwrap();
            assert!(new.generation > lease.generation);
            assert!(matches!(
                out.send(&a, NOW + 15_000_000, Mode::Normal).await,
                Err(Error::Fenced)
            ));
            assert!(matches!(
                fake.send(&a, NOW + 15_000_000, Mode::Normal),
                Outcome::Fenced
            ));
            assert_eq!(
                out.observe(&a, NOW + 15_000_000, response).await,
                Err(Error::Fenced)
            );
            assert_eq!(out.deliveries().await.unwrap()[0].state, State::Unknown);
            assert_eq!(
                fake.receipts("store-demo-slice").len(),
                usize::from(case == 1)
            );
            out.hold(false, NOW + 15_000_000).await.unwrap();
            let report = out.reconcile(NOW + 15_000_000, Mode::Normal).await.unwrap();
            out.resume(&report.digest, NOW + 15_000_000).await.unwrap();
            assert_eq!(
                out.deliveries().await.unwrap()[0].state,
                if case == 1 {
                    State::Delivered
                } else {
                    State::Pending
                }
            );
        }
        3 => {
            let mut now = NOW;
            let mut l = lease;
            for n in 1..=20 {
                if n > 1 {
                    out.hold(false, now).await.unwrap();
                    l = start(ledger, &fake, now).await;
                }
                let state = out
                    .dispatch_one(&l, now, Mode::FailBeforeReceipt)
                    .await
                    .unwrap()
                    .unwrap();
                let d = out.deliveries().await.unwrap().remove(0);
                assert_eq!(d.attempts, n);
                assert_eq!(
                    state,
                    if n == 20 {
                        State::Rejected
                    } else {
                        State::Retry
                    }
                );
                if n < 20 {
                    let expected = (1_000_000_i64 * 2_i64.pow((n - 1) as u32)).min(300_000_000);
                    assert_eq!(d.next_attempt_us, now + expected);
                    assert!(out.claim(&l, now + 1).await.unwrap().is_none());
                    now = d.next_attempt_us;
                }
            }
            assert!(fake.receipts("store-demo-slice").is_empty());
            out.hold(true, now).await.unwrap();
            let report = out.reconcile(now, Mode::Normal).await.unwrap();
            assert_eq!(
                out.resume(&report.digest, now).await,
                Err(Error::NeedsReview)
            );
        }
        4 | 5 => {
            assert_eq!(
                out.dispatch_one(&lease, NOW, Mode::Normal).await.unwrap(),
                Some(State::Delivered)
            );
            let mut request = fake.receipts("store-demo-slice")[0].request.clone();
            if case == 4 {
                request.key = "orphan-after-backup".into();
            } else {
                request.payload = b"different".to_vec();
            }
            fake.insert_receipt(request);
            out.hold(true, NOW + 1).await.unwrap();
            assert!(matches!(
                out.claim(&lease, NOW + 1).await,
                Err(Error::Fenced)
            ));
            let report = out.reconcile(NOW + 1, Mode::Normal).await.unwrap();
            assert_eq!(report.orphan_keys.len(), usize::from(case == 4));
            assert_eq!(report.unresolved.len(), usize::from(case == 5));
            assert_eq!(
                out.resume(&report.digest, NOW + 1).await,
                Err(Error::NeedsReview)
            );
        }
        6 => {
            out.hold(false, NOW).await.unwrap();
            let report = out.reconcile(NOW, Mode::Normal).await.unwrap();
            assert!(matches!(
                ledger
                    .accept(command(Some("new-after-report")))
                    .await
                    .unwrap(),
                AcceptResult::Accepted { .. }
            ));
            assert_eq!(
                out.resume(&report.digest, NOW).await,
                Err(Error::StaleReport)
            );
            let report = out.reconcile(NOW, Mode::Normal).await.unwrap();
            out.hold(true, NOW).await.unwrap();
            assert_eq!(
                out.resume(&report.digest, NOW).await,
                Err(Error::StaleReport)
            );
        }
        7 => {
            let a = out.claim(&lease, NOW).await.unwrap().unwrap();
            let one = out.send(&a, NOW, Mode::Normal).await.unwrap();
            let two = out.send(&a, NOW, Mode::Normal).await.unwrap();
            assert!(
                matches!(
                    out.send(&a, NOW, Mode::FailBeforeReceipt).await.unwrap(),
                    Outcome::Delivered(_)
                ),
                "retained receipt cannot become authoritative absence"
            );
            assert!(
                matches!(
                    out.send(&a, NOW, Mode::Reject).await.unwrap(),
                    Outcome::Delivered(_)
                ),
                "identical retry returns retained receipt"
            );
            assert!(matches!((&one,&two),(Outcome::Delivered(a),Outcome::Delivered(b)) if a==b));
            assert_eq!(fake.receipts("store-demo-slice").len(), 1);
            assert_eq!(out.observe(&a, NOW, one).await.unwrap(), State::Delivered);
            assert_eq!(out.observe(&a, NOW, two).await, Err(Error::Fenced));
            out.hold(true, NOW + 1).await.unwrap();
            assert_eq!(out.deliveries().await.unwrap()[0].state, State::Unknown);
            let report = out.reconcile(NOW + 1, Mode::Normal).await.unwrap();
            out.resume(&report.digest, NOW + 1).await.unwrap();
            assert_eq!(out.deliveries().await.unwrap()[0].state, State::Delivered);
        }
        8 => {
            out.hold(false, NOW).await.unwrap();
            let report = out.reconcile(NOW, Mode::Normal).await.unwrap();
            out.resume(&report.digest, NOW).await.unwrap();
            let barrier = tokio::sync::Barrier::new(2);
            let first = async {
                barrier.wait().await;
                out.acquire("race-a", NOW).await
            };
            let second = async {
                barrier.wait().await;
                out.acquire("race-b", NOW).await
            };
            let (a, b) = tokio::join!(first, second);
            let winner = match (a, b) {
                (Ok(l), Err(Error::Owned | Error::Retryable))
                | (Err(Error::Owned | Error::Retryable), Ok(l)) => l,
                other => panic!("one fenced owner: {other:?}"),
            };
            assert_eq!(
                out.dispatch_one(&winner, NOW, Mode::Normal).await.unwrap(),
                Some(State::Delivered)
            );
            assert_eq!(fake.receipts("store-demo-slice").len(), 1);
        }
        9 => {
            match &ledger.store {
                Backend::Sqlite(s) => rollback_probe(s).await,
                Backend::Postgres(s) => rollback_probe(s).await,
            }
            assert_eq!(
                out.dispatch_one(&lease, NOW, Mode::Normal).await.unwrap(),
                Some(State::Delivered)
            );
            assert_eq!(out.deliveries().await.unwrap()[0].attempts, 1);
        }
        _ => panic!("case"),
    }
    assert!(
        matches!(ledger.accept(command(None)).await.unwrap(),AcceptResult::Duplicate {receipt:r,..} if r==receipt)
    );
}
#[tokio::test]
async fn sqlite_delivery_recovery_histories() {
    for case in 0..10 {
        let (dir, ledger) = sqlite().await;
        assert!(matches!(
            ledger.accept(command(None)).await.unwrap(),
            AcceptResult::Accepted { .. }
        ));
        let before = observe(dir.path());
        scenario(&ledger, case).await;
        let after = observe(dir.path());
        for field in ["journal", "indexes"] {
            if case == 6 {
                for row in before[field].as_array().unwrap() {
                    assert!(after[field].as_array().unwrap().contains(row));
                }
            } else {
                assert_eq!(
                    before[field], after[field],
                    "immutable {field} changed in case {case}"
                );
            }
        }
        ledger.close().await;
    }
}
#[tokio::test]
async fn sqlite_reopen_and_pre_send_snapshot_restore() {
    let (dir, ledger) = sqlite().await;
    let fake = MemoryDestination::new();
    ledger.accept(command(None)).await.unwrap();
    ledger.close().await;
    // Closed source, including its WAL if present. This is a recovery simulation, not the product
    // backup protocol (no live main-file copying and no backup publication claim).
    let restored = tempfile::tempdir().unwrap();
    for name in ["local.db", "local.db-wal"] {
        if dir.path().join(name).exists() {
            std::fs::copy(dir.path().join(name), restored.path().join(name)).unwrap();
        }
    }
    let ledger = Ledger::open_sqlite(dir.path()).await.unwrap();
    let lease = start(&ledger, &fake, NOW).await;
    let a = ledger
        .outbox(&fake)
        .claim(&lease, NOW)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        ledger
            .outbox(&fake)
            .send(&a, NOW, Mode::LoseResponse)
            .await
            .unwrap(),
        Outcome::Unknown
    ));
    ledger.close().await;
    let ledger = Ledger::open_sqlite(dir.path()).await.unwrap();
    let out = ledger.outbox(&fake);
    assert_eq!(out.deliveries().await.unwrap()[0].state, State::Leased);
    out.acquire("reopened", NOW + 15_000_000).await.unwrap();
    assert_eq!(out.deliveries().await.unwrap()[0].state, State::Unknown);
    ledger.close().await;
    let ledger = Ledger::open_restored_sqlite(restored.path(), &fake, NOW + 15_000_000)
        .await
        .unwrap();
    ledger.close().await;
    let ledger = Ledger::open_sqlite(restored.path()).await.unwrap();
    let out = ledger.outbox(&fake);
    assert!(matches!(
        out.acquire("unsafe", NOW + 15_000_000).await,
        Err(Error::Held)
    ));
    let report = out.reconcile(NOW + 15_000_000, Mode::Normal).await.unwrap();
    out.resume(&report.digest, NOW + 15_000_000).await.unwrap();
    let lease = out.acquire("restored", NOW + 15_000_000).await.unwrap();
    assert!(out
        .dispatch_one(&lease, NOW + 15_000_000, Mode::Normal)
        .await
        .unwrap()
        .is_none());
    assert_eq!(fake.receipts("store-demo-slice").len(), 1);
    assert_eq!(
        out.deliveries().await.unwrap()[0].attempts,
        0,
        "restore does not invent an attempt"
    );
    ledger.close().await;
}

pub(crate) async fn exercise_evidence(ledger: &Ledger) {
    let fake = MemoryDestination::new();
    let lease = start(ledger, &fake, NOW).await;
    assert_eq!(
        ledger
            .outbox(&fake)
            .dispatch_one(&lease, NOW, Mode::Normal)
            .await
            .unwrap(),
        Some(State::Delivered)
    );
}

async fn rollback_probe<S: AcceptanceStore>(store: &S) {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let mut s = tx.load_outbox().await.unwrap();
    let key = s.items[0].0.id.clone();
    let (request, _) = s.items[0].0.request(&s).unwrap();
    let d = &mut s.items[0].1;
    d.state = State::Leased;
    d.attempts = 1;
    d.owner = s.head.owner.clone();
    d.until = Some(NOW + 30_000_000);
    d.generation = s.head.generation;
    // Intentionally reuse the observation sequence: the last statement fails,
    // after mutable state and the immutable attempt have already been written.
    let m = Mutation {
        snapshot: s,
        attempt: Some((key, 1, NOW, request.request_hash, b"{}".to_vec())),
        observation: b"{}".to_vec(),
        report: None,
    };
    assert!(tx
        .write(&crate::store::records::WriteOp::Outbox(Box::new(m)))
        .await
        .is_err());
    assert!(matches!(
        tx.commit().await,
        Err(crate::store::errors::CommitError::RolledBack(_))
    ));
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let s = tx.load_outbox().await.unwrap();
    assert_eq!(s.items[0].1.attempts, 0);
    assert_eq!(s.items[0].1.state, State::Pending);
    tx.rollback().await.unwrap();
}

fn observe(path: &std::path::Path) -> Value {
    let o = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/service/observe_sqlite.py"
        ))
        .arg(path.join("local.db"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

async fn stored_report<S: AcceptanceStore>(store: &S) -> Value {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let snapshot = tx.load_outbox().await.unwrap();
    tx.rollback().await.unwrap();
    serde_json::from_slice(&snapshot.report.unwrap().1).unwrap()
}

#[tokio::test]
async fn sqlite_restore_detects_real_post_snapshot_intention_orphan() {
    let (dir, ledger) = sqlite().await;
    let fake = MemoryDestination::new();
    let original = ledger.accept(command(None)).await.unwrap();
    let before = observe(dir.path());
    ledger.close().await;
    let restored = tempfile::tempdir().unwrap();
    for name in ["local.db", "local.db-wal"] {
        if dir.path().join(name).exists() {
            std::fs::copy(dir.path().join(name), restored.path().join(name)).unwrap();
        }
    }
    let ledger = Ledger::open_sqlite(dir.path()).await.unwrap();
    let lease = start(&ledger, &fake, NOW).await;
    assert_eq!(
        ledger
            .outbox(&fake)
            .dispatch_one(&lease, NOW, Mode::Normal)
            .await
            .unwrap(),
        Some(State::Delivered)
    );
    assert!(matches!(
        ledger.accept(command(Some("post-backup"))).await.unwrap(),
        AcceptResult::Accepted { .. }
    ));
    assert_eq!(
        ledger
            .outbox(&fake)
            .dispatch_one(&lease, NOW + 1, Mode::Normal)
            .await
            .unwrap(),
        Some(State::Delivered)
    );
    ledger.close().await;
    let ledger = Ledger::open_restored_sqlite(restored.path(), &fake, NOW + 2)
        .await
        .unwrap();
    let out = ledger.outbox(&fake);
    let report = out.reconcile(NOW + 2, Mode::Normal).await.unwrap();
    assert_eq!(report.orphan_keys.len(), 1);
    assert!(report.orphan_keys[0].starts_with("in_"));
    assert_eq!(
        out.resume(&report.digest, NOW + 2).await,
        Err(Error::NeedsReview)
    );
    assert_eq!(out.deliveries().await.unwrap().len(), 1);
    assert_eq!(fake.receipts("store-demo-slice").len(), 2);
    let after = observe(restored.path());
    assert_eq!(before["journal"], after["journal"]);
    assert_eq!(before["indexes"], after["indexes"]);
    let AcceptResult::Accepted { receipt } = original else {
        panic!("accepted")
    };
    assert!(
        matches!(ledger.accept(command(None)).await.unwrap(),AcceptResult::Duplicate {receipt:r,..} if r==receipt)
    );
    ledger.close().await;
}
