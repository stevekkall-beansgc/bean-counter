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

#[derive(Serialize, Deserialize, PartialEq, Eq)]
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
// Build-owned metadata only. These are the three existing exact profiles in
// the integrated Stage/Component/A-B packet, not a new compiler capability.
const CATALOG_SCHEMA: &str = "ledger-workflow-build-input-catalog/1";
const CATALOG_SCHEMA_V2: &str = "ledger-workflow-build-input-catalog/2";
const CATALOG_COMPILERS: [&str; 3] = [
    "1aa64de36f29072d1b850cfb52d30e94623bf1a66048f096383452d832b47613",
    "5c581b238a4387c7de3afdcbeccc6d7c19c1ec63227a8c58a4dd665dad9558bc",
    "f38dc832f70070165a5bbba7502a9599c00ecc483ddd26b9063b9e045f4e8e6b",
];
const CONDITIONAL_COMPILER: &str =
    "f7896c1fe7e1ad3c50ecd483286ffe200400f6a2bbd404ecedf33539af12607e";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema: String,
    entries: Vec<Value>,
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
fn same_build(a: &Inputs, b: &Inputs) -> bool {
    let (a_recipe, b_recipe) = (&a.recipe, &b.recipe);
    a.source == b.source
        && a_recipe.package_name == b_recipe.package_name
        && a_recipe.package_version == b_recipe.package_version
        && a_recipe.build_profile == b_recipe.build_profile
        && a_recipe.target == b_recipe.target
        && a_recipe.features == b_recipe.features
        && a_recipe.rustc_version == b_recipe.rustc_version
        && a_recipe.cargo_version == b_recipe.cargo_version
        && a_recipe.rustflags == b_recipe.rustflags
        && a_recipe.cargo_lock_digest == b_recipe.cargo_lock_digest
}
fn matches_compiler(
    inputs: &Inputs,
    compiler_ref: &ledgerlab_core::policy::workflow::DefinitionRef,
) -> bool {
    let reference = &inputs.recipe.compiler_ref;
    reference.kind == compiler_ref.kind()
        && reference.id == compiler_ref.id()
        && reference.digest == compiler_ref.digest()
}
/// Select only from complete compile-embedded entries. The caller's reference
/// comes from the already authenticated builtin compiler, not request metadata.
fn select_inputs(
    embedded: Value,
    compiler_ref: &ledgerlab_core::policy::workflow::DefinitionRef,
) -> Result<Inputs> {
    if !matches!(
        embedded.get("schema").and_then(Value::as_str),
        Some(CATALOG_SCHEMA | CATALOG_SCHEMA_V2)
    ) {
        let inputs = validate_inputs(embedded)?;
        require(
            matches_compiler(&inputs, compiler_ref),
            "BUILD_IDENTITY_REQUIRED",
        )?;
        return Ok(inputs);
    }
    require(
        records::bytes(&embedded)?.len() <= 65536,
        "BUILD_IDENTITY_REQUIRED",
    )?;
    let catalog: Catalog =
        serde_json::from_value(embedded).map_err(|_| reject("BUILD_IDENTITY_REQUIRED"))?;
    let expected: Vec<&str> = match catalog.schema.as_str() {
        CATALOG_SCHEMA => CATALOG_COMPILERS.to_vec(),
        CATALOG_SCHEMA_V2 => CATALOG_COMPILERS
            .into_iter()
            .chain([CONDITIONAL_COMPILER])
            .collect(),
        _ => return Err(reject("BUILD_IDENTITY_REQUIRED")),
    };
    require(
        catalog.entries.len() == expected.len(),
        "BUILD_IDENTITY_REQUIRED",
    )?;
    let mut common = None;
    let mut selected = None;
    // Fixed fullRef order rejects duplicates, unknown profiles, omissions and
    // reordering. Validate every entry, including entries not selected here.
    for (value, digest) in catalog.entries.into_iter().zip(expected) {
        let inputs = validate_inputs(value)?;
        require(
            inputs.recipe.compiler_ref.id == format!("wdoc_{digest}")
                && inputs.recipe.compiler_ref.digest == format!("sha256:{digest}"),
            "BUILD_IDENTITY_REQUIRED",
        )?;
        if let Some(first) = &common {
            require(same_build(first, &inputs), "BUILD_IDENTITY_REQUIRED")?;
        }
        if matches_compiler(&inputs, compiler_ref) {
            selected = Some(inputs);
        } else if common.is_none() {
            common = Some(inputs);
        }
    }
    // If the first entry was selected it still must agree with the others.
    if let (Some(first), Some(chosen)) = (&common, &selected) {
        require(same_build(first, chosen), "BUILD_IDENTITY_REQUIRED")?;
    }
    selected.ok_or_else(|| reject("BUILD_IDENTITY_REQUIRED"))
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
        let inputs = select_inputs(embedded, compiler_ref)?;
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

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use ledgerlab_core::policy::workflow::DefinitionRef;
    use serde_json::json;

    fn expected() -> Value {
        ledgerlab_core::canonical::parse(include_bytes!(
            "../../../../../contracts/candidates/full-suite-v1/core/build-provenance-expected.json"
        ))
        .unwrap()
    }
    // These tests exercise metadata selection, not executable authorization.
    // Production supplies only the already authenticated builtin's reference.
    fn reference(value: &Value) -> DefinitionRef {
        DefinitionRef::new(
            value["kind"].as_str().unwrap(),
            value["id"].as_str().unwrap(),
            value["digest"].as_str().unwrap(),
        )
        .unwrap()
    }
    fn catalog() -> Value {
        let original = expected()["build_inputs"]["body"].clone();
        let entries: Vec<_> = CATALOG_COMPILERS
            .into_iter()
            .map(|digest| {
                let mut entry = original.clone();
                entry["recipe"]["compiler_ref"] = json!({
                    "kind": "workflow-compiler-descriptor",
                    "id": format!("wdoc_{digest}"),
                    "digest": format!("sha256:{digest}"),
                });
                entry
            })
            .collect();
        json!({"schema": CATALOG_SCHEMA, "entries": entries})
    }
    #[test]
    fn catalog_v2_selects_conditional_and_rejects_mixed_versions() {
        let old = catalog();
        let mut v2 = old.clone();
        v2["schema"] = json!(CATALOG_SCHEMA_V2);
        let mut conditional = v2["entries"][0].clone();
        conditional["recipe"]["compiler_ref"] = json!({
            "kind": "workflow-compiler-descriptor",
            "id": format!("wdoc_{CONDITIONAL_COMPILER}"),
            "digest": format!("sha256:{CONDITIONAL_COMPILER}"),
        });
        v2["entries"]
            .as_array_mut()
            .unwrap()
            .push(conditional.clone());
        for entry in v2["entries"].as_array().unwrap() {
            assert_eq!(
                serde_json::to_value(
                    select_inputs(v2.clone(), &reference(&entry["recipe"]["compiler_ref"]))
                        .unwrap()
                )
                .unwrap(),
                *entry
            );
        }
        let conditional_ref = reference(&conditional["recipe"]["compiler_ref"]);
        assert!(select_inputs(old.clone(), &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["schema"] = json!(CATALOG_SCHEMA);
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"].as_array_mut().unwrap().swap(2, 3);
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"][1]["source"]["base_commit"] = json!("a".repeat(40));
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"].as_array_mut().unwrap().pop();
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"]
            .as_array_mut()
            .unwrap()
            .push(conditional.clone());
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"][2] = bad["entries"][1].clone();
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"][1]["recipe"]["target"] = json!("other-target");
        assert!(select_inputs(bad, &conditional_ref).is_err());
        let mut bad = v2.clone();
        bad["entries"][1]["unexpected"] = json!(true);
        assert!(select_inputs(bad, &conditional_ref).is_err());
    }
    #[test]
    fn workflow_build_catalog_preserves_single_profile_and_closed_selection() {
        let expected = expected();
        let original = &expected["build_inputs"]["body"];
        let old_ref = reference(&original["recipe"]["compiler_ref"]);
        let selected = select_inputs(original.clone(), &old_ref).unwrap();
        assert_eq!(serde_json::to_value(selected).unwrap(), *original);
        let saved = expected["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["name"] == "build-manifest")
            .unwrap();
        let retained: Manifest = serde_json::from_value(saved["body"].clone()).unwrap();
        let retained = serde_json::to_value(retained).unwrap();
        assert_eq!(
            records::bytes(&retained).unwrap(),
            saved["canonical_utf8"].as_str().unwrap().as_bytes()
        );
        assert_eq!(projection(&retained).unwrap(), expected["projection"]);
        let document = records::source_document("workflow-build-manifest", retained).unwrap();
        assert_eq!(
            serde_json::to_value(document.reference).unwrap(),
            saved["ref"]
        );

        let catalog = catalog();
        for entry in catalog["entries"].as_array().unwrap() {
            let selected = select_inputs(
                catalog.clone(),
                &reference(&entry["recipe"]["compiler_ref"]),
            )
            .unwrap();
            assert_eq!(serde_json::to_value(selected).unwrap(), *entry);
        }
        assert!(select_inputs(catalog.clone(), &old_ref).is_err());
        assert!(select_inputs(
            original.clone(),
            &reference(&catalog["entries"][0]["recipe"]["compiler_ref"])
        )
        .is_err());
    }
    #[test]
    fn workflow_build_catalog_rejects_mixed_unknown_and_malformed_entries() {
        let good = catalog();
        let selected = reference(&good["entries"][0]["recipe"]["compiler_ref"]);
        let mut bad = Vec::new();
        let mut value = good.clone();
        value["entries"].as_array_mut().unwrap().pop();
        bad.push(value);
        let mut value = good.clone();
        value["entries"][2] = value["entries"][1].clone();
        bad.push(value);
        let mut value = good.clone();
        value["entries"].as_array_mut().unwrap().swap(0, 1);
        bad.push(value);
        let mut value = good.clone();
        value["entries"][2]["recipe"]["compiler_ref"] =
            expected()["build_inputs"]["body"]["recipe"]["compiler_ref"].clone();
        bad.push(value);
        let mut value = good.clone();
        value["entries"][2]["source"]["base_commit"] = json!("a".repeat(40));
        bad.push(value);
        let mut value = good.clone();
        value["entries"][0]["recipe"]["target"] = json!("different-target");
        bad.push(value);
        let mut value = good.clone();
        value["entries"][1]["recipe"]["features"] = json!(["different-feature"]);
        bad.push(value);
        let mut value = good.clone();
        value["entries"][2]["unexpected"] = json!(true);
        bad.push(value);
        let mut value = good.clone();
        value["unexpected"] = json!(true);
        bad.push(value);
        let mut value = good.clone();
        value["schema"] = json!("ledger-workflow-build-input-catalog/2");
        bad.push(value);
        let mut value = good;
        // Each entry remains within its individual bound; the whole catalog
        // still must obey the unchanged 64 KiB embedded-metadata bound.
        for entry in value["entries"].as_array_mut().unwrap() {
            entry["recipe"]["rustflags"] = json!(vec!["x".repeat(4000); 6]);
        }
        bad.push(value);
        for value in bad {
            assert!(
                matches!(select_inputs(value, &selected), Err(ServiceError::Rejection(code)) if code == "BUILD_IDENTITY_REQUIRED")
            );
        }
    }
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
