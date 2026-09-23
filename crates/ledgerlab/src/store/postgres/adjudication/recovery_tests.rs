//! Real database/process recovery controls. Primitive writes remain explicitly
//! storage tests, not economic acceptance or fabricated validated plans.
use super::super::recovery::{Disposition, Gate, Work};
use super::*;
use crate::store::outcomes::OutcomeTx;
use std::{
    process::{Command, Stdio},
    sync::{atomic::AtomicI32, Arc},
};
fn parsed(s: &wire::Segment) -> r3::ParsedCommand {
    r3::ParsedCommand::parse(&r3::canonical_bytes(&s.command, r3::COMMAND_BYTES).unwrap()).unwrap()
}
async fn gate(f: &Fixture) -> Gate {
    Gate::acquire(
        &config(&f.name, false),
        Instant::now() + Duration::from_secs(3),
        &Arc::new(AtomicI32::new(0)),
    )
    .await
    .unwrap()
}
async fn slot(f: &Fixture) -> (Count, String) {
    let r = f
        .owner
        .client
        .query_one(
            "SELECT generation,state FROM ledgerlab.r3_unresolved_work WHERE singleton=1",
            &[],
        )
        .await
        .unwrap();
    (ordinal(r.get(0)).unwrap(), r.get(1))
}
async fn recovery_row(f: &Fixture) -> Vec<u8> {
    f.owner.client.query_one("SELECT sha256(convert_to(row_to_json(t)::text,'UTF8')) FROM ledgerlab.r3_unresolved_work t WHERE singleton=1",&[]).await.unwrap().get(0)
}
async fn recovered(f: &Fixture) -> Gate {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match Gate::acquire(
                &config(&f.name, false),
                Instant::now() + Duration::from_secs(2),
                &Arc::new(AtomicI32::new(0)),
            )
            .await
            {
                Ok(g) => return g,
                Err(StoreError::WritesDisabled) => {
                    tokio::time::sleep(Duration::from_millis(5)).await
                }
                Err(e) => panic!("recovery boundary: {e}"),
            }
        }
    })
    .await
    .expect("server must observe terminated synthetic backend")
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_recovery_supervised_commit_retry_cancel_and_reopen() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let command = parsed(&s);
    let mut tx = f
        .store
        .begin_native_storage(j.clone(), &command, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(slot(&f).await, (Count::ZERO, "RESOLVING".into()));
    tx.native_adjudication(Operation::Locks(j.clone(), guards(&j)))
        .await
        .unwrap();
    tx.native_adjudication(Operation::Primitive(Box::new(Primitive {
        journal: j.clone(),
        segment: s.clone(),
        writes: vec![],
        fail_after_segment: false,
    })))
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(slot(&f).await, (Count::ZERO, "IDLE".into()));
    let before = inventory(&f).await;
    let exact_slot = recovery_row(&f).await;
    let mut retry = f
        .store
        .begin_native_storage(j.clone(), &command, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    retry
        .native_adjudication(Operation::Locks(j.clone(), guards(&j)))
        .await
        .unwrap();
    let v = runtime::command_value(&s.command).unwrap();
    let key = serde_json::from_value(v["key"].clone()).unwrap();
    let Value::Saved(saved) = retry
        .native_adjudication(Operation::Lookup(j.clone(), key))
        .await
        .unwrap()
    else {
        panic!("saved")
    };
    assert_eq!(saved.unwrap().command, command.bytes());
    drop(retry); // Cancellation of the handle cannot erase unresolved state early.
    f.store.clone().close().await;
    assert_eq!(slot(&f).await, (Count::ZERO, "IDLE".into()));
    assert_eq!(inventory(&f).await, before);
    let store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    let tx = store
        .begin_native_storage(j, &command, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    store.close().await;
    assert_eq!(slot(&f).await, (Count::ZERO, "IDLE".into()));
    assert_eq!(inventory(&f).await, before);
    assert_eq!(
        recovery_row(&f).await,
        exact_slot,
        "saved retries retain the entire recovery singleton unchanged"
    );
    f.finish().await;
    let fresh = Fixture::new().await;
    let tx = fresh
        .store
        .begin_native_storage(
            sample().0,
            &command,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(slot(&fresh).await.1, "RESOLVING");
    drop(tx);
    fresh.store.clone().close().await;
    assert_eq!(slot(&fresh).await, (Count::ZERO, "IDLE".into()));
    assert!(inventory(&fresh).await.iter().all(Vec::is_empty));
    fresh.finish().await;
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_recovery_live_orphan_blocks_until_same_role_backend_termination() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let command = parsed(&s);
    let work = config(&f.name, false).connect().await.unwrap();
    let pid: i32 = work
        .client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    let mut controller = gate(&f).await;
    controller
        .arm(&work.client, Work::new(j.clone(), &command).unwrap())
        .await
        .unwrap();
    work.client
        .batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE")
        .await
        .unwrap();
    primitive(
        &work.client,
        &Primitive {
            journal: j.clone(),
            segment: s,
            writes: vec![],
            fail_after_segment: false,
        },
    )
    .await
    .unwrap();
    controller.abandon().await; // Control connection dies while work can still commit.
    assert!(matches!(
        Gate::acquire(
            &config(&f.name, false),
            Instant::now() + Duration::from_secs(2),
            &Arc::new(AtomicI32::new(0))
        )
        .await,
        Err(StoreError::WritesDisabled)
    ));
    assert_eq!(slot(&f).await.1, "RESOLVING");
    assert!(f
        .store
        .begin_native_storage(j, &command, Instant::now() + Duration::from_secs(2))
        .await
        .is_err());
    assert_eq!(slot(&f).await.0, Count::ZERO);
    let marker = recovery_row(&f).await;
    let mut legacy = f
        .store
        .begin(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    let Guard::Legacy(admission) = guards(&sample().0).remove(0) else {
        panic!("legacy admission")
    };
    assert!(matches!(
        legacy.lock_scopes(&[admission]).await,
        Err(StoreError::WritesDisabled)
    ));
    assert!(legacy.commit().await.is_err());
    assert_eq!(recovery_row(&f).await, marker);
    // Explicit test intervention on this exact owned connection; production
    // recovery never issues a PID-only termination command.
    let control = config(&f.name, false).connect().await.unwrap();
    let superuser: bool = control
        .client
        .query_one(
            "SELECT rolsuper FROM pg_roles WHERE rolname=current_user",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(!superuser);
    let terminated: bool = control
        .client
        .query_one("SELECT pg_terminate_backend($1)", &[&pid])
        .await
        .unwrap()
        .get(0);
    assert!(terminated);
    work.discard().await;
    control.discard().await;
    let g = recovered(&f).await;
    let r = g.prior_resolution.as_ref().unwrap();
    assert_eq!(r.disposition, Disposition::Absent);
    assert_eq!(r.prefix.ordinal(), Count::ZERO);
    assert_eq!(slot(&f).await.1, "IDLE");
    assert!(inventory(&f).await.iter().all(Vec::is_empty));
    g.abandon().await;
    let (j, s) = sample();
    let mut next = f
        .store
        .begin_native_storage(j.clone(), &command, Instant::now() + Duration::from_secs(3))
        .await
        .unwrap();
    next.native_adjudication(Operation::Locks(j.clone(), guards(&j)))
        .await
        .unwrap();
    next.native_adjudication(Operation::Primitive(Box::new(Primitive {
        journal: j,
        segment: s,
        writes: vec![],
        fail_after_segment: false,
    })))
    .await
    .unwrap();
    next.commit().await.unwrap();
    assert_eq!(slot(&f).await, (Count::ZERO, "IDLE".into()));
    f.finish().await;
}
async fn native_recovery_process_child() {
    let Ok(db) = std::env::var("LEDGERLAB_R3_CHILD_DATABASE") else {
        panic!("explicit child invocation required")
    };
    let mode = std::env::var("LEDGERLAB_R3_CHILD_MODE").unwrap();
    let ready = std::env::var("LEDGERLAB_R3_CHILD_READY").unwrap();
    let (j, s) = sample();
    let command = parsed(&s);
    let work = config(&db, false).connect().await.unwrap();
    let mut gate = Gate::acquire(
        &config(&db, false),
        Instant::now() + Duration::from_secs(5),
        &Arc::new(AtomicI32::new(0)),
    )
    .await
    .unwrap();
    gate.arm(&work.client, Work::new(j, &command).unwrap())
        .await
        .unwrap();
    work.client
        .batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE")
        .await
        .unwrap();
    primitive(
        &work.client,
        &Primitive {
            journal: sample().0,
            segment: s,
            writes: vec![],
            fail_after_segment: false,
        },
    )
    .await
    .unwrap();
    if mode == "after_commit" {
        work.client.batch_execute("COMMIT").await.unwrap();
    } else {
        assert_eq!(mode, "before_commit");
    }
    std::fs::write(ready, b"ready").unwrap();
    std::future::pending::<()>().await;
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_recovery_process_kill_before_and_after_commit_resolves_exact_primary() {
    if std::env::var_os("LEDGERLAB_R3_CHILD_DATABASE").is_some() {
        native_recovery_process_child().await;
        return;
    }
    for (mode, expected) in [
        ("before_commit", Disposition::Absent),
        ("after_commit", Disposition::Saved),
    ] {
        let f = Fixture::new().await;
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("ready");
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","store::postgres::adjudication::tests::recovery_tests::native_recovery_process_kill_before_and_after_commit_resolves_exact_primary","--ignored","--nocapture"])
            .env("LEDGERLAB_R3_CHILD_DATABASE",&f.name).env("LEDGERLAB_R3_CHILD_MODE",mode).env("LEDGERLAB_R3_CHILD_READY",&ready).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !ready.exists() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "child exited before durable cut"
            );
            assert!(Instant::now() < deadline, "child ready deadline");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(slot(&f).await.1, "RESOLVING");
        child.kill().unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        println!("actual child process killed at {mode}");
        assert_eq!(slot(&f).await.1, "RESOLVING"); // Durable marker survived process death.
        let g = recovered(&f).await;
        let r = g.prior_resolution.as_ref().unwrap();
        assert_eq!(r.disposition, expected);
        assert_eq!(r.generation, Count::ZERO);
        assert_eq!(
            r.prefix.ordinal().value(),
            if expected == Disposition::Saved { 1 } else { 0 }
        );
        assert_eq!(slot(&f).await.1, "IDLE");
        let after = inventory(&f).await;
        g.abandon().await;
        let g = gate(&f).await;
        assert!(g.prior_resolution.is_none());
        assert_eq!(inventory(&f).await, after);
        g.abandon().await;
        f.finish().await;
    }
}

#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_recovery_reuses_fixed_slot_for_repeated_absent_and_refused_attempts() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let before = inventory(&f).await;
    for attempt in 0..8 {
        let mut value = runtime::command_value(&s.command).unwrap();
        // Repeated original identity and distinct new identities share the same
        // one-row staging lane; neither path burns ordinal/epoch headroom.
        if attempt % 2 == 1 {
            value["key"][2] = json!(format!("refused-attempt-{attempt}"));
        }
        let command =
            r3::ParsedCommand::parse(&r3::canonical_bytes(&value, r3::COMMAND_BYTES).unwrap())
                .unwrap();
        let current = PostgresStore::open(config(&f.name, false)).await.unwrap();
        let mut tx = current
            .begin_native_storage(j.clone(), &command, Instant::now() + Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(slot(&f).await, (Count::ZERO, "RESOLVING".into()));
        if attempt % 3 == 0 {
            drop(tx); // Abandoned before any command can write.
            current.clone().close().await;
            // close is terminal for this handle; subsequent attempts use reopen.
            let reopen = PostgresStore::open(config(&f.name, false)).await.unwrap();
            let tx = reopen
                .begin_native_storage(j.clone(), &command, Instant::now() + Duration::from_secs(3))
                .await
                .unwrap();
            tx.rollback().await.unwrap();
            reopen.close().await;
        } else {
            assert!(tx
                .native_adjudication(Operation::Locks(j.clone(), vec![]))
                .await
                .is_err());
            assert!(tx.commit().await.is_err());
        }
        current.close().await;
        assert_eq!(slot(&f).await, (Count::ZERO, "IDLE".into()));
        assert_eq!(inventory(&f).await, before);
        let rows: i64 = f
            .owner
            .client
            .query_one("SELECT count(*) FROM ledgerlab.r3_unresolved_work", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(rows, 1);
    }
    f.finish().await;
}
