#!/usr/bin/env python3
"""Negative archive-security tests for verify-native-package.py, including -O mode."""

import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import copy

VERIFY = Path(__file__).with_name("verify-native-package.py")
ROOT = "bean-counter-v0.9.2-x86_64-unknown-linux-gnu"
ARCHIVE_NAME = ROOT + ".tar.gz"
COMMIT = "a" * 40
TREE = "b" * 40
SQLITE_COMMENT = "source_id=2026-03-13 10:38:09 abcdef; sqlite3.c SHA-256=" + "c" * 64


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fixture(extra_members=(), duplicate_manifest_key=False, mutate=None):
    files = {
        "ledger": b"synthetic executable payload",
        "BUILD-INFO.txt": (f"source_commit={COMMIT}\nsource_tree={TREE}\nsource_clean=true\nversion=0.9.2\n"
                           "target=x86_64-unknown-linux-gnu\nbuild_origin=local\nrunner_environment=local\n"
                           "binary_sha256=" + digest(b"synthetic executable payload") + "\n"
                           "sqlite_version=3.51.3\nsqlite_source_id=2026-03-13 10:38:09 abcdef\n"
                           "sqlite_amalgamation_sha256=" + "c" * 64 + "\n").encode(),
        "BUILD-LINKAGE.txt": b"synthetic runtime linkage\n",
    }
    sbom = {"spdxVersion": "SPDX-2.3", "packages": [{"name": "SQLite", "versionInfo": "3.51.3",
            "comment": SQLITE_COMMENT}] , "documentNamespace": f"urn:test:/{COMMIT}/x86_64-unknown-linux-gnu"}
    prov = {"schema": "bean-counter-native-build-provenance/3", "source_commit": COMMIT, "source_tree": TREE,
            "source_clean": True, "version": "0.9.2", "target": "x86_64-unknown-linux-gnu",
            "binary_sha256": digest(files["ledger"]),
            "sqlite": {"version": "3.51.3", "source_id": "2026-03-13 10:38:09 abcdef", "amalgamation_sha256": "c" * 64}}
    files["SBOM.spdx.json"] = json.dumps(sbom).encode()
    files["PROVENANCE.json"] = json.dumps(prov).encode()
    manifest = {"schema": "bean-counter-native-artifact/3", "source_commit": COMMIT, "source_tree": TREE,
                "source_clean": True, "version": "0.9.2", "target": "x86_64-unknown-linux-gnu",
                "binary_sha256": digest(files["ledger"]), "files": {name: digest(data) for name, data in files.items()}}
    if duplicate_manifest_key:
        files["MANIFEST.json"] = b'{"schema":"bean-counter-native-artifact/3","schema":"bean-counter-native-artifact/3"}'
    else:
        files["MANIFEST.json"] = json.dumps(manifest).encode()
    if mutate:
        mutate(files, manifest)
        if not duplicate_manifest_key:
            files["MANIFEST.json"] = json.dumps(manifest).encode()
    return files, extra_members


def write_case(directory, case_name, *, extra_members=(), duplicate_manifest_key=False, mutate=None):
    directory = directory / case_name
    directory.mkdir()
    archive_path = directory / ARCHIVE_NAME
    root_name = ARCHIVE_NAME[:-len(".tar.gz")]
    files, extras = fixture(extra_members, duplicate_manifest_key, mutate)
    with tarfile.open(archive_path, "w:gz", format=tarfile.PAX_FORMAT) as archive:
        root_info = tarfile.TarInfo(root_name)
        root_info.type = tarfile.DIRTYPE
        root_info.mode = 0o700
        archive.addfile(root_info)
        for name, data in files.items():
            info = tarfile.TarInfo(root_name + "/" + name)
            info.size = len(data)
            info.mode = 0o700 if name == "ledger" else 0o600
            archive.addfile(info, __import__("io").BytesIO(data))
        for member in extras:
            archive.addfile(member[0], member[1])
    sums = directory / (case_name + ".sums")
    sums.write_text(f"{digest(archive_path.read_bytes())}  {archive_path.name}\n")
    return archive_path, sums


