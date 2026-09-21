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
        10 => {
            let a = out.claim(&lease, NOW).await.unwrap().unwrap();
            out.renew(&lease, NOW + 14_000_000).await.unwrap();
            out.renew(&lease, NOW + 28_000_000).await.unwrap();
            let receipt = out.send(&a, NOW + 29_000_000, Mode::Normal).await.unwrap();
            assert!(matches!(
                out.send(&a, NOW + 30_000_000, Mode::Normal).await,
                Err(Error::Fenced)
            ));
            assert!(matches!(
                fake.send(&a, NOW + 30_000_000, Mode::Normal),
                Outcome::Fenced
            ));
            assert_eq!(
                out.observe(&a, NOW + 30_000_000, receipt).await,
                Err(Error::Fenced)
            );
            assert!(out.claim(&lease, NOW + 30_000_000).await.unwrap().is_none());
            assert_eq!(out.deliveries().await.unwrap()[0].state, State::Unknown);
            out.hold(false, NOW + 30_000_000).await.unwrap();
            let report = out.reconcile(NOW + 30_000_000, Mode::Normal).await.unwrap();
            out.resume(&report.digest, NOW + 30_000_000).await.unwrap();
            assert_eq!(out.deliveries().await.unwrap()[0].state, State::Delivered);
            assert_eq!(fake.receipts("store-demo-slice").len(), 1);
        }
        _ => panic!("case"),
    }
    assert!(
        matches!(ledger.accept(command(None)).await.unwrap(),AcceptResult::Duplicate {receipt:r,..} if r==receipt)
    );
}
#[tokio::test]
async fn sqlite_delivery_recovery_histories() {
    for case in 0..11 {
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
    let out = ledger.outbox(&fake);
    assert_eq!(
        out.dispatch_one(&lease, NOW, Mode::Reject).await.unwrap(),
        Some(State::Rejected)
    );
    out.hold(false, NOW).await.unwrap();
    out.reconcile(NOW, Mode::Normal).await.unwrap();
    let d = out.deliveries().await.unwrap().remove(0);
    out.quarantine(
        &d.intention_id,
        d.last_observation.as_deref().unwrap(),
        "test-operator",
        "immutable guard test",
        NOW,
    )
    .await
    .unwrap();
}

async fn rollback_probe<S: AcceptanceStore>(store: &S) {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let mut s = tx.load_outbox(Query::page("")).await.unwrap();
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
        sweep: None,
        quarantine: None,
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
    let s = tx.load_outbox(Query::page("")).await.unwrap();
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
    let snapshot = tx.load_outbox(Query::page("")).await.unwrap();
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

pub(crate) async fn rejection_regression(ledger: &Ledger, restore: bool) {
    let fake = MemoryDestination::new();
    ledger.accept(command(None)).await.unwrap();
    let out = ledger.outbox(&fake);
    let lease = start(ledger, &fake, NOW).await;
    assert_eq!(
        out.dispatch_one(&lease, NOW, Mode::Reject).await.unwrap(),
        Some(State::Rejected)
    );
    out.hold(restore, NOW + 1).await.unwrap();
    if !restore {
        out.reconcile(NOW + 1, Mode::UnknownLookup).await.unwrap();
    }
    assert_eq!(
        out.deliveries().await.unwrap()[0].state,
        State::Rejected,
        "a permanent rejection must survive restore and unavailable inventory"
    );
    let report = out.reconcile(NOW + 2, Mode::Normal).await.unwrap();
    assert_eq!(
        out.resume(&report.digest, NOW + 2).await,
        Err(Error::NeedsReview)
    );
    assert_eq!(out.deliveries().await.unwrap()[0].attempts, 1);
    assert!(fake.receipts("store-demo-slice").is_empty());
}
#[tokio::test]
async fn rejected_survives_unknown_inventory() {
    let (_dir, ledger) = sqlite().await;
    rejection_regression(&ledger, false).await;
    ledger.close().await;
}
#[tokio::test]
async fn rejected_survives_restore() {
    let (_dir, ledger) = sqlite().await;
    rejection_regression(&ledger, true).await;
    ledger.close().await;
}

pub(crate) fn capacity_seed() -> Vec<crate::store::records::WriteOp> {
    use crate::store::records::WriteOp;
    let mut ops = seed::seed();
    let mut chain = ops
        .iter()
        .find_map(|op| match op {
            WriteOp::SeedChain(c) => Some(c.clone()),
            _ => None,
        })
        .unwrap();
    chain.id = "capacity-second-chain".into();
    ops.push(WriteOp::SeedChain(chain));
    ops
}
pub(crate) async fn capacity_regression(ledger: &Ledger) {
    let fake = MemoryDestination::new();
    let out = ledger.outbox(&fake);
    for n in 0..1001 {
        let mut cmd = command(Some(&format!("capacity-{n}")));
        if n >= 999 {
            let mut v: Value = serde_json::from_slice(&cmd.bytes).unwrap();
            v["chain"] = json!("capacity-second-chain");
            cmd.bytes = serde_json::to_vec(&v).unwrap();
        }
        assert!(matches!(
            ledger.accept(cmd).await.unwrap(),
            AcceptResult::Accepted { .. }
        ));
        if [63, 64, 999, 1000].contains(&n) {
            out.hold(false, NOW)
                .await
                .expect("pause at and beyond batch limits");
            out.hold(true, NOW)
                .await
                .expect("restore hold at and beyond batch limits");
            let report = out
                .reconcile(NOW, Mode::Normal)
                .await
                .expect("full reconciliation across pages");
            assert_eq!(report.intention_count, n + 1);
            out.resume(&report.digest, NOW).await.unwrap();
        }
    }
    assert!(
        matches!(out.deliveries().await, Err(Error::ScanLimit)),
        "legacy complete-list query retains its bound"
    );
    let mut after = String::new();
    let mut count = 0;
    loop {
        let page = out.deliveries_after(&after).await.unwrap();
        assert!(page.len() <= PAGE_SIZE);
        if page.is_empty() {
            break;
        }
        for d in &page {
            assert!(d.intention_id > after);
        }
        after = page.last().unwrap().intention_id.clone();
        count += page.len();
    }
    assert_eq!(count, 1001);
    let lease = out.acquire("large-installation", NOW).await.unwrap();
    let attempt = out.claim(&lease, NOW).await.unwrap().unwrap();
    let response = out.send(&attempt, NOW, Mode::LoseResponse).await.unwrap();
    out.hold(true, NOW + 1).await.unwrap();
    assert!(matches!(
        out.send(&attempt, NOW + 1, Mode::Normal).await,
        Err(Error::Fenced)
    ));
    assert_eq!(
        out.observe(&attempt, NOW + 1, response).await,
        Err(Error::Fenced)
    );
    let report = out.reconcile(NOW + 1, Mode::Normal).await.unwrap();
    assert_eq!(report.intention_count, 1001);
    out.resume(&report.digest, NOW + 1).await.unwrap();
    assert_eq!(fake.receipts("store-demo-slice").len(), 1);
    out.hold(false, NOW + 2).await.unwrap();
    let unresolved = out.reconcile(NOW + 2, Mode::UnknownLookup).await.unwrap();
    assert_eq!(unresolved.unresolved_count, 1001);
    assert_eq!(
        unresolved.unresolved.len(),
        PAGE_SIZE,
        "bounded sample, never incomplete coverage"
    );
    assert!(out.resume(&unresolved.digest, NOW + 2).await.is_err());
    let report = out.reconcile(NOW + 3, Mode::Normal).await.unwrap();
    out.resume(&report.digest, NOW + 3).await.unwrap();
}

#[tokio::test]
async fn controls_survive_1001_intentions() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), seed::installation())
        .await
        .unwrap();
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    for op in capacity_seed() {
        tx.write(&op).await.unwrap();
    }
    tx.commit().await.unwrap();
    let ledger = Ledger {
        store: Backend::Sqlite(store),
    };
    capacity_regression(&ledger).await;
    ledger.close().await;
}

