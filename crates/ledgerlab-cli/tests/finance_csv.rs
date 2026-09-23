use std::{fs, path::Path, process::Command};
#[test]
fn installed_finance_workflow_reconciles_and_refuses_unsafe_outputs() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let parent = repo.join("work/finance-cli-tests");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    let result = Command::new("python3")
        .arg(repo.join("scripts/demo-finance.py"))
        .args([
            "--ledger",
            env!("CARGO_BIN_EXE_ledger"),
            "--checks",
            "--output",
        ])
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
