use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
fn run(root: &Path, args: &[&str], expected: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ledger"))
        .current_dir(root)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn configured_billing_cli_reopen_retry_statement_and_refusals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let mut setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
    setup["price"] = json!("3.75");
    fs::write(root.join("setup.json"), serde_json::to_vec(&setup).unwrap()).unwrap();
    let mut event: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/event.json")).unwrap();
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let accepted = run(
        root,
        &["billing", "--directory", "store", "accept", "event.json"],
        0,
    );
    assert_eq!(accepted["status"], "accepted");
    let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
    let explained = run(
        root,
        &["billing", "--directory", "store", "explain", target],
        0,
    );
    assert_eq!(explained["net_atoms"], "375");
    let duplicate = run(
        root,
        &["billing", "--directory", "store", "accept", "event.json"],
        0,
    );
    assert_eq!(duplicate["status"], "duplicate");
    assert_eq!(accepted["receipt"], duplicate["receipt"]);
    event["quantity"] = json!("2");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "store", "accept", "event.json"],
            4
        )["code"],
        "IDENTITY_CONFLICT"
    );
    event["customer"] = json!("another-customer");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    run(
        root,
        &["billing", "--directory", "store", "accept", "event.json"],
        6,
    );
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "another-customer",
        ],
        6,
    );
    let statement = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-1",
        ],
        0,
    );
    assert_eq!(statement["net_atoms"], "375");
    assert_eq!(statement["complete"], true);
    assert_eq!(statement["entries"].as_array().unwrap().len(), 1);
    // Draft changes cannot mutate the installed terms or booked charge.
    setup["price"] = json!("9.99");
    fs::write(root.join("setup.json"), serde_json::to_vec(&setup).unwrap()).unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "statement",
                "--customer",
                "customer-1"
            ],
            0
        ),
        statement
    );
}

#[test]
fn corrections_preserve_history_and_reconcile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::write(
        root.join("setup.json"),
        include_bytes!("../../../examples/billing/setup.json"),
    )
    .unwrap();
    fs::write(
        root.join("event.json"),
        include_bytes!("../../../examples/billing/event.json"),
    )
    .unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let accepted = run(
        root,
        &["billing", "--directory", "store", "accept", "event.json"],
        0,
    );
    let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
    let outcome = json!({"schema":"ledger-billing-outcome/1","id":"quality-1","target":target,"family":"quality","occurred_at":"2026-09-22T00:00:00.000000Z","evidence":"Operator observed quality issue","code":"rebate"});
    fs::write(
        root.join("outcome.json"),
        serde_json::to_vec(&outcome).unwrap(),
    )
    .unwrap();
    let original = run(
        root,
        &["billing", "--directory", "store", "outcome", "outcome.json"],
        0,
    );
    let duplicate = run(
        root,
        &["billing", "--directory", "store", "outcome", "outcome.json"],
        0,
    );
    assert_eq!(original["receipt"], duplicate["receipt"]);
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "store", "explain", target],
            0
        )["net_atoms"],
        "200"
    );
    let mut correction = json!({"schema":"ledger-billing-correction/1","id":"correction-1","target":target,"family":"quality","occurred_at":"2026-09-22T00:00:00.000000Z","evidence":"Operator corrected the mistaken quality assessment","expected_revision":"1","replacement":{"kind":"code","code":"none"}});
    fs::write(
        root.join("correction.json"),
        serde_json::to_vec(&correction).unwrap(),
    )
    .unwrap();
    let corrected = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "correct",
            "correction.json",
        ],
        0,
    );
    assert_eq!(
        corrected["receipt"],
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "correct",
                "correction.json"
            ],
            0
        )["receipt"]
    );
    let statement = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-1",
        ],
        0,
    );
    assert_eq!(statement["net_atoms"], "250");
    assert_eq!(statement["entries"].as_array().unwrap().len(), 3);
    correction["id"] = json!("stale-correction");
    fs::write(
        root.join("correction.json"),
        serde_json::to_vec(&correction).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "correct",
                "correction.json"
            ],
            3
        )["code"],
        "STALE_REVISION"
    );
}

