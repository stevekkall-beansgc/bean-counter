#!/usr/bin/env python3
"""One synthetic job: durable reserve, admission, process execution, verified outcome."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument("--ledger", required=True, type=Path)
parser.add_argument("--directory", required=True, type=Path)
args = parser.parse_args()
os.umask(0o077)
directory = args.directory.resolve()
# Candidate runs are intentionally confined to the supplied isolated worktree.
directory.relative_to(ROOT)
directory.mkdir(parents=True, exist_ok=False)
store = directory / "store"
binary = args.ledger.resolve()

def invoke(operation, request=None):
    argv = [str(binary), "zen-charge-candidate", operation, str(store)]
    if request is not None:
        argv.append(str(request))
    run = subprocess.run(argv + ["--json"], capture_output=True, text=True)
    if run.returncode:
        print(run.stdout or run.stderr, file=sys.stderr)
        raise SystemExit(run.returncode)
    return json.loads(run.stdout)

initial = invoke("init", ROOT / "contracts/candidates/admission-v1/setup.json")
binding = initial["orders"][0]["binding"]
request = {
    "schema": "admission/1-candidate.1", "binding": binding,
    "evidence": {"delivery_id": "demo-delivery", "attempt_id": "demo-attempt",
                 "session_id": "demo-session", "model": "synthetic/fixture-worker",
                 "outcome_id": "demo-outcome"},
}

def submit(operation, **extra):
    payload = dict(request, operation=operation, **extra)
    path = directory / (operation + ".json")
    with path.open("x") as f:
        json.dump(payload, f)
        f.flush()
        os.fsync(f.fileno())
    # Persist the directory entry before submission. Recovery uses this same file.
    fd = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)
    return invoke("submit", path)

submit("reserve")
submit("admit")
execution = subprocess.run([sys.executable, str(ROOT / "examples/zen-charge-pilot/fixture_worker.py")],
                           input=initial["deliverable_input"], text=True, capture_output=True)
try:
    artifact = json.loads(execution.stdout) if execution.returncode == 0 else None
except json.JSONDecodeError:
    artifact = None
if artifact is None:
    submit("fail")
else:
    # The independent host core checks exact artifact content and the preaccepted rule.
    submit("outcome", artifact=artifact)
print(json.dumps(invoke("statement"), indent=2))
