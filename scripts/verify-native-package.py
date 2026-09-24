#!/usr/bin/env python3
"""Verify one native release archive against SHA256SUMS and its file manifest."""

import hashlib
import json
from pathlib import Path
import sys
import tarfile


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: verify-native-package.py ARCHIVE SHA256SUMS EXPECTED_COMMIT")
    archive, sums, commit = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
    expected = []
    for line in sums.read_text().splitlines():
        fields = line.split(maxsplit=1)
        if len(fields) == 2 and fields[1].lstrip("*") == archive.name:
            expected.append(fields[0])
    assert len(expected) == 1 and len(expected[0]) == 64, "archive has no unique checksum entry"
    archive_hash = sha(archive.read_bytes())
    assert archive_hash == expected[0], "archive SHA-256 mismatch"
    root = archive.name.removesuffix(".tar.gz")
    files = {}
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar:
            name = member.name
            assert name == root or name.startswith(root + "/"), f"unexpected archive path: {name}"
            parts = Path(name).parts
            assert ".." not in parts and not member.issym() and not member.islnk(), f"unsafe member: {name}"
            if member.isfile():
                stream = tar.extractfile(member)
                assert stream is not None
                files[name[len(root) + 1:]] = stream.read()
    assert "MANIFEST.json" in files and "ledger" in files and "SBOM.spdx.json" in files and "PROVENANCE.json" in files
    manifest = json.loads(files.pop("MANIFEST.json"))
    assert manifest["version"] == "0.3.0" and manifest["source_commit"] == commit
    assert manifest["files"] == {name: sha(content) for name, content in files.items()}, "packaged file hashes differ from manifest"
    assert manifest["binary_sha256"] == sha(files["ledger"])
    provenance = json.loads(files["PROVENANCE.json"])
    assert provenance["source_commit"] == commit and provenance["binary_sha256"] == sha(files["ledger"])
    assert provenance["target"] == manifest["target"]
    sbom = json.loads(files["SBOM.spdx.json"])
    assert sbom["spdxVersion"] == "SPDX-2.3" and len(sbom["packages"]) > 100
    print(json.dumps({"result": "verified", "source_commit": commit, "target": manifest["target"], "archive_sha256": archive_hash, "binary_sha256": manifest["binary_sha256"], "files": len(files), "spdx_packages": len(sbom["packages"])}))


if __name__ == "__main__":
    main()
