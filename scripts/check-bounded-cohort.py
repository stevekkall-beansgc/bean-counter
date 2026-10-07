#!/usr/bin/env python3
"""Qualify all bundled bounded-cohort scenarios against one verified executable.

Usage: python3 scripts/check-bounded-cohort.py LEDGER [--evidence NEW_EVIDENCE_DIRECTORY]
Evidence is retained, including expected refusals and every subprocess command.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EXAMPLE = ROOT / "examples/integration/bounded-cohort.py"
FIXTURES = EXAMPLE.with_suffix("")
SCENARIOS = ("five-completed-artifacts", "failed-work", "completed-work-assessment-error", "evidence-classification")


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ledger", type=Path)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    ledger = args.ledger.absolute()
    require(ledger == ledger.resolve() and ledger.is_file(), "Pass the verified executable's physical path")
    if args.evidence is not None:
        evidence = args.evidence.absolute()
        require(evidence == evidence.resolve(), "Use a physical evidence path")
        evidence.mkdir(mode=0o700)
    else:
        evidence = Path(tempfile.mkdtemp(prefix="bean-bounded-check-")).resolve()
    results = []

    def snapshot(path):
        paths = [path, *sorted(path.rglob("*"))] if path.is_dir() else [path]
        return {str(item.relative_to(path.parent)): {
            "mode": stat.S_IMODE(item.lstat().st_mode),
            "type": "symlink" if item.is_symlink() else "file" if item.is_file() else "directory",
            "sha256": hashlib.sha256(item.read_bytes()).hexdigest() if item.is_file() and not item.is_symlink() else None,
        } for item in paths}

    def run(label, work, scenario="evidence-classification", optimized=False, example=EXAMPLE, extra=(), error=None,
            preserve_existing=False):
        before = snapshot(work) if preserve_existing else None
        argv = [sys.executable, *(["-O"] if optimized else []), str(example), "--ledger", str(ledger),
                "--work-dir", str(work), "--scenario", scenario, *map(str, extra)]
        outcome = subprocess.run(argv, capture_output=True)
        (evidence / (label + ".command.json")).write_text(json.dumps(argv, indent=2) + "\n")
        (evidence / (label + ".stdout")).write_bytes(outcome.stdout)
        (evidence / (label + ".stderr")).write_bytes(outcome.stderr)
        result = {"label": label, "exit_code": outcome.returncode, "expected_exit": 1 if error else 0}
        results.append(result)
        (evidence / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        require(outcome.returncode == result["expected_exit"], f"{label}: unexpected exit; preserve {evidence}")
        if error:
            require(error in outcome.stderr.decode(), f"{label}: diagnostic missing")
            if preserve_existing:
                after = snapshot(work)
                (evidence / (label + ".preservation.json")).write_text(json.dumps({"before": before, "after": after}, indent=2) + "\n")
                require(before == after, f"{label}: existing path or evidence changed")
                require(str(work) in outcome.stderr.decode() and "Create only the parent" in outcome.stderr.decode(),
                        f"{label}: existing path remedy missing")
            else:
                require(not work.exists(), f"{label}: before-write refusal missing")
        print(f"PASS {label} (exit {outcome.returncode})", flush=True)

    version = subprocess.run([str(ledger), "--version"], capture_output=True)
    (evidence / "version.command.json").write_text(json.dumps([str(ledger), "--version"]) + "\n")
    (evidence / "version.stdout").write_bytes(version.stdout)
    (evidence / "version.stderr").write_bytes(version.stderr)
    (evidence / "version.exit.json").write_text(json.dumps({"exit_code": version.returncode}) + "\n")
    require(version.returncode == 0, "Cannot inspect verified executable version")
    identity = {"ledger_version": version.stdout.decode().strip(), "ledger": str(ledger), "ledger_sha256": hashlib.sha256(ledger.read_bytes()).hexdigest(),
                "example_sha256": hashlib.sha256(EXAMPLE.read_bytes()).hexdigest(),
                "checker_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "python": sys.version, "platform": sys.platform}
    if (ROOT / "BUILD-INFO.txt").is_file():
        identity["package_build_info"] = (ROOT / "BUILD-INFO.txt").read_text()
    (evidence / "identity.json").write_text(json.dumps(identity, indent=2) + "\n")
    print(f"Evidence: {evidence}", flush=True)
    for optimized in (False, True):
        suffix = "optimized" if optimized else "normal"
        for scenario in SCENARIOS:
            label = scenario + "-" + suffix
            run(label, evidence / label, scenario, optimized)
        existing = evidence / ("existing-directory-" + suffix)
        existing.mkdir(mode=0o700)
        (existing / "original-evidence.json").write_text('{"preserve":"original evidence"}\n')
        run("existing-directory-" + suffix, existing, optimized=optimized,
            error="Trial work path already exists:", preserve_existing=True)
        existing_file = evidence / ("existing-file-" + suffix)
        existing_file.write_text("preserve original file\n")
        run("existing-file-" + suffix, existing_file, optimized=optimized,
            error="Trial work path already exists:", preserve_existing=True)
        public = evidence / ("public-parent-" + suffix)
        public.mkdir(mode=0o755)
        public.chmod(0o755)
        run("public-refusal-" + suffix, public / "refused", optimized=optimized, error="has mode 0755")
        public.chmod(0o700)  # literal documented remedy for this deliberately created trial parent
        run("private-remedy-" + suffix, public / "fresh", optimized=optimized)
        absent = evidence / ("missing-parent-" + suffix) / "trial"
        run("missing-parent-" + suffix, absent, optimized=optimized, error="Missing trial parent:")
        link = evidence / ("symlink-parent-" + suffix)
        link.symlink_to(public, target_is_directory=True)
        run("symlink-parent-" + suffix, link / "trial", optimized=optimized, error="no symlink components")
        copied = evidence / ("copy-" + suffix)
        copied.mkdir(mode=0o700)
        copy = copied / EXAMPLE.name
        shutil.copyfile(EXAMPLE, copy)
        run("missing-fixtures-" + suffix, copied / "refused", optimized=optimized, example=copy,
            error="Missing bounded-cohort fixtures")
        run("explicit-fixtures-" + suffix, copied / "explicit", optimized=optimized, example=copy,
            extra=("--fixtures-dir", FIXTURES))
        shutil.copytree(FIXTURES, copied / "bounded-cohort")
        run("full-copy-" + suffix, copied / "full", optimized=optimized, example=copy)
        linked_fixtures = evidence / ("symlink-fixtures-" + suffix)
        linked_fixtures.symlink_to(FIXTURES, target_is_directory=True)
        run("symlink-fixtures-" + suffix, copied / "linked", optimized=optimized, example=copy,
            extra=("--fixtures-dir", linked_fixtures), error="no symlink components")
        (copied / "bounded-cohort/artifacts.json").unlink()
        (copied / "bounded-cohort/artifacts.json").symlink_to(FIXTURES / "artifacts.json")
        run("symlink-fixture-file-" + suffix, copied / "linked-file", optimized=optimized, example=copy,
            error="no symlink components")
    print(f"PASS all {len(results)} bounded checks; retained evidence: {evidence}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
