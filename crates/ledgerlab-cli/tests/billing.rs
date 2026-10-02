use serde_json::{json, Value};
use std::{env, fs, path::Path, path::PathBuf, process::Command};
fn ledger_binary() -> PathBuf {
    let configured =
        env::var_os("LEDGER_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_ledger").into());
    fs::canonicalize(configured).expect("configured ledger binary must exist")
}
fn run(root: &Path, args: &[&str], expected: i32) -> Value {
    let out = Command::new(ledger_binary())
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
fn customer_term_command_uses_exact_retry_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let setup = include_bytes!("../../../examples/billing/setup.json");
    fs::write(root.join("setup.json"), setup).unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );

    let term = json!({
        "schema":"ledger-billing-term/1",
        "customer":"customer-1",
        "change_id":"term-change-1",
        "expected_revision":"0",
        "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
        "term":{
            "interval":1,
            "unit":"month",
            "alignment":"anchored",
            "anchor":{"date":"2026-09-01","time":"00:00:00"},
            "timezone":"UTC",
            "month_end_rule":"preserve_anchor_and_clamp",
            "boundary_rule_version":"billing-boundary/1",
            "timezone_rules_version":"IANA-2025b",
            "proration":"none"
        }
    });
    fs::write(root.join("term.json"), serde_json::to_vec(&term).unwrap()).unwrap();
    let result = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "term",
            "set",
            "term.json",
        ],
        0,
    );
    assert_eq!(result["status"], "term_updated");
    assert_eq!(result["revision"], "1");
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "term",
                "set",
                "term.json",
            ],
            0,
        ),
        result
    );

    let mut changed = term;
    changed["term"]["interval"] = json!(2);
    fs::write(
        root.join("term.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "term",
                "set",
                "term.json",
            ],
            4,
        )["code"],
        "IDENTITY_CONFLICT"
    );
}

#[test]
fn fiscal_calendar_and_report_are_runnable_from_the_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::write(
        root.join("setup.json"),
        include_bytes!("../../../examples/billing/setup.json"),
    )
    .unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let calendar = json!({
        "schema":"ledger-fiscal-calendar/1","change_id":"cli-fiscal-1",
        "expected_revision":"0","timezone":"UTC","timezone_rules_version":"IANA-2025b",
        "calendar":{"kind":"gregorian_years","fiscal_year_start_month":1,
            "fiscal_year_start_day":1}
    });
    fs::write(
        root.join("calendar.json"),
        serde_json::to_vec(&calendar).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "fiscal",
                "set",
                "calendar.json",
            ],
            0,
        )["status"],
        "calendar_updated"
    );
    let report = json!({
        "schema":"ledger-fiscal-report-request/1","command_id":"cli-report-1",
        "calendar_version":"1","start":"2026-01-01T00:00:00.000000Z",
        "end":"2027-01-01T00:00:00.000000Z"
    });
    fs::write(
        root.join("report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let result = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "fiscal",
            "report",
            "report.json",
        ],
        0,
    );
    assert_eq!(result["schema"], "ledger-fiscal-report/1");
    assert_eq!(result["status"], "complete");
    assert_eq!(result["monetary_lines"], json!([]));
}

