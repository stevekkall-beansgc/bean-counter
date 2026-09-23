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
