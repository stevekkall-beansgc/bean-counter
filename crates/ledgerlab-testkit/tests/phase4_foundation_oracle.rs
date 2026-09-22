//! Supplier-foundation observer sensitivity; actual adapter evidence is separate.
use std::{path::Path, process::Command};

#[test]
fn supplier_foundation_independent_observer() {
    let output = Command::new("python3")
        .args(["-B", "-m", "unittest", "-v", "test_foundation"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/phase4"))
        .output()
        .expect("start independent supplier foundation observer");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
