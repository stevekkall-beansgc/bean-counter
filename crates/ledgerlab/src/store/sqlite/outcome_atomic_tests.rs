//! Complete-plan evidence: plans come only from the real coordinator fixture.
use super::*;
use crate::{
    service::accept::outcome::fixture,
    store::{ports::AcceptanceTx, sqlite::tests as original},
};
use sqlx::AssertSqlSafe;
use std::{sync::Arc, time::Duration};
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

#[tokio::test]
async fn outcome_durable_independent_oracle() {
    use crate::service::accept::outcome::{run, OutcomeResult};
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f).await;
    let mut prefixes = Vec::new();
    for i in 0..f.commands.len() {
        let auth = SyntheticAuthority {
            proof: f.proofs[i].clone(),
            write_allowed: true,
        };
        assert!(matches!(
            run(&store, &f.commands[i], &auth).await.unwrap(),
            OutcomeResult::Accepted(_)
        ));
        let before = dump(&store).await;
        store.close().await;
        store = SqliteStore::open(dir.path()).await.unwrap();
        let physical = dump(&store).await;
        assert_eq!(before, physical);
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        let q = f.plans[i].resolution();
        tx.lock_scopes(&q.locks).await.unwrap();
        let OutcomeResolution::Complete(mut snapshot) = tx.resolve_outcome(q).await.unwrap() else {
            panic!("complete durable prefix")
        };
        let mut records: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT canonical_bytes FROM outcome_records")
                .fetch_all(tx.conn())
                .await
                .unwrap();
        records.sort();
        snapshot.records.sort();
        assert_eq!(
            records, snapshot.records,
            "observer must include every retained envelope"
        );
        let mut deliveries = Vec::new();
        for prior in &f.plans[..=i] {
            deliveries.push(
                tx.lookup_outcome_delivery(&prior.delivery().key)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        tx.rollback().await.unwrap();
        prefixes.push(crate::store::outcome_evidence::observe(
            snapshot, deliveries, physical,
        ));
    }
    store.close().await;
    crate::store::outcome_evidence::check("sqlite", prefixes);
}
async fn prepared(f: &fixture::Fixture) -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let mut installation = original::installation();
    installation.scope.tenant = f.plans[0].delivery().key.scope[0].clone();
    installation.scope.environment = f.plans[0].delivery().key.scope[1].clone();
    let store = SqliteStore::create(dir.path(), installation).await.unwrap();
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    for r in &f.provisioned_records {
        insert_record(tx.conn(), r, &mut Boundaries { fault: None })
            .await
            .unwrap();
    }
    for h in &f.provisioned_heads {
        if let (Some(rev), Some(value)) = (&h.revision, &h.value) {
            sqlx::query("INSERT INTO outcome_heads VALUES (?,?,?,?)")
                .bind(class(h.lock.class))
                .bind(&h.lock.key)
                .bind(rev)
                .bind(value)
                .execute(tx.conn())
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    (dir, store)
}
async fn append_plan(store: &SqliteStore, p: &ValidatedOutcomePlan) {
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    tx.lock_scopes(&p.resolution().locks).await.unwrap();
    tx.append_outcome(p).await.unwrap();
    tx.commit().await.unwrap();
}
async fn dump(store: &SqliteStore) -> Vec<(String, Vec<String>)> {
    let mut c = store.inner.readers.acquire().await.unwrap();
    let tables:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&mut *c).await.unwrap();
    let mut out = Vec::new();
    for t in tables {
        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&t)
                .fetch_all(&mut *c)
                .await
                .unwrap();
        let expr = cols
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join("||'|'||");
        let rows = sqlx::query_scalar(AssertSqlSafe(format!("SELECT {expr} FROM {t} ORDER BY 1")))
            .fetch_all(&mut *c)
            .await
            .unwrap();
        out.push((t, rows));
    }
    out
}
async fn lookup_plan(store: &SqliteStore, p: &ValidatedOutcomePlan) {
    let mut tx = store.begin_outcome(deadline()).await.unwrap();
    assert_eq!(
        tx.lookup_outcome_delivery(&p.delivery().key)
            .await
            .unwrap()
            .as_ref(),
        Some(p.delivery())
    );
    tx.rollback().await.unwrap();
}
#[tokio::test]
async fn composite_lifecycle_atomic_receipts_heads_and_reopen() {
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f).await;
    for (i, p) in f.plans.iter().enumerate() {
        append_plan(&store, p).await;
        let before = dump(&store).await;
        store.close().await;
        store = SqliteStore::open(dir.path()).await.unwrap();
        assert_eq!(dump(&store).await, before);
        for old in &f.plans[..=i] {
            lookup_plan(&store, old).await;
        }
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        tx.lock_scopes(&p.resolution().locks).await.unwrap();
        let OutcomeResolution::Complete(snapshot) =
            tx.resolve_outcome(p.resolution()).await.unwrap()
        else {
            panic!("complete prefix required")
        };
        for r in p.economic_records().iter().chain(p.settlement_records()) {
            assert!(snapshot.records.contains(r));
        }
        for w in p.head_writes() {
            let h = snapshot
                .heads
                .iter()
                .find(|h| lock_identity(&h.lock) == lock_identity(&w.lock))
                .unwrap();
            assert_eq!(h.revision.as_ref(), Some(&w.revision));
            assert_eq!(h.value.as_ref(), Some(&w.value));
        }
        tx.rollback().await.unwrap();
    }
    assert_eq!(
        f.plans[1]
            .observed_heads()
            .iter()
            .filter(|h| h.lock.class == OutcomeLockClass::Reservation)
            .count(),
        1
    );
    assert!(f.plans[2]
        .head_writes()
        .iter()
        .all(|w| w.lock.class != OutcomeLockClass::Reservation));
    store.close().await;
}
#[tokio::test]
async fn composite_every_write_edge_rollback_cancel_and_reopen() {
    let f = fixture::lifecycle();
    let mut positions = 0;
    for index in 0..f.plans.len() {
        let (dir, mut store) = prepared(&f).await;
        for p in &f.plans[..index] {
            append_plan(&store, p).await;
        }
        let before = dump(&store).await;
        let probe = Arc::new(Fault {
            cut: usize::MAX,
            ..Default::default()
        });
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        tx.lock_scopes(&f.plans[index].resolution().locks)
            .await
            .unwrap();
        tx.outcome_fault = Some(probe.clone());
        tx.append_outcome(&f.plans[index]).await.unwrap();
        tx.rollback().await.unwrap();
        let edges = probe.steps.load(Ordering::SeqCst);
        assert!(edges > 0);
        positions += edges;
        for cut in 0..edges {
            let mut tx = store.begin_outcome(deadline()).await.unwrap();
            tx.lock_scopes(&f.plans[index].resolution().locks)
                .await
                .unwrap();
            tx.outcome_fault = Some(Arc::new(Fault {
                cut,
                ..Default::default()
            }));
            assert!(matches!(
                tx.append_outcome(&f.plans[index]).await,
                Err(StoreError::Deadline)
            ));
            assert!(tx.failed);
            assert!(tx.commit().await.is_err());
            store.close().await;
            store = SqliteStore::open(dir.path()).await.unwrap();
            assert_eq!(dump(&store).await, before, "plan {index} edge {cut}");
            let fault = Arc::new(Fault {
                cut,
                pause: true,
                ..Default::default()
            });
            let mut tx = store.begin_outcome(deadline()).await.unwrap();
            tx.lock_scopes(&f.plans[index].resolution().locks)
                .await
                .unwrap();
            tx.outcome_fault = Some(fault.clone());
            let plan = f.plans[index].clone();
            let task = tokio::spawn(async move { tx.append_outcome(&plan).await });
            tokio::time::timeout(Duration::from_secs(5), fault.reached.notified())
                .await
                .unwrap();
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            // The next begin drains queued rollback before reusing the writer.
            store
                .begin_outcome(deadline())
                .await
                .unwrap()
                .rollback()
                .await
                .unwrap();
            store.close().await;
            store = SqliteStore::open(dir.path()).await.unwrap();
            assert_eq!(dump(&store).await, before, "cancel plan {index} edge {cut}");
        }
        store.close().await;
        store = SqliteStore::open(dir.path()).await.unwrap();
        assert_eq!(dump(&store).await, before);
        append_plan(&store, &f.plans[index]).await;
        lookup_plan(&store, &f.plans[index]).await;
        store.close().await;
    }
    eprintln!("validated composite lifecycle physical before/after positions: {positions}");
}
#[tokio::test]
async fn composite_cancelled_commit_reply_and_original_retry() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f).await;
    for p in &f.plans {
        let mut tx = store.begin_outcome(deadline()).await.unwrap();
        tx.lock_scopes(&p.resolution().locks).await.unwrap();
        tx.append_outcome(p).await.unwrap();
        let mut commit = Box::pin(tx.commit());
        assert!(matches!(
            commit
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        drop(commit);
        store.close().await;
        store = SqliteStore::open(dir.path()).await.unwrap();
        let before = dump(&store).await;
        lookup_plan(&store, p).await;
        assert_eq!(dump(&store).await, before);
    }
    for p in &f.plans {
        lookup_plan(&store, p).await;
    }
    store.close().await;
}
#[tokio::test]
async fn composite_real_contention_then_stale_plan_rolls_back() {
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f).await;
    append_plan(&store, &f.plans[0]).await;
    let contender = store.test_contender().await.unwrap();
    let mut leader = store.begin_outcome(deadline()).await.unwrap();
    leader
        .lock_scopes(&f.plans[1].resolution().locks)
        .await
        .unwrap();
    assert!(store.test_write_locked(dir.path()).await.unwrap());
    let attempt = contender
        .begin_outcome(Instant::now() + Duration::from_millis(40))
        .await;
    assert!(attempt.is_err(), "second physical writer must contend");
    leader.append_outcome(&f.plans[1]).await.unwrap();
    leader.commit().await.unwrap();
    let before = dump(&store).await;
    let mut loser = contender.begin_outcome(deadline()).await.unwrap();
    loser
        .lock_scopes(&f.plans[1].resolution().locks)
        .await
        .unwrap();
    assert!(matches!(
        loser.append_outcome(&f.plans[1]).await,
        Err(StoreError::ExpectedCurrent)
    ));
    loser.rollback().await.unwrap();
    lookup_plan(&contender, &f.plans[1]).await;
    assert_eq!(dump(&store).await, before);
    contender.close().await;
    store.close().await;
}

