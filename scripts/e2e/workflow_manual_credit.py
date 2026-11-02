#!/usr/bin/env python3
"""One operator credit on the preserved 28-operation synthetic history.

Initial historical calendar assignment is accepted at the real current time.
The original +3 statement closes before the separate -3 manual credit. All
workflow submissions are forbidden; automatic Stage quantity correction stays
deferred. No timeout ladder, financial replay, or retry of an uncertain write.
"""
import argparse
import base64
import copy
import datetime as dt
import hashlib
import json
import shutil
import sqlite3
import subprocess
import sys
from contextlib import contextmanager
from pathlib import Path

SOURCE_RUN_SHA = "33499efb0557081c46f48f6d4049c20df7483ae638b1a523b3d0d68c170503f6"
SOURCE_SNAPSHOT_SHA = "c7706ecd109155b694758b6fa05d428bd256358967bad6fdf40c1b31af025fe6"
CONFIG_SHA = "ef2e85798ccecff4c69464866c74c70c572e19bae57887c411e933630eca63d5"
INPUT_MAP_SHA = "2e4b9f2616fc65956588b484e8e7c933afc567e35d151d4db1160e6d8589845f"
CREDIT_SHA = "c54e6edbf1e718fdc97752223b6612975c64f492dbbfed2c402b622a1c84a647"
TERM_SHA = "667bf571c5b92020ceeb19e47e48d246a734acd8233afa67bed34971fe813e76"
CLOSE_SHA = "935b03b17d4d7f0a211c57afa5e5822d4bff608d184d47fc9fb8c1a5fdd72fec"
ORACLE_SHA = "9c06d0872b30d8e1429385564d4b06ef8226094626f6da14d13786f86e59d90b"
SETTINGS = {
    "schema": "ledger-local-resource-settings/1", "semantic_work": None,
    "compile_work": 1048576, "resolved_records": 4096, "resolved_bytes": 8388608,
    "new_body_bytes": 8388608, "arena_bytes": 67108864,
    "retained_operations": 100000, "retained_bytes": 268435456,
    "attempt_timeout_ms": 60000,
}
RECORD = "ledger-billing-manual-credit-record/1"


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def now():
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def pinned(path, digest):
    raw = path.read_bytes()
    assert sha(raw) == digest, str(path)
    return raw


def verify_pins(pins):
    for group in ("files", "source_files", "product_files"):
        for item in pins[group]:
            raw = Path(item["path"]).read_bytes()
            assert sha(raw) == item["sha256"] and len(raw) == int(item["bytes"]), item["path"]


@contextmanager
def connection(installation):
    db = sqlite3.connect("file:" + str(installation / ".ledger/local.db") + "?mode=ro", uri=True)
    try:
        yield db
    finally:
        db.close()


def snapshot(installation):
    with connection(installation) as db:
        operations = [(i, q, sha(r), sha(o)) for i, q, r, o in db.execute(
            "SELECT operation_id,commit_sequence,receipt_bytes,operation_bytes FROM workflow_operations ORDER BY commit_sequence")]
        documents = [(i, sha(b)) for i, b in db.execute("SELECT id,canonical_bytes FROM workflow_documents ORDER BY id")]
        heads = db.execute("SELECT * FROM workflow_financial_heads ORDER BY head_kind,key_a,key_b,key_c,key_d").fetchall()
        tables = {}
        for name, in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"):
            cursor = db.execute('SELECT * FROM "' + name.replace('"', '""') + '"')
            rows = [canonical([{"bytes": len(v), "sha256": sha(v)} if isinstance(v, bytes) else v for v in row]) for row in cursor]
            tables[name] = {"columns": [c[0] for c in cursor.description], "rows": len(rows),
                            "sha256": sha(canonical(sorted(sha(row) for row in rows)))}
    return {"operations": operations, "documents": documents, "heads": heads, "logical_tables": tables,
            "billing_configuration_sha256": sha((installation / "billing.json").read_bytes())}


