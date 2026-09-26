#!/usr/bin/env python3
"""Synthetic caller outbox for one ordinary billing accept request.

Usage: python3 billing_outbox.py LEDGER BILLING_DIR EVENT_JSON OUTBOX_DIR
The outbox and installation require a private, durable local filesystem.
Run one caller process per outbox; do not use real customer data here.
"""

import json
import os
from pathlib import Path
import stat
import subprocess
import sys


def stop(message):
    raise SystemExit(message)


def sync_dir(path):
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def sync_file(path):
    fd = os.open(path, os.O_RDWR)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def save_new(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
    finally:
        sync_dir(path.parent)


def save_receipt(path, receipt):
    data = (json.dumps(receipt, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if path.exists():
        if json.loads(path.read_bytes()) != receipt:
            stop("saved receipt differs; investigate before acknowledging")
        sync_file(path)
        sync_dir(path.parent)
        return
    temporary = path.with_name(path.name + ".pending")
    if temporary.exists():
        if temporary.read_bytes() != data:
            stop("incomplete or different pending receipt; investigate before acknowledging")
    else:
        save_new(temporary, data)
    sync_file(temporary)
    sync_dir(path.parent)
    os.replace(temporary, path)
    sync_file(path)
    sync_dir(path.parent)


def run_json(argv):
    result = subprocess.run(argv, capture_output=True, text=True)
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError:
        stop(f"ledger returned non-JSON output (exit {result.returncode})")
    return result.returncode, value


def verified_receipt(ledger, installation, operation_id, receipt):
    if not isinstance(receipt, dict) or receipt.get("kind") != "base-acceptance":
        stop("missing base-acceptance receipt")
    target = receipt.get("body", {}).get("target")
    if not isinstance(target, str) or not target or not isinstance(receipt.get("id"), str):
        stop("receipt ID or target is missing")
    code, history = run_json([ledger, "billing", "--directory", installation, "explain", target, "--json"])
    if code != 0 or history.get("schema") != "ledger-billing-statement/1" or history.get("complete") is not True:
        stop("complete target explanation unavailable; request remains unacknowledged")
    matches = [entry for entry in history.get("entries", [])
               if entry.get("target") == target and entry.get("receipt") == receipt
               and entry["receipt"].get("id") == receipt["id"]
               and len([record for record in entry.get("records", [])
                        if record.get("kind") == "event"
                        and record.get("body", {}).get("data", {}).get("operation_id") == operation_id]) == 1]
    if len(matches) != 1:
        stop("receipt does not match the retained original operation")
    return receipt


def main():
    if len(sys.argv) != 5:
        stop(__doc__)
    ledger, installation, event_file, outbox_name = sys.argv[1:]
    installation = str(Path(installation).resolve(strict=True))
    source = Path(event_file).read_bytes()
    try:
        event = json.loads(source)
    except (ValueError, UnicodeDecodeError):
        stop("event must be strict JSON")
    if not isinstance(event, dict) or event.get("schema") != "ledger-event/1" or any(
        not isinstance(event.get(key), str) or not event[key] for key in ("id", "operation_id")
    ):
        stop("event must contain stable id and operation_id")

    outbox = Path(outbox_name)
    if not outbox.exists():
        outbox.mkdir(mode=0o700)
    mode = outbox.stat().st_mode
    if outbox.is_symlink() or not stat.S_ISDIR(mode):
        stop("outbox must be a private directory, not a symlink")
    if mode & 0o077:
        stop("outbox must have no group or other permission bits")
    sync_dir(outbox.parent)
    marker = outbox / "installation.path"
    installed = (installation + "\n").encode("utf-8")
    if marker.exists():
        if marker.read_bytes() != installed:
            stop("outbox belongs to another canonical installation path")
    else:
        save_new(marker, installed)
    sync_file(marker)
    sync_dir(outbox)
    request = outbox / "request.json"
    if request.exists():
        saved = request.read_bytes()
        if saved != source:
            stop("event bytes differ from the pending request; retain the original IDs and bytes")
        saved_event = json.loads(saved)
        if (saved_event["id"], saved_event["operation_id"]) != (event["id"], event["operation_id"]):
            stop("saved request IDs differ")
    else:
        save_new(request, source)
    sync_file(request)
    sync_dir(outbox)

    code, response = run_json([ledger, "billing", "--directory", installation, "accept", str(request), "--json"])
    if code == 8:
        print("Outcome unknown. Keep request.json pending and retry this identical file.", file=sys.stderr)
        return 8
    if code != 0 or response.get("status") not in ("accepted", "duplicate"):
        print(f"Accept not acknowledged (exit {code}): {response}", file=sys.stderr)
        return code or 1
    receipt = verified_receipt(ledger, installation, event["operation_id"], response.get("receipt"))
    save_receipt(outbox / "receipt.json", receipt)
    print(f"Acknowledged {response['status']} with original receipt {receipt['id']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
