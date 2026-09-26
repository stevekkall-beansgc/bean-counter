use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
fn temp() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/cli-tests");
    fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap()
}
fn invoke(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ledger"));
    cmd.current_dir(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cmd.spawn().unwrap();
    if let Some(bytes) = input {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}
fn run(dir: &Path, args: &[&str], input: Option<&[u8]>, exit: i32) -> Value {
    let mut args = args.to_vec();
    args.push("--format");
    args.push("json");
    let o = invoke(dir, &args, input);
    assert_eq!(
        o.status.code(),
        Some(exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.stderr.is_empty(), "unexpected stderr: {:?}", o.stderr);
    let s = String::from_utf8(o.stdout).unwrap();
    assert_eq!(s.lines().count(), 1);
    let v: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["schema"], "ledger-cli/1");
    v
}
fn init() -> tempfile::TempDir {
    let d = temp();
    let v = run(d.path(), &["init", "--demo"], None, 0);
    assert_eq!(v["status"], "initialized");
    d
}
// Compare every logical cell (including immutable bytes and operational heads).
// WAL checkpointing may change the main database file without a journal write.
fn db(dir: &Path) -> Vec<(String, Vec<String>)> {
    use sqlx::Connection;
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let options = sqlx::sqlite::SqliteConnectOptions::new().filename(dir.join(".ledger/local.db")).read_only(true).create_if_missing(false);
        let mut conn = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
        let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&mut conn).await.unwrap();
        let mut snapshot=Vec::new();
        for table in tables {
            let columns:Vec<String>=sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid").bind(&table).fetch_all(&mut conn).await.unwrap();
            let quote=|s:&str| format!("\"{}\"",s.replace('"',"\"\""));
            let projection=columns.iter().map(|c|format!("quote({})",quote(c))).collect::<Vec<_>>().join("||'|'||");
            let sql=format!("SELECT {projection} FROM {} ORDER BY 1",quote(&table));
            let rows:Vec<String>=sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str())).fetch_all(&mut conn).await.unwrap();
            snapshot.push((table,rows));
        }
        conn.close().await.unwrap(); snapshot
    })
}
fn event(dir: &Path) -> Value {
    serde_json::from_slice(&fs::read(dir.join("examples/generated.json")).unwrap()).unwrap()
}
fn send(dir: &Path, command: &str, value: &Value, exit: i32) -> Value {
    run(
        dir,
        &[command, "-"],
        Some(&serde_json::to_vec(value).unwrap()),
        exit,
    )
}

#[test]
fn guided_billing_setup_refuses_noninteractive_use_without_creating_a_path() {
    let d = temp();
    let destination = d.path().join("billing");
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/integration/setup-synthetic.json")
        .canonicalize()
        .unwrap();
    let args = [
        "billing",
        "setup",
        destination.to_str().unwrap(),
        "--setup",
        setup.to_str().unwrap(),
        "--json",
    ];
    let output = invoke(d.path(), &args, None);
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(matches!(
        value["code"].as_str(),
        Some("SETUP_REQUIRES_TERMINAL" | "INCOMPATIBLE_PLATFORM")
    ));
    assert!(!destination.exists());
}