#[test]
fn billing_close_accepts_the_frozen_period_request() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::write(
        root.join("setup.json"),
        include_bytes!("../../../examples/billing/setup.json"),
    )
    .unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let term = json!({
        "schema":"ledger-billing-term/1","customer":"customer-1",
        "change_id":"close-cli-term","expected_revision":"0",
        "effective":{"mode":"initial","at":"2000-01-01T00:00:00.000000Z"},
        "term":{"interval":1,"unit":"month","alignment":"anchored",
            "anchor":{"date":"2000-01-01","time":"00:00:00"},"timezone":"UTC",
            "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
            "timezone_rules_version":"IANA-2025b","proration":"none"}
    });
    fs::write(root.join("term.json"), serde_json::to_vec(&term).unwrap()).unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "term",
            "set",
            "term.json",
        ],
        0,
    );
    let close = json!({
        "schema":"ledger-billing-period-close/1","customer":"customer-1",
        "period_id":{"term_version":"1","period_index":"0"}
    });
    let mut invalid = close.clone();
    invalid["period_id"]["period_index"] = json!(0);
    fs::write(
        root.join("close-invalid.json"),
        serde_json::to_vec(&invalid).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "close",
                "close-invalid.json",
            ],
            3,
        )["code"],
        "BILLING_M5_REQUEST"
    );
    fs::write(root.join("close.json"), serde_json::to_vec(&close).unwrap()).unwrap();
    let first = run(
        root,
        &["billing", "--directory", "store", "close", "close.json"],
        0,
    );
    assert_eq!(first["schema"], "ledger-billing-statement/4");
    assert_eq!(first["status"], "closed");
    assert_eq!(first["period_id"], close["period_id"]);
    assert_eq!(first["lines"], json!([]));
    assert_eq!(
        run(
            root,
            &["billing", "--directory", "store", "close", "close.json",],
            0,
        ),
        first
    );
    fs::write(
        root.join("mapping.json"),
        br#"{"schema":"ledger-finance-mapping/1","accounts":{}}"#,
    )
    .unwrap();
    let export = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "export-csv",
            "--customer",
            "customer-1",
            "--snapshot",
            first["statement_hash"].as_str().unwrap(),
            "--mapping",
            "mapping.json",
            "--output",
            "period.csv",
        ],
        0,
    );
    assert_eq!(export["schema"], "ledger-finance-export/4");
    assert_eq!(export["statement_hash"], first["statement_hash"]);
    assert_eq!(export["posting_count"], "0");
    assert_eq!(export["control_net_atoms"], "0");
    assert!(export.get("output").is_none());
    let csv = fs::read_to_string(root.join("period.csv")).unwrap();
    assert!(csv.starts_with("\"row_type\",\"export_id\",\"statement_hash\""));
    assert_eq!(csv.matches("\r\n").count(), 2);
    assert!(csv.contains("\r\n\"complete\",\"sha256:"));
}

#[test]
fn cumulative_activity_and_quantity_correction_are_runnable_from_the_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::write(
        root.join("setup.json"),
        include_bytes!("../../../examples/billing/setup.json"),
    )
    .unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );

    let term = json!({
        "schema":"ledger-billing-term/1","customer":"customer-1",
        "change_id":"cumulative-cli-term","expected_revision":"0",
        "effective":{"mode":"initial","at":"2000-01-01T00:00:00.000000Z"},
        "term":{"interval":1,"unit":"month","alignment":"anchored",
            "anchor":{"date":"2000-01-01","time":"00:00:00"},"timezone":"UTC",
            "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
            "timezone_rules_version":"IANA-2025b","proration":"none"}
    });
    fs::write(root.join("term.json"), serde_json::to_vec(&term).unwrap()).unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "term",
            "set",
            "term.json",
        ],
        0,
    );

    let basis = json!({
        "schema":"ledger-billing-cumulative-agreement/1","customer":"customer-1",
        "source":"urn:example:work","change_id":"cumulative-cli-basis",
        "expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1",
        "effective_at":"2026-09-01T00:00:00.000000Z",
        "basis":{"mode":"cumulative_period","source_unit":"token",
            "billable_unit":"billable-token","conversion_numerator":"1",
            "conversion_denominator":"2","rate_usd_per_billable_unit":"0.000000000000000002",
            "maximum_period_quantity":"1000"}
    });
    fs::write(root.join("basis.json"), serde_json::to_vec(&basis).unwrap()).unwrap();
    let setup = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "cumulative",
            "setup",
            "basis.json",
        ],
        0,
    );
    assert_eq!(setup["status"], "basis_updated");
    assert_eq!(setup["basis_version"], "1");

    let activity = json!({
        "schema":"ledger-billing-activity/1","customer":"customer-1",
        "source":"urn:example:work","id":"cumulative-cli-delivery",
        "operation_id":"cumulative-cli-operation","target":"cumulative-cli-target",
        "quantity":"10",
        "occurred_at":"2000-01-02T00:00:00.000000Z",
        "evidence":"synthetic CLI cumulative activity"
    });
    fs::write(
        root.join("activity.json"),
        serde_json::to_vec(&activity).unwrap(),
    )
    .unwrap();
    let accepted = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "activity",
            "activity.json",
        ],
        0,
    );
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(
        accepted["receipt"]["record_ids"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "activity",
                "activity.json",
            ],
            0,
        ),
        accepted
    );

    let correction = json!({
        "schema":"ledger-billing-quantity-correction/1","customer":"customer-1",
        "source":"urn:example:work","id":"cumulative-cli-correction",
        "target":"cumulative-cli-delivery","quantity_delta":"-2",
        "occurred_at":"2000-01-03T00:00:00.000000Z",
        "evidence":"synthetic CLI quantity correction"
    });
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
    assert_eq!(corrected["status"], "accepted");
    assert!(corrected.get("adjustment").is_none());
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "correct",
                "correction.json",
            ],
            0,
        ),
        corrected
    );
}