#[test]
fn composite_process_child() {
    use std::io::Write;
    let Ok(path) = std::env::var("LEDGERLAB_SQLITE_COMPOSITE_CHILD") else {
        return;
    };
    let index: usize = std::env::var("LEDGERLAB_SQLITE_COMPOSITE_INDEX")
        .unwrap()
        .parse()
        .unwrap();
    let cut = std::env::var("LEDGERLAB_SQLITE_COMPOSITE_CUT").unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let f = fixture::lifecycle();
            let store = SqliteStore::open(std::path::Path::new(&path))
                .await
                .unwrap();
            let mut tx = store.begin_outcome(deadline()).await.unwrap();
            tx.lock_scopes(&f.plans[index].resolution().locks)
                .await
                .unwrap();
            if cut == "committed" {
                tx.append_outcome(&f.plans[index]).await.unwrap();
                tx.commit().await.unwrap();
            } else {
                let fault = Arc::new(Fault {
                    cut: cut.parse().unwrap(),
                    pause: true,
                    ..Default::default()
                });
                tx.outcome_fault = Some(fault.clone());
                let plan = f.plans[index].clone();
                tokio::spawn(async move { tx.append_outcome(&plan).await });
                tokio::time::timeout(Duration::from_secs(5), fault.reached.notified())
                    .await
                    .unwrap();
            }
            println!("LEDGERLAB_COMPOSITE_CHILD_READY");
            std::io::stdout().flush().unwrap();
            std::future::pending::<()>().await;
        });
}
fn kill_at_marker(path: &std::path::Path, index: usize, cut: &str) {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::sqlite::outcomes::atomic_tests::composite_process_child",
            "--nocapture",
        ])
        .env("LEDGERLAB_SQLITE_COMPOSITE_CHILD", path)
        .env("LEDGERLAB_SQLITE_COMPOSITE_INDEX", index.to_string())
        .env("LEDGERLAB_SQLITE_COMPOSITE_CUT", cut)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let reached = BufReader::new(stdout)
            .lines()
            .any(|line| line.is_ok_and(|s| s.contains("LEDGERLAB_COMPOSITE_CHILD_READY")));
        let _ = send.send(reached);
    });
    let ready = receive.recv_timeout(Duration::from_secs(30));
    let killed = child.kill();
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(
        ready.is_ok_and(|r| r),
        "child failed before requested barrier: {status}"
    );
    killed.unwrap();
    assert!(!status.success());
}
#[tokio::test]
async fn composite_process_death_before_commit_and_after_durable_ack() {
    let f = fixture::lifecycle();
    for index in 0..f.plans.len() {
        for committed in [false, true] {
            let (dir, store) = prepared(&f).await;
            for p in &f.plans[..index] {
                append_plan(&store, p).await;
            }
            let before = dump(&store).await;
            let probe = Arc::new(Fault {
                cut: usize::MAX,
                ..Default::default()
            });
            let mut tx = store.begin_outcome(deadline()).await.unwrap();
            tx.lock_scopes(&f.plans[index].resolution().locks)
                .await
                .unwrap();
            tx.outcome_fault = Some(probe.clone());
            tx.append_outcome(&f.plans[index]).await.unwrap();
            tx.rollback().await.unwrap();
            // Last mutable head has been written; delivery index/commit not sent.
            let cut = if committed {
                "committed".into()
            } else {
                (probe.steps.load(Ordering::SeqCst) - 3).to_string()
            };
            store.close().await;
            kill_at_marker(dir.path(), index, &cut);
            let reopened = SqliteStore::open(dir.path()).await.unwrap();
            if committed {
                lookup_plan(&reopened, &f.plans[index]).await;
                let stable = dump(&reopened).await;
                lookup_plan(&reopened, &f.plans[index]).await;
                assert_eq!(dump(&reopened).await, stable, "lookup retry adds nothing");
            } else {
                assert_eq!(
                    dump(&reopened).await,
                    before,
                    "process death left residue at plan {index}"
                );
                append_plan(&reopened, &f.plans[index]).await;
                lookup_plan(&reopened, &f.plans[index]).await;
            }
            reopened.close().await;
        }
    }
}

