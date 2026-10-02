#!/usr/bin/env python3
"""Exercise the installed Bean Counter binary through its public CLI.

This portable acceptance journey uses only synthetic data. Run it from an
extracted package with the packaged examples beside this script; no checkout
or Rust/Python project imports are used.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import io
import json
import os
import platform
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


def fail(message: str) -> None:
    raise RuntimeError(message)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def iso(value: dt.datetime) -> str:
    return value.astimezone(dt.timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, separators=(",", ":"), ensure_ascii=False) + "\n", encoding="utf-8")


def run(binary: Path, cwd: Path, args: list[str], expected: int | tuple[int, ...] = 0) -> dict[str, Any]:
    completed = subprocess.run(
        [str(binary), *args, "--json"], cwd=cwd, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    allowed = (expected,) if isinstance(expected, int) else expected
    if completed.returncode not in allowed:
        fail(
            f"{binary.name} {' '.join(args)} exited {completed.returncode}, expected {allowed}; "
            f"stdout={completed.stdout!r}; stderr={completed.stderr!r}"
        )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        fail(f"{binary.name} {' '.join(args)} did not return one JSON result: {error}; stdout={completed.stdout!r}")
    if not isinstance(result, dict):
        fail(f"{binary.name} {' '.join(args)} returned a non-object JSON result")
    return result


def store_command(binary: Path, root: Path, store: str, *args: str, expected: int = 0) -> dict[str, Any]:
    return run(binary, root, ["billing", "--directory", store, *args], expected)


SQLITE_WAL_PATH = ".ledger/local.db-wal"


def file_hashes(root: Path, *, exclude_sqlite_shm: bool = False) -> dict[str, str]:
    return {
        str(path.relative_to(root)): digest(path)
        for path in sorted(root.rglob("*"))
        if path.is_file() and not path.is_symlink()
        and not (exclude_sqlite_shm
                 and str(path.relative_to(root)) == ".ledger/local.db-shm")
    }


def sqlite_sidecar_snapshot(path: Path) -> dict[str, Any]:
    require(not path.is_symlink(), f"SQLite sidecar is a symlink: {path}")
    if not path.exists():
        return {"present": False, "size_bytes": None, "sha256": None, "zero_byte": False}
    require(path.is_file() and not path.is_symlink(), f"SQLite sidecar is not a regular file: {path}")
    size = path.stat().st_size
    return {"present": True, "size_bytes": size, "sha256": digest(path), "zero_byte": size == 0}


def require_same_durable_files(
    before_hashes: dict[str, str], after_hashes: dict[str, str],
    before_wal: dict[str, Any], after_wal: dict[str, Any], phase: str,
    *, exclude_empty_sqlite_wal: bool = False,
) -> bool:
    if before_hashes == after_hashes:
        return False
    wal_states = (before_wal, after_wal)
    empty_wal_lifecycle = (
        exclude_empty_sqlite_wal
        and
        before_wal != after_wal
        and all(not state["present"] or state["size_bytes"] == 0 for state in wal_states)
    )
    if empty_wal_lifecycle:
        before_without_wal = dict(before_hashes)
        after_without_wal = dict(after_hashes)
        before_without_wal.pop(SQLITE_WAL_PATH, None)
        after_without_wal.pop(SQLITE_WAL_PATH, None)
        if before_without_wal == after_without_wal:
            return True
    changed_paths = sorted(set(before_hashes) | set(after_hashes))
    changed_paths = [path for path in changed_paths if before_hashes.get(path) != after_hashes.get(path)]
    fail(
        f"{phase} changed durable installation files: {changed_paths}; "
        f"SQLite WAL before={before_wal}, after={after_wal}"
    )


def sqlite_user_version(store: Path) -> int:
    database = (store / ".ledger" / "local.db").resolve(strict=True)
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        row = connection.execute("PRAGMA user_version").fetchone()
        require(row is not None and isinstance(row[0], int), f"could not read SQLite user_version from {database}")
        return row[0]
    finally:
        connection.close()


def legacy_billing_snapshot(store: Path) -> dict[str, Any]:
    database = (store / ".ledger" / "local.db").resolve(strict=True)
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        tables = connection.execute(
            "SELECT name,sql FROM sqlite_schema WHERE type='table' "
            "AND name LIKE 'billing_%' AND name NOT LIKE 'billing_m5_%' ORDER BY name"
        ).fetchall()
        snapshot: dict[str, Any] = {}
        for table, create_sql in tables:
            quoted_table = '"' + table.replace('"', '""') + '"'
            columns = [row[1] for row in connection.execute(f"PRAGMA table_info({quoted_table})")]
            projection = ",".join('"' + column.replace('"', '""') + '"' for column in columns)
            ordering = ",".join('"' + column.replace('"', '""') + '"' for column in columns)
            rows = connection.execute(f"SELECT {projection} FROM {quoted_table} ORDER BY {ordering}").fetchall()
            normalized_rows = [
                [({"sqlite_blob_hex": value.hex()} if isinstance(value, bytes) else value) for value in row]
                for row in rows
            ]
            snapshot[table] = {"create_sql": create_sql, "columns": columns, "rows": normalized_rows}
        require(snapshot, "schema-10 installation contains no retained billing tables")
        return snapshot
    finally:
        connection.close()


def presentation_claim_snapshot(store: Path) -> list[list[Any]]:
    database = (store / ".ledger" / "local.db").resolve(strict=True)
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        rows = connection.execute(
            "SELECT customer,source_scope,adjustment_id,presentation_kind,statement_id,record_sequence "
            "FROM billing_m5_presentation_claims ORDER BY customer,source_scope,adjustment_id"
        ).fetchall()
        return [[({"sqlite_blob_hex": value.hex()} if isinstance(value, bytes) else value) for value in row]
                for row in rows]
    finally:
        connection.close()


def tree_copy(source: Path, destination: Path) -> None:
    # Copy only after the prior subprocess has exited, following the documented
    # quiescent whole-installation recovery boundary.
    shutil.copytree(source, destination, copy_function=shutil.copy2)


def fixture_root(script: Path) -> Path:
    candidate = script.resolve().parent.parent
    if (candidate / "examples" / "billing" / "setup.json").is_file():
        return candidate
    fail("package journey requires the packaged examples/billing/setup.json beside scripts/")


def os_release() -> dict[str, str]:
    values: dict[str, str] = {}
    try:
        for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
            if "=" in line and not line.startswith("#"):
                key, value = line.split("=", 1)
                values[key] = value.strip().strip('"')
    except OSError:
        pass
    return {key: values[key] for key in ("ID", "VERSION_ID", "PRETTY_NAME") if key in values}


def setup_store(binary: Path, root: Path, store: str, setup: dict[str, Any]) -> None:
    fixture = root / "setup.json"
    write_json(fixture, setup)
    result = run(binary, root, ["billing", "init", store, "--setup", fixture.name])
    require(result.get("status") == "initialized", f"fresh store initialization failed: {result}")


def daily_term(customer: str, change_id: str, start: dt.datetime) -> dict[str, Any]:
    anchor = start.date().isoformat()
    return {
        "schema": "ledger-billing-term/1", "customer": customer,
        "change_id": change_id, "expected_revision": "0",
        "effective": {"mode": "initial", "at": iso(start)},
        "term": {
            "interval": 1, "unit": "day", "alignment": "anchored",
            "anchor": {"date": anchor, "time": start.strftime("%H:%M:%S")}, "timezone": "UTC",
            "month_end_rule": "preserve_anchor_and_clamp",
            "boundary_rule_version": "billing-boundary/1",
            "timezone_rules_version": "IANA-2025b", "proration": "none",
        },
    }


def set_term(binary: Path, root: Path, store: str, value: dict[str, Any], name: str) -> dict[str, Any]:
    write_json(root / name, value)
    first = store_command(binary, root, store, "term", "set", name)
    require(first.get("status") == "term_updated", f"customer terms were not accepted: {first}")
    require(store_command(binary, root, store, "term", "set", name) == first,
            "exact customer-term retry did not return the retained response")
    changed = dict(value)
    changed["term"] = dict(value["term"], interval=2)
    write_json(root / name, changed)
    conflict = store_command(binary, root, store, "term", "set", name, expected=4)
    require(conflict.get("code") == "IDENTITY_CONFLICT", f"changed term identity was not rejected: {conflict}")
    write_json(root / name, value)
    return first


def run_journey(binary: Path, package_root: Path, old_binary: Path | None) -> dict[str, Any]:
    binary = binary.resolve(strict=True)
    require(binary.is_file() and os.access(binary, os.X_OK), f"installed ledger binary is not executable: {binary}")
    binary_hash = digest(binary)
    version = subprocess.run([str(binary), "--version"], text=True, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, check=False)
    require(version.returncode == 0, f"installed binary --version failed: {version.stderr}")
    manifest_path = binary.parent / "MANIFEST.json"
    manifest_evidence: dict[str, Any] = {"present": False}
    if manifest_path.is_file():
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            fail(f"installed package MANIFEST.json is unreadable: {error}")
        require(manifest.get("binary_sha256") == binary_hash,
                "installed binary digest differs from adjacent MANIFEST.json")
        manifest_evidence = {
            "present": True,
            "schema": manifest.get("schema"),
            "source_commit": manifest.get("source_commit"),
            "target": manifest.get("target"),
            "version": manifest.get("version"),
            "binary_sha256_verified": True,
        }
    evidence: dict[str, Any] = {
        "schema": "bean-counter-native-package-journey/1",
        "binary": {"path_basename": binary.name, "version_output": version.stdout.strip(), "sha256": binary_hash,
                   "package_manifest": manifest_evidence},
        "host": {
            "system": platform.system(), "release": platform.release(),
            "version": platform.version(), "machine": platform.machine(),
            "python": platform.python_version(), "sqlite_runtime": sqlite3.sqlite_version,
            "os_release": os_release(), "libc": list(platform.libc_ver()),
        },
        "acceptance_time_utc": iso(dt.datetime.now(dt.timezone.utc)),
        "assertions": {},
    }
    assertions: dict[str, Any] = evidence["assertions"]
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0)
    # Put the current actual-time period boundary about 90 seconds ahead. This
    # lets the CLI accept a pre-close correction, wait for the real UTC
    # boundary, close it, and then accept a post-close adjustment without
    # overriding the program clock or fabricating accepted_at values.
    yesterday = (now - dt.timedelta(days=1)).replace(hour=0, minute=0, second=0, microsecond=0)
    current_start = now.replace(hour=0, minute=0, second=0, microsecond=0)
    event_time = iso(now)

    with tempfile.TemporaryDirectory(prefix="bean-counter-native-journey-") as temporary:
        root = Path(temporary)

        base = json.loads((package_root / "examples/billing/setup.json").read_text(encoding="utf-8"))
        base["accepted_at"] = iso(yesterday)
        base["outcome_policy"]["families"][0]["ordinary"]["starts_at"] = event_time
        base["outcome_policy"]["families"][0]["corrections"]["starts_at"] = event_time
        setup_store(binary, root, "store", base)
        term_anchor = dt.datetime.now(dt.timezone.utc).replace(microsecond=0) - dt.timedelta(days=1) + dt.timedelta(seconds=90)
        fixed_event = json.loads((package_root / "examples/billing/event.json").read_text(encoding="utf-8"))
        fixed_event["id"] = "native-journey-fixed-work"
        fixed_event["operation_id"] = "native-journey-fixed-operation"
        write_json(root / "fixed-event.json", fixed_event)

        fixed = store_command(binary, root, "store", "accept", "--customer", base["customer"],
                              "--source", base["source"], "fixed-event.json")
        require(fixed.get("status") == "accepted", f"fixed-price work did not accept: {fixed}")
        fixed_retry = store_command(binary, root, "store", "accept", "--customer", base["customer"],
                                    "--source", base["source"], "fixed-event.json")
        require(fixed_retry.get("status") == "duplicate" and fixed_retry.get("receipt") == fixed.get("receipt"),
                f"fixed-price exact retry did not return the original receipt: {fixed_retry}")
        fixed_conflict = dict(fixed_event, quantity="2")
        write_json(root / "fixed-event.json", fixed_conflict)
        conflict = store_command(binary, root, "store", "accept", "--customer", base["customer"],
                                 "--source", base["source"], "fixed-event.json", expected=4)
        require(conflict.get("code") == "IDENTITY_CONFLICT", f"fixed-price retry conflict was not rejected: {conflict}")
        write_json(root / "fixed-event.json", fixed_event)
        assertions["fixed_price_accept_retry_conflict"] = {"accepted": True, "retry_equal": True, "conflict_code": conflict["code"]}

        term = daily_term(base["customer"], "native-journey-term", term_anchor)
        term_result = set_term(binary, root, "store", term, "term.json")
        assertions["customer_terms_retry_conflict"] = {"revision": term_result.get("revision"), "passed": True}

        # Use the packaged scale-18 per-work fixture as a second local
        # installation. Its usage work is accepted at the actual local time,
        # with no clock override.
        usage = json.loads((package_root / "examples/billing/usage/setup.json").read_text(encoding="utf-8"))
        usage["customer"] = "native-journey-usage"
        usage["accepted_at"] = iso(yesterday)
        usage["outcome_policy"]["families"][0]["ordinary"]["starts_at"] = event_time
        usage["outcome_policy"]["families"][0]["corrections"]["starts_at"] = event_time
        usage["outcome_policy"]["families"][0]["binding_id"] = usage["binding"]
        usage["outcome_policy"]["limits"][0]["binding_id"] = usage["binding"]
        usage["outcome_policy"]["families"][0]["source"] = usage["source"]
        usage["outcome_policy"]["families"][0]["correction_source"] = usage["source"]
        setup_store(binary, root, "usage-store", usage)
        usage_term = daily_term(usage["customer"], "native-journey-usage-term", term_anchor)
        set_term(binary, root, "usage-store", usage_term, "usage-term.json")
        usage_event = json.loads((package_root / "examples/billing/usage/event.json").read_text(encoding="utf-8"))
        usage_event["id"] = "native-journey-scale18-work"
        usage_event["operation_id"] = "native-journey-scale18-operation"
        usage_event["customer"] = usage["customer"]
        write_json(root / "usage-event.json", usage_event)
        usage_result = store_command(binary, root, "usage-store", "accept", "--customer", usage["customer"],
                                     "--source", usage["source"], "usage-event.json")
        require(usage_result.get("status") == "accepted", f"scale-18 usage work did not accept: {usage_result}")
        usage_retry = store_command(binary, root, "usage-store", "accept", "--customer", usage["customer"],
                                    "--source", usage["source"], "usage-event.json")
        require(usage_retry.get("status") == "duplicate" and usage_retry.get("receipt") == usage_result.get("receipt"),
                f"scale-18 exact retry did not return original receipt: {usage_retry}")
        usage_statement = store_command(binary, root, "usage-store", "statement", "--customer", usage["customer"])
        require(usage_statement.get("scale") == 18 and usage_statement.get("net_atoms") == "25000000000000",
                f"scale-18 usage statement did not preserve exact atoms: {usage_statement}")
        assertions["scale18_usage_work"] = {"scale": 18, "net_atoms": usage_statement["net_atoms"], "passed": True}

        # Cumulative activity uses the fixed customer and its existing source.
        # The daily term gives period 0 an end about 90 seconds ahead; period 1
        # remains live for default post-close adjustments.
        basis = {
            "schema": "ledger-billing-cumulative-agreement/1", "customer": base["customer"],
            "source": base["source"], "change_id": "native-journey-cumulative-basis",
            "expected_revision": "0", "agreement_id": base["agreement"], "agreement_version": "1",
            "effective_at": iso(term_anchor),
            "basis": {"mode": "cumulative_period", "source_unit": "token", "billable_unit": "billable-token",
                      "conversion_numerator": "1", "conversion_denominator": "2",
                      "rate_usd_per_billable_unit": "0.000000000000000002", "maximum_period_quantity": "1000"},
        }
        write_json(root / "basis.json", basis)
        basis_result = store_command(binary, root, "store", "cumulative", "setup", "basis.json")
        require(basis_result.get("status") == "basis_updated", f"cumulative basis setup failed: {basis_result}")
        activity = {
            "schema": "ledger-billing-activity/1", "customer": base["customer"], "source": base["source"],
            "id": "native-journey-cumulative-activity", "operation_id": "native-journey-activity-operation",
            "target": "native-journey-cumulative-target", "quantity": "10", "occurred_at": event_time,
            "evidence": "Synthetic native package qualification activity.",
        }
        write_json(root / "activity.json", activity)
        activity_result = store_command(binary, root, "store", "activity", "activity.json")
        require(activity_result.get("status") == "accepted", f"cumulative activity did not accept: {activity_result}")
        require(store_command(binary, root, "store", "activity", "activity.json") == activity_result,
                "cumulative activity exact retry changed its retained result")

        # Correct while the live actual-time period is open; close after its
        # real boundary; then apply a second delta and prove it appears only as
        # a next-period adjustment while the original statement stays fixed.
        preclose_delta = {
            "schema": "ledger-billing-quantity-correction/1", "customer": base["customer"],
            "source": base["source"], "id": "native-journey-preclose-delta",
            "target": activity["id"], "quantity_delta": "-2", "occurred_at": event_time,
            "evidence": "Synthetic pre-close cumulative quantity correction.",
        }
        write_json(root / "preclose-correction.json", preclose_delta)
        preclose_result = store_command(binary, root, "store", "correct", "preclose-correction.json")
        require(preclose_result.get("status") == "accepted", f"pre-close quantity delta failed: {preclose_result}")
        require(store_command(binary, root, "store", "correct", "preclose-correction.json") == preclose_result,
                "pre-close correction exact retry changed its retained response")
        period_end = term_anchor + dt.timedelta(days=1)
        boundary_wait = max(0.0, (period_end - dt.datetime.now(dt.timezone.utc)).total_seconds())
        if boundary_wait > 125:
            fail(f"unexpectedly distant live billing-period boundary: {boundary_wait:.3f}s")
        if boundary_wait > 0:
            time.sleep(boundary_wait + 0.25)
        close_request = {"schema": "ledger-billing-period-close/1", "customer": base["customer"],
                         "period_id": {"term_version": "1", "period_index": "0"}}
        write_json(root / "close.json", close_request)
        closed = store_command(binary, root, "store", "close", "close.json")
        require(closed.get("schema") == "ledger-billing-statement/4" and closed.get("status") == "closed",
                f"live period close after its actual boundary failed: {closed}")
        require(closed.get("scale") == 18 and closed.get("net_atoms") == "2500000000000000008",
                f"close did not aggregate $2.50 fixed work plus the exact 8-atom cumulative booking: {closed}")
        close_lines = closed.get("lines")
        require(isinstance(close_lines, list), f"period close omitted statement lines: {closed}")
        cumulative_lines = [line for line in close_lines if line.get("basis") == "cumulative_close"]
        require(len(cumulative_lines) == 1 and cumulative_lines[0].get("amount_atoms") == "8",
                f"cumulative aggregate did not convert 8 source units to exactly 8 booked atoms: {close_lines}")
        require(store_command(binary, root, "store", "close", "close.json") == closed,
                "period close exact retry changed immutable statement")

        postclose_delta = dict(preclose_delta, id="native-journey-postclose-delta", quantity_delta="2",
                               evidence="Synthetic post-close cumulative quantity correction.")
        write_json(root / "correction.json", postclose_delta)
        corrected = store_command(binary, root, "store", "correct", "correction.json")
        require(corrected.get("status") == "accepted", f"post-close quantity delta failed: {corrected}")
        require(store_command(binary, root, "store", "correct", "correction.json") == corrected,
                "post-close correction exact retry changed retained adjustment")
        after_close = store_command(binary, root, "store", "close", "close.json")
        require(after_close == closed, "post-close correction changed the immutable closed statement")
        adjustment = corrected.get("adjustment")
        require(isinstance(adjustment, dict) and adjustment.get("signed_delta_atoms") == "2",
                f"post-close cumulative telescoping did not create the exact 2-atom adjustment: {corrected}")
        adhoc = {
            "schema": "ledger-billing-ad-hoc-statement/1", "customer": base["customer"],
            "command_id": "native-journey-adjustment-statement",
            "adjustments": [{"source": base["source"], "adjustment_id": adjustment["adjustment_id"]}],
        }
        write_json(root / "adjustment-statement.json", adhoc)
        ad_hoc_statement = store_command(binary, root, "store", "adjustment", "statement", "adjustment-statement.json")
        ad_hoc_lines = ad_hoc_statement.get("lines")
        require(ad_hoc_statement.get("schema") == "ledger-billing-ad-hoc-statement-result/1"
                and ad_hoc_statement.get("net_atoms") == "2"
                and isinstance(ad_hoc_lines, list) and len(ad_hoc_lines) == 1
                and ad_hoc_lines[0].get("basis") == "post_close_adjustment"
                and ad_hoc_lines[0].get("amount_atoms") == "2",
                f"ad hoc statement did not present the post-close effect exactly once: {ad_hoc_statement}")
        require(store_command(binary, root, "store", "adjustment", "statement", "adjustment-statement.json") == ad_hoc_statement,
                "ad hoc adjustment statement exact retry changed its retained result")
        claims_before_conflict = presentation_claim_snapshot(root / "store")
        changed_presentation = dict(adhoc, command_id="native-journey-adjustment-second-presentation")
        write_json(root / "adjustment-statement.json", changed_presentation)
        repeated_presentation = store_command(binary, root, "store", "adjustment", "statement",
                                              "adjustment-statement.json", expected=3)
        require(repeated_presentation.get("code") == "BILLING_M5_PRESENTED",
                f"a second ad hoc statement presented an already-claimed adjustment: {repeated_presentation}")
        require(presentation_claim_snapshot(root / "store") == claims_before_conflict,
                "second ad hoc presentation attempt changed unique presentation-claim state")
        write_json(root / "adjustment-statement.json", adhoc)
        require(store_command(binary, root, "store", "adjustment", "statement", "adjustment-statement.json") == ad_hoc_statement,
                "original ad hoc statement changed after second-presentation refusal")
        require(store_command(binary, root, "store", "close", "close.json") == closed,
                "second ad hoc presentation attempt changed the closed customer statement")

        write_json(root / "mapping.json", {"schema": "ledger-finance-mapping/1", "accounts": {
            base["customer"]: "synthetic-customer-account", base["host"]: "synthetic-merchant-account"}})
        export_args = ["export-csv", "--customer", base["customer"], "--snapshot",
                       closed["statement_hash"], "--mapping", "mapping.json", "--output", "finance.csv"]
        exported = store_command(binary, root, "store", *export_args)
        require(exported.get("schema") == "ledger-finance-export/4" and exported.get("complete") is True,
                f"statement/4 finance export/4 failed: {exported}")
        export_bytes = (root / "finance.csv").read_bytes()
        try:
            csv_rows = list(csv.DictReader(io.StringIO(export_bytes.decode("utf-8"), newline="")))
        except (UnicodeDecodeError, csv.Error) as error:
            fail(f"finance export is not valid UTF-8 CSV: {error}")
        posting_rows = [row for row in csv_rows if row.get("row_type") == "posting"]
        trailers = [row for row in csv_rows if row.get("row_type") == "complete"]
        csv_amount_sum = sum(int(row["amount_atoms"]) for row in posting_rows)
        require(export_bytes.endswith(b"\r\n") and export_bytes.count(b"\r\n") == len(csv_rows) + 1,
                "finance export is incomplete or not CRLF delimited")
        require(len(posting_rows) == int(exported["posting_count"]) and len(trailers) == 1
                and csv_rows[-1] is trailers[0], "finance export posting count or completion trailer is invalid")
        require(csv_amount_sum == 2500000000000000008
                and exported.get("control_net_atoms") == str(csv_amount_sum) == closed.get("net_atoms")
                and trailers[0].get("control_net_atoms") == str(csv_amount_sum),
                f"finance CSV posting amount sum does not reconcile independently to statement/4: {csv_amount_sum}")
        repeat = store_command(binary, root, "store", *export_args[:-1], "finance-repeat.csv")
        require(repeat == exported and (root / "finance-repeat.csv").read_bytes() == export_bytes,
                "statement/4 finance export exact retry changed result or bytes")
        assertions["cumulative_close_postclose_adjustment_export"] = {
            "closed_statement_hash_stable": True,
            "preclose_delta": preclose_result.get("status"),
            "adjustment_status": corrected.get("status"),
            "closed_net_atoms": closed.get("net_atoms"),
            "cumulative_close_atoms": cumulative_lines[0]["amount_atoms"],
            "postclose_adjustment_atoms": adjustment["signed_delta_atoms"],
            "ad_hoc_adjustment_lines": len(ad_hoc_lines),
            "second_presentation_refusal": repeated_presentation.get("code"),
            "presentation_claims_unchanged_on_conflict": True,
            "boundary_wait_seconds": round(boundary_wait, 3),
            "finance_schema": exported.get("schema"),
            "posting_count": exported.get("posting_count"),
            "control_net_atoms": exported.get("control_net_atoms"),
            "export_retry_bytes_equal": True,
        }

        fiscal = {
            "schema": "ledger-fiscal-calendar/1", "change_id": "native-journey-fiscal-calendar",
            "expected_revision": "0", "timezone": "UTC", "timezone_rules_version": "IANA-2025b",
            "calendar": {"kind": "gregorian_years", "fiscal_year_start_month": 1, "fiscal_year_start_day": 1},
        }
        write_json(root / "fiscal.json", fiscal)
        fiscal_set = store_command(binary, root, "store", "fiscal", "set", "fiscal.json")
        require(fiscal_set.get("status") == "calendar_updated", f"fiscal calendar setup failed: {fiscal_set}")
        fiscal_report_request = {
            "schema": "ledger-fiscal-report-request/1", "command_id": "native-journey-fiscal-report",
            "calendar_version": "1", "start": f"{now.year:04d}-01-01T00:00:00.000000Z",
            "end": f"{now.year + 1:04d}-01-01T00:00:00.000000Z",
        }
        write_json(root / "fiscal-report.json", fiscal_report_request)
        report = store_command(binary, root, "store", "fiscal", "report", "fiscal-report.json")
        require(report.get("schema") == "ledger-fiscal-report/1" and report.get("status") == "complete",
                f"fiscal report failed: {report}")
        fiscal_lines = report.get("monetary_lines")
        fiscal_sum = sum(int(line["amount_atoms"]) for line in fiscal_lines) if isinstance(fiscal_lines, list) else None
        require(isinstance(fiscal_lines, list) and len(fiscal_lines) == 3
                and fiscal_sum == 2500000000000000010
                and report.get("net_atoms") == str(fiscal_sum),
                f"fiscal report lines did not independently reconcile to fixed work plus close and post-close adjustment: {report}")
        require(store_command(binary, root, "store", "fiscal", "report", "fiscal-report.json") == report,
                "fiscal report exact retry changed its pinned result")
        assertions["fiscal_report_retry"] = {
            "calendar_version": "1", "status": report.get("status"),
            "monetary_line_count": len(fiscal_lines), "net_atoms": report.get("net_atoms"), "passed": True,
        }

        # Quiescent backup of the complete installation (all processes above
        # have exited), then verify statement and every exercised retry using
        # the same installed binary against the copied tree.
        m3_statement = store_command(binary, root, "store", "statement", "--customer", base["customer"])
        store_hashes = file_hashes(root / "store")
        tree_copy(root / "store", root / "store-backup")
        backup_hashes = file_hashes(root / "store-backup")
        require(backup_hashes == store_hashes,
                "quiescent whole-installation backup file hashes differ before restore/reopen")
        restored = store_command(binary, root, "store-backup", "statement", "--customer", base["customer"])
        require(restored == m3_statement, "restored installation changed the complete retained M3 statement")
        require(store_command(binary, root, "store-backup", "close", "close.json") == closed,
                "restored installation lost the immutable period close identity")
        require(store_command(binary, root, "store-backup", "correct", "correction.json") == corrected,
                "restored installation lost the post-close correction identity")
        require(store_command(binary, root, "store-backup", "adjustment", "statement", "adjustment-statement.json") == ad_hoc_statement,
                "restored installation lost the adjustment presentation identity")
        backup_export_args = [*export_args[:-1], "backup-finance.csv"]
        backup_export = store_command(binary, root, "store-backup", *backup_export_args)
        require(backup_export == exported and (root / "backup-finance.csv").read_bytes() == export_bytes,
                "restored installation lost the statement/export identity or bytes")
        assertions["quiescent_whole_installation_backup_restore"] = {
            "source_file_count": len(store_hashes), "backup_file_count": len(backup_hashes),
            "all_source_and_backup_file_hashes_equal_before_reopen": True,
            "statement_and_retry_identities_preserved": True,
        }

        # Recurrence uses current acceptance-time dates and an explicit manual
        # renewal rule. Query and accept one due occurrence; retry the durable
        # occurrence identity and a changed identity payload.
        # Keep the recurrence fixture independent of the deliberately closed
        # daily period above. It qualifies the recurrence lifecycle through
        # the same installed binary without depending on term-period timing.
        recurrence_store = json.loads((package_root / "examples/billing/setup.json").read_text(encoding="utf-8"))
        recurrence_store["store_id"] = "native-journey-recurrence-store"
        recurrence_store["accepted_at"] = iso(yesterday)
        recurrence_store["outcome_policy"]["families"][0]["ordinary"]["starts_at"] = event_time
        recurrence_store["outcome_policy"]["families"][0]["corrections"]["starts_at"] = event_time
        setup_store(binary, root, "recurrence-store", recurrence_store)
        recurrence_term = {
            "schema": "ledger-billing-term/1", "customer": recurrence_store["customer"],
            "change_id": "native-journey-recurrence-term", "expected_revision": "0",
            "effective": {"mode": "initial", "at": iso(current_start)},
            "term": {"interval": 1, "unit": "month", "alignment": "anchored",
                     "anchor": {"date": now.date().isoformat(), "time": "00:00:00"}, "timezone": "UTC",
                     "month_end_rule": "preserve_anchor_and_clamp", "boundary_rule_version": "billing-boundary/1",
                     "timezone_rules_version": "IANA-2025b", "proration": "none"},
        }
        set_term(binary, root, "recurrence-store", recurrence_term, "recurrence-term.json")
        recurrence = {
            "schema": "ledger-billing-recurrence/1", "customer": recurrence_store["customer"],
            "source": recurrence_store["source"], "change_id": "native-journey-recurrence", "expected_revision": "0",
            "agreement_id": base["agreement"], "agreement_version": "1",
            "rule": {"interval": 1, "unit": "month", "anchor": {"date": now.date().isoformat(), "time": "00:00:00"},
                     "timezone": "UTC", "effective_from": iso(current_start),
                     "boundary_rule_version": "billing-boundary/1", "timezone_rules_version": "IANA-2025b",
                     "proration": "none"},
            "renewal": {"mode": "manual"},
        }
        write_json(root / "recurrence.json", recurrence)
        recurrence_result = store_command(binary, root, "recurrence-store", "recurrence", "set", "recurrence.json")
        require(recurrence_result.get("recurrence_version") == "1", f"recurrence setup failed: {recurrence_result}")
        recurrence_query = {
            "schema": "ledger-billing-occurrence-query/1", "customer": recurrence_store["customer"],
            "source": recurrence_store["source"], "recurrence_version": "1", "due_through": iso(now + dt.timedelta(days=1)), "limit": 10,
        }
        write_json(root / "occurrences.json", recurrence_query)
        due = store_command(binary, root, "recurrence-store", "occurrences", "occurrences.json")
        occurrences = due.get("occurrences")
        require(isinstance(occurrences, list) and len(occurrences) == 1, f"expected one current recurrence occurrence: {due}")
        occurrence_id = occurrences[0]["occurrence_id"]
        acceptance = {
            "schema": "ledger-billing-occurrence-acceptance/1", "customer": recurrence_store["customer"],
            "source": recurrence_store["source"], "occurrence_id": occurrence_id,
            "event": {"schema": "ledger-event/1", "id": occurrence_id,
                      "operation_id": "native-journey-occurrence-operation", "type": "content.generated",
                      "customer": recurrence_store["customer"], "occurred_at": occurrences[0]["scheduled_at"], "status": "succeeded"},
        }
        write_json(root / "occurrence-acceptance.json", acceptance)
        accepted_occurrence = store_command(binary, root, "recurrence-store", "occurrence", "accept", "occurrence-acceptance.json")
        require(accepted_occurrence.get("status") == "accepted", f"recurrence occurrence was not accepted: {accepted_occurrence}")
        require(store_command(binary, root, "recurrence-store", "occurrence", "accept", "occurrence-acceptance.json") == accepted_occurrence,
                "recurrence occurrence exact retry changed its retained receipt")
        changed_acceptance = dict(acceptance, event=dict(acceptance["event"],
                                                         operation_id="native-journey-occurrence-conflict"))
        write_json(root / "occurrence-acceptance.json", changed_acceptance)
        occurrence_conflict = store_command(binary, root, "recurrence-store", "occurrence", "accept",
                                            "occurrence-acceptance.json", expected=4)
        require(occurrence_conflict.get("code") == "IDENTITY_CONFLICT",
                f"changed recurrence acceptance identity was not rejected: {occurrence_conflict}")
        assertions["recurrence_occurrence_retry_conflict"] = {
            "occurrence_id": occurrence_id, "conflict_code": occurrence_conflict["code"], "passed": True,
        }

        # Optional v0.7.0 schema-10 binary path: seed authentic schema 10 with
        # that writer, preserve its retained M4 receipt/statement, explicitly
        # upgrade with the candidate binary, then prove the old writer refuses
        # a schema-11 write without changing any installation file.
        if old_binary is not None:
            old_binary = old_binary.resolve(strict=True)
            require(old_binary.is_file() and os.access(old_binary, os.X_OK), f"old binary is not executable: {old_binary}")
            old_version = subprocess.run([str(old_binary), "--version"], text=True, stdout=subprocess.PIPE,
                                         stderr=subprocess.PIPE, check=False)
            require(old_version.returncode == 0, f"schema-10 old binary --version failed: {old_version.stderr}")
            old_setup = json.loads((package_root / "examples/billing/setup.json").read_text(encoding="utf-8"))
            old_setup["store_id"] = "native-journey-schema10-store"
            old_setup["accepted_at"] = iso(yesterday)
            old_setup["outcome_policy"]["families"][0]["ordinary"]["starts_at"] = event_time
            old_setup["outcome_policy"]["families"][0]["corrections"]["starts_at"] = event_time
            setup_store(old_binary, root, "schema10-store", old_setup)
            legacy_event = dict(fixed_event, id="native-journey-schema10-event",
                                operation_id="native-journey-schema10-operation")
            write_json(root / "legacy-event.json", legacy_event)
            legacy_accepted = store_command(old_binary, root, "schema10-store", "accept", "--customer",
                                            old_setup["customer"], "--source", old_setup["source"], "legacy-event.json")
            require(legacy_accepted.get("status") == "accepted", f"v0.7.0 schema-10 writer failed to seed history: {legacy_accepted}")
            legacy_alias_event = dict(legacy_event, id="native-journey-schema10-semantic-alias")
            write_json(root / "legacy-alias-event.json", legacy_alias_event)
            legacy_alias = store_command(old_binary, root, "schema10-store", "accept", "--customer",
                                         old_setup["customer"], "--source", old_setup["source"], "legacy-alias-event.json")
            require(legacy_alias.get("status") == "duplicate"
                    and legacy_alias.get("receipt") == legacy_accepted.get("receipt"),
                    f"v0.7.0 writer did not retain an M4 semantic alias with the original receipt: {legacy_alias}")
            legacy_statement = store_command(old_binary, root, "schema10-store", "statement", "--customer", old_setup["customer"])
            require(sqlite_user_version(root / "schema10-store") == 10,
                    "the v0.7.0 binary did not create an authentic schema-10 installation")
            before_upgrade_hashes = file_hashes(root / "schema10-store")
            legacy_rows_before = legacy_billing_snapshot(root / "schema10-store")
            semantic_alias_tables = {
                table: contents for table, contents in legacy_rows_before.items()
                if table.endswith("_aliases") and contents["rows"]
            }
            require(semantic_alias_tables,
                    "schema-10 semantic alias was not retained in an existing M4 alias table")
            upgrade = store_command(binary, root, "schema10-store", "upgrade")
            require(upgrade.get("status") == "upgraded", f"installed binary schema-10 upgrade failed: {upgrade}")
            require(sqlite_user_version(root / "schema10-store") == 11,
                    "installed binary did not explicitly migrate the schema-10 store to schema 11")
            upgraded_statement = store_command(binary, root, "schema10-store", "statement", "--customer", old_setup["customer"])
            require(upgraded_statement == legacy_statement,
                    "schema-10 to schema-11 upgrade changed the retained statement bytes/identity")
            legacy_retry = store_command(binary, root, "schema10-store", "accept", "--customer", old_setup["customer"],
                                         "--source", old_setup["source"], "legacy-event.json")
            require(legacy_retry.get("status") == "duplicate" and legacy_retry.get("receipt") == legacy_accepted.get("receipt"),
                    "schema-10 exact retry identity did not survive upgrade")
            legacy_alias_retry = store_command(binary, root, "schema10-store", "accept", "--customer", old_setup["customer"],
                                               "--source", old_setup["source"], "legacy-alias-event.json")
            require(legacy_alias_retry.get("status") == "duplicate"
                    and legacy_alias_retry.get("receipt") == legacy_accepted.get("receipt"),
                    f"schema-10 semantic alias retry identity did not survive upgrade: {legacy_alias_retry}")
            legacy_rows_after = legacy_billing_snapshot(root / "schema10-store")
            require(legacy_rows_after == legacy_rows_before,
                    "schema-10 upgrade changed existing billing table schemas, bytes, rows, or identities")
            # Take the refusal baseline after read-only SQLite inspection so
            # transient journal/WAL sidecar lifecycle cannot look like an old
            # writer mutation.
            schema11_hashes = file_hashes(root / "schema10-store", exclude_sqlite_shm=True)
            schema11_database_hash = digest(root / "schema10-store/.ledger/local.db")
            schema11_wal = sqlite_sidecar_snapshot(root / "schema10-store/.ledger/local.db-wal")
            schema11_shm = sqlite_sidecar_snapshot(root / "schema10-store/.ledger/local.db-shm")
            unsupported_event = dict(legacy_event, id="native-journey-unsupported-old-write",
                                     operation_id="native-journey-unsupported-old-operation")
            write_json(root / "unsupported-old-event.json", unsupported_event)
            old_write = store_command(old_binary, root, "schema10-store", "accept", "--customer", old_setup["customer"],
                                      "--source", old_setup["source"], "unsupported-old-event.json", expected=9)
            require(old_write.get("code") == "INTEGRITY_FAILURE",
                    f"pinned schema-10 writer returned an unexpected upgraded-store refusal: {old_write}")
            after_old_writer_hashes = file_hashes(root / "schema10-store", exclude_sqlite_shm=True)
            after_old_writer_wal = sqlite_sidecar_snapshot(root / "schema10-store/.ledger/local.db-wal")
            after_old_writer_shm = sqlite_sidecar_snapshot(root / "schema10-store/.ledger/local.db-shm")
            old_refusal_empty_wal_lifecycle = require_same_durable_files(
                schema11_hashes, after_old_writer_hashes, schema11_wal, after_old_writer_wal,
                "old schema-10 writer refusal", exclude_empty_sqlite_wal=True,
            )
            require(digest(root / "schema10-store/.ledger/local.db") == schema11_database_hash,
                    "old writer changed the schema-11 SQLite database before refusing")
            post_refusal_retry = store_command(binary, root, "schema10-store", "accept", "--customer",
                                               old_setup["customer"], "--source", old_setup["source"], "legacy-event.json")
            require(post_refusal_retry.get("status") == "duplicate"
                    and post_refusal_retry.get("receipt") == legacy_accepted.get("receipt"),
                    "current writer did not reopen the installation and preserve its original receipt after old-writer refusal")
            after_current_hashes = file_hashes(root / "schema10-store", exclude_sqlite_shm=True)
            after_current_wal = sqlite_sidecar_snapshot(root / "schema10-store/.ledger/local.db-wal")
            after_current_shm = sqlite_sidecar_snapshot(root / "schema10-store/.ledger/local.db-shm")
            current_reopen_empty_wal_lifecycle = require_same_durable_files(
                after_old_writer_hashes, after_current_hashes, after_old_writer_wal, after_current_wal,
                "current writer reopen and exact receipt replay", exclude_empty_sqlite_wal=True,
            )
            require(digest(root / "schema10-store/.ledger/local.db") == schema11_database_hash,
                    "current writer changed the SQLite database while replaying the retained legacy receipt")
            require(before_upgrade_hashes != schema11_hashes,
                    "explicit schema-10 upgrade did not persist its schema transition")
            assertions["schema10_upgrade_and_old_writer_refusal"] = {
                "old_binary_version": old_version.stdout.strip(),
                "old_binary_sha256": digest(old_binary),
                "old_binary_source_identity": {
                    "expected_commit": "48d76ae249fbfcb79a5012b265da51a71cb996e3",
                    "attestation": "caller-pinned v0.7.0 schema-10 build; source identity is not inferred from binary bytes",
                },
                "upgrade_status": upgrade.get("status"), "schema_before": 10, "schema_after": 11,
                "legacy_billing_tables_preserved": sorted(legacy_rows_before),
                "legacy_alias_tables_with_rows": sorted(semantic_alias_tables),
                "legacy_semantic_alias_receipt_preserved": True,
                "preserved_net_atoms": upgraded_statement.get("net_atoms"),
                "old_writer_error": old_write.get("code"), "old_writer_exit": 9,
                "durable_installation_files_unchanged_on_refusal": True,
                "sqlite_database_sha256_unchanged_on_refusal": True,
                "sqlite_wal_sidecar_observations": {
                    "path": SQLITE_WAL_PATH,
                    "before_old_writer_refusal": schema11_wal,
                    "after_old_writer_refusal": after_old_writer_wal,
                    "after_current_reopen_and_replay": after_current_wal,
                    "empty_lifecycle_allowed_on_refusal_comparison": old_refusal_empty_wal_lifecycle,
                    "empty_lifecycle_allowed_on_current_reopen_comparison": current_reopen_empty_wal_lifecycle,
                    "policy": "only an observed absent/zero-byte lifecycle may be omitted from refusal file-map equality",
                },
                "sqlite_shm_sidecar_observations": {
                    "path": ".ledger/local.db-shm",
                    "before_old_writer_refusal": schema11_shm,
                    "after_old_writer_refusal": after_old_writer_shm,
                    "after_current_reopen_and_replay": after_current_shm,
                    "excluded_only_from_refusal_comparison": True,
                    "classification": "SQLite WAL shared-memory coordination metadata; exact size and SHA-256 observed at each phase",
                },
                "current_writer_reopened_after_refusal": True,
            }

    return evidence


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ledger", type=Path, help="installed ledger executable")
    parser.add_argument("--old-binary", type=Path, help="optional v0.7.0 schema-10 ledger executable")
    parser.add_argument("--evidence", type=Path, help="write JSON evidence to this new path; otherwise print it")
    arguments = parser.parse_args()
    try:
        package = fixture_root(Path(__file__))
        evidence = run_journey(arguments.ledger, package, arguments.old_binary)
        encoded = json.dumps(evidence, indent=2, sort_keys=True) + "\n"
        if arguments.evidence:
            if arguments.evidence.exists():
                fail(f"refusing to overwrite existing evidence: {arguments.evidence}")
            arguments.evidence.parent.mkdir(parents=True, exist_ok=True)
            arguments.evidence.write_text(encoded, encoding="utf-8")
        sys.stdout.write(encoded)
        return 0
    except Exception as error:  # Keep acceptance failures explicit in shell/package CI.
        sys.stderr.write(f"native package journey failed: {error}\n")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