#[test]
fn full_local_workflow_matches_frozen_receipt_and_snapshots() {
    let d = init();
    let dir = d.path();
    let before = db(dir);
    run(dir, &["explain", "--chain", "demo-slice"], None, 3);
    let p = run(dir, &["preview", "examples/generated.json"], None, 0);
    assert_eq!(p["outcome"], "would_accept");
    assert_eq!(p["committed"], false);
    assert!(p["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["kind"] != "receipt"));
    assert!(p.get("receipt").is_none());
    assert!(db(dir) == before, "preview changed persisted database");
    let human = invoke(dir, &["preview", "examples/generated.json"], None);
    assert!(human.status.success());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        include_str!("snapshots/preview.txt")
    );
    let accepted = run(dir, &["accept", "examples/generated.json"], None, 0);
    let expected: Value = serde_json::from_str(include_str!(
        "../../../fixtures/journals/first-slice/receipt.json"
    ))
    .unwrap();
    assert_eq!(accepted["receipt"], expected);
    let after = db(dir);
    let repeated = send(dir, "accept", &event(dir), 0);
    assert_eq!(repeated["status"], "duplicate");
    assert_eq!(repeated["kind"], "identity");
    assert_eq!(repeated["receipt"], expected);
    assert!(db(dir) == after, "logical database changed");
    let explained = run(dir, &["explain", "generation-1"], None, 0);
    let e = &explained["events"][0];
    assert_eq!(e["receipt"], expected);
    assert_eq!(e["links"], json!([]));
    assert_eq!(e["postings"].as_array().unwrap().len(), 2);
    assert_eq!(e["postings"][0]["amount"]["atoms"], "100");
    assert_eq!(e["postings"][1]["amount"]["atoms"], "-20");
    assert_eq!(e["intentions"][0]["amount"]["atoms"], "80");
    assert_eq!(e["decision"]["members"].as_array().unwrap().len(), 29);
    assert_eq!(e["documents"].as_array().unwrap().len(), 7);
    assert_eq!(
        run(dir, &["explain", "--chain", "demo-slice"], None, 0),
        explained
    );
    assert_eq!(
        run(
            dir,
            &["explain", expected["event_id"].as_str().unwrap()],
            None,
            0
        ),
        explained
    );
    let human = invoke(dir, &["explain", "generation-1"], None);
    assert!(human.status.success());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        include_str!("snapshots/explain.txt")
    );
    assert!(db(dir) == after, "explain changed persisted database");
    run(dir, &["init", "--demo"], None, 2);
    assert!(db(dir) == after, "init overwrote existing state");
}
#[test]
fn preview_duplicate_and_alias_never_reserve_identity() {
    let d = init();
    let dir = d.path();
    let e = event(dir);
    let a = send(dir, "accept", &e, 0);
    let original = db(dir);
    assert_eq!(send(dir, "preview", &e, 0)["outcome"], "duplicate");
    assert!(db(dir) == original, "logical database changed");
    let mut alias = e.clone();
    alias["id"] = json!("renamed-delivery");
    assert_eq!(send(dir, "preview", &alias, 0)["kind"], "semantic");
    assert!(db(dir) == original, "preview wrote an alias");
    run(dir, &["explain", "renamed-delivery"], None, 3);
    let recorded = send(dir, "accept", &alias, 0);
    assert_eq!(recorded["kind"], "semantic");
    assert_eq!(recorded["receipt"], a["receipt"]);
    assert_eq!(
        run(dir, &["explain", "renamed-delivery"], None, 0)["events"][0]["receipt"],
        a["receipt"]
    );
    let mut conflict = e;
    conflict["quantity"] = json!("2");
    let before = db(dir);
    assert_eq!(send(dir, "accept", &conflict, 4)["kind"], "identity");
    assert_eq!(send(dir, "preview", &conflict, 4)["outcome"], "conflict");
    assert!(db(dir) == before, "logical database changed");
}
#[test]
fn waiting_invalid_unauthorized_and_zero_action() {
    let d = init();
    let dir = d.path();
    let mut e = event(dir);
    let before = db(dir);
    e["chain"] = json!("not-provisioned");
    assert_eq!(
        send(dir, "accept", &e, 5)["missing"],
        json!(["chain:not-provisioned"])
    );
    assert_eq!(send(dir, "preview", &e, 5)["can_accept"], false);
    assert!(db(dir) == before, "logical database changed");
    run(
        dir,
        &["accept", "-"],
        Some(b"{\"id\":\"a\",\"id\":\"b\"}"),
        3,
    );
    run(dir, &["preview", "-"], Some(b"{broken"), 3);
    e = event(dir);
    e["source"] = json!("urn:other");
    assert_eq!(send(dir, "accept", &e, 6)["code"], "SOURCE_UNAUTHORIZED");
    assert!(db(dir) == before, "logical database changed");
    e = event(dir);
    e["type"] = json!("outcome.acquired");
    send(dir, "accept", &e, 3);
    assert!(db(dir) == before, "logical database changed");
    e = event(dir);
    e["status"] = json!("failed");
    e["quantity"] = json!("0");
    let a = send(dir, "accept", &e, 0);
    assert_eq!(a["receipt"]["action_ids"], json!([]));
    assert_eq!(a["receipt"]["intention_ids"], json!([]));
    let x = run(dir, &["explain", "generation-1"], None, 0);
    assert_eq!(x["events"][0]["explanations"][0]["code"], "FAILED_WORK");
}
#[test]
fn config_is_relative_strict_and_identity_checked() {
    let d = temp();
    let dir = d.path();
    run(dir, &["init", "demo", "--demo"], None, 0);
    run(
        dir,
        &[
            "--config",
            "demo/ledger.json",
            "accept",
            "demo/examples/generated.json",
        ],
        None,
        0,
    );
    let path = dir.join("demo/ledger.json");
    let bytes = fs::read(&path).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, value) in [
        ("schema", json!("ledger/v2")),
        ("mode", json!("real")),
        ("unknown", json!(true)),
    ] {
        let mut changed = v.clone();
        changed[field] = value;
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        run(
            dir,
            &["--config", "demo/ledger.json", "explain", "generation-1"],
            None,
            2,
        );
    }
    let mut changed = v.clone();
    changed["identity"]["store_id"] = json!("other-store");
    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    run(
        dir,
        &["--config", "demo/ledger.json", "explain", "generation-1"],
        None,
        2,
    );
    changed = v;
    changed["storage"]["data_dir"] = json!("../escape");
    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    run(
        dir,
        &["--config", "demo/ledger.json", "explain", "generation-1"],
        None,
        2,
    );
    fs::write(
        &path,
        b"{\"schema\":\"ledger/v1\",\"schema\":\"ledger/v1\"}",
    )
    .unwrap();
    run(
        dir,
        &["--config", "demo/ledger.json", "explain", "generation-1"],
        None,
        2,
    );
    fs::write(path, bytes).unwrap();
    run(
        dir,
        &["--config", "demo/ledger.json", "explain", "generation-1"],
        None,
        0,
    );
}
#[test]
fn useful_usage_errors_and_bounded_input() {
    let d = init();
    let dir = d.path();
    for args in [
        &["serve"][..],
        &["accept"],
        &["accept", "a", "b"],
        &["init"],
        &["explain", "--chain"],
        &["preview", "--policy", "x"],
        &["--config"],
        &["accept", "absent.json"],
    ] {
        run(dir, args, None, 2);
    }
    fs::write(dir.join("large.json"), vec![b' '; 262145]).unwrap();
    run(dir, &["accept", "large.json"], None, 2);
    fs::create_dir(dir.join("input-dir")).unwrap();
    run(dir, &["accept", "input-dir"], None, 2);
    for command in ["init", "accept", "preview", "explain"] {
        assert!(invoke(dir, &[command, "--help"], None).status.success());
    }
    let output = invoke(dir, &["--help"], None);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        include_str!("snapshots/help.txt")
    );
}
#[cfg(unix)]
#[test]
fn private_permissions_symlinks_and_owner_contention() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let d = init();
    let dir = d.path();
    for name in ["ledger.json", "examples/generated.json", ".ledger/local.db"] {
        assert_eq!(
            fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::metadata(dir.join(".ledger"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    symlink("examples/generated.json", dir.join("linked.json")).unwrap();
    run(dir, &["accept", "linked.json"], None, 2);
    symlink(".ledger", dir.join("linked-state")).unwrap();
    let path = dir.join("ledger.json");
    let bytes = fs::read(&path).unwrap();
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v["storage"]["data_dir"] = json!("linked-state");
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    run(dir, &["explain", "generation-1"], None, 2);
    fs::write(path, bytes).unwrap();
    fs::set_permissions(dir.join(".ledger"), fs::Permissions::from_mode(0o755)).unwrap();
    run(dir, &["accept", "examples/generated.json"], None, 2);
    fs::set_permissions(dir.join(".ledger"), fs::Permissions::from_mode(0o700)).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(".ledger/owner.lock"))
        .unwrap();
    lock.lock().unwrap();
    run(dir, &["accept", "examples/generated.json"], None, 7);
    lock.unlock().unwrap();
    run(dir, &["accept", "examples/generated.json"], None, 0);
}

fn sql(dir: &Path, statement: &'static str) {
    use sqlx::Connection;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let options = sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.join(".ledger/local.db"))
                .create_if_missing(false);
            let mut conn = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&mut conn)
                .await
                .unwrap();
            conn.close().await.unwrap();
        });
}
#[test]
fn preview_does_not_even_attempt_journal_or_alias_writes() {
    let d = init();
    let dir = d.path();
    let e = event(dir);
    sql(dir,"CREATE TRIGGER test_block_documents BEFORE INSERT ON documents BEGIN SELECT RAISE(ABORT,'preview must not write'); END");
    let before = db(dir);
    assert_eq!(send(dir, "preview", &e, 0)["outcome"], "would_accept");
    assert!(db(dir) == before);
    // Negative control: the same acceptance does attempt writes and hits the guard.
    send(dir, "accept", &e, 7);
    assert!(db(dir) == before);
    sql(dir, "DROP TRIGGER test_block_documents");
    send(dir, "accept", &e, 0);
    sql(dir,"CREATE TRIGGER test_block_alias BEFORE INSERT ON delivery_keys BEGIN SELECT RAISE(ABORT,'preview must not alias'); END");
    let mut alias = e;
    alias["id"] = json!("alias-preview-only");
    let before = db(dir);
    assert_eq!(send(dir, "preview", &alias, 0)["kind"], "semantic");
    assert!(db(dir) == before);
    send(dir, "accept", &alias, 7);
    assert!(db(dir) == before);
}
#[test]
fn explains_retained_history_and_detects_tampering_without_rerating() {
    let d = init();
    let dir = d.path();
    let e = event(dir);
    send(dir, "accept", &e, 0);
    let original = run(dir, &["explain", "generation-1"], None, 0);
    sql(dir, "UPDATE authority_heads SET active=0,revision=2");
    assert_eq!(run(dir, &["explain", "generation-1"], None, 0), original);
    assert_eq!(send(dir, "preview", &e, 6)["code"], "SOURCE_UNAUTHORIZED");
    // Privileged test corruption, beyond ordinary immutable write guards.
    sql(dir, "DROP TRIGGER actions_no_update");
    sql(dir,"UPDATE actions SET content_hash='sha256:0000000000000000000000000000000000000000000000000000000000000000'");
    assert_eq!(
        run(dir, &["explain", "generation-1"], None, 9)["code"],
        "INTEGRITY_FAILURE"
    );
    // Restore current read authority so acceptance can return the old receipt.
    sql(dir, "UPDATE authority_heads SET active=1,revision=3");
    // A failed human breakdown read must not relabel a committed retry as a
    // failed acceptance. Its original machine receipt remains retrievable.
    let before = db(dir);
    let human = invoke(dir, &["accept", "examples/generated.json"], None);
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("original receipt returned"));
    assert!(text.contains("breakdown could not be read"));
    assert!(!text.contains("USD 0.80"));
    assert_eq!(db(dir), before);
}