class ManualCredit:
    def __init__(self, args):
        self.args = args
        self.source = args.source.resolve()
        parent_raw = pinned(self.source / "RUN.json", SOURCE_RUN_SHA)
        self.parent = json.loads(parent_raw)
        assert self.parent["final_snapshot_sha256"] == SOURCE_SNAPSHOT_SHA
        self.source_installation = self.source / "installation"
        self.config = pinned(self.source_installation / "billing.json", CONFIG_SHA)
        assert json.loads(self.config)["resources"] == SETTINGS
        self.baseline = snapshot(self.source_installation)
        assert sha(canonical(self.baseline)) == SOURCE_SNAPSHOT_SHA
        assert canonical(self.baseline) == canonical(self.parent["final_snapshot"])
        self.input_map = json.loads(pinned(args.input_map, INPUT_MAP_SHA))
        self.credit_raw = pinned(args.credit_request, CREDIT_SHA)
        self.credit = json.loads(self.credit_raw)
        self.term_raw = pinned(args.term_request, TERM_SHA)
        self.term = json.loads(self.term_raw)
        self.close_raw = pinned(args.close_request, CLOSE_SHA)
        self.close = json.loads(self.close_raw)
        self.oracle = json.loads(pinned(args.oracle, ORACLE_SHA))
        self.candidate = json.loads(args.candidate.read_bytes())
        self.binary = Path(self.candidate["binary_path"])
        assert self.candidate["build_exit"] == 0
        assert sha(self.binary.read_bytes()) == self.candidate["binary_sha256"]
        self.root = args.output.resolve()
        assert not self.root.exists(), "One fresh copied installation only"
        self.root.mkdir(mode=0o700)
        self.inputs = self.root / "inputs"
        self.inputs.mkdir(mode=0o700)
        self.installation = self.root / "installation"
        shutil.copytree(self.source_installation, self.installation, copy_function=shutil.copy2)
        assert canonical(snapshot(self.installation)) == canonical(self.baseline)
        self.credit_path = self.write("manual-credit.json", self.credit_raw)
        self.term_path = self.write("initial-calendar.json", self.term_raw)
        self.close_path = self.write("original-close.json", self.close_raw)
        self.status_selector = {"schema": "ledger-billing-manual-credit-status/1",
            **{k: self.credit[k] for k in ("customer", "workflow_scope", "adjustment_id")}}
        self.status_path = self.write("manual-credit-status.json", self.status_selector)
        self.claimed = None
        self.frozen_closed = None
        self.issued = None
        self.attempts = set()
        self.recovery_snapshot = None
        self.retained_presentation_rows = None
        self.meta = {"schema": "wave2b-manual-credit-proof/1", "status": "PREPARED_NOT_RUN",
            "binary": str(self.binary), "binary_sha256": sha(self.binary.read_bytes()),
            "source_run_sha256": SOURCE_RUN_SHA, "source_snapshot_sha256": SOURCE_SNAPSHOT_SHA,
            "installation": str(self.installation), "configuration_sha256": CONFIG_SHA,
            "commands": [], "checks": [], "settings": SETTINGS,
            "scope": {"original_workflow_operations": 28, "workflow_submissions": 0,
                "manual_credit_identities": 1, "manual_amount_atoms": "-3000000000000000000",
                "original_charge_atoms": "3000000000000000000", "aggregate_atoms": "0",
                "automatic_stage_quantity_correction": "deferred, exact old request status only",
                "calendar": "New initial calendar accepted now for actual retained history; no prior term modified",
                "post_close": "original +3 close precedes separate manual credit",
                "later_standard_exclusion": "focused custody qualification only; no later period fabricated",
                "default_budget_qualification": False, "read_authority": "local filesystem owner only"}}
        (self.root / "PARENT-RUN.json").write_bytes(parent_raw)
        self.validate_original_inputs()
        self.save()

    def write(self, name, value):
        path = self.inputs / name
        raw = value if isinstance(value, bytes) else canonical(value)
        assert not path.exists() or path.read_bytes() == raw
        path.write_bytes(raw)
        return path

    def save(self):
        (self.root / "RUN.json").write_bytes(canonical(self.meta))

    def version(self):
        with connection(self.installation) as db:
            return db.execute("PRAGMA user_version").fetchone()[0]

    def validate_original_inputs(self):
        assert len(self.baseline["operations"]) == 28 and len(self.baseline["documents"]) == 109
        assert len(self.baseline["heads"]) == 7
        with connection(self.installation) as db:
            for item in self.input_map["all28exact_original_receipt_request_pairs"]:
                raw = pinned(Path(item["request"]["path"]), item["request"]["sha256"])
                response = json.loads(pinned(Path(item["committed_response"]["path"]), item["committed_response"]["sha256"]))
                row = db.execute("SELECT receipt_bytes,operation_bytes,manifest_bytes FROM workflow_operations WHERE commit_sequence=?", (item["sequence"],)).fetchone()
                assert json.loads(row[0]) == item["receipt"] == response["receipt"]
                assert sha(row[1]) == item["operation_bytes_sha256"] and sha(row[2]) == item["manifest_bytes_sha256"]
                assert json.loads(raw)["key"] == item["receipt"]["key"]
                assert sha(raw) == item["receipt"]["original_request_bytes_sha256"]
            for item in self.input_map["all109exact_documents"]:
                ref = item["ref"]
                row = db.execute("SELECT canonical_bytes FROM workflow_documents WHERE kind=? AND id=? AND digest=?",
                    (ref["kind"], ref["id"], ref["digest"])).fetchone()
                assert row and row[0] == canonical(item["body"])
                assert sha(row[0]) == item["body_sha256"] and len(row[0]) == item["bytes"]
            for table in ("billing_m5_term_versions", "billing_m5_commands", "billing_m5_records", "billing_m5_period_closes"):
                assert db.execute('SELECT count(*) FROM "' + table + '"').fetchone()[0] == 0
        assert self.version() == 19

    def assert_preserved(self):
        current = snapshot(self.installation)
        for field in ("operations", "documents", "heads"):
            assert canonical(current[field]) == canonical(self.baseline[field]), field
        for table, value in self.baseline["logical_tables"].items():
            if not table.startswith("billing_m5_") and table not in {"_sqlx_migrations", "workflow_statement_membership"}:
                assert current["logical_tables"][table] == value, table
        assert (self.installation / "billing.json").read_bytes() == self.config
        assert canonical(snapshot(self.source_installation)) == canonical(self.baseline)
        assert (self.source_installation / "billing.json").read_bytes() == self.config
        if self.recovery_snapshot is not None:
            assert canonical(current) == canonical(self.recovery_snapshot)
        if self.retained_presentation_rows is not None:
            with connection(self.installation) as db:
                for table, original_rows in self.retained_presentation_rows.items():
                    current_rows = self.table_rows(db, table)
                    assert all(row in current_rows for row in original_rows), table
                    if table not in {"billing_m5_commands", "billing_m5_records"}:
                        assert current_rows == original_rows, table
        return current

    @staticmethod
    def table_rows(db, table):
        cursor = db.execute('SELECT * FROM "' + table.replace('"', '""') + '"')
        return sorted(canonical([{"base64": base64.b64encode(v).decode()} if isinstance(v, bytes) else v for v in row])
            for row in cursor)

    def pin_original_close(self):
        with connection(self.installation) as db:
            self.retained_presentation_rows = {table: self.table_rows(db, table) for table in (
                "billing_m5_commands", "billing_m5_records", "billing_m5_term_versions",
                "billing_m5_period_resolutions", "billing_m5_period_closes", "workflow_statement_membership")}
        self.meta["original_close_retained_rows"] = {table: [json.loads(row) for row in rows]
            for table, rows in self.retained_presentation_rows.items()}
        self.save()

    def census(self):
        with connection(self.installation) as db:
            assert db.execute("SELECT count(*) FROM billing_manual_credits").fetchone()[0] == 1
            assert db.execute("SELECT count(*) FROM billing_manual_credit_membership").fetchone()[0] == 1
            assert db.execute("SELECT count(*) FROM workflow_statement_membership").fetchone()[0] == 2
            rows = db.execute("SELECT c.request_bytes,c.response_bytes,r.payload_bytes,p.credit_kind,p.credit_id,p.credit_digest FROM billing_manual_credits p JOIN billing_m5_records r ON r.sequence=p.record_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE c.family='ledger-billing-manual-credit/1' AND r.family='ledger-billing-manual-credit-record/1'").fetchall()
            assert len(rows) == 1
            raw, result, record, kind, identity, digest = rows[0]
            assert raw == self.credit_raw and result == canonical(self.issued) and record == canonical(self.issued["record"])
            assert {"kind": kind, "id": identity, "digest": digest} == self.issued["credit_ref"]
            rows = db.execute("SELECT p.credit_kind,p.credit_id,p.credit_digest,p.presentation_kind,p.statement_id,p.statement_hash,r.payload_bytes,c.response_bytes FROM billing_manual_credit_membership p JOIN billing_m5_records r ON r.sequence=p.claim_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence").fetchall()
            assert len(rows) == 1
            kind, identity, digest, presentation, statement_id, statement_hash, claim, result = rows[0]
            assert {"kind": kind, "id": identity, "digest": digest} == self.issued["credit_ref"]
            assert presentation == "ad-hoc" and statement_id == self.claimed["statement_id"] and statement_hash == self.claimed["statement_hash"]
            assert result == canonical(self.claimed)
            claim = json.loads(claim)
            assert claim["schema"] == "ledger-billing-manual-credit-presentation-claim/1"
            assert claim["manual_credit_ref"] == self.issued["credit_ref"]
            expected_claim = {"schema": "ledger-billing-manual-credit-presentation-claim/1", "record": claim["record"],
                "customer": self.credit["customer"], "workflow_scope": self.credit["workflow_scope"],
                "manual_credit_ref": self.issued["credit_ref"], "presentation_kind": "ad-hoc",
                "statement_id": self.claimed["statement_id"], "statement_hash": self.claimed["statement_hash"]}
            assert claim == expected_claim
            credit_identity = {"schema": "ledger-billing-m5-command-identity/1", "domain": "customer-admin",
                "family": "ledger-billing-manual-credit/1", "customer": self.credit["customer"],
                "key_kind": "manual_credit", "key": {key: self.credit[key] for key in ("workflow_scope", "adjustment_id")}}
            claim_identity = {"schema": "ledger-billing-m5-command-identity/1", "domain": "customer-admin",
                "family": "ledger-billing-ad-hoc-statement/1", "customer": self.credit["customer"],
                "key_kind": "command_id", "key": self.claimed["statement_id"]}
            for family, retained, identity, key, expected_command in (
                (RECORD, self.issued["record"], credit_identity, {"role": "manual-credit", **{key: self.credit[key] for key in ("customer", "workflow_scope", "adjustment_id")}}, 3),
                ("ledger-billing-manual-credit-presentation-claim/1", claim, claim_identity,
                    {"role": "manual-credit-membership", "customer": self.credit["customer"],
                     "workflow_scope": self.credit["workflow_scope"], "manual_credit_ref": self.issued["credit_ref"]}, 4),
            ):
                rows = db.execute("SELECT r.sequence,r.command_sequence,r.payload_bytes,r.content_sha256,c.identity_key,c.accepted_at_us,c.request_bytes,c.request_sha256,c.response_bytes,c.response_sha256 FROM billing_m5_records r JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE r.family=?", (family,)).fetchall()
                assert len(rows) == 1
                sequence, command, body, content_hash, actual_identity, accepted_us, request_raw, request_hash, response_raw, response_hash = rows[0]
                assert command == expected_command and retained["record"]["command_sequence"] == str(command)
                assert retained["record"]["sequence"] == str(sequence) and body == canonical(retained)
                assert actual_identity == canonical(identity)
                assert retained["record"]["record_id"] == "m5r_" + sha(b"bean-counter/m5/record-id/1\0" + actual_identity + b"\0" + canonical(key))
                accepted = dt.datetime.fromisoformat(retained["record"]["accepted_at"].replace("Z", "+00:00")) - dt.datetime(1970, 1, 1, tzinfo=dt.timezone.utc)
                assert accepted_us == (accepted.days * 86400 + accepted.seconds) * 1000000 + accepted.microseconds
                unsigned = copy.deepcopy(retained)
                payload_hash = unsigned["record"].pop("payload_hash")
                assert payload_hash == sha(b"bean-counter/m5/record/1\0" + family.encode() + b"\0" + canonical(unsigned)) == content_hash.hex()
                assert request_hash.hex() == sha(b"bean-counter/m5/request/1\0" + request_raw)
                assert response_hash.hex() == sha(b"bean-counter/m5/response/1\0" + response_raw)
            commands = dict(db.execute("SELECT family,count(*) FROM billing_m5_commands GROUP BY family"))
            assert commands == {"ledger-billing-term/1": 1, "ledger-billing-period-close/2": 1,
                "ledger-billing-manual-credit/1": 1, "ledger-billing-ad-hoc-statement/3": 1}
            records = dict(db.execute("SELECT family,count(*) FROM billing_m5_records GROUP BY family"))
            assert records == {"ledger-billing-boundary-resolution/1": 1, "ledger-billing-term-version/1": 1,
                "ledger-billing-period-close-record/2": 1, "ledger-billing-workflow-presentation-claim/1": 2,
                "ledger-billing-manual-credit-record/1": 1, "ledger-billing-ad-hoc-statement-record/3": 1,
                "ledger-billing-manual-credit-presentation-claim/1": 1}
        self.meta["retained_census"] = {"commands": commands, "records": records,
            "manual_credit_count": 1, "manual_claim_count": 1, "original_action_claim_count": 2}
        self.save()

    def command(self, name, family, route, path, *, readonly=True, error_code=None):
        assert family in {"billing", "workflow"}
        route = tuple(route)
        if family == "workflow":
            assert route in {("read",), ("status",)} and readonly
        else:
            assert route in {("term", "set"), ("close",), ("adjustment", "credit"),
                ("adjustment", "status"), ("adjustment", "read"), ("adjustment", "statement")}
        if not readonly:
            assert name in {"initial-calendar", "original-close", "manual-credit", "manual-presentation"}
            assert name not in self.attempts and self.recovery_snapshot is None
            self.attempts.add(name)
        if route == ("adjustment", "credit"):
            if name == "manual-credit":
                assert path.read_bytes() == self.credit_raw
            elif name == "credit-identical-retry":
                assert self.issued is not None and readonly and path.read_bytes() == self.credit_raw
            else:
                assert name == "credit-identity-conflict" and readonly and error_code == "IDENTITY_CONFLICT"
                altered = json.loads(path.read_bytes())
                assert {**altered, "reason": self.credit["reason"]} == self.credit
        if route == ("adjustment", "statement"):
            request = json.loads(path.read_bytes())
            expected = {"schema": "ledger-billing-ad-hoc-statement/3", "customer": self.credit["customer"],
                "command_id": "manual-credit-presentation-20261101", "workflow_scope": self.credit["workflow_scope"],
                "workflow_actions": [], "adjustments": [], "manual_credits": [self.issued["credit_ref"]]}
            if name == "manual-presentation":
                assert not readonly and request == expected and self.claimed is None
            else:
                expected["command_id"] = "manual-credit-competing-presentation-20261101"
                assert name == "already-presented-credit" and readonly and self.claimed is not None
                assert request == expected and error_code == "BILLING_M5_PRESENTED"
        if route == ("term", "set"):
            assert name == "initial-calendar" and path.read_bytes() == self.term_raw and not readonly
        if route == ("close",):
            assert name == "original-close" and path.read_bytes() == self.close_raw and not readonly
        if self.recovery_snapshot is not None:
            assert readonly and (family == "workflow" or route in {("adjustment", "status"), ("adjustment", "read")})
        self.assert_preserved()
        before = snapshot(self.installation)
        stem = self.root / (f"{len(self.meta['commands']):03d}-" + name)
        stem.with_suffix(".before.json").write_bytes(canonical(before))
        argv = [str(self.binary), family, "--directory", str(self.installation), *route, str(path), "--json"]
        entry = {"name": name, "argv": argv, "request_sha256": sha(path.read_bytes()),
            "started_at": now(), "executable_sha256": sha(self.binary.read_bytes()),
            "before_snapshot_sha256": sha(canonical(before)), "before_schema_version": self.version()}
        out = err = b""
        active_error = None
        try:
            result = subprocess.run(argv, capture_output=True, timeout=120)
            out, err = result.stdout, result.stderr
            entry["exit"] = result.returncode
        except subprocess.TimeoutExpired as error:
            out, err = error.stdout or b"", error.stderr or b""
            entry.update(exit=None, outcome="unknown_outer_timeout_no_retry")
            active_error = error
        except BaseException as error:
            active_error = error
            entry.update(exit=None, outcome="unknown_process_error_no_retry", error=repr(error))
        finally:
            stem.with_suffix(".stdout").write_bytes(out)
            stem.with_suffix(".stderr").write_bytes(err)
            entry.update(completed_at=now(), stdout=stem.with_suffix(".stdout").name,
                stderr=stem.with_suffix(".stderr").name, stdout_sha256=sha(out), stderr_sha256=sha(err))
            errors = []
            try:
                after = snapshot(self.installation)
                stem.with_suffix(".after.json").write_bytes(canonical(after))
                entry.update(after_snapshot_sha256=sha(canonical(after)), after_schema_version=self.version(),
                    configuration_sha256=sha((self.installation / "billing.json").read_bytes()))
                if readonly:
                    assert canonical(before) == canonical(after), "Read/refusal/retry changed state"
            except BaseException as error:
                errors.append(repr(error))
            try:
                self.assert_preserved()
            except BaseException as error:
                errors.append(repr(error))
            if errors:
                entry["preservation_errors"] = errors
            self.meta["commands"].append(entry)
            self.save()
            if errors and active_error is None:
                active_error = AssertionError(errors)
        print(name, entry["exit"], flush=True)
        if active_error is not None:
            raise active_error
        value = json.loads(out) if out.strip() else {}
        if error_code is None:
            assert entry["exit"] == 0, (name, entry["exit"], value)
        else:
            assert entry["exit"] != 0 and value.get("code") == error_code, (name, entry["exit"], value)
        return value

    def workflow_status(self, name, key):
        return self.command(name, "workflow", ("status",), self.write(name + ".json", key))

    def deferred_status(self, name):
        expected = self.input_map["deferred_quantity_status"]
        assert self.workflow_status(name, expected["key"]) == expected

    def credit_status(self, name, observed):
        result = self.command(name, "billing", ("adjustment", "status"), self.status_path)
        expected = {**self.status_selector, "schema": "ledger-billing-manual-credit-status-result/1",
            "status": "committed" if observed else "not_observed"}
        if observed:
            expected.update(request_bytes_sha256=CREDIT_SHA, result=self.issued)
        assert result == expected
        return result

    def credit_record_oracle(self, result, before, after):
        assert set(result) == {"schema", "status", "credit_ref", "record"}
        assert result["schema"] == "ledger-billing-manual-credit-result/1" and result["status"] == "committed"
        record = result["record"]
        expected = self.oracle["manual_record_static_fields"]
        assert set(record) == set(expected) | {"schema", "accepted_at", "record"}
        assert {key: record[key] for key in expected} == expected
        assert record["schema"] == RECORD and before <= record["accepted_at"] <= after
        assert self.frozen_closed["close_acceptance_time"] <= record["accepted_at"]
        identity = {"schema": "ledger-billing-m5-command-identity/1", "domain": "customer-admin",
            "family": "ledger-billing-manual-credit/1", "customer": self.credit["customer"],
            "key_kind": "manual_credit", "key": {key: self.credit[key] for key in ("workflow_scope", "adjustment_id")}}
        child = {"role": "manual-credit", **{key: self.credit[key] for key in ("customer", "workflow_scope", "adjustment_id")}}
        record_id = "m5r_" + sha(b"bean-counter/m5/record-id/1\0" + canonical(identity) + b"\0" + canonical(child))
        assert record["record"]["record_id"] == record_id
        assert record["record"]["accepted_at"] == record["accepted_at"]
        unsigned = copy.deepcopy(record)
        payload_hash = unsigned["record"].pop("payload_hash")
        assert payload_hash == sha(b"bean-counter/m5/record/1\0" + RECORD.encode() + b"\0" + canonical(unsigned))
        assert result["credit_ref"] == {"kind": "billing-manual-credit", "id": record_id,
            "digest": "sha256:" + sha(canonical(record))}
        self.issued = result
        self.meta["manual_credit_result"] = result
        self.save()

    def statement_read(self, name, statement):
        request = {"schema": "ledger-read/1", "kind": "billing.statement",
            "payload": {"customer": self.credit["customer"], "statement_hash": statement["statement_hash"]}}
        result = self.command(name, "workflow", ("read",), self.write(name + ".json", request))
        raw = base64.b64decode(result["canonical_bytes_base64"], validate=True)
        assert result["schema"] == "ledger-billing-statement-read/1" and result["statement"] == statement
        assert raw == canonical(statement) and result["canonical_bytes_sha256"] == sha(raw)
        assert result["body_bytes"] == str(len(raw))

    def manual_read(self, name):
        selector = {"schema": "ledger-billing-manual-credit-read/1", "customer": self.credit["customer"],
            "workflow_scope": self.credit["workflow_scope"], "credit_ref": self.issued["credit_ref"]}
        result = self.command(name, "billing", ("adjustment", "read"), self.write(name + ".json", selector))
        raw = canonical(self.issued["record"])
        assert result == {"schema": "ledger-billing-manual-credit-read-result/1", "credit_ref": selector["credit_ref"],
            "record": self.issued["record"], "canonical_bytes_base64": base64.b64encode(raw).decode(),
            "canonical_bytes_sha256": sha(raw), "body_bytes": str(len(raw))}

    def run(self):
        self.credit_status("manual-absence-schema19", False)
        assert self.version() == 19 and canonical(snapshot(self.installation)) == canonical(self.baseline)
        self.deferred_status("deferred-quantity-remains-absent")
        before = now()
        assert self.term["effective"]["at"] < "2026-11-01T00:00:00.000000Z" < before
        term = self.command("initial-calendar", "billing", ("term", "set"), self.term_path, readonly=False)
        after = now()
        assert term["status"] == "term_updated" and term["revision"] == term["term_version"] == "1"
        assert term["effective_at"] == self.term["effective"]["at"]
        assert before <= term["receipt"]["accepted_at"] <= after
        self.meta["term_result"] = term
        self.save()
        statement = self.command("original-close", "billing", ("close",), self.close_path, readonly=False)
        assert statement["schema"] == "ledger-billing-statement/5" and statement["status"] == "closed"
        assert statement["complete"] is True and statement["period_id"] == self.close["period_id"]
        assert statement["start_utc"] == "2026-10-31T00:00:00.000000Z"
        assert statement["end_utc"] == "2026-11-01T00:00:00.000000Z"
        assert statement["lines"] == self.oracle["original_standard_lines"]
        assert statement["totals"] == self.oracle["original_standard_totals"]
        assert statement["workflow_boundary"] == self.oracle["manual_record_static_fields"]["workflow_boundary"]
        unsigned = dict(statement)
        assert unsigned.pop("statement_hash") == sha(b"bean-counter/m5/statement/5\0" + canonical(unsigned))
        self.frozen_closed = statement
        self.meta["original_statement"] = statement
        self.save()
        self.pin_original_close()
        self.credit_status("manual-absence-before-issue", False)
        before = now()
        try:
            result = self.command("manual-credit", "billing", ("adjustment", "credit"), self.credit_path, readonly=False)
        except BaseException:
            # One exact original manual identity observation, never a submit.
            try:
                self.meta["failed_manual_status"] = self.command("manual-failure-status", "billing",
                    ("adjustment", "status"), self.status_path)
            except BaseException as error:
                self.meta["failed_manual_status_error"] = repr(error)
            self.save()
            raise
        self.credit_record_oracle(result, before, now())
        assert self.version() == 21
        assert self.command("credit-identical-retry", "billing", ("adjustment", "credit"), self.credit_path) == self.issued
        conflict = self.write("credit-conflict.json", {**self.credit, "reason": self.credit["reason"] + " Different bytes."})
        self.command("credit-identity-conflict", "billing", ("adjustment", "credit"), conflict, error_code="IDENTITY_CONFLICT")
        ad_hoc = {"schema": "ledger-billing-ad-hoc-statement/3", "customer": self.credit["customer"],
            "command_id": "manual-credit-presentation-20261101", "workflow_scope": self.credit["workflow_scope"],
            "workflow_actions": [], "adjustments": [], "manual_credits": [self.issued["credit_ref"]]}
        result = self.command("manual-presentation", "billing", ("adjustment", "statement"),
            self.write("manual-presentation.json", ad_hoc), readonly=False)
        assert result["schema"] == "ledger-billing-ad-hoc-statement-result/3" and result["status"] == "issued"
        for field in ("workflow_actions", "adjustments", "manual_credits"):
            assert result[field] == ad_hoc[field]
        line = {"manual_credit_ref": self.issued["credit_ref"], **{key: self.issued["record"][key] for key in (
            "binding_ref", "agreement_id", "agreement_version", "book", "role_ref", "slot_terms_ref", "funding_leg",
            "amount", "reason", "work_ref", "stage_instance_ref", "source_action_refs")}}
        identity = {"view_kind": "ad_hoc", "view_identity": {"customer": self.credit["customer"], "command_id": ad_hoc["command_id"]},
            "workflow_scope": self.credit["workflow_scope"], "line": line}
        line["line_id"] = sha(b"bean-counter/m5/manual-credit-line/1\0" + canonical(identity))
        assert result["lines"] == [line] and result["totals"] == [self.oracle["manual_total"]]
        unsigned = dict(result)
        assert unsigned.pop("statement_hash") == sha(b"bean-counter/m5/ad-hoc-statement/3\0" + canonical(unsigned))
        self.claimed = result
        self.meta["manual_statement"] = result
        self.save()
        competing = {**ad_hoc, "command_id": "manual-credit-competing-presentation-20261101"}
        self.command("already-presented-credit", "billing", ("adjustment", "statement"),
            self.write("competing-presentation.json", competing), error_code="BILLING_M5_PRESENTED")
        self.census()
        self.recovery_snapshot = snapshot(self.installation)
        self.meta["closed_snapshot"] = self.recovery_snapshot
        self.meta["closed_snapshot_sha256"] = sha(canonical(self.recovery_snapshot))
        self.save()
        self.recover()
        self.meta["status"] = "PASS_MANUAL_POST_CLOSE_CREDIT_AND_RECOVERY"
        self.meta["financial_oracle"] = {"original_charge_atoms": "3000000000000000000", "manual_credit_atoms": "-3000000000000000000",
            "aggregate_atoms": "0", "manual_statement_atoms": "-3000000000000000000", "supplier_atoms": "0"}
        self.save()

    def recover(self):
        original = self.installation
        for phase in ("reopened", "whole-copy"):
            if phase == "whole-copy":
                self.installation = self.root / "restored-installation"
                shutil.copytree(original, self.installation, copy_function=shutil.copy2)
            try:
                self.assert_preserved()
                self.census()
                self.credit_status(phase + "-manual-status", True)
                self.manual_read(phase + "-manual-read")
                self.statement_read(phase + "-original-statement", self.frozen_closed)
                self.statement_read(phase + "-manual-statement", self.claimed)
                for index, item in enumerate(self.oracle["recovery_documents"]):
                    name = phase + "-original-document-" + str(index)
                    request = {"schema": "ledger-read/1", "kind": "policy.document",
                        "payload": {"ref": item["ref"], "representation": "canonical_bytes"}}
                    result = self.command(name, "workflow", ("read",), self.write(name + ".json", request))
                    raw = base64.b64decode(result["canonical_bytes_base64"], validate=True)
                    assert result["ref"] == item["ref"] and raw == canonical(item["body"])
                    assert result["canonical_bytes_sha256"] == sha(raw) == item["body_sha256"] and len(raw) == item["bytes"]
                for item in self.input_map["all28exact_original_receipt_request_pairs"]:
                    status = self.workflow_status(phase + "-" + item["name"], item["receipt"]["key"])
                    original_result = json.loads(Path(item["committed_response"]["path"]).read_bytes())
                    assert status["status"] == "committed" and status["receipt"] == item["receipt"]
                    assert status["request_evidence"] == original_result["request_evidence"]
                    assert base64.b64decode(status["request_evidence"]["original_request_bytes_base64"], validate=True) == Path(item["request"]["path"]).read_bytes()
                self.deferred_status(phase + "-deferred-quantity-absent")
                self.meta.setdefault("completed_recovery_phases", []).append(phase)
                self.meta["recovery_counts_per_phase"] = {"original_documents": 4, "original_statuses": 28,
                    "manual_credit_status": 1, "manual_credit_read": 1, "statements": 2, "deferred_absence": 1}
                self.save()
            finally:
                try:
                    self.assert_preserved()
                finally:
                    self.installation = original
                    self.assert_preserved()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "pins", "source", "input-map", "credit-request", "term-request", "close-request", "oracle", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    journey = None
    try:
        journey = ManualCredit(args)
        journey.run()
    except BaseException as error:
        if journey is not None:
            journey.meta.update(status="STOPPED_FIRST_SUBSTANTIVE_BLOCKER", error=repr(error))
        raise
    finally:
        active_error = sys.exc_info()[0] is not None
        errors = []
        try:
            if journey is not None:
                try:
                    journey.meta["final_snapshot"] = snapshot(journey.installation)
                    journey.meta["final_snapshot_sha256"] = sha(canonical(journey.meta["final_snapshot"]))
                    journey.meta["final_configuration_sha256"] = sha((journey.installation / "billing.json").read_bytes())
                    journey.meta["final_schema_version"] = journey.version()
                except BaseException as error:
                    errors.append({"stage": "capture_final", "error": repr(error)})
                try:
                    journey.assert_preserved()
                    journey.meta["source_preserved"] = True
                except BaseException as error:
                    journey.meta["source_preserved"] = False
                    errors.append({"stage": "preservation", "error": repr(error)})
        finally:
            try:
                verify_pins(pins)
            except BaseException as error:
                errors.append({"stage": "final_pins", "error": repr(error)})
            if journey is not None:
                if errors:
                    journey.meta["final_verification_errors"] = errors
                journey.save()
        if errors and not active_error:
            raise AssertionError(errors)


if __name__ == "__main__":
    main()
