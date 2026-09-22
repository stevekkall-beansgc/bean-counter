//! Independent arithmetic/assertion checks; not store conformance evidence.
use std::path::Path;
use std::process::Command;

#[test]
fn phase4_independent_oracle() {
    let output = Command::new("python3")
        .args(["-B", "-m", "unittest", "-v", "test_reference"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/phase4"))
        .output()
        .expect("start independent Phase 4 oracle");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
