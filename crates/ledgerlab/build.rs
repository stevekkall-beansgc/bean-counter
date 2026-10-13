use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=LEDGER_WORKFLOW_BUILD_INPUTS_JSON");
    let bytes = match env::var_os("LEDGER_WORKFLOW_BUILD_INPUTS_JSON") {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", PathBuf::from(&path).display());
            let bytes = fs::read(path).expect("build-owned inputs unavailable");
            assert!(bytes.len() <= 65536, "build-owned inputs exceed bound");
            bytes
        }
        None => b"null".to_vec(),
    };
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"))
            .join("workflow-build-inputs.json"),
        bytes,
    )
    .expect("embed build-owned inputs");
}
