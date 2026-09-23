//! A genuinely suspended old process excludes takeover, then releases ownership
//! and resumes with cached old-epoch data after a replacement process commits.
use super::*;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

async fn barrier(path: &Path, name: &str) {
    let until = Instant::now() + Duration::from_secs(30);
    while !path.join(name).exists() {
        assert!(Instant::now() < until, "process barrier timed out: {name}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
fn signal(path: &Path, name: &str) {
    std::fs::write(path.join(name), b"ready").unwrap();
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn child_host(db: &Path, anchor: &Path) -> RemainingHost {
    let input = fixture();
    let sources: Vec<wire::AuthoritySource> =
        serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
    let store = SqliteStore::open_fenced(db, anchor).await.unwrap();
    let head = store
        .provision_adjudication_authority(&journal("g1"), &sources[0], Some(Count::new(1).unwrap()))
        .await
        .unwrap();
    RemainingHost(FlowHost {
        stores: BTreeMap::from([("g1".into(), store)]),
        heads: BTreeMap::from([("g1".into(), head)]),
        sources,
        base: BaseFixture::new(&input),
        now: serde_json::from_value(input["commands"][0]["authority"]["observed_at"].clone())
            .unwrap(),
    })
}
#[tokio::test]
async fn replacement_process_entry() {
    let Ok(path) = std::env::var("LEDGERLAB_REPLACEMENT_PROCESS") else {
        return;
    };
    let path = Path::new(&path);
    let input: Value =
        serde_json::from_slice(&std::fs::read(path.join("input.json")).unwrap()).unwrap();
    let db = Path::new(input["db"].as_str().unwrap());
    let anchor = Path::new(input["anchor"].as_str().unwrap());
    let old_command = input["command"].clone();
    let mut host = child_host(db, anchor).await;
    let initial = host.0.stores["g1"].test_full_inventory().await;
    signal(path, "owned");
    barrier(path, "release").await;
    assert_eq!(host.0.stores["g1"].test_full_inventory().await, initial);
    host.0.stores.remove("g1").unwrap().close().await;
    drop(host);
    signal(path, "released");
    barrier(path, "new-owner").await;
    assert!(matches!(
        SqliteStore::open_fenced(db, anchor).await,
        Err(StoreError::Owned)
    ));
    signal(path, "new-owner-excludes-old");
    barrier(path, "resume").await;
    let mut host = child_host(db, anchor).await;
    let configured = host.0.stores["g1"]
        .provision_adjudication(
            journal("g1"),
            flow_budget("g1"),
            65536,
            Count::new(1u128 << 40).unwrap(),
        )
        .await
        .unwrap();
    let before = host.0.stores["g1"].test_full_inventory().await;
    let result = run(
        &configured,
        &host,
        journal("g1"),
        parsed(&old_command),
        deadline(),
    )
    .await;
    assert!(matches!(result, Err(ServiceError::Rejection(code)) if code == "AUTH_HEAD"));
    assert_eq!(host.0.stores["g1"].test_full_inventory().await, before);
    let new_root: Digest =
        serde_json::from_slice(&std::fs::read(path.join("new-root.json")).unwrap()).unwrap();
    let mut fresh_head_old_epoch = old_command;
    hydrate(&mut fresh_head_old_epoch, &BTreeMap::new(), &new_root);
    let result = run(
        &configured,
        &host,
        journal("g1"),
        parsed(&fresh_head_old_epoch),
        deadline(),
    )
    .await;
    assert!(matches!(result, Err(ServiceError::Rejection(code)) if code == "WRITER_EPOCH"));
    assert_eq!(host.0.stores["g1"].test_full_inventory().await, before);
    drop(configured);
    host.0.stores.remove("g1").unwrap().close().await;
    signal(path, "refused-unchanged");
}
#[tokio::test]
async fn actual_suspended_old_process_excludes_takeover_and_resumes_after_replacement() {
    let mut h = Harness::new().await;
    h.grant_and_register(1).await;
    let c = h.issue_command(1);
    h.step(c).await;
    h.activate_token(1).await;
    let mut old_command = h.receive_command(1, 1);
    hydrate(&mut old_command, &h.proofs, &h.roots["g1"]);
    let before = h.host.0.stores["g1"].test_full_inventory().await;
    let db = h._dirs[2].0.path().to_owned();
    let anchor = h._dirs[2].1.path().to_owned();
    h.host.0.stores.remove("g1").unwrap().close().await;
    let ipc = tempfile::tempdir().unwrap();
    std::fs::write(
        ipc.path().join("input.json"),
        serde_json::to_vec(&json!({
            "db":db, "anchor":anchor, "command":old_command,
        }))
        .unwrap(),
    )
    .unwrap();
    let output = std::fs::File::create(ipc.path().join("child.log")).unwrap();
    let mut child = OwnedChild(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "service::accept::adjudication::sqlite_tests::remaining_tests::race_tests::replacement_process_tests::replacement_process_entry", "--nocapture"])
        .env("LEDGERLAB_REPLACEMENT_PROCESS", ipc.path())
        .stdout(Stdio::from(output.try_clone().unwrap())).stderr(Stdio::from(output))
        .spawn().unwrap());
    barrier(ipc.path(), "owned").await;
    assert!(Command::new("/bin/kill")
        .args(["-STOP", &child.0.id().to_string()])
        .status()
        .unwrap()
        .success());
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let state = Command::new("ps")
            .args(["-o", "stat=", "-p", &child.0.id().to_string()])
            .output()
            .unwrap();
        assert!(state.status.success());
        if String::from_utf8_lossy(&state.stdout).contains('T') {
            break;
        }
        assert!(Instant::now() < until, "owned child was not suspended");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(matches!(
        SqliteStore::open_fenced(&db, &anchor).await,
        Err(StoreError::Owned)
    ));
    assert!(Command::new("/bin/kill")
        .args(["-CONT", &child.0.id().to_string()])
        .status()
        .unwrap()
        .success());
    signal(ipc.path(), "release");
    barrier(ipc.path(), "released").await;
    h.host.0.stores.insert(
        "g1".into(),
        SqliteStore::open_fenced(&db, &anchor).await.unwrap(),
    );
    assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, before);
    let configured = h.host.0.stores["g1"]
        .provision_adjudication(
            journal("g1"),
            flow_budget("g1"),
            65536,
            Count::new(1u128 << 40).unwrap(),
        )
        .await
        .unwrap();
    let fence = configured.writer_fence(deadline()).await.unwrap();
    drop(configured);
    h.command_step("REPLACE_WRITER", json!({"gateway":"g1", "old_epoch":"1", "new_epoch":"2", "journal_head":h.roots["g1"], "fence":fence})).await;
    let after = h.host.0.stores["g1"].test_full_inventory().await;
    std::fs::write(
        ipc.path().join("new-root.json"),
        serde_json::to_vec(&h.roots["g1"]).unwrap(),
    )
    .unwrap();
    signal(ipc.path(), "new-owner");
    barrier(ipc.path(), "new-owner-excludes-old").await;
    assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, after);
    h.host.0.stores.remove("g1").unwrap().close().await;
    signal(ipc.path(), "resume");
    barrier(ipc.path(), "refused-unchanged").await;
    let status = child.0.wait().unwrap();
    let log = std::fs::read_to_string(ipc.path().join("child.log")).unwrap();
    assert!(status.success(), "{status}: {log}");
    eprintln!("old process resumed: {log}");
    h.host.0.stores.insert(
        "g1".into(),
        SqliteStore::open_fenced(&db, &anchor).await.unwrap(),
    );
    assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, after);
    let c = h.receive_command(1, 2);
    h.step(c).await;
    h.settle_token(1, true, None).await;
    h.command_step("ADVANCE_RECEIPT", json!({"gateway":"g1", "through":"1"}))
        .await;
    h.finish(1, 0).await;
    h.reopen().await;
    h.close().await;
    eprintln!("actual suspended old process: ownership excluded takeover; released process resumed after epoch2; stale head and fresh-head old epoch refused unchanged; epoch2 receipt and prepaid finish reopened");
}
