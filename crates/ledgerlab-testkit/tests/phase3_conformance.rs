//! Offline oracle/assertion checks only; no implied real-store conformance.
use std::path::Path;
use std::process::Command;

#[test]
fn phase3_independent_assertion_sensitivity() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/phase3");
    let output = Command::new("python3")
        .args([
            "-B",
            "-m",
            "unittest",
            "-v",
            "test_conformance",
            "test_lifecycle",
        ])
        .current_dir(dir)
        .output()
        .expect("start independent Phase 3 oracle");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
