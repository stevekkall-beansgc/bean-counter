#!/usr/bin/env python3
"""Negative archive-security tests for verify-native-package.py, including -O mode."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

VERIFY = Path(__file__).with_name("verify-native-package.py")
ROOT = "bean-counter-v0.9.0-x86_64-unknown-linux-gnu"
ARCHIVE_NAME = ROOT + ".tar.gz"
COMMIT = "a" * 40
TREE = "b" * 40
SQLITE_COMMENT = "source_id=2026-03-13 10:38:09 abcdef; sqlite3.c SHA-256=" + "c" * 64


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fixture(extra_members=(), duplicate_manifest_key=False, mutate=None):
    files = {
        "ledger": b"synthetic executable payload",
        "BUILD-INFO.txt": (f"source_commit={COMMIT}\nsource_tree={TREE}\nsource_clean=true\nversion=0.9.0\n"
                           "target=x86_64-unknown-linux-gnu\nbuild_origin=local\nrunner_environment=local\n"
                           "binary_sha256=" + digest(b"synthetic executable payload") + "\n"
                           "sqlite_version=3.51.3\nsqlite_source_id=2026-03-13 10:38:09 abcdef\n"
                           "sqlite_amalgamation_sha256=" + "c" * 64 + "\n").encode(),
        "BUILD-LINKAGE.txt": b"synthetic runtime linkage\n",
    }
    sbom = {"spdxVersion": "SPDX-2.3", "packages": [{"name": "SQLite", "versionInfo": "3.51.3",
            "comment": SQLITE_COMMENT}] , "documentNamespace": f"urn:test:/{COMMIT}/x86_64-unknown-linux-gnu"}
    prov = {"schema": "bean-counter-native-build-provenance/3", "source_commit": COMMIT, "source_tree": TREE,
            "source_clean": True, "version": "0.9.0", "target": "x86_64-unknown-linux-gnu",
            "binary_sha256": digest(files["ledger"]),
            "sqlite": {"version": "3.51.3", "source_id": "2026-03-13 10:38:09 abcdef", "amalgamation_sha256": "c" * 64}}
    files["SBOM.spdx.json"] = json.dumps(sbom).encode()
    files["PROVENANCE.json"] = json.dumps(prov).encode()
    manifest = {"schema": "bean-counter-native-artifact/3", "source_commit": COMMIT, "source_tree": TREE,
                "source_clean": True, "version": "0.9.0", "target": "x86_64-unknown-linux-gnu",
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
    archive_path = directory / (case_name + ".tar.gz")
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


def rejected(directory, name, **kwargs):
    archive, sums = write_case(directory, name, **kwargs)
    result = subprocess.run([sys.executable, "-O", str(VERIFY), str(archive), str(sums), COMMIT],
                            text=True, capture_output=True)
    if result.returncode == 0:
        raise RuntimeError(f"verifier accepted negative test {name}: {result.stdout}")


def main():
    with tempfile.TemporaryDirectory(prefix="native-package-security-") as temporary:
        directory = Path(temporary)
        good, sums = write_case(directory, ARCHIVE_NAME[:-len(".tar.gz")])
        positive = subprocess.run([sys.executable, "-O", str(VERIFY), str(good), str(sums), COMMIT],
                                  text=True, capture_output=True)
        if positive.returncode != 0:
            raise RuntimeError(f"optimized verifier rejected valid fixture: {positive.stderr}")
        rejected(directory, "duplicate-json-key", duplicate_manifest_key=True)
        rejected(directory, "mismatched-target", mutate=lambda _files, manifest: manifest.update(target="aarch64-apple-darwin"))
        rejected(directory, "missing-manifest-file", mutate=lambda _files, manifest: manifest["files"].update({"absent": "d" * 64}))
        rejected(directory, "noncanonical-manifest-path", mutate=lambda _files, manifest: manifest["files"].update({"./ledger": digest(b"x")}))
        duplicate = tarfile.TarInfo(ROOT + "/ledger")
        duplicate.size = 1
        rejected(directory, "duplicate-member", extra_members=[(duplicate, __import__("io").BytesIO(b"x"))])
        traversal = tarfile.TarInfo(ROOT + "/../escape")
        traversal.size = 1
        rejected(directory, "traversal", extra_members=[(traversal, __import__("io").BytesIO(b"x"))])
        absolute = tarfile.TarInfo("/escape")
        absolute.size = 1
        rejected(directory, "absolute-path", extra_members=[(absolute, __import__("io").BytesIO(b"x"))])
        symlink = tarfile.TarInfo(ROOT + "/link")
        symlink.type = tarfile.SYMTYPE
        symlink.linkname = "ledger"
        rejected(directory, "symlink", extra_members=[(symlink, None)])
        device = tarfile.TarInfo(ROOT + "/device")
        device.type = tarfile.CHRTYPE
        rejected(directory, "device", extra_members=[(device, None)])
        bad_mode = tarfile.TarInfo(ROOT + "/unsafe")
        bad_mode.size = 1
        bad_mode.mode = 0o4755
        rejected(directory, "mode", extra_members=[(bad_mode, __import__("io").BytesIO(b"x"))])
        wrong_sums = directory / "wrong.sums"
        wrong_sums.write_text("0" * 64 + "  " + good.name + "\n")
        result = subprocess.run([sys.executable, "-O", str(VERIFY), str(good), str(wrong_sums), COMMIT], capture_output=True)
        if result.returncode == 0:
            raise RuntimeError("verifier accepted a bad archive checksum")
    print("native package verifier: optimized positive case and 11 negative security cases passed")


if __name__ == "__main__":
    main()
