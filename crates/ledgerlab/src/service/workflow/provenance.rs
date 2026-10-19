//! PUB-BUILD-01 closed metadata and stable local executable observation.
use super::*;
use crate::store::sqlite::workflow::Document;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata},
    io::Read,
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    base_commit: String,
    state: String,
    inventory_digest: String,
    file_count: u32,
    total_bytes: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    package_name: String,
    package_version: String,
    build_profile: String,
    target: String,
    features: Vec<String>,
    rustc_version: String,
    cargo_version: String,
    rustflags: Vec<String>,
    cargo_lock_digest: String,
    compiler_ref: crate::store::sqlite::workflow::RecordRef,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    schema: String,
    source: Source,
    recipe: Recipe,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Executable {
    sha256: String,
    bytes: String,
    observation: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    build_inputs: Inputs,
    executable: Executable,
}
fn hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn counter(s: &str) -> bool {
    !s.is_empty()
        && (s == "0" || !s.starts_with('0'))
        && s.bytes().all(|c| c.is_ascii_digit())
        && s.parse::<i64>().is_ok_and(|n| n >= 0)
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn validate_inputs(v: Value) -> Result<Inputs> {
    require(
        records::bytes(&v)?.len() <= 65536,
        "BUILD_IDENTITY_REQUIRED",
    )?;
    let inputs: Inputs =
        serde_json::from_value(v).map_err(|_| reject("BUILD_IDENTITY_REQUIRED"))?;
    let s = &inputs.source;
    let r = &inputs.recipe;
    require(
        inputs.schema == "ledger-workflow-build-inputs/1"
            && hex(&s.base_commit, 40)
            && matches!(s.state.as_str(), "clean_commit" | "worktree_snapshot")
            && request::valid_digest(&s.inventory_digest)
            && (1..=8192).contains(&s.file_count)
            && counter(&s.total_bytes),
        "BUILD_IDENTITY_REQUIRED",
    )?;
    require(
        [
            &r.package_name,
            &r.package_version,
            &r.build_profile,
            &r.target,
        ]
        .iter()
        .all(|s| text(s, 256))
            && text(&r.rustc_version, 4096)
            && text(&r.cargo_version, 4096)
            && r.features.len() <= 128
            && r.rustflags.len() <= 128
            && r.features.iter().all(|s| text(s, 256) && s.is_ascii())
            && r.rustflags.iter().all(|s| text(s, 4096))
            && request::valid_digest(&r.cargo_lock_digest)
            && r.compiler_ref.kind == "workflow-compiler-descriptor"
            && request::valid_reference(&r.compiler_ref),
        "BUILD_IDENTITY_REQUIRED",
    )?;
    let mut features = r.features.clone();
    ledgerlab_core::canonical::sort_set(&mut features)
        .map_err(|_| reject("BUILD_IDENTITY_REQUIRED"))?;
    require(features == r.features, "BUILD_IDENTITY_REQUIRED")?;
    Ok(inputs)
}
pub(super) struct VerifiedBuildProvenance {
    pub(super) projection: Value,
    pub(super) manifest: Document,
}
impl VerifiedBuildProvenance {
    /// The caller supplies only the code-owned embedded factory's output.
    pub(super) fn capture(
        embedded: Value,
        compiler_ref: &ledgerlab_core::policy::workflow::DefinitionRef,
    ) -> Result<Self> {
        let inputs = validate_inputs(embedded)?;
        require(
            serde_json::to_value(&inputs.recipe.compiler_ref)
                .map_err(|_| ServiceError::IntegrityFailure)?
                == serde_json::to_value(compiler_ref)
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            "BUILD_IDENTITY_REQUIRED",
        )?;
        let path = std::env::current_exe().map_err(|_| ServiceError::Unavailable)?;
        let (sha256, len) = observe_file(&path)?;
        let manifest = Manifest {
            schema: "ledger-workflow-build-manifest/1".into(),
            build_inputs: inputs,
            executable: Executable {
                sha256,
                bytes: len.to_string(),
                observation: "stable-current-executable-file/1".into(),
            },
        };
        let body = serde_json::to_value(manifest).map_err(|_| ServiceError::IntegrityFailure)?;
        let projection = projection(&body)?;
        let manifest = records::source_document("workflow-build-manifest", body)?;
        Ok(Self {
            projection,
            manifest,
        })
    }
}
pub(super) fn projection(body: &Value) -> Result<Value> {
    require(
        records::bytes(body)?.len() <= 65536,
        "BUILD_IDENTITY_REQUIRED",
    )?;
    let manifest: Manifest =
        serde_json::from_value(body.clone()).map_err(|_| reject("BUILD_IDENTITY_REQUIRED"))?;
    validate_inputs(
        serde_json::to_value(&manifest.build_inputs).map_err(|_| ServiceError::IntegrityFailure)?,
    )?;
    require(
        manifest.schema == "ledger-workflow-build-manifest/1"
            && hex(&manifest.executable.sha256, 64)
            && counter(&manifest.executable.bytes)
            && manifest.executable.observation == "stable-current-executable-file/1",
        "BUILD_IDENTITY_REQUIRED",
    )?;
    Ok(
        serde_json::json!({"source_sha":manifest.build_inputs.source.base_commit,"binary_sha256":manifest.executable.sha256,"build_manifest_digest":format!("sha256:{}",records::raw_hash(&records::bytes(body)?))}),
    )
}
fn same(a: &Metadata, b: &Metadata) -> bool {
    let common =
        a.is_file() && b.is_file() && a.len() == b.len() && a.modified().ok() == b.modified().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        common
            && a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        let _ = common;
        false
    }
}
fn observe_file(path: &Path) -> Result<(String, u64)> {
    observe_file_checked(
        path,
        |before| {
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::fs::MetadataExt;
                let image =
                    std::fs::metadata("/proc/self/exe").map_err(|_| ServiceError::Unavailable)?;
                require(
                    (image.dev(), image.ino()) == (before.dev(), before.ino()),
                    "BUILD_IDENTITY_REQUIRED",
                )?;
            }
            #[cfg(not(target_os = "linux"))]
            let _ = before;
            Ok(())
        },
        || {},
    )
}
// Private deterministic observation seam. Production always supplies the
// hardcoded current-image check above and no mutation callback.
fn observe_file_checked(
    path: &Path,
    identity: impl FnOnce(&Metadata) -> Result<()>,
    after_read: impl FnOnce(),
) -> Result<(String, u64)> {
    let before_path = std::fs::symlink_metadata(path).map_err(|_| ServiceError::Unavailable)?;
    require(
        before_path.is_file() && !before_path.file_type().is_symlink(),
        "BUILD_IDENTITY_REQUIRED",
    )?;
    let mut file = File::open(path).map_err(|_| ServiceError::Unavailable)?;
    let before = file.metadata().map_err(|_| ServiceError::Unavailable)?;
    require(
        same(&before_path, &before) && before.len() <= i64::MAX as u64,
        "BUILD_IDENTITY_REQUIRED",
    )?;
    identity(&before)?;
    let mut hash = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| ServiceError::Unavailable)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or_else(|| reject("BUILD_IDENTITY_REQUIRED"))?;
        require(count <= before.len(), "BUILD_IDENTITY_REQUIRED")?;
        hash.update(&buffer[..n]);
    }
    after_read();
    let after = file.metadata().map_err(|_| ServiceError::Unavailable)?;
    let after_path = std::fs::symlink_metadata(path).map_err(|_| ServiceError::Unavailable)?;
    require(
        count == before.len()
            && same(&before, &after)
            && same(&after, &after_path)
            && !after_path.file_type().is_symlink(),
        "BUILD_IDENTITY_REQUIRED",
    )?;
    Ok((ledgerlab_core::canonical::hex(&hash.finalize()), count))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn workflow_executable_observation_detects_changed_replaced_and_symlinked_file() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image");
        let body = b"private observation seam bytes";
        std::fs::write(&path, body).unwrap();
        let got = observe_file_checked(&path, |_| Ok(()), || {}).unwrap();
        assert_eq!(got, (records::raw_hash(body), body.len() as u64));
        assert!(observe_file_checked(
            &path,
            |_| Ok(()),
            || std::fs::write(&path, b"changed").unwrap()
        )
        .is_err());
        std::fs::write(&path, body).unwrap();
        let replacement = dir.path().join("replacement");
        std::fs::write(&replacement, body).unwrap();
        assert!(observe_file_checked(
            &path,
            |_| Ok(()),
            || std::fs::rename(&replacement, &path).unwrap()
        )
        .is_err());
        std::fs::write(&path, body).unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, body).unwrap();
        assert!(observe_file_checked(
            &path,
            |_| Ok(()),
            || {
                std::fs::remove_file(&path).unwrap();
                symlink(&target, &path).unwrap();
            }
        )
        .is_err());
        assert!(observe_file_checked(&path, |_| Ok(()), || {}).is_err());
        // The actual entry uses current_exe plus the hardcoded platform identity
        // check; this temporary-file seam claims no executable qualification.
    }
}