#[test]
fn migrated_cli_ledger_coexists_with_fake_outbox_and_preserves_receipts() {
    use ledgerlab::{
        outbox::{
            fake::{MemoryDestination, Mode},
            State,
        },
        Ledger,
    };
    let d = init();
    let dir = d.path();
    let receipt = run(dir, &["accept", "examples/generated.json"], None, 0)["receipt"].clone();
    let explanation = run(dir, &["explain", "generation-1"], None, 0);
    let fake = MemoryDestination::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let ledger = Ledger::open_sqlite(&dir.join(".ledger")).await.unwrap();
        let outbox = ledger.outbox(&fake);
        let deliveries = outbox.deliveries().await.unwrap();
        let now = deliveries[0].next_attempt_us;
        assert_eq!(deliveries[0].state, State::Held);
        let report = outbox.reconcile(now, Mode::Normal).await.unwrap();
        outbox.resume(&report.digest, now).await.unwrap();
        let lease = outbox.acquire("comparison-worker", now).await.unwrap();
        assert_eq!(
            outbox
                .dispatch_one(&lease, now, Mode::LoseResponse)
                .await
                .unwrap(),
            Some(State::Unknown)
        );
        // Local commands require a held installation. Pausing is explicit and
        // must preserve the independent fake's receipt through reopen.
        outbox.hold(false, now + 1).await.unwrap();
        let report = outbox.reconcile(now + 2, Mode::Normal).await.unwrap();
        assert!(report.unresolved.is_empty());
        assert_eq!(
            outbox.deliveries().await.unwrap()[0].state,
            State::Delivered
        );
        ledger.close().await;
    });
    assert_eq!(fake.receipts("store-demo-slice").len(), 1);
    let before = db(dir);
    assert_eq!(
        before
            .iter()
            .find(|(t, _)| t == "_sqlx_migrations")
            .unwrap()
            .1
            .len(),
        9
    );
    assert_eq!(
        before
            .iter()
            .find(|(t, _)| t == "dispatch_attempts")
            .unwrap()
            .1
            .len(),
        1
    );
    assert_eq!(run(dir, &["explain", "generation-1"], None, 0), explanation);
    assert_eq!(
        run(dir, &["accept", "examples/generated.json"], None, 0)["receipt"],
        receipt
    );
    assert_eq!(
        run(dir, &["preview", "examples/generated.json"], None, 0)["outcome"],
        "duplicate"
    );
    assert_eq!(
        db(dir),
        before,
        "CLI reads/retries must preserve outbox evidence and economics"
    );
}