#[test]
fn reconciliation_transition_matrix() {
    let request = Request {
        store_id: "matrix".into(),
        key: "key".into(),
        request_hash: "hash".into(),
        payload: b"{}".to_vec(),
    };
    let receipt = fake::Receipt {
        request: request.clone(),
        remote_id: "remote".into(),
    };
    let mut mismatch = receipt.clone();
    mismatch.request.payload = b"different".to_vec();
    let states = [
        State::Held,
        State::Pending,
        State::Leased,
        State::Delivered,
        State::Retry,
        State::Unknown,
        State::Rejected,
    ];
    for prior in states {
        for attempts in [0, 19, 20] {
            for quarantine in [None, Some("operator-veto".into())] {
                let d = Delivery {
                    intention_id: "key".into(),
                    state: prior,
                    attempts,
                    next_attempt_us: 0,
                    owner: None,
                    generation: 0,
                    until: None,
                    last_observation: None,
                    quarantine,
                };
                for (outcome, expected) in [
                    (Outcome::Delivered(receipt.clone()), State::Delivered),
                    (Outcome::Delivered(mismatch.clone()), State::Rejected),
                    (
                        Outcome::Absent,
                        if attempts >= 20 {
                            State::Rejected
                        } else {
                            State::Pending
                        },
                    ),
                    (Outcome::Unknown, State::Unknown),
                    (Outcome::Rejected, State::Rejected),
                    (Outcome::Fenced, State::Unknown),
                ] {
                    assert_eq!(
                        reconciled_state(&d, &request, &outcome),
                        if prior == State::Rejected {
                            State::Rejected
                        } else {
                            expected
                        },
                        "{prior:?}, {attempts}, {outcome:?}"
                    );
                }
            }
        }
    }
}