def rejected(directory, name, diagnostic, **kwargs):
    archive, sums = write_case(directory, name, **kwargs)
    result = subprocess.run([sys.executable, "-O", str(VERIFY), str(archive), str(sums), COMMIT, "--version", "0.9.2"],
                            text=True, capture_output=True)
    if result.returncode == 0 or diagnostic not in result.stderr:
        raise RuntimeError(f"negative test {name} did not reach {diagnostic!r}: exit={result.returncode}, stderr={result.stderr!r}")


def check_committed_example_isolation(directory):
    spec = importlib.util.spec_from_file_location("native_packager", VERIFY.with_name("package-native.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    repo = directory / "source"
    repo.mkdir()
    def git(*args):
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Synthetic Test",
                        "-c", "user.email=test@example.invalid", *args], check=True, capture_output=True)
    git("init", "-q")
    examples = repo / "examples" / "billing"
    examples.mkdir(parents=True)
    (examples / "setup.json").write_text("synthetic fixture")
    (repo / ".gitignore").write_text(".env\n*.db\n")
    git("add", ".")
    git("commit", "-qm", "synthetic fixture")
    (examples / ".env").write_text("SYNTHETIC PRIVATE SENTINEL")
    (examples / "private.db").write_text("SYNTHETIC PRIVATE SENTINEL")
    output = directory / "committed-only"
    module.copy_committed_examples(repo, "HEAD", "billing", output)
    if sorted(path.name for path in output.iterdir()) != ["setup.json"]:
        raise RuntimeError("packager included ignored operational data")
    (examples / "link").symlink_to("/synthetic-outside-target")
    git("add", "examples")
    git("commit", "-qm", "synthetic tracked link")
    try:
        module.copy_committed_examples(repo, "HEAD", "billing", directory / "reject-link")
    except SystemExit as error:
        if "not a committed regular file" not in str(error):
            raise
    else:
        raise RuntimeError("packager accepted a tracked example symlink")


