#!/usr/bin/env python3
"""Execute the README's literal quickstart against an installed binary.

Default: package docs/helper/binary from one installation. --ledger also supports
an explicitly identified source-correction test; it does not qualify an archive.
Temporary synthetic stores are removed unless --evidence-dir is supplied.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def documented_command(readme):
    section = readme.read_text(encoding="utf-8").split(
        "\n## Synthetic product journey\n", 1
    )[1].split("\n## ", 1)[0]
    blocks = re.findall(r"^```sh\n(.*?)^```$", section, re.MULTILINE | re.DOTALL)
    require(len(blocks) == 1, "expected exactly one synthetic quickstart shell block")
    return blocks[0]


def check(package, ledger, evidence):
    helper = package / "examples/integration/run-synthetic.sh"
    readme = package / "examples/integration/README.md"
    command = documented_command(readme)
    working_alias = evidence / "fresh working directory"
    working_alias.mkdir(mode=0o700)
    working = working_alias.resolve(strict=True)
    require(not any(working.iterdir()), "working directory must start empty")
    env = dict(os.environ, PACKAGE_ROOT=str(package), LEDGER=str(ledger))
    # Preserve a legitimate logical cwd alias to exercise the README's pwd -P.
    env["PWD"] = str(working_alias)
    before_binary = sha256(ledger)
    started = time.monotonic()
    result = subprocess.run(["sh", "-eu", "-c", command], cwd=working_alias, env=env,
                            text=True, capture_output=True, timeout=120)
    duration = time.monotonic() - started
    (evidence / "stdout.txt").write_text(result.stdout, encoding="utf-8")
    (evidence / "stderr.txt").write_text(result.stderr, encoding="utf-8")
    require(result.returncode == 0,
            f"documented quickstart exited {result.returncode}: {result.stderr}")
    results = working / "bean-counter-demo-results"

    def load(name):
        return json.loads((results / name).read_text(encoding="utf-8"))

    statement = load("statement.json")
    require(statement["complete"] is True and statement["net_atoms"] == "0",
            "expected a complete zero-net statement")
    expected = {
        "product-work-success-1": [("2", "base-posting")],
        "product-outcome-success-1": [("98", "replacement")],
        "product-work-unsuccessful-1": [("2", "base-posting")],
        "product-outcome-unsuccessful-1": [("-2", "replacement")],
        "product-correction-1": [("-2", "replacement"), ("-98", "inverse")],
    }
    entries = statement["entries"]
    require(len(entries) == len(expected), "expected exactly five retained entries")
    actual = {
        entry["external_id"]: [
            (p["body"]["amount"]["atoms"], p["body"].get("slot", "base-posting"))
            for p in entry["postings"]
        ] for entry in entries
    }
    require(actual == expected, f"unexpected signed posting set: {actual}")
    for first, retry in (
        ("accept-success.json", "retry-success.json"),
        ("accept-unsuccessful.json", "retry-unsuccessful.json"),
        ("outcome-success-result.json", "retry-outcome-success.json"),
        ("correction-result.json", "retry-correction.json"),
        ("accept-success.json", "backup-retry.json"),
    ):
        require(load(first)["receipt"] == load(retry)["receipt"],
                f"receipt changed: {first} / {retry}")
    for name in ("backup-statement.json", "backup-statement-after-retry.json"):
        backup = load(name)
        require(backup["complete"] is True and backup["net_atoms"] == "0"
                and backup["snapshot_hash"] == statement["snapshot_hash"],
                f"backup/retry changed statement: {name}")

    # Deliberately unsupported identities must fail before initialization. A
    # probe records every call, so accidental billing calls cannot masquerade
    # as a version refusal. Spaces also exercise quoting of executable paths.
    rejected = []
    for index, version in enumerate(("ledger 0.9.0 (local development)",
                                     "ledger 0.9.2 (local development)",
                                     "ledger 1.0.0 (local development)",
                                     "ledger 0.9.1 (unexpected build)")):
        probe = evidence / f"unsupported ledger {index}"
        calls = evidence / f"probe-{index}.jsonl"
        probe.write_text(
            "#!/usr/bin/env python3\nimport json,sys\n"
            f"with open({str(calls)!r}, 'a') as stream: "
            "stream.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            f"print({version!r}) if sys.argv[1:] == ['--version'] else sys.exit(99)\n",
            encoding="utf-8")
        probe.chmod(0o700)
        store, output = evidence / f"refused-store-{index}", evidence / f"refused-results-{index}"
        refusal = subprocess.run(["sh", str(helper), str(probe), str(store), str(output)],
                                 cwd=working, text=True, capture_output=True, timeout=10)
        require(refusal.returncode == 2 and not store.exists() and not output.exists(),
                f"unsupported identity was not refused before writes: {version}")
        require(calls.read_text(encoding="utf-8") == '["--version"]\n',
                "unsupported binary received a billing call or was not quoted")
        rejected.append({"version": version, "exit_code": refusal.returncode})

    require(sha256(ledger) == before_binary, "binary changed during quickstart")
    report = {
        "schema": "bean-counter-synthetic-quickstart-check/1",
        "passed": True,
        "documentation_root": str(package),
        "ledger": str(ledger),
        "ledger_sha256": before_binary,
        "ledger_version": subprocess.check_output([str(ledger), "--version"], text=True).strip(),
        "readme_sha256": sha256(readme),
        "helper_sha256": sha256(helper),
        "exact_documented_command": command,
        "fresh_working_directory": str(working),
        "execution_seconds": round(duration, 3),
        "timing_scope": "author/CI execution duration; not unfamiliar-user adoption timing",
        "retained_entries": len(entries),
        "net_atoms": statement["net_atoms"],
        "snapshot_hash": statement["snapshot_hash"],
        "identical_receipts_and_backup": True,
        "refused_identities": rejected,
    }
    (evidence / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--ledger", type=Path, help="explicit source-correction binary override")
    parser.add_argument("--evidence-dir", type=Path, help="new private directory to retain synthetic evidence")
    args = parser.parse_args()
    package = args.package.resolve(strict=True)
    ledger = (args.ledger or package / "ledger").resolve(strict=True)
    if args.evidence_dir:
        evidence = args.evidence_dir.absolute()
        evidence.mkdir(mode=0o700)
        report = check(package, ledger, evidence)
    else:
        with tempfile.TemporaryDirectory(prefix="bean-counter-quickstart-") as directory:
            report = check(package, ledger, Path(directory))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