#[test]
fn recurrence_occurrence_and_cancel_are_runnable_from_the_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::write(
        root.join("setup.json"),
        include_bytes!("../../../examples/billing/setup.json"),
    )
    .unwrap();
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let term = json!({
        "schema":"ledger-billing-term/1","customer":"customer-1",
        "change_id":"recurrence-cli-term","expected_revision":"0",
        "effective":{"mode":"initial","at":"2000-01-01T00:00:00.000000Z"},
        "term":{"interval":1,"unit":"month","alignment":"anchored",
            "anchor":{"date":"2000-01-01","time":"00:00:00"},"timezone":"UTC",
            "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
            "timezone_rules_version":"IANA-2025b","proration":"none"}
    });
    fs::write(root.join("term.json"), serde_json::to_vec(&term).unwrap()).unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "term",
            "set",
            "term.json",
        ],
        0,
    );
    let recurrence = json!({
        "schema":"ledger-billing-recurrence/1","customer":"customer-1",
        "source":"urn:example:work","change_id":"recurrence-cli-1",
        "expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1",
        "rule":{"interval":1,"unit":"month","anchor":{"date":"2026-09-01","time":"00:00:00"},
            "timezone":"UTC","effective_from":"2026-09-01T00:00:00.000000Z",
            "boundary_rule_version":"billing-boundary/1","timezone_rules_version":"IANA-2025b",
            "proration":"none"},"renewal":{"mode":"manual"}
    });
    fs::write(
        root.join("recurrence.json"),
        serde_json::to_vec(&recurrence).unwrap(),
    )
    .unwrap();
    let recurrence_result = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "recurrence",
            "set",
            "recurrence.json",
        ],
        0,
    );
    assert_eq!(recurrence_result["recurrence_version"], "1");
    let query = json!({
        "schema":"ledger-billing-occurrence-query/1","customer":"customer-1",
        "source":"urn:example:work","recurrence_version":"1",
        "due_through":"2026-09-30T00:00:00.000000Z","limit":10
    });
    fs::write(root.join("query.json"), serde_json::to_vec(&query).unwrap()).unwrap();
    let due = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "occurrences",
            "query.json",
        ],
        0,
    );
    assert_eq!(due["occurrences"].as_array().unwrap().len(), 1);
    let occurrence_id = due["occurrences"][0]["occurrence_id"].as_str().unwrap();
    let accept = json!({
        "schema":"ledger-billing-occurrence-acceptance/1","customer":"customer-1",
        "source":"urn:example:work","occurrence_id":occurrence_id,
        "event":{"schema":"ledger-event/1","id":occurrence_id,
            "operation_id":"recurrence-cli-operation","type":"content.generated",
            "customer":"customer-1","occurred_at":"2026-09-01T00:00:00.000000Z",
            "status":"succeeded"}
    });
    fs::write(
        root.join("accept.json"),
        serde_json::to_vec(&accept).unwrap(),
    )
    .unwrap();
    let accepted = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "occurrence",
            "accept",
            "accept.json",
        ],
        0,
    );
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["receipt"]["kind"], "base-acceptance");
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "occurrence",
                "accept",
                "accept.json",
            ],
            0,
        ),
        accepted
    );
    let cancel = json!({
        "schema":"ledger-billing-recurrence-cancel/1","customer":"customer-1",
        "source":"urn:example:work","change_id":"recurrence-cli-cancel",
        "expected_revision":"1","recurrence_version":"1"
    });
    fs::write(
        root.join("cancel.json"),
        serde_json::to_vec(&cancel).unwrap(),
    )
    .unwrap();
    let cancelled = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "recurrence",
            "cancel",
            "cancel.json",
        ],
        0,
    );
    assert_eq!(cancelled["status"], "recurrence_cancelled");

    // Every CLI command has exited, so copying the whole installation is a
    // quiescent restore. The restored M5 term, recurrence, occurrence receipt,
    // cancellation and exact command identities must all reopen together.
    assert!(Command::new("cp")
        .current_dir(root)
        .args(["-Rp", "store", "restored-m5"])
        .status()
        .unwrap()
        .success());
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "restored-m5",
                "recurrence",
                "set",
                "recurrence.json",
            ],
            0,
        ),
        recurrence_result
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "restored-m5",
                "occurrence",
                "accept",
                "accept.json",
            ],
            0,
        ),
        accepted
    );
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "restored-m5",
                "recurrence",
                "cancel",
                "cancel.json",
            ],
            0,
        ),
        cancelled
    );
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
    let mut outcome_alias = outcome.clone();
    outcome_alias["id"] = json!("quality-alias");
    fs::write(
        root.join("outcome-alias.json"),
        serde_json::to_vec(&outcome_alias).unwrap(),
    )
    .unwrap();
    let semantic_alias = run(
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
            "outcome-alias.json",
        ],
        0,
    );
    assert_eq!(semantic_alias["kind"], "semantic");
    assert_eq!(semantic_alias["receipt"], original["receipt"]);
    let identity_alias = run(
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
            "outcome-alias.json",
        ],
        0,
    );
    assert_eq!(identity_alias["kind"], "identity");
    assert_eq!(identity_alias["receipt"], original["receipt"]);
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