#[test]
fn linked_events_stop_at_facade_boundary_without_any_state_change() {
    let d = init();
    let dir = d.path();
    run(dir, &["accept", "examples/generated.json"], None, 0);
    let before = db(dir);
    let mut linked = event(dir);
    linked["id"] = json!("linked-generation");
    linked["operation_id"] = json!("linked-generation");
    linked["links"] =
        json!([{"relation":"generated_from","from":{"source":"urn:demo:app","id":"generation-1"}}]);
    for command in ["accept", "preview"] {
        let result = send(dir, command, &linked, 3);
        assert_eq!(result["code"], "UNSUPPORTED_SLICE");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("generation demo"));
        assert!(result.get("receipt").is_none());
        assert_eq!(
            db(dir),
            before,
            "{command} must not reserve a Phase 2 identity or decision"
        );
    }
}

#[test]
fn explicit_relative_paths_and_contained_storage_normalize_safely() {
    let d = temp();
    let root = d.path();
    fs::create_dir(root.join("scratch")).unwrap();
    run(root, &["init", "./scratch/../demo", "--demo"], None, 0);
    let demo = root.join("demo");
    let config = demo.join("ledger.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    for storage in ["./.ledger", "examples/../.ledger"] {
        value["storage"]["data_dir"] = json!(storage);
        fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = db(&demo);
        run(
            &demo.join("examples"),
            &["--config", "../ledger.json", "preview", "./generated.json"],
            None,
            0,
        );
        run(
            root,
            &[
                "--config",
                "./scratch/../demo/ledger.json",
                "preview",
                "demo/examples/../examples/generated.json",
            ],
            None,
            0,
        );
        assert_eq!(db(&demo), before);
    }
    run(
        &demo.join("examples"),
        &["--config", "../ledger.json", "accept", "./generated.json"],
        None,
        0,
    );
}

