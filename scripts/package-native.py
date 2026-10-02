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
import hashlib


def output(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True, stderr=subprocess.STDOUT).strip()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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


def copy_tree(source, destination):
    shutil.copytree(source, destination, dirs_exist_ok=True, symlinks=False,
                    ignore=shutil.ignore_patterns("install-v0.3.0-macos.sh"))


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: package-native.py REPO ARTIFACT_DIRECTORY TARGET")
    repo, artifacts, target = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    version = tomllib.loads((repo / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    commit = output("git", "rev-parse", "HEAD", cwd=repo)
    tree = output("git", "rev-parse", "HEAD^{tree}", cwd=repo)
    if output("git", "status", "--porcelain", "--untracked-files=all", cwd=repo):
        raise SystemExit("refusing to package a dirty source checkout")
    if artifacts.exists() or artifacts.is_symlink():
        raise SystemExit(f"refusing existing artifact directory: {artifacts}")
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
        for filename in ("LICENSE", "NOTICE", "CURRENT-REQUIREMENTS.md", "WORKFLOW.md"):
            shutil.copyfile(repo / filename, stage / filename)
        docs = stage / "docs"
        docs.mkdir(mode=0o700)
        for filename in ("billing-quickstart.md", "billing-recovery.md", "m5-qualification.md", "m5-contracts.md",
                         "finance-e2e.md", "finance-csv.md", "resources-and-costs.md", "billing-m2-cli-contract.md"):
            shutil.copyfile(repo / "docs" / filename, docs / filename)
        examples = stage / "examples"
        examples.mkdir(mode=0o700)
        for section in ("billing", "finance", "integration"):
            copy_tree(repo / "examples" / section, examples / section)
        scripts = stage / "scripts"
        scripts.mkdir(mode=0o700)
        required_scripts = ["demo-finance.py", "check-native-package-journey.py", "verify-native-package.py",
                            "install-native-package.py", "install-linux-x86_64.sh", "install-macos-arm64.sh"]
        for filename in required_scripts:
            source = repo / "scripts" / filename
            if not source.is_file():
                raise SystemExit(f"required standalone package tool is missing: {source}")
            shutil.copyfile(source, scripts / filename)
            (scripts / filename).chmod(0o600)
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