#[test]
fn unit_rate_profile_preserves_token_precision_and_rejects_failed_work() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::write(
        root.join("setup.json"),
        include_bytes!("../../../examples/billing/usage/setup.json"),
    )
    .unwrap();
    let event: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/usage/event.json"))
            .unwrap();
    fs::write(root.join("event.json"), serde_json::to_vec(&event).unwrap()).unwrap();
    let mut failed = event.clone();
    failed["id"] = json!("failed-usage-work");
    failed["operation_id"] = json!("failed-usage-operation");
    failed["status"] = json!("failed");
    fs::write(
        root.join("failed.json"),
        serde_json::to_vec(&failed).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("mapping.json"),
        include_bytes!("../../../examples/billing/usage/mapping.json"),
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
            "customer-usage-1",
            "--source",
            "urn:example:usage-work",
            "event.json",
        ],
        0,
    );
    let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
    let initial = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-usage-1",
        ],
        0,
    );
    assert_eq!(initial["schema"], "ledger-billing-statement/3");
    assert_eq!(initial["scale"], 18);
    assert_eq!(initial["net_atoms"], "25000000000000");

    let failed_result = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "accept",
            "--customer",
            "customer-usage-1",
            "--source",
            "urn:example:usage-work",
            "failed.json",
        ],
        3,
    );
    assert_eq!(failed_result["code"], "BILLING_FAILED_WORK");

    let mut over_limit = event.clone();
    over_limit["id"] = json!("over-limit-usage-work");
    over_limit["operation_id"] = json!("over-limit-usage-operation");
    over_limit["quantity"] = json!("1000001");
    fs::write(
        root.join("over-limit.json"),
        serde_json::to_vec(&over_limit).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-usage-1",
                "--source",
                "urn:example:usage-work",
                "over-limit.json",
            ],
            3
        )["code"],
        "QUANTITY"
    );

    let mut fractional = event.clone();
    fractional["id"] = json!("fractional-usage-work");
    fractional["operation_id"] = json!("fractional-usage-operation");
    fractional["quantity"] = json!("100.5");
    fs::write(
        root.join("fractional.json"),
        serde_json::to_vec(&fractional).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            root,
            &[
                "billing",
                "--directory",
                "store",
                "accept",
                "--customer",
                "customer-usage-1",
                "--source",
                "urn:example:usage-work",
                "fractional.json",
            ],
            3
        )["code"],
        "BILLING_QUANTITY"
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
                "customer-usage-1"
            ],
            0
        ),
        initial
    );

    let mut outcome: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/billing/usage/outcome.json"
    ))
    .unwrap();
    outcome["target"] = json!(target);
    fs::write(
        root.join("outcome.json"),
        serde_json::to_vec(&outcome).unwrap(),
    )
    .unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "outcome",
            "--customer",
            "customer-usage-1",
            "--source",
            "urn:example:usage-work",
            "outcome.json",
        ],
        0,
    );
    let discounted = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-usage-1",
        ],
        0,
    );
    assert_eq!(discounted["net_atoms"], "15000000000000");

    let mut correction: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/billing/usage/correction.json"
    ))
    .unwrap();
    correction["target"] = json!(target);
    fs::write(
        root.join("correction.json"),
        serde_json::to_vec(&correction).unwrap(),
    )
    .unwrap();
    run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "correct",
            "--customer",
            "customer-usage-1",
            "--source",
            "urn:example:usage-work",
            "correction.json",
        ],
        0,
    );
    let corrected = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "statement",
            "--customer",
            "customer-usage-1",
        ],
        0,
    );
    assert_eq!(corrected["net_atoms"], "25000000000000");
    let snapshot = corrected["snapshot_hash"].as_str().unwrap();
    let export = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "export-csv",
            "--customer",
            "customer-usage-1",
            "--snapshot",
            snapshot,
            "--mapping",
            "mapping.json",
            "--output",
            "usage.csv",
        ],
        0,
    );
    assert_eq!(export["schema"], "ledger-finance-export/3");
    assert_eq!(export["scale"], 18);
    assert_eq!(export["net_atoms"], "25000000000000");
    let csv = fs::read_to_string(root.join("usage.csv")).unwrap();
    assert!(csv.contains("\"USD\",\"18\",\"increase\",\"25000000000000\""));
    assert!(csv.contains("\"USD\",\"18\",\"decrease\",\"-10000000000000\""));
}

