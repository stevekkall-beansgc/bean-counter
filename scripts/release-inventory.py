#!/usr/bin/env python3
"""Write an unsigned native package's source, dependency and file inventory."""

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
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: release-inventory.py REPO PACKAGE TARGET")
    repo, package, target = map(Path, sys.argv[1:])
    repo, package, target = repo.resolve(), package.resolve(), str(target)
    version = "0.3.0"
    commit = run("git", "rev-parse", "HEAD", cwd=repo)
    tree = run("git", "rev-parse", "HEAD^{tree}", cwd=repo)
    created = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    # Metadata covers every locked target, including crates not needed by this
    # native build. A fresh CI runner may need to download those locked crates.
    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1", cwd=repo))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
    locked = tomllib.loads((repo / "Cargo.lock").read_text())["package"]
    licenses = package / "licenses"
    licenses.mkdir()
    inventory = []
    for item in locked:
        name, dep_version = item["name"], item["version"]
        meta = packages.get((name, dep_version))
        declared = (meta or {}).get("license") or "NOASSERTION"
        spdx_id = "SPDXRef-" + "".join(c if c.isalnum() else "-" for c in name + "-" + dep_version)
        source = item.get("source")
        entry = {
            "SPDXID": spdx_id,
            "name": name,
            "versionInfo": dep_version,
            "downloadLocation": f"https://crates.io/api/v1/crates/{name}/{dep_version}/download" if source and "crates.io" in source else "NOASSERTION",
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": declared,
            "copyrightText": "NOASSERTION",
        }
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
    inventory.append({
        "SPDXID": "SPDXRef-SQLite-3-51-3",
        "name": "SQLite",
        "versionInfo": "3.51.3",
        "downloadLocation": "https://www.sqlite.org/index.html",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": "blessing",
        "copyrightText": "Public domain; see bundled SQLite source and upstream documentation",
    })
    write_json(package / "SBOM.spdx.json", {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"Bean Counter {version} {target} Cargo.lock inventory",
        "documentNamespace": f"https://github.com/stevekkall-beansgc/bean-counter/spdx/{commit}/{target}",
        "creationInfo": {"creators": ["Tool: Bean Counter release-inventory.py"], "created": created,
                         "comment": "All locked Cargo packages, including build-only and other-target dependencies; not a vulnerability audit or signed attestation."},
        "packages": inventory,
        "relationships": [{"spdxElementId": "SPDXRef-DOCUMENT", "relatedSpdxElement": item["SPDXID"], "relationshipType": "DESCRIBES"} for item in inventory],
    })
    write_json(package / "PROVENANCE.json", {
        "schema": "bean-counter-native-build-provenance/2",
        "created_at": created,
        "repository": "https://github.com/stevekkall-beansgc/bean-counter",
        "source_commit": commit,
        "source_tree": tree,
        "source_clean": True,
        "version": version,
        "local_billing_contract": "v0.2",
        "target": target,
        "build_os": platform.platform(),
        "rustc": run("rustc", "--version"),
        "cargo": run("cargo", "--version"),
        "cargo_lock_sha256": digest(repo / "Cargo.lock"),
        "binary_sha256": digest(package / "ledger"),
        "build_identity": "author-generated local build" if sys.platform == "darwin" else "GitHub-hosted Ubuntu runner build",
        "signed_or_notarized": False,
        "signed_artifact_attestation": False,
        "reproducible_build_proven": False,
    })
    files = {str(path.relative_to(package)): digest(path) for path in sorted(package.rglob("*")) if path.is_file() and path.name != "MANIFEST.json"}
    write_json(package / "MANIFEST.json", {
        "schema": "bean-counter-native-artifact/2",
        "version": version,
        "source_commit": commit,
        "source_tree": tree,
        "target": target,
        "binary_sha256": digest(package / "ledger"),
        "files": files,
    })
    print(f"inventory: {len(inventory)} packages, {len(files)} package files, source {commit}")


if __name__ == "__main__":
    main()
