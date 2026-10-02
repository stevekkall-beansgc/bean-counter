use std::{env, fs, path::Path, process::Command};
#[test]
fn installed_finance_workflow_reconciles_and_refuses_unsafe_outputs() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let parent = repo.join("work/finance-cli-tests");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    let ledger = fs::canonicalize(
        env::var_os("LEDGER_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_ledger").into()),
    )
    .expect("configured ledger binary must exist");
    let result = Command::new("python3")
        .arg(repo.join("scripts/demo-finance.py"))
        .arg("--ledger")
        .arg(ledger)
        .args(["--checks", "--output"])
        .arg(root.path().join("example"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
