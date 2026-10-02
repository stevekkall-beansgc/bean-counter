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
    match = re.fullmatch(r"bean-counter-v([0-9]+\.[0-9]+\.[0-9]+)-([a-z0-9_]+-[a-z0-9_-]+)", root_name)
    require(match is not None, "archive filename does not identify a supported Bean Counter package")
    filename_version, filename_target = match.groups()
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
    parser.add_argument("--version", default="0.9.1")
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
