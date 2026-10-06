use serde_json::Value;
use std::{env, fs, path::Path, path::PathBuf, process::Command};

fn ledger_binary() -> PathBuf {
    let configured =
        env::var_os("LEDGER_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_ledger").into());
    fs::canonicalize(configured).expect("configured ledger binary must exist")
}
fn help(root: &Path, args: &[&str]) -> String {
    let out = Command::new(ledger_binary())
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
    assert!(out.stderr.is_empty(), "{args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn root_help_leads_to_current_billing_and_scopes_the_legacy_profile() {
    let root = tempfile::tempdir().unwrap();
    let text = help(root.path(), &["--help"]);
    assert_eq!(text, include_str!("snapshots/help.txt"));
    assert!(text.starts_with("Bean Counter"));
    assert!(text.find("Current billing profile").unwrap() < text.find("Legacy synthetic").unwrap());
    assert!(!text.contains("outcomes and reversals cannot"));
    for args in [
        &["-h"][..],
        &["--json", "--help"],
        &["--help", "--format", "json", "--no-color"],
        &["--config", "missing.json", "-h"],
    ] {
        assert_eq!(help(root.path(), args), text);
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn billing_command_help_routes_every_supported_command_path() {
    let root = tempfile::tempdir().unwrap();
    let paths: &[&[&str]] = &[
        &["setup"],
        &["init"],
        &["upgrade"],
        &["accept"],
        &["outcome"],
        &["correct"],
        &["agreement"],
        &["permissions"],
        &["explain"],
        &["statement"],
        &["export-csv"],
        &["activity"],
        &["close"],
        &["occurrences"],
        &["term"],
        &["term", "set"],
        &["fiscal"],
        &["fiscal", "set"],
        &["fiscal", "report"],
        &["cumulative"],
        &["cumulative", "setup"],
        &["adjustment"],
        &["adjustment", "statement"],
        &["recurrence"],
        &["recurrence", "set"],
        &["recurrence", "cancel"],
        &["occurrence"],
        &["occurrence", "accept"],
    ];
    for path in paths {
        let mut args = vec!["billing"];
        args.extend_from_slice(path);
        args.push("--help");
        let text = help(root.path(), &args);
        assert!(
            text.contains(&format!(
                "Usage: ledger billing [--directory DIR] {}",
                path.join(" ")
            )),
            "{args:?}: {text}"
        );
        assert!(!text.contains("Legacy"), "{args:?}: {text}");
        assert!(!text.contains("cannot be saved"), "{args:?}: {text}");
        args.pop();
        args.insert(0, "-h");
        assert_eq!(help(root.path(), &args), text);
    }
    let text = help(root.path(), &["billing", "--help"]);
    assert!(text.contains("billing [--directory DIR] outcome"));
    assert!(!text.contains("Legacy"));
    assert!(help(root.path(), &["billing", "correct", "-h"]).contains("correct FILE|-"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn help_accepts_supported_option_placement_and_omitted_operation_arguments() {
    let root = tempfile::tempdir().unwrap();
    let expected = help(root.path(), &["billing", "accept", "--help"]);
    for args in [
        &[
            "--json",
            "billing",
            "--directory",
            "missing",
            "accept",
            "-h",
        ][..],
        &[
            "billing",
            "--help",
            "accept",
            "--directory",
            "missing",
            "--format",
            "json",
        ],
        &[
            "billing",
            "accept",
            "--customer",
            "c",
            "--source",
            "s",
            "absent.json",
            "--help",
        ],
        &[
            "--help",
            "billing",
            "accept",
            "--directory",
            "missing",
            "--no-color",
        ],
    ] {
        assert_eq!(help(root.path(), args), expected);
    }
    for args in [
        &[
            "billing",
            "setup",
            "new-store",
            "--setup",
            "absent.json",
            "--help",
        ][..],
        &[
            "billing",
            "init",
            "new-store",
            "--setup",
            "absent.json",
            "-h",
        ],
        &[
            "billing",
            "export-csv",
            "--mapping",
            "absent.json",
            "--output",
            "new.csv",
            "-h",
        ],
        &["--config", "absent.json", "accept", "absent.json", "-h"],
    ] {
        help(root.path(), args);
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn help_does_not_inspect_an_invalid_existing_installation() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ledger.json"), b"invalid config").unwrap();
    let store = root.path().join("store");
    fs::create_dir(&store).unwrap();
    fs::write(store.join("billing.json"), b"invalid config").unwrap();
    fs::write(store.join("billing.db"), b"invalid database").unwrap();
    let before_config = fs::read(store.join("billing.json")).unwrap();
    let before_db = fs::read(store.join("billing.db")).unwrap();
    for command in [
        "upgrade",
        "accept",
        "outcome",
        "correct",
        "explain",
        "statement",
        "export-csv",
    ] {
        help(
            root.path(),
            &["billing", "--directory", "store", command, "-h"],
        );
    }
    help(root.path(), &["accept", "-h"]);
    assert_eq!(fs::read(store.join("billing.json")).unwrap(), before_config);
    assert_eq!(fs::read(store.join("billing.db")).unwrap(), before_db);
    assert_eq!(fs::read_dir(&store).unwrap().count(), 2);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn invalid_help_paths_and_global_options_have_deterministic_usage_errors() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        &["unknown", "--help"][..],
        &["--help", "unknown"],
        &["serve", "-h"],
        &["billing", "unknown", "-h"],
        &["billing", "fiscal", "unknown", "--help"],
        &["billing", "term", "unknown", "--help"],
        &["billing", "cumulative", "unknown", "-h"],
        &["billing", "adjustment", "unknown", "-h"],
        &["billing", "recurrence", "unknown", "-h"],
        &["billing", "occurrence", "unknown", "-h"],
        &["--config", "--help"],
        &["--format", "invalid", "--help"],
        &["--config", "a", "--config", "b", "-h"],
        &["--format", "text", "--format", "text", "-h"],
        &["billing", "--directory", "--help"],
        &["billing", "--directory", "a", "--directory", "b", "-h"],
        &["--config", "legacy.json", "billing", "-h"],
    ] {
        let out = Command::new(ledger_binary())
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("USAGE:"));
        // The same refusal uses the established one-object JSON error surface.
        // --format text already selects a format, so choose JSON in its place.
        let mut json_args = args.to_vec();
        if let Some(index) = json_args.iter().position(|value| *value == "text") {
            json_args[index] = "json";
        }
        if !json_args.contains(&"--json") && !json_args.contains(&"json") {
            json_args.insert(0, "--json");
        }
        let out = Command::new(ledger_binary())
            .current_dir(root.path())
            .args(json_args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stderr.is_empty());
        assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 1);
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], "ledger-cli/1");
        assert_eq!(value["status"], "error");
        assert_eq!(value["code"], "USAGE");
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn legacy_help_is_specific_to_the_generation_profile() {
    let root = tempfile::tempdir().unwrap();
    for command in ["init", "accept", "preview", "explain"] {
        let text = help(root.path(), &[command, "-h"]);
        assert!(text.contains(&format!("{command} ")));
        assert!(text.contains("Legacy synthetic Phase 1 generation profile only"));
        assert!(
            text.contains("outcomes and reversals cannot be saved or previewed in this profile")
        );
        assert!(!text.contains("Usage: ledger billing"));
    }
}
