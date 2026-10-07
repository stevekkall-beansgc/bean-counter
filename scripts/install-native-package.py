#!/usr/bin/env python3
"""Verify and install a native archive into a new private directory."""

from io import BytesIO
import hashlib
import importlib.util
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import tempfile

_verifier_path = Path(__file__).with_name("verify-native-package.py")
_spec = importlib.util.spec_from_file_location("verify_native_package", _verifier_path)
if _spec is None or _spec.loader is None:
    raise RuntimeError(f"cannot load standalone verifier: {_verifier_path}")
_verifier = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_verifier)
verify = _verifier.verify


def safe_extract(data, destination, root_name):
    root = destination / root_name
    with tarfile.open(fileobj=BytesIO(data), mode="r:gz") as archive:
        for member in archive:
            relative = PurePosixPath(member.name)
            parts = relative.parts
            if parts[0] != root_name:
                raise ValueError("verified archive root changed before extraction")
            if member.isdir():
                path = destination.joinpath(*parts)
                path.mkdir(mode=0o700, parents=True, exist_ok=True)
                path.chmod(0o700)
                continue
            path = destination.joinpath(*parts)
            path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError(f"cannot read verified archive member: {member.name}")
            with path.open("xb") as output:
                shutil.copyfileobj(stream, output)
            path.chmod(member.mode)
    return root


def main():
    if len(sys.argv) != 6:
        raise SystemExit("usage: install-native-package.py ARCHIVE SHA256SUMS EXPECTED_COMMIT TARGET NEW_INSTALL_DIRECTORY")
    archive_path, sums_path = Path(sys.argv[1]), Path(sys.argv[2])
    commit, target, destination = sys.argv[3], sys.argv[4], Path(sys.argv[5])
    if destination.exists() or destination.is_symlink():
        raise SystemExit(f"refusing existing destination: {destination}")
    # The verifier validates checksum, members, hashes, modes and all release identities first.
    report = verify(archive_path, sums_path, commit, "0.9.6", target)
    data = archive_path.read_bytes()
    if hashlib.sha256(data).hexdigest() != report["archive_sha256"]:
        raise SystemExit("archive changed after verification")
    destination = Path(os.path.abspath(destination))
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.mkdir(mode=0o700)
    try:
        with tempfile.TemporaryDirectory(prefix="bean-counter-install-") as temp:
            stage = Path(temp)
            root_name = archive_path.name[:-len(".tar.gz")]
            package = safe_extract(data, stage, root_name)
            binary = package / "ledger"
            expected = f"ledger {report['version']} (local development)"
            actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
            if actual != expected:
                raise ValueError(f"unexpected binary version: {actual}")
            shutil.copytree(package, destination, dirs_exist_ok=True)
            destination.chmod(0o700)
            (destination / "ledger").chmod(0o700)
    except BaseException:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    print(f"Installed verified v{report['version']} at {destination / 'ledger'}")
    print("This package is unsigned; BUILD-INFO.txt records its build host and runtime linkage.")


if __name__ == "__main__":
    main()