#[derive(Clone)]
struct SyntheticAuthority {
    proof: crate::service::accept::outcome::AuthorityProof,
    write_allowed: bool,
}
impl crate::service::accept::outcome::OutcomeAuthority for SyntheticAuthority {
    fn verify(
        &self,
        c: &crate::service::accept::outcome::OutcomeCommand,
        _snapshot: &OutcomeSnapshot,
        write: bool,
    ) -> Result<crate::service::accept::outcome::AuthorityProof, crate::ServiceError> {
        // Test-only trusted verifier: exact fixture principal/scope/target/source.
        // It supplies no production default and certifies no external assent.
        if [c.principal.scope.tenant(), c.principal.scope.environment()]
            != [self.proof.scope[0].as_str(), self.proof.scope[1].as_str()]
            || c.target != self.proof.target
            || c.invocation_id != self.proof.invocation_id
            || c.principal.source != self.proof.source
            || !c.principal.can_read
            || c.principal.principal_id != self.proof.authority["principal"].as_str().unwrap()
            || (write && !self.write_allowed)
        {
            return Err(crate::ServiceError::Unavailable);
        }
        Ok(self.proof.clone())
    }
}
#[tokio::test]
async fn coordinator_on_sqlite_retries_aliases_and_conflicts_preserve_receipts() {
    use crate::service::accept::outcome::{run, OutcomeOperation, OutcomeResult};
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f).await;
    for i in 0..f.commands.len() {
        let auth = SyntheticAuthority {
            proof: f.proofs[i].clone(),
            write_allowed: true,
        };
        assert_eq!(
            run(&store, &f.commands[i], &auth).await.unwrap(),
            OutcomeResult::Accepted(f.plans[i].delivery().clone()),
            "step {i}"
        );
        store.close().await;
        store = SqliteStore::open(dir.path()).await.unwrap();
    }
    let stable = dump(&store).await;
    for i in 0..f.commands.len() {
        let auth = SyntheticAuthority {
            proof: f.proofs[i].clone(),
            write_allowed: false,
        };
        let mut retry = f.commands[i].clone();
        retry.principal.can_submit = false;
        assert_eq!(
            run(&store, &retry, &auth).await.unwrap(),
            OutcomeResult::Duplicate(f.plans[i].delivery().clone())
        );
        assert_eq!(dump(&store).await, stable);
    }
    let auth = SyntheticAuthority {
        proof: f.proofs[1].clone(),
        write_allowed: true,
    };
    let mut conflict = f.commands[1].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut conflict.operation else {
        panic!("ordinary command")
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["code"] = serde_json::json!("different-code");
    *ingress = canonical(&event).unwrap();
    assert_eq!(
        run(&store, &conflict, &auth).await.unwrap(),
        OutcomeResult::IdentityConflict
    );
    assert_eq!(dump(&store).await, stable);
    let mut alias = f.commands[1].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut alias.operation else {
        panic!("ordinary command")
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["external_id"] = serde_json::json!("renamed-ordinary");
    *ingress = canonical(&event).unwrap();
    let OutcomeResult::Duplicate(duplicate) = run(&store, &alias, &auth).await.unwrap() else {
        panic!("semantic alias required")
    };
    assert_eq!(
        duplicate.economic_receipt,
        f.plans[1].delivery().economic_receipt
    );
    assert_eq!(
        duplicate.settlement_receipt,
        f.plans[1].delivery().settlement_receipt
    );
    assert_eq!(duplicate.canonical_key, f.plans[1].delivery().key);
    let after = dump(&store).await;
    for (table, rows) in &stable {
        if table != "outcome_deliveries" {
            assert_eq!(
                after.iter().find(|(t, _)| t == table).unwrap().1,
                *rows,
                "alias mutated {table}"
            );
        }
    }
    assert!(matches!(
        run(&store, &alias, &auth).await.unwrap(),
        OutcomeResult::Duplicate(_)
    ));
    assert_eq!(dump(&store).await, after);
    store.close().await;
}

#[tokio::test]
async fn accepted_zero_ordinary_consumes_slot_and_post_hoc_does_not_consume_capacity() {
    use crate::service::accept::outcome::{run, OutcomeOperation, OutcomeResult};
    let f = fixture::lifecycle();
    let (_dir, store) = prepared(&f).await;
    append_plan(&store, &f.plans[0]).await;
    let mut zero = f.commands[1].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut zero.operation else {
        panic!()
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["code"] = serde_json::json!("none");
    *ingress = canonical(&event).unwrap();
    let auth = SyntheticAuthority {
        proof: f.proofs[1].clone(),
        write_allowed: true,
    };
    let OutcomeResult::Accepted(accepted) = run(&store, &zero, &auth).await.unwrap() else {
        panic!("zero is accepted")
    };
    let economic = json(accepted.economic_receipt.as_ref().unwrap()).unwrap();
    assert_eq!(economic["body"]["action_ids"], serde_json::json!([]));
    assert_eq!(economic["body"]["intention_ids"], serde_json::json!([]));
    let state = json(&accepted.settlement_receipt).unwrap()["body"]["result"].clone();
    assert_eq!(state["revision"], "1");
    assert_eq!(state["held"], "12000");
    assert!(state["families"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["status"] == "claimed"));
    let before = dump(&store).await;
    assert!(matches!(
        run(&store, &zero, &auth).await.unwrap(),
        OutcomeResult::Duplicate(_)
    ));
    assert_eq!(dump(&store).await, before);
    assert_eq!(
        run(&store, &f.commands[1], &auth).await.unwrap(),
        OutcomeResult::IdentityConflict
    );
    assert_eq!(dump(&store).await, before);
    let mut correction = f.commands[2].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut correction.operation else {
        panic!()
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["replacement"]["code"] = serde_json::json!("fee");
    *ingress = canonical(&event).unwrap();
    let auth = SyntheticAuthority {
        proof: f.proofs[2].clone(),
        write_allowed: true,
    };
    let OutcomeResult::Accepted(changed) = run(&store, &correction, &auth).await.unwrap() else {
        panic!()
    };
    assert_eq!(
        json(&changed.settlement_receipt).unwrap()["body"]["result"],
        state
    );
    assert_eq!(
        json(changed.economic_receipt.as_ref().unwrap()).unwrap()["body"]["action_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    store.close().await;
}
#[tokio::test]
async fn zero_net_correction_retains_inverse_and_replacement_without_intention() {
    use crate::service::accept::outcome::{run, OutcomeOperation, OutcomeResult};
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f).await;
    append_plan(&store, &f.plans[0]).await;
    append_plan(&store, &f.plans[1]).await;
    let state = json(&f.plans[1].delivery().settlement_receipt).unwrap()["body"]["result"].clone();
    let mut correction = f.commands[2].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut correction.operation else {
        panic!()
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["replacement"]["code"] = serde_json::json!("fee");
    *ingress = canonical(&event).unwrap();
    let auth = SyntheticAuthority {
        proof: f.proofs[2].clone(),
        write_allowed: true,
    };
    let OutcomeResult::Accepted(accepted) = run(&store, &correction, &auth).await.unwrap() else {
        panic!()
    };
    let receipt = json(accepted.economic_receipt.as_ref().unwrap()).unwrap();
    assert_eq!(receipt["body"]["action_ids"].as_array().unwrap().len(), 2);
    assert_eq!(receipt["body"]["intention_ids"], serde_json::json!([]));
    assert_eq!(
        json(&accepted.settlement_receipt).unwrap()["body"]["result"],
        state
    );
    let before = dump(&store).await;
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    assert_eq!(
        run(&reopened, &correction, &auth).await.unwrap(),
        OutcomeResult::Duplicate(accepted)
    );
    assert_eq!(dump(&reopened).await, before);
    reopened.close().await;
}

#[tokio::test]
async fn explicit_release_and_later_authorized_reversal_never_replenish() {
    use crate::service::accept::outcome::{run, OutcomeOperation, OutcomeResult};
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f).await;
    append_plan(&store, &f.plans[0]).await;
    append_plan(&store, &f.plans[1]).await;
    let auth = SyntheticAuthority {
        proof: f.proofs[3].clone(),
        write_allowed: true,
    };
    let OutcomeResult::Accepted(closed) = run(&store, &f.commands[3], &auth).await.unwrap() else {
        panic!()
    };
    let state = json(&closed.settlement_receipt).unwrap()["body"]["result"].clone();
    assert_eq!(state["held"], "0");
    assert_eq!(state["released"], "9500");
    let mut reversal = f.commands[2].clone();
    reversal.received_at = f.commands[3].received_at.clone();
    reversal.accepted_at = f.commands[3].accepted_at.clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut reversal.operation else {
        panic!()
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["replacement"] = serde_json::json!({"kind":"reverse"});
    *ingress = canonical(&event).unwrap();
    let auth = SyntheticAuthority {
        proof: f.proofs[2].clone(),
        write_allowed: true,
    };
    let OutcomeResult::Accepted(reversed) = run(&store, &reversal, &auth).await.unwrap() else {
        panic!()
    };
    assert_eq!(
        json(&reversed.settlement_receipt).unwrap()["body"]["result"],
        state
    );
    let before = dump(&store).await;
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    assert_eq!(
        run(&reopened, &reversal, &auth).await.unwrap(),
        OutcomeResult::Duplicate(reversed)
    );
    assert_eq!(dump(&reopened).await, before);
    reopened.close().await;
}

struct FailAtActionBoundary<'a>(&'a SqliteStore);
impl OutcomeStore for FailAtActionBoundary<'_> {
    type Tx = SqliteTx;
    async fn begin_outcome(&self, deadline: Instant) -> Result<SqliteTx, StoreError> {
        let mut tx = self.0.begin_outcome(deadline).await?;
        // The projector orders action records first: cut after the first action
        // and its membership, before writing the second. Repeat on every retry.
        tx.outcome_fault = Some(Arc::new(Fault {
            cut: 4,
            ..Default::default()
        }));
        Ok(tx)
    }
}
#[tokio::test]
async fn inverse_and_replacement_failure_has_no_partial_economic_or_head_change() {
    use crate::service::accept::outcome::{run, OutcomeOperation};
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f).await;
    append_plan(&store, &f.plans[0]).await;
    append_plan(&store, &f.plans[1]).await;
    let before = dump(&store).await;
    let mut correction = f.commands[2].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut correction.operation else {
        panic!()
    };
    let mut event: serde_json::Value = serde_json::from_slice(ingress).unwrap();
    event["data"]["replacement"]["code"] = serde_json::json!("fee");
    *ingress = canonical(&event).unwrap();
    let auth = SyntheticAuthority {
        proof: f.proofs[2].clone(),
        write_allowed: true,
    };
    assert!(run(&FailAtActionBoundary(&store), &correction, &auth)
        .await
        .is_err());
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    assert_eq!(dump(&reopened).await, before);
    assert!(run(&reopened, &correction, &auth).await.is_ok());
    reopened.close().await;
}
