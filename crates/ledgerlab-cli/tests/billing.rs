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
    assert_eq!(
        run(root, &["billing", "--directory", "store", "upgrade"], 0)["status"],
        "already_current"
    );
    let accepted = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        0,
    );
    assert_eq!(accepted["status"], "accepted");
    let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
    let explained = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "explain",
            "--customer",
            "customer-1",
            target,
        ],
        0,
    );
    assert_eq!(explained["net_atoms"], "375");
    let duplicate = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        0,
    );
    assert_eq!(duplicate["status"], "duplicate");
    assert_eq!(accepted["receipt"], duplicate["receipt"]);
    event["quantity"] = json!("2");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "event.json"
            ],
            4
        )["code"],
        "IDENTITY_CONFLICT"
    );
    event["customer"] = json!("another-customer");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
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
fn customers_share_source_and_application_ids_without_sharing_history() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let mut setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
    fs::write(root.join("setup.json"), serde_json::to_vec(&setup).unwrap()).unwrap();
    let mut event: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/event.json")).unwrap();
    event["id"] = json!("shared-delivery");
    event["operation_id"] = json!("shared-operation");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let first_customer = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        0,
    );

    setup["customer"] = json!("customer-2");
    setup["agreement"] = json!("agreement-2");
    setup["binding"] = json!("binding-2");
    setup["outcome_policy"]["families"][0]["binding_id"] = json!("binding-2");
    setup["outcome_policy"]["limits"][0]["binding_id"] = json!("binding-2");
    let registration = json!({
        "schema":"ledger-billing-registration/2",
        "customer":"customer-2",
        "source":"urn:example:work",
        "change_id":"register-customer-2",
        "expected_revision":"0",
        "effective_at":"2026-09-01T00:00:00.000000Z",
        "setup":setup
    });
    fs::write(
        root.join("registration.json"),
        serde_json::to_vec(&registration).unwrap(),
    )
    .unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "agreement",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:work",
            "registration.json",
        ],
        0,
    );

    event["customer"] = json!("customer-2");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    let second_customer = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        0,
    );
    assert_eq!(first_customer["status"], "accepted");
    assert_eq!(second_customer["status"], "accepted");
    assert_ne!(
        first_customer["receipt"]["body"]["target"],
        second_customer["receipt"]["body"]["target"]
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-2",
                "--source",
                "urn:example:work",
                "event.json",
            ],
            0,
        )["receipt"],
        second_customer["receipt"]
    );

    let first_statement = run(
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
    let second_statement = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-2",
        ],
        0,
    );
    assert_eq!(first_statement["schema"], "ledger-billing-statement/2");
    assert_eq!(first_statement["entries"].as_array().unwrap().len(), 1);
    assert_eq!(second_statement["entries"].as_array().unwrap().len(), 1);
    assert_ne!(first_statement["scope"], second_statement["scope"]);

    setup["source"] = json!("urn:example:other");
    setup["agreement"] = json!("agreement-3");
    setup["binding"] = json!("binding-3");
    setup["outcome_policy"]["families"][0]["binding_id"] = json!("binding-3");
    setup["outcome_policy"]["families"][0]["source"] = json!("urn:example:other");
    setup["outcome_policy"]["families"][0]["correction_source"] = json!("urn:example:other");
    setup["outcome_policy"]["limits"][0]["binding_id"] = json!("binding-3");
    let second_source_registration = json!({
        "schema":"ledger-billing-registration/2",
        "customer":"customer-2",
        "source":"urn:example:other",
        "change_id":"register-second-source",
        "expected_revision":"0",
        "effective_at":"2026-09-01T00:00:00.000000Z",
        "setup":setup
    });
    fs::write(
        root.join("registration.json"),
        serde_json::to_vec(&second_source_registration).unwrap(),
    )
    .unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "agreement",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:other",
            "registration.json",
        ],
        0,
    );
    event["id"] = json!("other-source-work");
    event["operation_id"] = json!("other-source-operation");
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    let unreadable_source_entry = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:other",
            "event.json",
        ],
        0,
    );
    let cross_source_outcome = json!({
        "schema":"ledger-billing-outcome/2",
        "customer":"customer-2",
        "source":"urn:example:work",
        "id":"cross-source-target-probe",
        "target":unreadable_source_entry["receipt"]["body"]["target"],
        "family":"quality",
        "occurred_at":"2026-09-22T00:00:00.000000Z",
        "evidence":"Test that target lookup is scoped to customer and source",
        "code":"rebate"
    });
    fs::write(
        root.join("cross-source-outcome.json"),
        serde_json::to_vec(&cross_source_outcome).unwrap(),
    )
    .unwrap();
    let cross_source_response = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "outcome",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:work",
            "cross-source-outcome.json",
        ],
        6,
    );
    assert_eq!(cross_source_response["code"], "BILLING_NOT_FOUND");
    let mut missing_target_outcome = cross_source_outcome;
    missing_target_outcome["id"] = json!("missing-target-probe");
    missing_target_outcome["target"] = json!("target-that-does-not-exist");
    fs::write(
        root.join("missing-target-outcome.json"),
        serde_json::to_vec(&missing_target_outcome).unwrap(),
    )
    .unwrap();
    let missing_target_response = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "outcome",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:work",
            "missing-target-outcome.json",
        ],
        6,
    );
    assert_eq!(cross_source_response, missing_target_response);

    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "statement",
                "--customer",
                "customer-2"
            ],
            0,
        )["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let revoke_source_read = json!({
        "schema":"ledger-billing-permissions/2",
        "customer":"customer-2",
        "source":"urn:example:other",
        "change_id":"revoke-second-source-read",
        "expected_revision":"1",
        "permissions":["submit", "correct"],
        "reason":"Test that a customer statement cannot omit a source"
    });
    fs::write(
        root.join("permissions.json"),
        serde_json::to_vec(&revoke_source_read).unwrap(),
    )
    .unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:other",
            "permissions.json",
        ],
        0,
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
                "customer-2"
            ],
            6,
        )["code"],
        "BILLING_UNAUTHORIZED"
    );
    let readable_target = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "explain",
            "--customer",
            "customer-2",
            second_customer["receipt"]["body"]["target"]
                .as_str()
                .unwrap(),
        ],
        0,
    );
    assert_eq!(readable_target["complete"], true);
    assert_eq!(readable_target["cutoff"], "1");
    assert_eq!(readable_target["entries"].as_array().unwrap().len(), 1);
    assert_eq!(readable_target["agreements"].as_array().unwrap().len(), 1);
    assert_eq!(
        readable_target["agreements"][0]["source"],
        "urn:example:work"
    );
    let unreadable_target = unreadable_source_entry["receipt"]["body"]["target"]
        .as_str()
        .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "explain",
                "--customer",
                "customer-2",
                unreadable_target
            ],
            6,
        )["code"],
        "BILLING_NOT_FOUND"
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "explain",
                "--customer",
                "customer-2",
                "unknown-target"
            ],
            6,
        )["code"],
        "BILLING_NOT_FOUND"
    );
}

