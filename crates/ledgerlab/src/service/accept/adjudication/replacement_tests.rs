//! Same-authoritative-store writer replacement with every retained obligation
//! class present. This is real restart/copy exclusion, not a process-kill test.
use super::*;
impl Harness {
    async fn stale_call_refused(&self, c: &Value) {
        let owner = "g1";
        let configured = self.host.0.stores[owner]
            .provision_adjudication_with_ceiling(
                journal(owner),
                self.budgets[owner].clone(),
                self.ceilings[owner].clone(),
                65536,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        let before = self.host.0.stores[owner].test_full_inventory().await;
        let result = run(
            &configured,
            &self.host,
            journal(owner),
            parsed(c),
            deadline(),
        )
        .await;
        assert!(matches!(result,Err(ServiceError::Rejection(code)) if code=="AUTH_HEAD"));
        assert_eq!(
            self.host.0.stores[owner].test_full_inventory().await,
            before
        );
    }
    async fn retained_obligations(&self) -> Vec<ObservedHead> {
        let mut rows = Vec::new();
        for n in 1..=5 {
            let (head, _) = self
                .state(
                    "g1",
                    HeadKind::Grant,
                    rt::points::Point::id(
                        rt::points::PointKind::Grant,
                        *b"GRANT___",
                        &self.grant_name(n),
                    )
                    .unwrap(),
                    GuardClass::CapacityAllocation,
                )
                .await;
            rows.push(head);
        }
        for n in 2..=4 {
            let (head, _) = self
                .state(
                    "g1",
                    HeadKind::Token,
                    rt::points::Point::id(
                        rt::points::PointKind::Token,
                        *b"TOKEN___",
                        &format!("race{n}"),
                    )
                    .unwrap(),
                    GuardClass::CapacityAllocation,
                )
                .await;
            rows.push(head);
        }
        rows
    }
}
#[tokio::test]
async fn actual_replacement_recovers_unactivated_active_receipt_and_tombstone_obligations() {
    let mut h = Harness::new().await;
    let mut saved_receipt = None;
    let mut saved_tombstone = None;
    for n in 1..=5 {
        h.grant_and_register(n).await;
        if n == 5 {
            continue;
        } // Registered but not claimed; no allocation exists.
        let c = h.issue_command(n);
        h.step(c).await;
        if n == 1 {
            continue;
        } // Claimed backing, not yet delivered/activated locally.
        if n == 2 || n == 3 {
            h.activate_token(n).await;
        }
        if n == 3 {
            let c = h.receive_command(n, 1);
            h.step(c).await;
            saved_receipt = h.last.clone();
        }
        if n == 4 {
            let c = h.return_command(n);
            h.step(c).await;
            saved_tombstone = h.last.clone();
        }
    }
    let proof = h.proof("center", "CLAIM", json!("race1"));
    let mut stale_activate = h.command(
        "ACTIVATE",
        json!({"gateway":"g1","token":"race1","proof":proof}),
    );
    hydrate(&mut stale_activate, &h.proofs, &h.roots["g1"]);
    let mut stale_return = h.return_command(2);
    hydrate(&mut stale_return, &h.proofs, &h.roots["g1"]);
    let obligations = h.retained_obligations().await;
    let configured = h.host.0.stores["g1"]
        .provision_adjudication_with_ceiling(
            journal("g1"),
            h.budgets["g1"].clone(),
            h.ceilings["g1"].clone(),
            65536,
            Count::new(1u128 << 40).unwrap(),
        )
        .await
        .unwrap();
    let fence = configured.writer_fence(deadline()).await.unwrap();
    drop(configured);
    let replacement = json!({"gateway":"g1","old_epoch":"1","new_epoch":"2","journal_head":h.roots["g1"],"fence":fence});
    h.command_step("REPLACE_WRITER", replacement.clone()).await;
    let replacement_saved = h.last.clone().unwrap();
    h.reopen().await;
    let recovered = h.retained_obligations().await;
    assert_eq!(obligations.len(), recovered.len());
    for (old, new) in obligations.iter().zip(&recovered) {
        assert_eq!(old.key, new.key);
        assert_eq!(old.revision, new.revision);
        assert_eq!(old.value, new.value);
    }
    h.stale_call_refused(&stale_activate).await;
    h.stale_call_refused(&stale_return).await;
    let c = h.command("REPLACE_WRITER", replacement);
    h.refuses_unchanged("g1", c, "WRITER_EPOCH").await;
    let (_, saved_receipt, receipt_result) = saved_receipt.unwrap();
    let (_, saved_tombstone, tombstone_result) = saved_tombstone.unwrap();
    h.retry_exact("g1", &saved_receipt, &receipt_result).await;
    h.retry_exact("g1", &saved_tombstone, &tombstone_result)
        .await;
    assert_eq!(
        serde_json::to_value(&receipt_result.effects[0]).unwrap()["body"]["epoch"],
        "1"
    );
    let mut changed = saved_receipt.clone();
    changed["payload"]["epoch"] = json!("2");
    // Retry identity is the exact submission, not the newly supplied epoch.
    h.retry_exact("g1", &changed, &receipt_result).await;
    changed["payload"]["submission"]["sender_backfill"] = json!(true);
    h.refuses_unchanged("g1", changed, "IDENTITY_CONFLICT")
        .await;
    let proof = h.proof("center", "CLAIM", json!("race4"));
    let c = h.command(
        "ACTIVATE",
        json!({"gateway":"g1","token":"race4","proof":proof}),
    );
    h.refuses_unchanged("g1", c, "TOKEN_STATE").await;
    let c = h.receive_command(4, 2);
    h.refuses_unchanged("g1", c, "TOKEN_STATE").await;
    h.activate_token(1).await;
    for n in 1..=2 {
        let old = h.receive_command(n, 1);
        h.refuses_unchanged("g1", old, "WRITER_EPOCH").await;
        let fresh = h.receive_command(n, 2);
        let result = h.step(fresh).await;
        assert_eq!(
            serde_json::to_value(&result.effects[0]).unwrap()["body"]["epoch"],
            "2"
        );
    }
    // Original receipt position1 survives; new writer adds positions2 and3.
    for n in 1..=4 {
        h.settle_token(n, n != 4, None).await;
    }
    for through in 1..=3 {
        h.command_step(
            "ADVANCE_RECEIPT",
            json!({"gateway":"g1","through":through.to_string()}),
        )
        .await;
    }
    h.command_step("RETIRE_GRANT", json!({"grant":h.grant_name(5)}))
        .await;
    let proof = h.proof("center", "RETIREMENT", json!(h.grant_name(5)));
    h.command_step(
        "LOCAL_TERMINAL",
        json!({"gateway":"g1","grant":h.grant_name(5),"proof":proof}),
    )
    .await;
    h.finish(1, 0).await;
    h.reopen().await;
    h.retry_exact("g1", &saved_receipt, &receipt_result).await;
    h.retry_exact("g1", &saved_tombstone, &tombstone_result)
        .await;
    h.retry_exact("g1", &replacement_saved.1, &replacement_saved.2)
        .await;
    let (_, State::Resource(resource)) = h
        .state(
            "g1",
            HeadKind::Resource,
            rt::points::Point::id(rt::points::PointKind::Resource, *b"RESOURCE", "g1").unwrap(),
            GuardClass::CapacityAllocation,
        )
        .await
    else {
        panic!("resource")
    };
    assert_eq!(resource.q.writer_epoch.value(), 2);
    // Close all actual handles before copying the exact checkpointed database.
    // A fresh inode must not become a second writer under the trusted old anchor.
    for store in std::mem::take(&mut h.host.0.stores).into_values() {
        store.close().await;
    }
    let copy = tempfile::tempdir().unwrap();
    std::fs::copy(
        h._dirs[2].0.path().join("local.db"),
        copy.path().join("local.db"),
    )
    .unwrap();
    assert!(SqliteStore::open_fenced(copy.path(), h._dirs[2].1.path())
        .await
        .is_err());
    h.reopen().await;
    h.retry_exact("g1", &saved_receipt, &receipt_result).await;
    h.retry_exact("g1", &saved_tombstone, &tombstone_result)
        .await;
    eprintln!("actual complete replacement: epoch2; five retained grant states, three original/new receipts, permanent unused tombstone; copied inode refused; original fenced recovery exact");
    h.close().await;
}

fn overwrite_same_inode(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    // Only this test's closed, disposable store is changed. A stale backup
    // must not recover newer acknowledged rows from a leftover WAL.
    for name in ["local.db-wal", "local.db-shm"] {
        match std::fs::remove_file(path.parent().unwrap().join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{e}"),
        }
    }
}
impl Harness {
    async fn gateway_backup(&mut self) -> Vec<u8> {
        self.host.0.stores.remove("g1").unwrap().close().await;
        let bytes = std::fs::read(self._dirs[2].0.path().join("local.db")).unwrap();
        self.host.0.stores.insert(
            "g1".into(),
            SqliteStore::open_fenced(self._dirs[2].0.path(), self._dirs[2].1.path())
                .await
                .unwrap(),
        );
        bytes
    }
}
#[tokio::test]
async fn actual_same_inode_backup_missing_grant_or_receipt_refuses_without_central() {
    use std::os::unix::fs::MetadataExt;
    for missing in ["grant", "receipt"] {
        let mut h = Harness::new().await;
        h.quiet = true;
        let mut backup = h.gateway_backup().await;
        h.grant_and_register(1).await;
        let c = h.issue_command(1);
        h.step(c).await;
        h.activate_token(1).await;
        if missing == "receipt" {
            backup = h.gateway_backup().await;
        }
        let c = h.receive_command(1, 1);
        let saved = h.step(c).await;
        assert!(saved
            .effects
            .iter()
            .any(|e| matches!(e, wire::Effect::Receipt { .. })));
        let acknowledged = h.host.0.stores["g1"].test_full_inventory().await;
        for store in std::mem::take(&mut h.host.0.stores).into_values() {
            store.close().await;
        }
        // All five owners, including central, are now offline. The local
        // rollback-independent anchor is the only admissible recovery witness.
        let db = h._dirs[2].0.path();
        let anchor = h._dirs[2].1.path();
        let path = db.join("local.db");
        let inode = std::fs::metadata(&path).unwrap().ino();
        let current = std::fs::read(&path).unwrap();
        let witness = std::fs::read(anchor.join("state.json")).unwrap();
        assert_ne!(current, backup);
        overwrite_same_inode(&path, &backup);
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert!(matches!(
            SqliteStore::open_fenced(db, anchor).await,
            Err(StoreError::InvalidStore(_))
        ));
        assert!(SqliteStore::open(db).await.is_err());
        assert_eq!(std::fs::read(anchor.join("state.json")).unwrap(), witness);
        // Positive control restores the exact latest closed test database, not
        // the anchor. It must recover the acknowledged receipt and saved retry.
        overwrite_same_inode(&path, &current);
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        h.reopen().await;
        assert_eq!(
            h.host.0.stores["g1"].test_full_inventory().await,
            acknowledged
        );
        h.settle_token(1, true, None).await;
        h.command_step("ADVANCE_RECEIPT", json!({"gateway":"g1","through":"1"}))
            .await;
        h.finish(1, 0).await;
        h.reopen().await;
        h.assert_all_owner_accounts().await;
        h.close().await;
        eprintln!("actual same-inode stale backup missing {missing}: centraloffline; originalanchor unchanged; staleopen and anchoromission refused; exactlatest restore preserved receipt/retry and prepaid finish");
    }
}