#[test]
fn mixed_fixed_and_usage_history_exports_legacy_records_at_exact_scale_18() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/billing-cli-tests");
    fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let legacy_setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
    fs::write(
        root.join("setup.json"),
        serde_json::to_vec(&legacy_setup).unwrap(),
    )
    .unwrap();
    let mut usage_setup: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/usage/setup.json"))
            .unwrap();
    usage_setup["store_id"] = legacy_setup["store_id"].clone();
    usage_setup["operator"] = legacy_setup["operator"].clone();
    usage_setup["customer"] = legacy_setup["customer"].clone();
    usage_setup["source"] = json!("urn:example:usage-work");
    usage_setup["agreement"] = json!("agreement-usage");
    usage_setup["binding"] = json!("binding-usage");
    usage_setup["outcome_policy"]["families"][0]["binding_id"] = json!("binding-usage");
    usage_setup["outcome_policy"]["families"][0]["source"] = json!("urn:example:usage-work");
    usage_setup["outcome_policy"]["families"][0]["correction_source"] =
        json!("urn:example:usage-work");
    usage_setup["outcome_policy"]["limits"][0]["binding_id"] = json!("binding-usage");
    run(
        root,
        &["billing", "init", "store", "--setup", "setup.json"],
        0,
    );
    let registration = json!({
        "schema":"ledger-billing-registration/2",
        "customer":"customer-1",
        "source":"urn:example:usage-work",
        "change_id":"register-usage-source",
        "expected_revision":"0",
        "effective_at":"2026-09-01T00:00:00.000001Z",
        "setup":usage_setup
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
            "customer-1",
            "--source",
            "urn:example:usage-work",
            "registration.json",
        ],
        0,
    );
    fs::write(
        root.join("fixed-event.json"),
        include_bytes!("../../../examples/billing/event.json"),
    )
    .unwrap();
    let mut usage_event: Value =
        serde_json::from_slice(include_bytes!("../../../examples/billing/usage/event.json"))
            .unwrap();
    usage_event["customer"] = json!("customer-1");
    fs::write(
        root.join("usage-event.json"),
        serde_json::to_vec(&usage_event).unwrap(),
    )
    .unwrap();
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
            "fixed-event.json",
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
            "urn:example:usage-work",
            "usage-event.json",
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
    assert_eq!(statement["schema"], "ledger-billing-statement/3");
    assert_eq!(statement["scale"], 18);
    assert_eq!(statement["entries"].as_array().unwrap().len(), 2);
    assert_eq!(statement["entries"][0]["net_atoms"], "2500000000000000000");
    assert_eq!(statement["entries"][1]["net_atoms"], "25000000000000");
    assert_eq!(statement["net_atoms"], "2500025000000000000");

    let mapping = json!({
        "schema":"ledger-finance-mapping/1",
        "accounts":{"customer-1":"customer-account","example-company":"merchant-account"}
    });
    fs::write(
        root.join("mapping.json"),
        serde_json::to_vec(&mapping).unwrap(),
    )
    .unwrap();
    let exported = run(
        root,
        &[
            "billing",
            "--directory",
            "store",
            "export-csv",
            "--customer",
            "customer-1",
            "--snapshot",
            statement["snapshot_hash"].as_str().unwrap(),
            "--mapping",
            "mapping.json",
            "--output",
            "mixed.csv",
        ],
        0,
    );
    assert_eq!(exported["schema"], "ledger-finance-export/3");
    assert_eq!(exported["net_atoms"], "2500025000000000000");
    let csv = fs::read_to_string(root.join("mixed.csv")).unwrap();
    assert!(csv.contains("\"USD\",\"18\",\"increase\",\"2500000000000000000\""));
    assert!(csv.contains("\"USD\",\"18\",\"increase\",\"25000000000000\""));
}
