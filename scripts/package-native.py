#!/usr/bin/env python3
"""Assemble a native archive after a native locked release build."""

import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import build_workflow


def output(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def workflow_build_evidence(inputs_path, commit, version, target):
    if not inputs_path.is_file():
        raise SystemExit("workflow package requires its captured build-inputs file")
    raw = inputs_path.read_bytes()
    inputs = json.loads(raw)
    recipe = inputs["recipe"]
    inventory_raw = inputs_path.with_name("inventory.json").read_bytes()
    inventory = json.loads(inventory_raw)
    inventory_digest = "sha256:" + hashlib.sha256(
        b"bean-counter/build-source-inventory/1\0" + build_workflow.canonical(inventory).encode()).hexdigest()
    descriptor = json.loads((Path(__file__).resolve().parents[1] /
        "contracts/candidates/full-suite-v1/core/runtime-support/local-compiler8-data.json").read_bytes())
    compiler_digest = "sha256:" + hashlib.sha256(b"bean-counter/workflow-document/1\0" +
        build_workflow.canonical([descriptor["schema"], descriptor]).encode()).hexdigest()
    compiler_ref = {"kind": "workflow-compiler-descriptor", "id": "wdoc_" + compiler_digest[7:], "digest": compiler_digest}
    if (inputs["source"]["base_commit"] != commit
            or inputs["source"]["state"] != "clean_commit"
            or inputs["source"]["inventory_digest"] != inventory_digest
            or recipe["package_name"] != "ledgerlab-cli"
            or recipe["package_version"] != version
            or recipe["build_profile"] != "release"
            or recipe["target"] != target
            or recipe["compiler_ref"] != compiler_ref
            or recipe["features"] != []):
        raise SystemExit("workflow build inputs do not match this exact clean release candidate")
    return raw, inventory_raw


def sqlite_evidence(repo):
    metadata = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1", cwd=repo))
    package = next(item for item in metadata["packages"] if item["name"] == "libsqlite3-sys")
    root = Path(package["manifest_path"]).parent
    amalgamation = root / "sqlite3" / "sqlite3.c"
    source = amalgamation.read_text(encoding="utf-8")
    version_line = next(line for line in source.splitlines() if line.startswith("#define SQLITE_VERSION "))
    source_line = next(line for line in source.splitlines() if line.startswith("#define SQLITE_SOURCE_ID "))
    return {"sqlite_version": version_line.split('"', 2)[1],
            "sqlite_source_id": source_line.split('"', 2)[1],
            "sqlite_amalgamation_sha256": sha(amalgamation)}


def copy_committed_examples(repo, commit, section, destination):
    prefix = f"examples/{section}/"
    entries = subprocess.check_output(
        ["git", "ls-tree", "-rz", commit, "--", prefix], cwd=repo)
    destination.mkdir(mode=0o700)
    copied = 0
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        header, raw_path = entry.split(b"\t", 1)
        mode, kind, object_id = header.decode("ascii").split()
        path = raw_path.decode("utf-8")
        if section == "integration" and path in (prefix + "install-v0.2.1-macos.sh", prefix + "install-v0.3.0-macos.sh"):
            continue
        if mode not in ("100644", "100755") or kind != "blob":
            raise SystemExit(f"example is not a committed regular file: {path}")
        if not path.startswith(prefix):
            raise SystemExit(f"example lies outside its committed section: {path}")
        relative = Path(path[len(prefix):])
        if relative.is_absolute() or any(part in (".", "..") for part in relative.parts):
            raise SystemExit(f"noncanonical committed example path: {path}")
        target = destination / relative
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        target.write_bytes(subprocess.check_output(["git", "cat-file", "blob", object_id], cwd=repo))
        copied += 1
    if not copied:
        raise SystemExit(f"committed example section is empty: {section}")


def copy_committed_contracts(repo, commit, stage):
    entries = subprocess.check_output(
        ["git", "ls-tree", "-rz", commit, "--",
         "contracts/candidates/billing-lifecycle-m5/",
         "contracts/candidates/v2/schemas/canonical-records.schema.json"], cwd=repo)
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        header, raw_path = entry.split(b"\t", 1)
        mode, kind, object_id = header.decode("ascii").split()
        path = Path(raw_path.decode("utf-8"))
        if mode not in ("100644", "100755") or kind != "blob" or path.is_absolute() or ".." in path.parts:
            raise SystemExit(f"contract is not a committed regular file: {path}")
        target = stage / path
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        target.write_bytes(subprocess.check_output(["git", "cat-file", "blob", object_id], cwd=repo))


def main():
    if len(sys.argv) not in (4, 5) or (len(sys.argv) == 5 and sys.argv[4] != "--workflow-candidate"):
        raise SystemExit("usage: package-native.py REPO ARTIFACT_DIRECTORY TARGET [--workflow-candidate]")
    workflow_candidate = len(sys.argv) == 5
    repo, artifacts, target = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    native_targets = {("Darwin", "arm64"): "aarch64-apple-darwin",
                      ("Linux", "x86_64"): "x86_64-unknown-linux-gnu"}
    if native_targets.get((platform.system(), platform.machine())) != target:
        raise SystemExit("package target does not match a supported native build host")
    version = tomllib.loads((repo / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    commit = output("git", "rev-parse", "HEAD", cwd=repo)
    tree = output("git", "rev-parse", "HEAD^{tree}", cwd=repo)
    if output("git", "status", "--porcelain", "--untracked-files=all", cwd=repo):
        raise SystemExit("refusing to package a dirty source checkout")
    if artifacts.exists() or artifacts.is_symlink():
        raise SystemExit(f"refusing existing artifact directory: {artifacts}")
    workflow_inputs = None
    workflow_inventory = None
    if workflow_candidate:
        inputs_path = Path(os.environ.get("LEDGER_WORKFLOW_BUILD_INPUTS_JSON", ""))
        workflow_inputs, workflow_inventory = workflow_build_evidence(inputs_path, commit, version, target)
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", repo / "target")) / target / "release" / "ledger"
    if not target_dir.is_file():
        raise SystemExit(f"missing native release binary: {target_dir}")
    # Only the advertised architecture/OS wrappers are permitted to call this builder.
    expected_version = f"ledger {version} (local development)"
    actual_version = output(str(target_dir), "--version")
    if actual_version != expected_version:
        raise SystemExit(f"binary version mismatch: expected {expected_version!r}, got {actual_version!r}")
    artifacts.mkdir(mode=0o700, parents=True)
    name = f"bean-counter-v{version}-{target}"
    with tempfile.TemporaryDirectory(prefix="bean-counter-native-package-") as temp_name:
        stage = Path(temp_name) / name
        stage.mkdir(mode=0o700)
        shutil.copyfile(target_dir, stage / "ledger")
        (stage / "ledger").chmod(0o700)
        for filename in ("LICENSE", "NOTICE", "STATUS.md", "CURRENT-REQUIREMENTS.md", "WORKFLOW.md"):
            shutil.copyfile(repo / filename, stage / filename)
        (stage / "README.md").write_text(
            f"# Bean Counter native package\n\nVersion `{version}`, source `{commit}`, target `{target}`.\n\n"
            "Start with [START-HERE](START-HERE.md) for installation, first use and supported scope. "
            "Read [agreement and integration](docs/agreement-and-integration.md), "
            "[operator checks](docs/billing-operations.md) and "
            "[finance walkthrough](docs/finance-e2e.md) before adapting synthetic examples. "
            "Agent callers use [the integration guide](docs/integration-agent-guide.md).\n\n"
            "BUILD-INFO.txt, PROVENANCE.json, MANIFEST.json and SBOM.spdx.json identify these exact bytes. "
            "A locally built or CI candidate is not a published release or outside-adoption acceptance. "
            "Historical qualification and frozen contract documents retain their original dated status; "
            "[current readiness](docs/oss1-readiness.md) distinguishes current evidence and remaining work.\n\n"
            f"Repository development and historical phase notes are in the "
            f"[source README](https://github.com/stevekkall-beansgc/bean-counter/blob/{commit}/README.md).\n",
            encoding="utf-8")
        docs = stage / "docs"
        docs.mkdir(mode=0o700)
        for filename in ("billing-quickstart.md", "billing-recovery.md", "m5-qualification.md", "m5-contracts.md",
                         "finance-e2e.md", "finance-csv.md", "resources-and-costs.md", "billing-m2-cli-contract.md",
                         "compatibility.md", "billing-roadmap.md", "m8-native-package-qualification.md",
                         "billing-operations.md", "integration-agent-guide.md", "integration-capabilities.json",
                         "live-app-integration.md",
                         "oss1-readiness.md", "open-beta.md", "agreement-and-integration.md", "beana-adoption-packet.md",
                         "participant-adoption-tasks.md", "billing-cli-contract.md", "m1-current-format-qualification.md",
                         "m2-migration-qualification.md", "m2-implementation-design.md", "m3-qualification.md",
                         "m4-qualification.md", "m5-decision-register.md", "m5-architecture.md",
                         "m5-contract-traceability.md", "m5-execution-plan.md"):
            shutil.copyfile(repo / "docs" / filename, docs / filename)
        if workflow_candidate:
            shutil.copyfile(repo / "docs/local-workflow-supported-recipe.md", docs / "local-workflow-supported-recipe.md")
            shutil.copyfile(repo / "docs/workflow-candidate-testing.md", docs / "workflow-candidate-testing.md")
            (stage / "WORKFLOW-BUILD-INPUTS.json").write_bytes(workflow_inputs)
            (stage / "WORKFLOW-SOURCE-INVENTORY.json").write_bytes(workflow_inventory)
            with (stage / "README.md").open("a", encoding="utf-8") as readme:
                readme.write("\n## Unreleased supported-workflow candidate\n\nThis optional candidate material is not a release or full-registry capability claim. Read [the supported scope](docs/local-workflow-supported-recipe.md) and [the synthetic walkthrough](examples/workflow-supported/README.md). The two input-only helpers are in `helpers/`.\n")
            helpers = stage / "helpers"
            helpers.mkdir(mode=0o700)
            for helper in ("prepare_local_publication", "prepare_local_terms"):
                source = target_dir.parent / "examples" / helper
                if not source.is_file():
                    raise SystemExit(f"workflow candidate helper not built: {source}")
                shutil.copyfile(source, helpers / helper)
                (helpers / helper).chmod(0o600)
        copy_committed_contracts(repo, commit, stage)
        (stage / "release").mkdir(mode=0o700)
        shutil.copyfile(repo / "release" / "local-sqlite.md", stage / "release" / "local-sqlite.md")
        if workflow_candidate:
            shutil.copyfile(repo / "release/notes-v0.10.0.md", stage / "release/notes-v0.10.0.md")
        shutil.copyfile(repo / "START-HERE.md", stage / "START-HERE.md")
        (stage / "AGENTS.md").write_text(
            "# Integrate the installed local billing CLI\n\n"
            "Start with START-HERE.md, docs/integration-agent-guide.md and "
            "docs/integration-capabilities.json. Use docs/billing-operations.md for "
            "reconciliation and supported recovery. Capability metadata grants no authority. "
            "Do not change economic contracts, edit retained databases or retry unknown "
            "results with new identities. Repository contributor rules are separate.\n",
            encoding="utf-8")
        examples = stage / "examples"
        examples.mkdir(mode=0o700)
        for section in ("billing", "finance", "integration") + (("workflow-supported",) if workflow_candidate else ()):
            copy_committed_examples(repo, commit, section, examples / section)
        scripts = stage / "scripts"
        scripts.mkdir(mode=0o700)
        required_scripts = ["demo-finance.py", "check-native-package-journey.py", "verify-native-package.py",
                            "install-native-package.py", "install-linux-x86_64.sh", "install-macos-arm64.sh",
                            "check-package-docs.py", "check-synthetic-quickstart.py", "check-bounded-cohort.py"]
        for filename in required_scripts:
            source = repo / "scripts" / filename
            if not source.is_file():
                raise SystemExit(f"required standalone package tool is missing: {source}")
            shutil.copyfile(source, scripts / filename)
            (scripts / filename).chmod(0o600)
        if workflow_candidate:
            e2e = scripts / "e2e"
            e2e.mkdir(mode=0o700)
            for filename in ("workflow_supported.py", "workflow_supported_negatives.py"):
                shutil.copyfile(repo / "scripts/e2e" / filename, e2e / filename)
                (e2e / filename).chmod(0o600)
        subprocess.run([sys.executable, str(stage / "scripts/check-package-docs.py"), str(stage)], check=True)
        try:
            if platform.system() == "Linux":
                linkage = output("ldd", str(stage / "ledger"))
                runtime = output("ldd", "--version").splitlines()[0]
                release = Path("/etc/os-release").read_text(encoding="utf-8") if Path("/etc/os-release").exists() else "unavailable"
                linkage_file = f"host_runtime={runtime}\n{linkage}\n\nOS_RELEASE_BEGIN\n{release}OS_RELEASE_END\n"
                libc = output("getconf", "GNU_LIBC_VERSION")
            elif platform.system() == "Darwin":
                linkage = output("otool", "-L", str(stage / "ledger"))
                sdk = output("xcrun", "--sdk", "macosx", "--show-sdk-version")
                deployment = output("otool", "-l", str(stage / "ledger"))
                linkage_file = f"macos_sdk={sdk}\n{linkage}\n\nLOAD_COMMANDS_BEGIN\n{deployment}\nLOAD_COMMANDS_END\n"
                libc = "system"
            else:
                raise SystemExit("unsupported package build host")
        except (OSError, subprocess.CalledProcessError) as error:
            raise SystemExit(f"could not capture actual native runtime/linkage: {error}") from error
        (stage / "BUILD-LINKAGE.txt").write_text(linkage_file, encoding="utf-8")
        github_actions = os.environ.get("GITHUB_ACTIONS", "").lower() == "true"
        origin = "github-actions" if github_actions else "local"
        runner_environment = os.environ.get("RUNNER_ENVIRONMENT", "local") if github_actions else "local"
        build_info = {
            "source_commit": commit, "source_tree": tree, "source_clean": "true", "version": version,
            "target": target, "build_origin": origin, "runner_environment": runner_environment,
            "host_os": platform.platform(), "uname": output("uname", "-a"), "libc_runtime": libc,
            "rustc": output("rustc", "--version"), "cargo": output("cargo", "--version"),
            "cargo_build_command": f"cargo build --locked --release --target {target} --manifest-path Cargo.toml -p ledgerlab-cli",
            "c_compiler": output("cc", "--version").splitlines()[0],
            "binary_sha256": sha(stage / "ledger"),
        }
        build_info.update(sqlite_evidence(repo))
        if workflow_candidate:
            build_info.update({
                "workflow_build_inputs_sha256": hashlib.sha256(workflow_inputs).hexdigest(),
                "workflow_source_inventory_sha256": hashlib.sha256(workflow_inventory).hexdigest(),
                "workflow_capture_command": "python3 scripts/build_workflow.py --local-execution --operator-resources --output <CAPTURE>",
                "cargo_build_command": f"cargo build --locked --offline --package ledgerlab-cli --target {target} --target-dir <CAPTURE>/target --release (cwd <CAPTURE>/source)",
                "workflow_helper_build_command": f"LEDGER_WORKFLOW_BUILD_INPUTS_JSON=<CAPTURE>/build-inputs.json CARGO_TARGET_DIR=<CAPTURE>/target cargo build --manifest-path <CAPTURE>/source/Cargo.toml --package ledgerlab --example prepare_local_publication --example prepare_local_terms --target {target} --release --locked --offline",
                "workflow_path_notation": "<CAPTURE> denotes the declared build capture; private absolute paths are omitted",
            })
        build_info["toolchain_verbose"] = output("rustc", "--version", "--verbose").replace("\n", "; ")
        (stage / "BUILD-INFO.txt").write_text("".join(f"{key}={value}\n" for key, value in build_info.items()), encoding="utf-8")
        for path in stage.rglob("*"):
            if path.is_dir():
                path.chmod(0o700)
            elif path.name != "ledger":
                path.chmod(0o600)
        subprocess.run([sys.executable, str(repo / "scripts" / "release-inventory.py"), str(repo), str(stage), target], check=True)
        for path in stage.rglob("*"):
            if path.is_dir():
                path.chmod(0o700)
            elif path.name != "ledger":
                path.chmod(0o600)
        archive = artifacts / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz", format=tarfile.PAX_FORMAT) as tar:
            tar.add(stage, arcname=name, recursive=True)
    sums = artifacts / "SHA256SUMS"
    sums.write_text(f"{sha(archive)}  {archive.name}\n", encoding="ascii")
    print(archive)


if __name__ == "__main__":
    main()