#[test]
fn aliases_revocation_isolation_and_quiescent_restore() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let mut setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
    let mut event: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/event.json")).unwrap();
    let write =
        |name: &str, v: &Value| fs::write(root.join(name), serde_json::to_vec(v).unwrap()).unwrap();
    write("setup.json", &setup);
    write("event.json", &event);
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let original = run(
        root,
        &["billing", "--directory", "store", "accept", "event.json"],
        0,
    );
    event["id"] = json!("renamed-delivery");
    write("alias.json", &event);
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "store", "accept", "alias.json"],
            0
        )["receipt"],
        original["receipt"]
    );
    event["operation_id"] = json!("different-operation");
    write("conflict.json", &event);
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "store", "accept", "conflict.json"],
            4
        )["code"],
        "IDENTITY_CONFLICT"
    );
    let mut change = json!({"schema":"ledger-billing-permissions/1","expected_revision":"1","permissions":["read"],"reason":"Revoked submissions and corrections"});
    write("permissions.json", &change);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "permissions.json",
        ],
        0,
    );
    event["id"] = json!("new-delivery");
    write("new.json", &event);
    run(
        root,
        &["billing", "--directory", "store", "accept", "new.json"],
        6,
    );
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "store", "accept", "alias.json"],
            0
        )["receipt"],
        original["receipt"]
    );
    let target = original["receipt"]["body"]["target"].as_str().unwrap();
    let correction = json!({"schema":"ledger-billing-correction/1","id":"denied-correction","target":target,"family":"quality","occurred_at":"2026-09-22T00:00:00.000000Z","evidence":"Denied request","expected_revision":"1","replacement":{"kind":"reverse"}});
    write("correction.json", &correction);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "correct",
            "correction.json",
        ],
        6,
    );
    change["expected_revision"] = json!("2");
    change["permissions"] = json!([]);
    write("permissions.json", &change);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "permissions.json",
        ],
        0,
    );
    run(
        root,
        &["billing", "--directory", "store", "accept", "alias.json"],
        6,
    );
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-1",
        ],
        6,
    );
    assert_eq!(
        run(root, &["billing", "--directory", "store", "permissions"], 0)["revision"],
        "3"
    );
    change["expected_revision"] = json!("3");
    change["permissions"] = json!(["read", "submit", "correct"]);
    write("permissions.json", &change);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "permissions.json",
        ],
        0,
    );
    run(
        root,
        &["billing", "--directory", "store", "accept", "new.json"],
        0,
    );
    let statement = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-1",
        ],
        0,
    );
    assert_eq!(statement["net_atoms"], "500");
    // Each command exited; the whole installation is quiescent. Copy metadata,
    // database, owner file and any SQLite sidecars, never a live database alone.
    assert!(Command::new("cp")
        .current_dir(root)
        .args(["-Rp", "store", "restored"])
        .status()
        .unwrap()
        .success());
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "restored",
                "statement",
                "--customer",
                "customer-1"
            ],
            0
        ),
        statement
    );
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "restored", "accept", "alias.json"],
            0
        )["receipt"],
        original["receipt"]
    );
    setup["customer"] = json!("customer-2");
    setup["store_id"] = json!("second-store");
    setup["agreement"] = json!("agreement-2");
    write("setup2.json", &setup);
    run(
        root,
        &["billing", "init", "second", "--setup", "setup2.json"],
        0,
    );
    run(
        root,
        &["billing", "--directory", "second", "accept", "event.json"],
        6,
    );
    event["customer"] = json!("customer-2");
    write("second.json", &event);
    run(
        root,
        &["billing", "--directory", "second", "accept", "second.json"],
        0,
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "second",
                "statement",
                "--customer",
                "customer-2"
            ],
            0
        )["net_atoms"],
        "250"
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "statement",
                "--customer",
                "customer-1"
            ],
            0
        ),
        statement
    );
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("store/.ledger/owner.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    run(
        root,
        &["billing", "--directory", "store", "accept", "event.json"],
        7,
    );
    lock.unlock().unwrap();
    fs::write(root.join("oversize.json"), vec![b' '; 262145]).unwrap();
    run(
        root,
        &["billing", "--directory", "store", "accept", "oversize.json"],
        2,
    );
    setup["outcome_policy"]["families"][0]["codes"][0]["amount"]["kind"] = json!("unrecognized");
    write("bad-setup.json", &setup);
    run(
        root,
        &["billing", "init", "invalid", "--setup", "bad-setup.json"],
        3,
    );
    assert!(!root.join("invalid").exists());
}