#[test]
fn billing_accepts_the_legacy_256_byte_source_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let source = format!("urn:{}", "a".repeat(252));
    assert_eq!(source.len(), 256);
    let mut setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
    setup["source"] = json!(source);
    setup["outcome_policy"]["families"][0]["source"] = json!(source);
    setup["outcome_policy"]["families"][0]["correction_source"] = json!(source);
    fs::write(root.join("setup.json"), serde_json::to_vec(&setup).unwrap()).unwrap();
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
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            &source,
            "event.json",
        ],
        0,
    );
    assert_eq!(accepted["status"], "accepted");
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
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        0,
    );
    let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
    let outcome = json!({"schema":"ledger-billing-outcome/2","customer":"customer-1","source":"urn:example:work","id":"quality-1","target":target,"family":"quality","occurred_at":"2026-09-22T00:00:00.000000Z","evidence":"Operator observed quality issue","code":"rebate"});
    fs::write(
        root.join("outcome.json"),
        serde_json::to_vec(&outcome).unwrap(),
    )
    .unwrap();
    let original = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "outcome",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "outcome.json",
        ],
        0,
    );
    let duplicate = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "outcome",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "outcome.json",
        ],
        0,
    );
    assert_eq!(original["receipt"], duplicate["receipt"]);
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "explain",
                "--customer",
                "customer-1",
                target
            ],
            0
        )["net_atoms"],
        "200"
    );
    let mut correction = json!({"schema":"ledger-billing-correction/2","customer":"customer-1","source":"urn:example:work","id":"correction-1","target":target,"family":"quality","occurred_at":"2026-09-22T00:00:00.000000Z","evidence":"Operator corrected the mistaken quality assessment","expected_revision":"1","replacement":{"kind":"code","code":"none"}});
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
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
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
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "correction.json"
            ],
            0
        )["receipt"]
    );
    let corrected_explanation = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "explain",
            "--customer",
            "customer-1",
            target,
        ],
        0,
    );
    assert_eq!(corrected_explanation["complete"], true);
    assert_eq!(corrected_explanation["cutoff"], "3");
    assert_eq!(
        corrected_explanation["entries"].as_array().unwrap().len(),
        3
    );
    assert_eq!(corrected_explanation["net_atoms"], "250");
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
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
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
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        0,
    );
    event["id"] = json!("renamed-delivery");
    write("alias.json", &event);
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "alias.json"
            ],
            0
        )["receipt"],
        original["receipt"]
    );
    event["operation_id"] = json!("different-operation");
    write("conflict.json", &event);
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "conflict.json"
            ],
            4
        )["code"],
        "IDENTITY_CONFLICT"
    );
    let mut change = json!({"schema":"ledger-billing-permissions/2","customer":"customer-1","source":"urn:example:work","change_id":"revoke-submit","expected_revision":"1","permissions":["read"],"reason":"Revoked submissions and corrections"});
    write("permissions.json", &change);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "permissions.json",
        ],
        0,
    );
    event["id"] = json!("new-delivery");
    write("new.json", &event);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "new.json",
        ],
        6,
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "alias.json"
            ],
            0
        )["receipt"],
        original["receipt"]
    );
    let target = original["receipt"]["body"]["target"].as_str().unwrap();
    let correction = json!({"schema":"ledger-billing-correction/2","customer":"customer-1","source":"urn:example:work","id":"denied-correction","target":target,"family":"quality","occurred_at":"2026-09-22T00:00:00.000000Z","evidence":"Denied request","expected_revision":"1","replacement":{"kind":"reverse"}});
    write("correction.json", &correction);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "correct",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "correction.json",
        ],
        6,
    );
    change["expected_revision"] = json!("2");
    change["change_id"] = json!("revoke-all");
    change["permissions"] = json!([]);
    write("permissions.json", &change);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "permissions.json",
        ],
        0,
    );
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "alias.json",
        ],
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
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "permissions",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work"
            ],
            0
        )["revision"],
        "3"
    );
    change["expected_revision"] = json!("3");
    change["change_id"] = json!("restore-rights");
    change["permissions"] = json!(["read", "submit", "correct"]);
    write("permissions.json", &change);
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "permissions",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "permissions.json",
        ],
        0,
    );
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "new.json",
        ],
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
            &[
                "billing",
                "--directory",
                "restored",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "alias.json"
            ],
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
        &[
            "billing",
            "--directory",
            "second",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        6,
    );
    event["customer"] = json!("customer-2");
    write("second.json", &event);
    run(
        root,
        &[
            "billing",
            "--directory",
            "second",
            "accept",
            "--customer",
            "customer-2",
            "--source",
            "urn:example:work",
            "second.json",
        ],
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
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "event.json",
        ],
        7,
    );
    lock.unlock().unwrap();
    fs::write(root.join("oversize.json"), vec![b' '; 262145]).unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-1",
            "--source",
            "urn:example:work",
            "oversize.json",
        ],
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

#[test]
fn statement_sums_valid_charges_beyond_the_individual_money_bound() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let mut setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
    setup["price"] = json!("6000000000000000000000000000.00");
    fs::write(root.join("setup.json"), serde_json::to_vec(&setup).unwrap()).unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    for i in 1..=2 {
        let mut event: Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/event.json")).unwrap();
        event["id"] = json!(format!("large-{i}"));
        event["operation_id"] = json!(format!("large-operation-{i}"));
        fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "event.json",
            ],
            0,
        );
    }
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
    assert_eq!(statement["complete"], true);
    assert_eq!(statement["net_atoms"], "1200000000000000000000000000000");
    assert_eq!(
        statement["entries"][0]["net_atoms"],
        "600000000000000000000000000000"
    );
    assert_eq!(
        statement["entries"][1]["net_atoms"],
        "600000000000000000000000000000"
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-1",
                "--source",
                "urn:example:work",
                "event.json"
            ],
            0
        )["status"],
        "duplicate"
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
}
