//! Proposed semantic fixture self-tests; no Phase 2 production/store claim.
use std::path::Path;
use std::process::Command;

#[test]
fn proposed_phase2_reference_histories_and_legacy_compatibility() {
    let output = Command::new("python3")
        .arg("-B")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/phase2/test_reference.py"))
        .output()
        .expect("run independent Phase 2 reference tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