pub(crate) async fn quarantine_regression(ledger: &Ledger, mode: Mode) {
    let fake = MemoryDestination::new();
    ledger.accept(command(None)).await.unwrap();
    let out = ledger.outbox(&fake);
    let mut now = NOW;
    let mut lease = start(ledger, &fake, now).await;
    let times = if matches!(mode, Mode::FailBeforeReceipt) {
        20
    } else {
        1
    };
    for n in 1..=times {
        out.dispatch_one(&lease, now, mode).await.unwrap();
        if n < times {
            now = out.deliveries().await.unwrap()[0].next_attempt_us;
            out.hold(false, now).await.unwrap();
            lease = start(ledger, &fake, now).await;
        }
    }
    let d = out.deliveries().await.unwrap().remove(0);
    let key = d.intention_id;
    let observation = d.last_observation.unwrap();
    assert_eq!(
        out.quarantine(&key, &observation, "operator", "investigate", now)
            .await,
        Err(Error::Held)
    );
    out.hold(false, now).await.unwrap();
    let old_report = out.reconcile(now, Mode::UnknownLookup).await.unwrap();
    let d = out.deliveries().await.unwrap().remove(0);
    let observation = d.last_observation.unwrap();
    assert!(out
        .quarantine(&key, &observation, "", "reason", now)
        .await
        .is_err());
    assert!(out
        .quarantine(&key, &observation, "operator", " ", now)
        .await
        .is_err());
    assert_eq!(
        out.quarantine(&key, "stale", "operator", "reason", now)
            .await,
        Err(Error::StaleReport)
    );
    let isolation = out
        .quarantine(
            &key,
            &observation,
            "operator",
            "Investigate receipt; do not retry this intent",
            now,
        )
        .await
        .unwrap();
    assert_eq!(
        out.quarantine(&key, &observation, "operator", "retry resolution", now)
            .await,
        Err(Error::StaleReport)
    );
    assert_eq!(
        out.resume(&old_report.digest, now).await,
        Err(Error::StaleReport)
    );
    let unavailable = out.reconcile(now, Mode::UnknownLookup).await.unwrap();
    assert_eq!(unavailable.unresolved_count, 0);
    assert!(
        out.resume(&unavailable.digest, now).await.is_err(),
        "unknown global inventory still holds even with quarantine"
    );
    let before = out.deliveries().await.unwrap().remove(0);
    match &ledger.store {
        Backend::Sqlite(s) => terminal_guards(s, &key).await,
        Backend::Postgres(s) => terminal_guards(s, &key).await,
    }
    assert!(matches!(
        ledger
            .accept(command(Some("unrelated-after-quarantine")))
            .await
            .unwrap(),
        AcceptResult::Accepted { .. }
    ));
    let report = out.reconcile(now, Mode::Normal).await.unwrap();
    assert_eq!(report.intention_count, 2);
    assert_eq!(report.unresolved_count, 0);
    out.resume(&report.digest, now).await.unwrap();
    let lease = out.acquire("unrelated-worker", now).await.unwrap();
    assert_eq!(
        out.dispatch_one(&lease, now, Mode::Normal).await.unwrap(),
        Some(State::Delivered)
    );
    assert!(out.claim(&lease, now).await.unwrap().is_none());
    let d = out
        .deliveries()
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.intention_id == key)
        .unwrap();
    assert_eq!(d.quarantine, Some(isolation.clone()));
    assert_eq!(d.attempts, before.attempts);
    if before.state == State::Rejected {
        assert_eq!(d.state, State::Rejected);
    }
    let receipts = fake.receipts("store-demo-slice");
    assert_eq!(
        receipts.len(),
        1 + usize::from(matches!(mode, Mode::LoseResponse))
    );
    assert_eq!(
        receipts.iter().filter(|r| r.request.key == key).count(),
        usize::from(matches!(mode, Mode::LoseResponse))
    );
    out.hold(true, now + 1).await.unwrap();
    assert_eq!(
        out.deliveries()
            .await
            .unwrap()
            .iter()
            .find(|d| d.intention_id == key)
            .unwrap()
            .quarantine,
        Some(isolation)
    );
    let report = out.reconcile(now + 1, Mode::Normal).await.unwrap();
    out.resume(&report.digest, now + 1).await.unwrap();
    let lease = out.acquire("restored-worker", now + 1).await.unwrap();
    assert!(out.claim(&lease, now + 1).await.unwrap().is_none());
    out.hold(false, now + 2).await.unwrap();
    let delivered = out
        .deliveries()
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.intention_id != key)
        .unwrap();
    assert_eq!(
        out.quarantine(
            &delivered.intention_id,
            delivered.last_observation.as_deref().unwrap(),
            "operator",
            "invalid resolution",
            now + 2
        )
        .await,
        Err(Error::InvalidInput)
    );
}
async fn terminal_guards<S: AcceptanceStore>(store: &S, key: &str) {
    // Runtime-role writes cannot reduce attempts, undo rejection or lease quarantine.
    for state in [
        State::Held,
        State::Pending,
        State::Leased,
        State::Delivered,
        State::Retry,
        State::Unknown,
    ] {
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let mut s = tx.load_outbox(Query::Key(key.into())).await.unwrap();
        let d = &mut s.items[0].1;
        let rejected = d.state == State::Rejected;
        if !rejected && state != State::Leased {
            tx.rollback().await.unwrap();
            continue;
        }
        d.state = state;
        d.owner = (state == State::Leased).then(|| "illegal-worker".into());
        d.until = (state == State::Leased).then_some(NOW + 30_000_000);
        s.head.revision += 1;
        let m = Mutation {
            snapshot: s,
            sweep: None,
            quarantine: None,
            attempt: None,
            observation: b"{}".to_vec(),
            report: None,
        };
        assert!(tx
            .write(&crate::store::records::WriteOp::Outbox(Box::new(m)))
            .await
            .is_err());
        tx.rollback().await.unwrap();
    }
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let mut s = tx.load_outbox(Query::Key(key.into())).await.unwrap();
    s.items[0].1.attempts = 0;
    s.head.revision += 1;
    let m = Mutation {
        snapshot: s,
        sweep: None,
        quarantine: None,
        attempt: None,
        observation: b"{}".to_vec(),
        report: None,
    };
    assert!(tx
        .write(&crate::store::records::WriteOp::Outbox(Box::new(m)))
        .await
        .is_err());
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn sqlite_quarantine_rejected_exhausted_unknown() {
    for mode in [Mode::Reject, Mode::FailBeforeReceipt, Mode::LoseResponse] {
        let (dir, ledger) = sqlite().await;
        ledger.accept(command(None)).await.unwrap();
        let before = observe(dir.path());
        quarantine_regression(&ledger, mode).await;
        let after = observe(dir.path());
        for field in ["journal", "indexes"] {
            for row in before[field].as_array().unwrap() {
                assert!(after[field].as_array().unwrap().contains(row));
            }
        }
        ledger.close().await;
        let reopened = Ledger::open_sqlite(dir.path()).await.unwrap();
        assert_eq!(observe(dir.path()), after);
        reopened.close().await;
    }
}

// Storage-only adversarial intentions exercise operational paging/dependencies. These
// deliberately bypass acceptance and are NOT canonical economic conformance fixtures.
async fn storage_intentions<S: AcceptanceStore>(
    store: &S,
    count: usize,
    padding: usize,
    dependencies: bool,
    malformed_last: bool,
) {
    use crate::store::records::{
        CanonicalRecord, HeldDelivery, JournalRecord, JournalRow, WriteOp,
    };
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(60))
        .await
        .unwrap();
    let s = tx.load_outbox(Query::page("")).await.unwrap();
    let source: Value = serde_json::from_slice(&s.items[0].0.bytes).unwrap();
    for n in 0..count {
        let id = format!("in_{n:064x}");
        let mut body = source.clone();
        body["id"] = json!(id);
        body["idempotency_key"] = json!(id);
        if padding > 0 {
            body["storage_probe_padding"] = json!("x".repeat(padding));
        }
        if dependencies && n + 1 < count {
            body["depends_on"] = json!([format!("in_{:064x}", count - 1)]);
        }
        if malformed_last && n + 1 == count {
            body["id"] = json!("invalid-index-projection");
        }
        let record = JournalRecord {
            scope: s.installation.scope.clone(),
            canonical: CanonicalRecord {
                canonical_bytes: bytes(&body).unwrap(),
                content_hash: canonical::digest(
                    Domain::RecordContent,
                    &json!(["intention", 1, body]),
                )
                .unwrap(),
            },
            row: JournalRow::Intention {
                id: id.clone(),
                event_id: body["event_id"].as_str().unwrap().into(),
                obligation_id: body["obligation_id"].as_str().unwrap().into(),
                destination_id: "fake".into(),
                idempotency_key: id.clone(),
            },
        };
        tx.write(&WriteOp::Journal(Box::new(record))).await.unwrap();
        tx.write(&WriteOp::HoldDelivery(HeldDelivery {
            scope: s.installation.scope.clone(),
            intention_id: id,
            next_attempt_us: NOW,
        }))
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}
pub(crate) async fn storage_paging_regression(ledger: &Ledger, large_bytes: bool) {
    ledger.accept(command(None)).await.unwrap();
    let count = if large_bytes { 4 } else { PAGE_SIZE + 2 };
    match &ledger.store {
        Backend::Sqlite(s) => {
            storage_intentions(
                s,
                count,
                if large_bytes { 3 * 1024 * 1024 } else { 0 },
                !large_bytes,
                false,
            )
            .await
        }
        Backend::Postgres(s) => {
            storage_intentions(
                s,
                count,
                if large_bytes { 3 * 1024 * 1024 } else { 0 },
                !large_bytes,
                false,
            )
            .await
        }
    }
    match &ledger.store {
        Backend::Sqlite(s) => assert_page_byte_bounds(s).await,
        Backend::Postgres(s) => assert_page_byte_bounds(s).await,
    }
    let fake = MemoryDestination::new();
    let out = ledger.outbox(&fake);
    out.hold(true, NOW).await.unwrap();
    let mut after = String::new();
    let mut total = 0;
    loop {
        let page = out.deliveries_after(&after).await.unwrap();
        if page.is_empty() {
            break;
        }
        assert!(page.len() <= if large_bytes { 3 } else { PAGE_SIZE });
        total += page.len();
        after = page.last().unwrap().intention_id.clone();
    }
    assert_eq!(total, count + 1);
    let report = out.reconcile(NOW, Mode::Normal).await.unwrap();
    assert_eq!(report.intention_count, (count + 1) as i64);
    out.resume(&report.digest, NOW).await.unwrap();
    if !large_bytes {
        let lease = out.acquire("dependency-worker", NOW).await.unwrap();
        // First 65 candidates are blocked by a dependency outside their page.
        let a = out.claim(&lease, NOW).await.unwrap().unwrap();
        assert_eq!(a.request.key, format!("in_{:064x}", count - 1));
        let rejected = out.send(&a, NOW, Mode::Reject).await.unwrap();
        assert_eq!(
            out.observe(&a, NOW, rejected).await.unwrap(),
            State::Rejected
        );
        out.hold(false, NOW).await.unwrap();
        out.reconcile(NOW, Mode::Normal).await.unwrap();
        let d = out
            .deliveries()
            .await
            .unwrap()
            .into_iter()
            .find(|d| d.intention_id == a.request.key)
            .unwrap();
        out.quarantine(
            &d.intention_id,
            d.last_observation.as_deref().unwrap(),
            "operator",
            "blocked dependency",
            NOW,
        )
        .await
        .unwrap();
        let report = out.reconcile(NOW, Mode::Normal).await.unwrap();
        out.resume(&report.digest, NOW).await.unwrap();
        let lease = out.acquire("unrelated-worker", NOW).await.unwrap();
        assert_eq!(
            out.dispatch_one(&lease, NOW, Mode::Normal).await.unwrap(),
            Some(State::Delivered)
        );
        assert!(
            out.claim(&lease, NOW).await.unwrap().is_none(),
            "quarantine never satisfies a dependency"
        );
        assert_eq!(fake.receipts("store-demo-slice").len(), 1);
    }
}
#[tokio::test]
async fn sqlite_storage_bytes_and_cross_page_dependencies() {
    for large in [false, true] {
        let (_dir, ledger) = sqlite().await;
        storage_paging_regression(&ledger, large).await;
        ledger.close().await;
    }
}

async fn assert_page_byte_bounds<S: AcceptanceStore>(store: &S) {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(tx
        .load_outbox(Query::Control)
        .await
        .unwrap()
        .items
        .is_empty());
    let mut after = String::new();
    let mut total = 0;
    loop {
        let s = tx.load_outbox(Query::page(&after)).await.unwrap();
        if s.items.is_empty() {
            break;
        }
        assert!(s.items.len() <= PAGE_SIZE);
        assert!(s.items.iter().map(|(i, _)| i.bytes.len()).sum::<usize>() <= 8 * 1024 * 1024);
        total += s.items.len();
        after = s.items.last().unwrap().0.id.clone();
    }
    assert!(total > 0);
    tx.rollback().await.unwrap();
}

pub(crate) async fn prepare_bad_page(ledger: &Ledger) {
    ledger.accept(command(None)).await.unwrap();
    match &ledger.store {
        Backend::Sqlite(s) => storage_intentions(s, PAGE_SIZE + 2, 0, false, true).await,
        Backend::Postgres(s) => storage_intentions(s, PAGE_SIZE + 2, 0, false, true).await,
    }
}
pub(crate) async fn reject_bad_page(ledger: &Ledger) {
    let fake = MemoryDestination::new();
    let out = ledger.outbox(&fake);
    // Page one writes pending states/evidence before a bad indexed projection on
    // page two fails. The caller compares every physical cell across this failure.
    assert_eq!(
        out.reconcile(NOW, Mode::Normal).await.err(),
        Some(Error::Integrity)
    );
    assert!(matches!(
        out.acquire("still-held", NOW).await,
        Err(Error::Held)
    ));
    assert!(fake.receipts("store-demo-slice").is_empty());
}
#[tokio::test]
async fn sqlite_reconciliation_later_page_failure_rolls_back_every_cell() {
    let (dir, ledger) = sqlite().await;
    prepare_bad_page(&ledger).await;
    let before = observe(dir.path());
    reject_bad_page(&ledger).await;
    assert_eq!(observe(dir.path()), before);
    ledger.close().await;
    let ledger = Ledger::open_sqlite(dir.path()).await.unwrap();
    assert_eq!(observe(dir.path()), before);
    ledger.close().await;
}