def check_workflow_build_identity(directory):
    spec = importlib.util.spec_from_file_location("native_packager", VERIFY.with_name("package-native.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    capture = directory / "workflow-capture"
    capture.mkdir()
    inventory = {"schema": "ledger-build-source-inventory/1", "recipe": "bean-counter-build-inputs/1",
                 "entries": [{"path": "synthetic.rs", "bytes": "4", "sha256": digest(b"test")}]}
    canonical = module.build_workflow.canonical
    inventory_digest = "sha256:" + digest(b"bean-counter/build-source-inventory/1\0" + canonical(inventory).encode())
    descriptor = json.loads((VERIFY.parent.parent / "contracts/candidates/full-suite-v1/core/runtime-support/local-conditional-compiler8-data.json").read_bytes())
    compiler_digest = "sha256:" + digest(b"bean-counter/workflow-document/1\0" + canonical([descriptor["schema"], descriptor]).encode())
    inputs = {"source": {"base_commit": COMMIT, "state": "clean_commit", "inventory_digest": inventory_digest},
              "recipe": {"package_name": "ledgerlab-cli", "package_version": "0.11.0", "build_profile": "release",
                         "target": "x86_64-unknown-linux-gnu", "features": [],
                         "compiler_ref": {"kind": "workflow-compiler-descriptor", "id": "wdoc_" + compiler_digest[7:], "digest": compiler_digest}}}
    path = capture / "build-inputs.json"
    path.write_text(canonical(inputs))
    (capture / "inventory.json").write_text(canonical(inventory))
    observed = module.workflow_build_evidence(path, COMMIT, "0.11.0", "x86_64-unknown-linux-gnu")
    if observed != (path.read_bytes(), (capture / "inventory.json").read_bytes()):
        raise RuntimeError("workflow package changed captured evidence bytes")
    old_descriptor = json.loads((VERIFY.parent.parent / "contracts/candidates/full-suite-v1/core/runtime-support/local-compiler8-data.json").read_bytes())
    old_digest = "sha256:" + digest(b"bean-counter/workflow-document/1\0" + canonical([old_descriptor["schema"], old_descriptor]).encode())
    old_ref = {"kind": "workflow-compiler-descriptor", "id": "wdoc_" + old_digest[7:], "digest": old_digest}
    cases = [("source", "base_commit", "c" * 40), ("source", "state", "worktree_snapshot"),
             ("source", "inventory_digest", "sha256:" + "0" * 64),
             ("recipe", "package_version", "0.9.7"), ("recipe", "target", "aarch64-apple-darwin"),
             ("recipe", "compiler_ref", old_ref),
             ("recipe", "build_profile", "test"), ("recipe", "features", ["unexpected"]),
             ("recipe", "compiler_ref", {"kind": "workflow-compiler-descriptor", "id": "wrong", "digest": "sha256:" + "0" * 64})]
    for section, field, value in cases:
        wrong = copy.deepcopy(inputs)
        wrong[section][field] = value
        path.write_text(canonical(wrong))
        try:
            module.workflow_build_evidence(path, COMMIT, "0.11.0", "x86_64-unknown-linux-gnu")
        except SystemExit:
            continue
        raise RuntimeError(f"workflow packager accepted mismatched {section}.{field}")


def check_integrated_catalog_identity(directory):
    spec = importlib.util.spec_from_file_location("native_packager_catalog", VERIFY.with_name("package-native.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    capture = directory / "integrated-capture"
    source = capture / "source"
    files = module.build_workflow.selected(VERIFY.parent.parent)
    for name, raw in files.items():
        path = source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
    inventory, inv_digest = module.build_workflow.inventory(files)
    canonical = module.build_workflow.canonical
    (capture / "inventory.json").write_text(canonical(inventory))
    base = {"schema": "ledger-workflow-build-inputs/1",
            "source": {"base_commit": COMMIT, "state": "clean_commit", "inventory_digest": inv_digest,
                       "file_count": len(files), "total_bytes": str(sum(map(len, files.values())))},
            "recipe": {"package_name": "ledgerlab-cli", "package_version": "1.0.0-alpha.1",
                       "build_profile": "release", "target": "x86_64-unknown-linux-gnu", "features": [],
                       "rustc_version": module.output("rustc", "--version"),
                       "cargo_version": module.output("cargo", "--version"), "rustflags": [],
                       "cargo_lock_digest": "sha256:" + digest(files["Cargo.lock"]), "compiler_ref": {}}}
    catalog = module.build_workflow.integrated_catalog(files, base)
    inputs = capture / "build-inputs.json"
    inputs.write_text(canonical(catalog))
    binary = capture / "target/x86_64-unknown-linux-gnu/release/ledger"
    binary.parent.mkdir(parents=True)
    binary.write_bytes(b"synthetic executable")
    qualified = capture / "qualified.json"
    qualified.write_text(json.dumps({"source_inventory_digest": inv_digest, "build_inputs_sha256": digest(inputs.read_bytes()),
        "status": "captured inputs unchanged; local build observation only", "binary_sha256": digest(binary.read_bytes())}))
    def check():
        return module.workflow_build_evidence(inputs, COMMIT, "1.0.0-alpha.1", "x86_64-unknown-linux-gnu", binary)
    if check() != (inputs.read_bytes(), (capture / "inventory.json").read_bytes()):
        raise RuntimeError("integrated catalog evidence bytes changed")
    verify_spec = importlib.util.spec_from_file_location("native_verifier_catalog", VERIFY)
    verifier = importlib.util.module_from_spec(verify_spec)
    verify_spec.loader.exec_module(verifier)
    packaged = {"ledger": binary.read_bytes(), "WORKFLOW-BUILD-INPUTS.json": inputs.read_bytes(),
                "WORKFLOW-SOURCE-INVENTORY.json": (capture / "inventory.json").read_bytes(),
                "WORKFLOW-CAPTURE-QUALIFIED.json": qualified.read_bytes()}
    build = {"workflow_build_inputs_sha256": digest(packaged["WORKFLOW-BUILD-INPUTS.json"]),
             "workflow_source_inventory_sha256": digest(packaged["WORKFLOW-SOURCE-INVENTORY.json"]),
             "workflow_capture_command": "--integrated-local-profiles", "rustc": base["recipe"]["rustc_version"],
             "cargo": base["recipe"]["cargo_version"]}
    identity = {"source_commit": COMMIT, "version": "1.0.0-alpha.1", "target": "x86_64-unknown-linux-gnu"}
    verifier.verify_catalog_evidence(packaged, build, identity)
    for label, changed in [("missing qualification", {k: v for k, v in packaged.items() if k != "WORKFLOW-CAPTURE-QUALIFIED.json"}),
                           ("changed binary", {**packaged, "ledger": b"wrong"}),
                           ("changed inventory", {**packaged, "WORKFLOW-SOURCE-INVENTORY.json": b"{}"})]:
        try:
            verifier.verify_catalog_evidence(changed, build, identity)
        except verifier.VerificationError:
            pass
        else:
            raise RuntimeError(f"archive verifier accepted {label}")
    try:
        verifier.verify_catalog_evidence(packaged, {**build, "rustc": "wrong"}, identity)
    except verifier.VerificationError:
        pass
    else:
        raise RuntimeError("archive verifier accepted mismatched toolchain")
    def reject(label):
        try:
            check()
        except SystemExit:
            return
        raise RuntimeError(f"integrated catalog accepted {label}")
    original = inputs.read_bytes()
    for label, mutation in [
        ("missing entry", lambda v: v["entries"].pop()),
        ("mixed target", lambda v: v["entries"][1]["recipe"].update(target="aarch64-apple-darwin")),
        ("extra field", lambda v: v["entries"][0].update(extra=True)),
        ("changed compiler", lambda v: v["entries"][0]["recipe"]["compiler_ref"].update(id="wrong")),
        ("wrong toolchain", lambda v: v["entries"][0]["recipe"].update(rustc_version="wrong")),
    ]:
        changed = copy.deepcopy(catalog)
        mutation(changed)
        inputs.write_text(canonical(changed))
        reject(label)
    inputs.write_bytes(b'{"schema":"x","schema":"y"}')
    reject("duplicate key")
    inputs.write_bytes(original)
    binary.write_bytes(b"different executable")
    reject("swapped binary")
    binary.write_bytes(b"synthetic executable")
    alternate = binary.with_name("alternate")
    alternate.write_bytes(binary.read_bytes())
    binary.unlink()
    binary.symlink_to(alternate.name)
    reject("symlinked executable")
    binary.unlink()
    binary.write_bytes(alternate.read_bytes())
    saved = qualified.read_bytes()
    qualified.write_text('{}')
    reject("changed qualification")
    qualified.write_bytes(saved)
    (source / "Cargo.lock").write_bytes(b"changed")
    reject("changed captured source")


def check_prerelease_archive(directory):
    global ROOT, ARCHIVE_NAME
    old_root, old_archive = ROOT, ARCHIVE_NAME
    ROOT = "bean-counter-v1.0.0-alpha.1-x86_64-unknown-linux-gnu"
    ARCHIVE_NAME = ROOT + ".tar.gz"
    try:
        def prerelease(files, manifest):
            manifest["version"] = "1.0.0-alpha.1"
            provenance = json.loads(files["PROVENANCE.json"])
            provenance["version"] = "1.0.0-alpha.1"
            files["PROVENANCE.json"] = json.dumps(provenance).encode()
            files["BUILD-INFO.txt"] = files["BUILD-INFO.txt"].replace(b"version=0.9.2", b"version=1.0.0-alpha.1")
            manifest["files"]["PROVENANCE.json"] = digest(files["PROVENANCE.json"])
            manifest["files"]["BUILD-INFO.txt"] = digest(files["BUILD-INFO.txt"])
        archive, sums = write_case(directory, "prerelease", mutate=prerelease)
        result = subprocess.run([sys.executable, "-O", str(VERIFY), str(archive), str(sums), COMMIT,
                                 "--version", "1.0.0-alpha.1"], text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(f"prerelease rejected: {result.stderr}")
        invalid = archive.with_name("bean-counter-v1.0.0-alpha.01-x86_64-unknown-linux-gnu.tar.gz")
        invalid.write_bytes(archive.read_bytes())
        invalid_sums = invalid.with_suffix(".sums")
        invalid_sums.write_text(f"{digest(invalid.read_bytes())}  {invalid.name}\n")
        result = subprocess.run([sys.executable, "-O", str(VERIFY), str(invalid), str(invalid_sums), COMMIT,
                                 "--version", "1.0.0-alpha.01"], text=True, capture_output=True)
        if result.returncode == 0 or "noncanonical prerelease version" not in result.stderr:
            raise RuntimeError(f"noncanonical prerelease not rejected: {result.stderr}")
    finally:
        ROOT, ARCHIVE_NAME = old_root, old_archive


def main():
    with tempfile.TemporaryDirectory(prefix="native-package-security-") as temporary:
        directory = Path(temporary)
        check_committed_example_isolation(directory)
        check_workflow_build_identity(directory)
        check_integrated_catalog_identity(directory)
        check_prerelease_archive(directory)
        good, sums = write_case(directory, ARCHIVE_NAME[:-len(".tar.gz")])
        positive = subprocess.run([sys.executable, "-O", str(VERIFY), str(good), str(sums), COMMIT, "--version", "0.9.2"],
                                  text=True, capture_output=True)
        if positive.returncode != 0:
            raise RuntimeError(f"optimized verifier rejected valid fixture: {positive.stderr}")
        rejected(directory, "duplicate-json-key", "duplicate JSON key", duplicate_manifest_key=True)
        rejected(directory, "mismatched-target", "manifest target disagrees", mutate=lambda _files, manifest: manifest.update(target="aarch64-apple-darwin"))
        rejected(directory, "missing-manifest-file", "manifest file set differs", mutate=lambda _files, manifest: manifest["files"].update({"absent": "d" * 64}))
        rejected(directory, "noncanonical-manifest-path", "noncanonical archive path", mutate=lambda _files, manifest: manifest["files"].update({"./ledger": digest(b"x")}))
        duplicate = tarfile.TarInfo(ROOT + "/ledger")
        duplicate.size = 1
        rejected(directory, "duplicate-member", "duplicate archive member", extra_members=[(duplicate, __import__("io").BytesIO(b"x"))])
        traversal = tarfile.TarInfo(ROOT + "/../escape")
        traversal.size = 1
        rejected(directory, "traversal", "noncanonical archive path", extra_members=[(traversal, __import__("io").BytesIO(b"x"))])
        absolute = tarfile.TarInfo("/escape")
        absolute.size = 1
        rejected(directory, "absolute-path", "absolute or empty archive path", extra_members=[(absolute, __import__("io").BytesIO(b"x"))])
        symlink = tarfile.TarInfo(ROOT + "/link")
        symlink.type = tarfile.SYMTYPE
        symlink.linkname = "ledger"
        rejected(directory, "symlink", "special archive member rejected", extra_members=[(symlink, None)])
        device = tarfile.TarInfo(ROOT + "/device")
        device.type = tarfile.CHRTYPE
        rejected(directory, "device", "special archive member rejected", extra_members=[(device, None)])
        bad_mode = tarfile.TarInfo(ROOT + "/unsafe")
        bad_mode.size = 1
        bad_mode.mode = 0o4755
        rejected(directory, "mode", "unsafe or unexpected mode", extra_members=[(bad_mode, __import__("io").BytesIO(b"x"))])
        wrong_sums = directory / "wrong.sums"
        wrong_sums.write_text("0" * 64 + "  " + good.name + "\n")
        result = subprocess.run([sys.executable, "-O", str(VERIFY), str(good), str(wrong_sums), COMMIT, "--version", "0.9.2"], capture_output=True, text=True)
        if result.returncode == 0 or "archive SHA-256 mismatch" not in result.stderr:
            raise RuntimeError(f"bad checksum did not reach checksum validation: {result.stderr!r}")
    print("native package verifier: stable and prerelease positives; archive security, committed isolation, single-profile and integrated catalog mismatch refusals passed")


if __name__ == "__main__":
    main()