#[test]
fn storage_escape_and_invalid_targets_do_not_write_anything() {
    let d = init();
    let dir = d.path();
    let path = dir.join("ledger.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let before = db(dir);
    for storage in [
        "../outside",
        ".",
        "examples/../../outside",
        "/tmp",
        "examples/generated.json/../.ledger",
    ] {
        config["storage"]["data_dir"] = json!(storage);
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let v = run(dir, &["preview", "examples/generated.json"], None, 2);
        assert!(v["message"].as_str().unwrap().contains("ledger.json"));
        assert_eq!(db(dir), before);
    }
    assert!(!dir.parent().unwrap().join("outside").exists());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_be_hidden_by_parent_normalization() {
    use std::os::unix::fs::symlink;
    let d = init();
    let dir = d.path();
    symlink("examples", dir.join("shortcut")).unwrap();
    let before = db(dir);
    for args in [
        vec!["preview", "shortcut/../examples/generated.json"],
        vec![
            "--config",
            "shortcut/../ledger.json",
            "explain",
            "generation-1",
        ],
        vec!["init", "shortcut/../new-demo", "--demo"],
    ] {
        let result = run(dir, &args, None, 2);
        assert!(result["message"].as_str().unwrap().contains("symlink"));
    }
    let config = dir.join("ledger.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["storage"]["data_dir"] = json!("shortcut/../.ledger");
    fs::write(config, serde_json::to_vec(&value).unwrap()).unwrap();
    run(dir, &["preview", "examples/generated.json"], None, 2);
    assert_eq!(db(dir), before);
    assert!(!dir.join("new-demo").exists());
}

#[test]
fn legacy_json_config_requires_explicit_selection() {
    let d = init();
    let dir = d.path();
    assert!(!dir.join("ledger.yaml").exists());
    fs::rename(dir.join("ledger.json"), dir.join("ledger.yaml")).unwrap();
    let before = db(dir);
    run(dir, &["preview", "examples/generated.json"], None, 2);
    run(
        dir,
        &[
            "--config",
            "ledger.yaml",
            "preview",
            "examples/generated.json",
        ],
        None,
        0,
    );
    fs::write(dir.join("ledger.yaml"), "schema: ledger/v1\n").unwrap();
    let v = run(
        dir,
        &[
            "--config",
            "ledger.yaml",
            "preview",
            "examples/generated.json",
        ],
        None,
        2,
    );
    assert!(v["message"].as_str().unwrap().contains("strict JSON"));
    assert_eq!(db(dir), before);
}

#[test]
fn config_errors_name_path_field_and_recovery_action() {
    let d = init();
    let dir = d.path();
    let path = dir.join("ledger.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (pointer, value, field, action) in [
        (
            "/storage/backend",
            json!("postgres"),
            "storage.backend",
            "set to",
        ),
        (
            "/dispatch/enabled",
            json!(true),
            "dispatch.enabled",
            "set to",
        ),
        (
            "/auth/source",
            json!(null),
            "auth.source",
            "nonempty string",
        ),
    ] {
        let mut config = original.clone();
        *config.pointer_mut(pointer).unwrap() = value;
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = run(dir, &["preview", "examples/generated.json"], None, 2);
        let message = result["message"].as_str().unwrap();
        for part in ["ledger.json", field, action] {
            assert!(message.contains(part), "{message}");
        }
    }
    let mut config = original.clone();
    config["storage"]
        .as_object_mut()
        .unwrap()
        .remove("data_dir");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let v = run(dir, &["preview", "examples/generated.json"], None, 2);
    assert!(v["message"]
        .as_str()
        .unwrap()
        .contains("storage.data_dir: missing field"));
    config = original;
    config["storage"]["extra"] = json!(true);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let v = run(dir, &["preview", "examples/generated.json"], None, 2);
    assert!(v["message"]
        .as_str()
        .unwrap()
        .contains("storage.extra: unknown field; remove it"));
    let human = invoke(dir, &["preview", "examples/generated.json"], None);
    assert!(human.stdout.is_empty());
    assert_eq!(
        String::from_utf8(human.stderr).unwrap(),
        include_str!("snapshots/config-error.txt")
    );
}

#[test]
fn human_acceptance_reads_saved_money_and_keeps_machine_receipt() {
    let d = init();
    let dir = d.path();
    let human = invoke(dir, &["accept", "examples/generated.json"], None);
    assert!(human.status.success());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        include_str!("snapshots/accept.txt")
    );
    let before = db(dir);
    let retry = invoke(dir, &["accept", "examples/generated.json"], None);
    let retry = String::from_utf8(retry.stdout).unwrap();
    assert!(retry.contains("Already recorded; original receipt returned."));
    assert!(retry.contains("demo-customer -> demo-host: USD 0.80"));
    let json = run(dir, &["accept", "examples/generated.json"], None, 0);
    assert!(json.get("human_history").is_none());
    assert_eq!(
        json["receipt"],
        serde_json::from_str::<Value>(include_str!(
            "../../../fixtures/journals/first-slice/receipt.json"
        ))
        .unwrap()
    );
    assert_eq!(db(dir), before);
    // A failed work event has no postings: never manufacture the demo's charge.
    let fresh = init();
    let mut e = event(fresh.path());
    e["status"] = json!("failed");
    e["quantity"] = json!("0");
    let bytes = serde_json::to_vec(&e).unwrap();
    let human = invoke(fresh.path(), &["accept", "-"], Some(&bytes));
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("No charge booked"));
    assert!(!text.contains("USD 0.80"));
}
