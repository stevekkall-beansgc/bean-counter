#!/usr/bin/env python3
"""Strictly verify one native archive before extraction or execution."""

import argparse
import hashlib
from io import BytesIO
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile


class VerificationError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise VerificationError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise VerificationError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def parse_json(data, name):
    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=unique_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"invalid {name}: {error}") from error
    require(isinstance(value, dict), f"{name} must be a JSON object")
    return value


def canonical(value):
    if isinstance(value, dict):
        return '{' + ','.join(canonical(k) + ':' + canonical(value[k])
                              for k in sorted(value, key=lambda s: s.encode('utf-16-be'))) + '}'
    if isinstance(value, list):
        return '[' + ','.join(map(canonical, value)) + ']'
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'))


def verify_catalog_evidence(files, build, identity):
    raw = files.get("WORKFLOW-BUILD-INPUTS.json")
    if raw is None:
        require("WORKFLOW-CAPTURE-QUALIFIED.json" not in files
                and "--integrated-local-profiles" not in build.get("workflow_capture_command", ""),
                "missing workflow catalog inputs")
        return
    catalog = parse_json(raw, "workflow build inputs")
    if catalog.get("schema") != "ledger-workflow-build-input-catalog/1":
        require("WORKFLOW-CAPTURE-QUALIFIED.json" not in files
                and "--integrated-local-profiles" not in build.get("workflow_capture_command", ""),
                "invalid workflow catalog schema")
        return  # Historical single-profile packages retain their existing path.
    require(set(catalog) == {"schema", "entries"}, "invalid workflow catalog fields")
    required = {"WORKFLOW-SOURCE-INVENTORY.json", "WORKFLOW-CAPTURE-QUALIFIED.json"}
    require(required <= files.keys(), "missing workflow catalog evidence")
    inventory_raw = files["WORKFLOW-SOURCE-INVENTORY.json"]
    inventory = parse_json(inventory_raw, "workflow source inventory")
    qualified = parse_json(files["WORKFLOW-CAPTURE-QUALIFIED.json"], "workflow qualification")
    inventory_digest = "sha256:" + sha(b"bean-counter/build-source-inventory/1\0" + canonical(inventory).encode())
    require(build.get("workflow_build_inputs_sha256") == sha(raw)
            and build.get("workflow_source_inventory_sha256") == sha(inventory_raw),
            "workflow evidence hashes disagree")
    require(qualified == {"source_inventory_digest": inventory_digest,
                          "build_inputs_sha256": sha(raw),
                          "status": "captured inputs unchanged; local build observation only",
                          "binary_sha256": sha(files["ledger"])}, "workflow qualification disagrees")
    entries = catalog["entries"]
    expected = ("1aa64de36f29072d1b850cfb52d30e94623bf1a66048f096383452d832b47613",
                "5c581b238a4387c7de3afdcbeccc6d7c19c1ec63227a8c58a4dd665dad9558bc",
                "f38dc832f70070165a5bbba7502a9599c00ecc483ddd26b9063b9e045f4e8e6b")
    require(isinstance(entries, list) and len(entries) == len(expected), "invalid workflow catalog size")
    rows = inventory.get("entries")
    require(set(inventory) == {"schema", "recipe", "entries"}
            and inventory.get("schema") == "ledger-build-source-inventory/1"
            and inventory.get("recipe") == "bean-counter-build-inputs/1" and isinstance(rows, list),
            "invalid workflow inventory")
    require(all(isinstance(row, dict) and set(row) == {"path", "bytes", "sha256"}
                and isinstance(row["path"], str) and row["path"] and not row["path"].startswith("/")
                and all(part not in ("", ".", "..") for part in row["path"].split("/"))
                and isinstance(row["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", row["sha256"])
                and isinstance(row["bytes"], str) and row["bytes"].isdigit() for row in rows),
            "invalid workflow inventory rows")
    require([row["path"] for row in rows] == sorted({row["path"] for row in rows}, key=lambda s: s.encode("utf-16-be")),
            "workflow inventory paths are not canonical")
    lock = [row for row in rows if row["path"] == "Cargo.lock"]
    require(len(lock) == 1, "missing workflow lockfile")
    source = {"base_commit": identity["source_commit"], "state": "clean_commit",
              "inventory_digest": inventory_digest, "file_count": len(rows),
              "total_bytes": str(sum(int(row["bytes"]) for row in rows))}
    common = None
    for entry, digest in zip(entries, expected):
        require(isinstance(entry, dict) and set(entry) == {"schema", "source", "recipe"}
                and entry["schema"] == "ledger-workflow-build-inputs/1" and entry["source"] == source,
                "workflow catalog source differs")
        recipe = entry["recipe"]
        require(isinstance(recipe, dict) and set(recipe) == {"package_name", "package_version", "build_profile", "target", "features", "rustc_version", "cargo_version", "rustflags", "cargo_lock_digest", "compiler_ref"},
                "invalid workflow recipe")
        ref = {"kind": "workflow-compiler-descriptor", "id": "wdoc_" + digest, "digest": "sha256:" + digest}
        require(recipe["compiler_ref"] == ref and recipe["package_name"] == "ledgerlab-cli"
                and recipe["package_version"] == identity["version"] and recipe["target"] == identity["target"]
                and recipe["build_profile"] == "release" and recipe["features"] == [] and recipe["rustflags"] == []
                and recipe["rustc_version"] == build.get("rustc") and recipe["cargo_version"] == build.get("cargo")
                and recipe["cargo_lock_digest"] == "sha256:" + lock[0]["sha256"], "workflow recipe differs")
        other = {key: value for key, value in recipe.items() if key != "compiler_ref"}
        require(common is None or common == other, "workflow catalog entries differ")
        common = other


def parse_sums(path, archive_name):
    try:
        lines = path.read_text(encoding="ascii").splitlines()
    except (OSError, UnicodeError) as error:
        raise VerificationError(f"cannot read SHA256SUMS: {error}") from error
    matching = []
    for number, line in enumerate(lines, 1):
        if not line:
            continue
        match = re.fullmatch(r"([0-9a-fA-F]{64}) ([ *])([^\x00/\\]+)", line)
        require(match is not None, f"malformed SHA256SUMS line {number}")
        digest, _, name = match.groups()
        require(name not in (".", ".."), f"noncanonical SHA256SUMS filename on line {number}")
        if name == archive_name:
            matching.append(digest.lower())
    require(len(matching) == 1, "SHA256SUMS must contain exactly one entry for the archive")
    return matching[0]


def canonical_member_name(name, is_dir):
    require("\\" not in name and "\x00" not in name, f"noncanonical archive path: {name!r}")
    candidate = name[:-1] if is_dir and name.endswith("/") else name
    require(candidate and not candidate.startswith("/"), f"absolute or empty archive path: {name!r}")
    path = PurePosixPath(candidate)
    require(not path.is_absolute() and all(part not in ("", ".", "..") for part in candidate.split("/")),
            f"noncanonical archive path: {name!r}")
    require(str(path) == candidate, f"noncanonical archive path: {name!r}")
    return candidate


def read_members(archive_data, root_name):
    try:
        archive = tarfile.open(fileobj=BytesIO(archive_data), mode="r:gz")
    except (OSError, tarfile.TarError) as error:
        raise VerificationError(f"cannot read gzip tar archive: {error}") from error
    file_bytes = {}
    directories = set()
    seen = set()
    with archive:
        for member in archive:
            require(member.isfile() or member.isdir(), f"special archive member rejected: {member.name!r}")
            name = canonical_member_name(member.name, member.isdir())
            require(name == root_name or name.startswith(root_name + "/"), f"member outside package root: {name!r}")
            require(name not in seen, f"duplicate archive member: {name!r}")
            seen.add(name)
            if member.isdir():
                require(member.mode == 0o700, f"directory must have private mode 0700: {name}")
                directories.add(name)
                continue
            require(member.mode in (0o600, 0o700), f"file has unsafe or unexpected mode: {name} ({member.mode:o})")
            if name == root_name + "/ledger":
                require(member.mode == 0o700, "ledger must be executable and private (mode 0700)")
            else:
                require(member.mode == 0o600, f"non-binary file must have mode 0600: {name}")
            stream = archive.extractfile(member)
            require(stream is not None, f"could not read archive member: {name}")
            file_bytes[name[len(root_name) + 1:]] = stream.read()
    require(root_name in directories, "archive package root directory is missing")
    require("ledger" in file_bytes, "archive ledger binary is missing")
    full_paths = [root_name + "/" + name for name in file_bytes] + list(directories)
    for name in full_paths:
        if name == root_name:
            continue
        parent = PurePosixPath(name).parent
        while str(parent) not in (".", root_name):
            require(str(parent) in directories, f"archive member has a missing or non-directory parent: {name}")
            parent = parent.parent
    return file_bytes


def parse_build_info(data):
    try:
        lines = data.decode("utf-8").splitlines()
    except UnicodeError as error:
        raise VerificationError("BUILD-INFO.txt is not UTF-8") from error
    fields = {}
    for line in lines:
        require("=" in line, "malformed BUILD-INFO.txt field")
        key, value = line.split("=", 1)
        require(key and value and key not in fields, f"empty or duplicate BUILD-INFO.txt field: {key!r}")
        fields[key] = value
    return fields


def verify(archive_path, sums_path, expected_commit, expected_version, expected_target):
    archive_name = archive_path.name
    require(archive_name.endswith(".tar.gz"), "archive filename must end in .tar.gz")
    expected_sum = parse_sums(sums_path, archive_name)
    archive_data = archive_path.read_bytes()
    archive_hash = sha(archive_data)
    require(archive_hash == expected_sum, "archive SHA-256 mismatch")
    root_name = archive_name[:-len(".tar.gz")]
    match = re.fullmatch(
        r"bean-counter-v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
        r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)-(aarch64-apple-darwin|x86_64-unknown-linux-gnu)",
        root_name,
    )
    require(match is not None, "archive filename does not identify a supported Bean Counter package")
    filename_version, filename_target = match.groups()
    if "-" in filename_version:
        prerelease = filename_version.split("-", 1)[1].split(".")
        require(all(not (part.isdigit() and len(part) > 1 and part.startswith("0"))
                    for part in prerelease), "noncanonical prerelease version")
    require(filename_target in ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu"), "unsupported package target")
    require(filename_version == expected_version, f"unexpected package version: {filename_version}")
    if expected_target:
        require(filename_target == expected_target, f"unexpected package target: {filename_target}")
    require(re.fullmatch(r"[0-9a-f]{40,64}", expected_commit) is not None, "expected commit must be lowercase Git SHA-1 or SHA-256")
    files = read_members(archive_data, root_name)
    required_files = {"MANIFEST.json", "ledger", "SBOM.spdx.json", "PROVENANCE.json", "BUILD-INFO.txt", "BUILD-LINKAGE.txt"}
    require(required_files <= files.keys(), "archive is missing required release metadata or binary files")
    manifest = parse_json(files["MANIFEST.json"], "MANIFEST.json")
    provenance = parse_json(files["PROVENANCE.json"], "PROVENANCE.json")
    sbom = parse_json(files["SBOM.spdx.json"], "SBOM.spdx.json")
    build = parse_build_info(files["BUILD-INFO.txt"])
    target = filename_target
    identity = {"version": filename_version, "target": target, "source_commit": expected_commit}
    require(manifest.get("schema") == "bean-counter-native-artifact/3", "unsupported manifest schema")
    require(provenance.get("schema") == "bean-counter-native-build-provenance/3", "unsupported provenance schema")
    require(manifest.get("source_clean") is True, "manifest does not report a clean source tree")
    for field, value in identity.items():
        require(manifest.get(field) == value, f"manifest {field} disagrees with expected identity")
        require(provenance.get(field) == value, f"provenance {field} disagrees with expected identity")
        require(build.get(field) == str(value), f"BUILD-INFO {field} disagrees with expected identity")
    tree = manifest.get("source_tree")
    require(isinstance(tree, str) and re.fullmatch(r"[0-9a-f]{40,64}", tree), "manifest source tree is invalid")
    require(provenance.get("source_tree") == tree and build.get("source_tree") == tree, "source tree identity disagrees")
    require(provenance.get("source_clean") is True, "provenance does not report a clean source tree")
    require(build.get("source_clean") == "true", "BUILD-INFO does not report a clean source tree")
    require(build.get("build_origin") in ("local", "github-actions"), "build origin is missing or unsupported")
    require(build.get("runner_environment") in ("local", "github-hosted", "self-hosted"), "runner environment is invalid")
    require(build.get("binary_sha256") == sha(files["ledger"]), "BUILD-INFO binary hash mismatch")
    require(manifest.get("binary_sha256") == sha(files["ledger"]), "manifest binary hash mismatch")
    require(provenance.get("binary_sha256") == sha(files["ledger"]), "provenance binary hash mismatch")
    manifest_files = manifest.get("files")
    require(isinstance(manifest_files, dict), "manifest files must be an object")
    require(all(isinstance(name, str) and canonical_member_name(name, False) == name for name in manifest_files), "manifest contains a noncanonical path")
    payload_files = {name: data for name, data in files.items() if name != "MANIFEST.json"}
    require(set(manifest_files) == set(payload_files), "manifest file set differs from archive file set")
    require(all(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) for value in manifest_files.values()), "manifest contains an invalid SHA-256")
    require(manifest_files == {name: sha(data) for name, data in payload_files.items()}, "packaged file hashes differ from manifest")
    verify_catalog_evidence(files, build, identity)
    require(sbom.get("spdxVersion") == "SPDX-2.3" and isinstance(sbom.get("packages"), list), "invalid SPDX SBOM")
    sqlite_items = [item for item in sbom["packages"] if isinstance(item, dict) and item.get("name") == "SQLite"]
    require(len(sqlite_items) == 1, "SBOM must identify exactly one bundled SQLite package")
    sqlite = sqlite_items[0]
    sqlite_build = {key.removeprefix("sqlite_"): value for key, value in build.items() if key.startswith("sqlite_")}
    require(sqlite.get("versionInfo") == sqlite_build.get("version"), "SBOM SQLite version differs from build evidence")
    expected_sqlite_comment = f"source_id={sqlite_build.get('source_id')}; sqlite3.c SHA-256={sqlite_build.get('amalgamation_sha256')}"
    require(sqlite.get("comment") == expected_sqlite_comment, "SBOM SQLite source identity differs from build evidence")
    require(provenance.get("sqlite") == {"version": sqlite_build.get("version"), "source_id": sqlite_build.get("source_id"),
                                           "amalgamation_sha256": sqlite_build.get("amalgamation_sha256")},
            "provenance SQLite identity differs from build evidence")
    namespace = sbom.get("documentNamespace", "")
    require(f"/{expected_commit}/{target}" in namespace, "SBOM source namespace disagrees with package identity")
    return {"result": "verified", "source_commit": expected_commit, "target": target,
            "version": filename_version, "archive_sha256": archive_hash,
            "binary_sha256": manifest["binary_sha256"], "files": len(payload_files),
            "spdx_packages": len(sbom["packages"])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive")
    parser.add_argument("sha256sums")
    parser.add_argument("expected_commit")
    parser.add_argument("--version", default="0.11.0")
    parser.add_argument("--target")
    args = parser.parse_args()
    try:
        result = verify(Path(args.archive), Path(args.sha256sums), args.expected_commit, args.version, args.target)
    except (OSError, VerificationError, tarfile.TarError) as error:
        print(f"verification failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
