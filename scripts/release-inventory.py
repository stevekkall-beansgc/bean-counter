#!/usr/bin/env python3
"""Write an unsigned native package's actual source, dependency and file inventory."""

import datetime
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tomllib


def run(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sqlite_source(repo):
    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1", cwd=repo))
    packages = {p["id"]: p for p in metadata["packages"]}
    sqlite_id = next(p["id"] for p in metadata["packages"] if p["name"] == "libsqlite3-sys")
    node = next(n for n in metadata["resolve"]["nodes"] if n["id"] == sqlite_id)
    if "bundled" not in node["features"]:
        raise SystemExit("libsqlite3-sys is not resolved with its bundled feature")
    root = Path(packages[sqlite_id]["manifest_path"]).parent
    amalgamation = root / "sqlite3" / "sqlite3.c"
    if not amalgamation.is_file():
        raise SystemExit(f"bundled SQLite amalgamation is missing: {amalgamation}")
    text = amalgamation.read_text(encoding="utf-8", errors="strict")
    version_line = next((line for line in text.splitlines() if line.startswith("#define SQLITE_VERSION ")), None)
    source_line = next((line for line in text.splitlines() if line.startswith("#define SQLITE_SOURCE_ID ")), None)
    if not version_line or not source_line:
        raise SystemExit("bundled SQLite version/source identity could not be derived")
    version = version_line.split('"', 2)[1]
    source_id = source_line.split('"', 2)[1]
    return {"version": version, "source_id": source_id, "amalgamation_sha256": digest(amalgamation),
            "license": root / "sqlite3" / "LICENSE.md"}


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: release-inventory.py REPO PACKAGE TARGET")
    repo, package, target = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    workspace = tomllib.loads((repo / "Cargo.toml").read_text())
    version = workspace["workspace"]["package"]["version"]
    commit = run("git", "rev-parse", "HEAD", cwd=repo)
    tree = run("git", "rev-parse", "HEAD^{tree}", cwd=repo)
    if run("git", "status", "--porcelain", "--untracked-files=all", cwd=repo):
        raise SystemExit("refusing inventory from a dirty source checkout")
    created = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1", cwd=repo))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
    locked = tomllib.loads((repo / "Cargo.lock").read_text())["package"]
    licenses = package / "licenses"
    licenses.mkdir()
    inventory = []
    sqlite = sqlite_source(repo)
    for item in locked:
        name, dep_version = item["name"], item["version"]
        meta = packages.get((name, dep_version))
        declared = (meta or {}).get("license") or "NOASSERTION"
        spdx_id = "SPDXRef-" + "".join(c if c.isalnum() else "-" for c in name + "-" + dep_version)
        source = item.get("source")
        entry = {"SPDXID": spdx_id, "name": name, "versionInfo": dep_version,
                 "downloadLocation": f"https://crates.io/api/v1/crates/{name}/{dep_version}/download" if source and "crates.io" in source else "NOASSERTION",
                 "filesAnalyzed": False, "licenseConcluded": "NOASSERTION", "licenseDeclared": declared,
                 "copyrightText": "NOASSERTION"}
        if item.get("checksum"):
            entry["checksums"] = [{"algorithm": "SHA256", "checksumValue": item["checksum"]}]
        inventory.append(entry)
        if meta and source and "crates.io" in source:
            source_dir = Path(meta["manifest_path"]).parent
            dest = licenses / f"{name}-{dep_version}"
            for path in source_dir.iterdir():
                if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE")):
                    dest.mkdir(exist_ok=True)
                    shutil.copyfile(path, dest / path.name)
    sqlite_license = sqlite["license"]
    if sqlite_license.is_file():
        dest = licenses / f"sqlite-{sqlite['version']}"
        dest.mkdir()
        shutil.copyfile(sqlite_license, dest / sqlite_license.name)
    sqlite_package = {
        "SPDXID": "SPDXRef-SQLite-" + sqlite["version"].replace(".", "-"),
        "name": "SQLite", "versionInfo": sqlite["version"],
        "downloadLocation": "https://www.sqlite.org/", "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION", "licenseDeclared": "blessing",
        "copyrightText": "Public domain; source identity derived from the bundled SQLite amalgamation",
        "comment": f"source_id={sqlite['source_id']}; sqlite3.c SHA-256={sqlite['amalgamation_sha256']}",
    }
    inventory.append(sqlite_package)
    build = dict(line.split("=", 1) for line in (package / "BUILD-INFO.txt").read_text().splitlines() if "=" in line)
    write_json(package / "SBOM.spdx.json", {
        "spdxVersion": "SPDX-2.3", "dataLicense": "CC0-1.0", "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"Bean Counter {version} {target} Cargo.lock inventory",
        "documentNamespace": f"https://github.com/stevekkall-beansgc/bean-counter/spdx/{commit}/{target}",
        "creationInfo": {"creators": ["Tool: Bean Counter release-inventory.py"], "created": created,
                         "comment": "All locked Cargo packages, including build-only and other-target dependencies; not a vulnerability audit or signed attestation."},
        "packages": inventory,
        "relationships": [{"spdxElementId": "SPDXRef-DOCUMENT", "relatedSpdxElement": item["SPDXID"], "relationshipType": "DESCRIBES"} for item in inventory],
    })
    write_json(package / "PROVENANCE.json", {
        "schema": "bean-counter-native-build-provenance/3", "created_at": created,
        "repository": "https://github.com/stevekkall-beansgc/bean-counter", "source_commit": commit,
        "source_tree": tree, "source_clean": True, "version": version,
        "local_billing_contract": "v0.3", "target": target,
        "build_os": platform.platform(), "build_origin": build["build_origin"],
        "runner_environment": build.get("runner_environment", "local"),
        "rustc": run("rustc", "--version"), "cargo": run("cargo", "--version"),
        "cargo_lock_sha256": digest(repo / "Cargo.lock"), "binary_sha256": digest(package / "ledger"),
        "sqlite": {"version": sqlite["version"], "source_id": sqlite["source_id"], "amalgamation_sha256": sqlite["amalgamation_sha256"]},
        "signed_or_notarized": False, "signed_artifact_attestation": False, "reproducible_build_proven": False,
    })
    files = {str(path.relative_to(package)): digest(path) for path in sorted(package.rglob("*")) if path.is_file() and path.name != "MANIFEST.json"}
    write_json(package / "MANIFEST.json", {
        "schema": "bean-counter-native-artifact/3", "version": version,
        "source_commit": commit, "source_tree": tree, "source_clean": True, "target": target,
        "binary_sha256": digest(package / "ledger"), "files": files,
    })
    print(f"inventory: {len(inventory)} packages, {len(files)} package files, source {commit}")


if __name__ == "__main__":
    main()
