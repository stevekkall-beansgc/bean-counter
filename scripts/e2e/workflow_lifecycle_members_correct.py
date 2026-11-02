#!/usr/bin/env python3
"""One exact D correction on a copy of the preserved 32-operation Stage.

Diagnostic mode submits the same request once, observes its status once and stops.
Normal mode continues only a successful correction through exact inverse checks
and separate-process/whole-copy recovery. No setup, settings, helper or replay.
"""
import argparse
import json
import shutil
import sys
from pathlib import Path

from workflow_lifecycle_finite import FiniteLifecycle, SETTINGS
from workflow_supported import ATOMS, canonical, now, sha, unique

SOURCE_RUN_SHA = "3b0c0d8dbdcb1528fa8a81eaeeee20bfb04231316d1a3aed091456af8e220fda"
SOURCE_SNAPSHOT_SHA = "a65e0e4063a3eacdd919232c5dee60f653932192062eb1f88d7a36bd6fb7c090"
CONFIG_SHA = "4f6afdb9326b3218feab3fdfcf23808225cff74859486b0d96a3d67eb04ab9ef"
REQUEST_SHA = "9c12a3bf406fe4624c641ca03e29d5b9d221b8fee778f1941e421a74a6d98803"
INPUT_MAP_SHA = "c711fcd648ebe89dfe9f189b3be8fe40e0cf1122436141dfc7b12ffeed58c202"


def verify_pins(pins):
    for group in ("files", "source_files", "product_files"):
        for item in pins[group]:
            raw = Path(item["path"]).read_bytes()
            assert sha(raw) == item["sha256"] and len(raw) == int(item["bytes"]), item["path"]


class MembersCorrection(FiniteLifecycle):
    source_filename = "source-initial-template.json"

    def __init__(self, args):
        self.source_root = args.source.resolve()
        parent_raw = (self.source_root / "RUN.json").read_bytes()
        assert sha(parent_raw) == SOURCE_RUN_SHA
        self.parent = json.loads(parent_raw)
        assert self.parent["final_snapshot_sha256"] == SOURCE_SNAPSHOT_SHA
        assert sha(canonical(self.parent["final_snapshot"])) == SOURCE_SNAPSHOT_SHA
        self.source_installation = self.source_root / "installation"
        self.original_config = (self.source_installation / "billing.json").read_bytes()
        assert sha(self.original_config) == CONFIG_SHA
        assert json.loads(self.original_config)["resources"] == SETTINGS
        self.correction_raw = (self.source_root / "correct-d-quantity.request.json").read_bytes()
        assert sha(self.correction_raw) == REQUEST_SHA
        self.correction_request = json.loads(self.correction_raw)
        assert canonical(self.correction_request) == self.correction_raw
        map_raw = args.input_map.read_bytes()
        assert sha(map_raw) == INPUT_MAP_SHA
        self.input_map = json.loads(map_raw)
        super().__init__(args)
        self.store = json.loads(self.original_config)["store_id"]
        self.config_bytes = self.original_config
        self.original_snapshot = self.source_snapshot()
        assert canonical(self.original_snapshot) == canonical(self.parent["final_snapshot"])
        shutil.copytree(self.source_installation, self.installation, copy_function=shutil.copy2)
        shutil.copytree(self.source_root / "inputs", self.inputs, dirs_exist_ok=True, copy_function=shutil.copy2)
        assert canonical(self.snapshot()) == canonical(self.original_snapshot)
        (self.root / "PARENT-RUN.json").write_bytes(parent_raw)
        (self.root / "SAVED-INPUT-MAP.json").write_bytes(map_raw)
        (self.root / "original-correct-d-quantity.request.json").write_bytes(self.correction_raw)
        saved = []
        for path in self.source_root.glob("*.committed.json"):
            result = json.loads(path.read_bytes())
            saved.append((int(result["receipt"]["commit_sequence"]), path, result))
        assert [n for n, _, _ in sorted(saved)] == list(range(1, 33))
        with self.connection() as db:
            receipts = [json.loads(b) for b, in db.execute("SELECT receipt_bytes FROM workflow_operations ORDER BY commit_sequence")]
        assert len(receipts) == 32
        for sequence, path, result in sorted(saved):
            name = path.name.removesuffix(".committed.json")
            raw = (self.source_root / (name + ".request.json")).read_bytes()
            request = json.loads(raw)
            assert result["receipt"] == receipts[sequence - 1]
            assert result["receipt"]["key"] == request["key"]
            assert result["receipt"]["original_request_bytes_sha256"] == sha(raw)
            (self.root / path.name).write_bytes(path.read_bytes())
            (self.root / (name + ".request.json")).write_bytes(raw)
            self.commits[name] = result
            self.attempted.add(canonical(request["key"]))
        self.original_names = tuple(self.commits)
        self.grant = unique(self.commits["source-enroll"], "workflow-source-grant")
        for label in ("worker", "corrector", "closer", "alias"):
            setattr(self, label + "_grant", unique(self.commits[label + "-enroll"], "workflow-source-grant"))
        self.binding_ref = self.input_map["named_inputs"]["binding"]["ref"]
        self.binding = self.document(self.binding_ref)
        self.d_ref = unique(self.commits["d-work"], "workflow-work")
        self.b_ref = unique(self.commits["b-work"], "workflow-work")
        self.d_body, self.b_body = self.document(self.d_ref), self.document(self.b_ref)
        self.terminal_ref = unique(self.commits["close-complete"], "workflow-stage-state")
        self.terminal_body = self.document(self.terminal_ref)
        self.old_action_ref = unique(self.commits["close-complete"], "workflow-action")
        self.old_action = self.document(self.old_action_ref)
        self.meta["proof_scope"] = {
            "case": "members", "fresh_only": False, "resume_original_operations": 32,
            "existing_operations_replayed": False, "settings": SETTINGS,
            "default_budget_qualification": False, "mode": "diagnostic" if args.diagnostic else "normal",
            "source_run_sha256": SOURCE_RUN_SHA, "source_snapshot_sha256": SOURCE_SNAPSHOT_SHA,
            "exact_correction_request_sha256": REQUEST_SHA,
            "diagnostic_timing_is_production_qualification": False,
        }
        self.meta["configuration_sha256"] = CONFIG_SHA
        self.save()

    def source_snapshot(self):
        installation = self.installation
        self.installation = self.source_installation
        try:
            return self.snapshot()
        finally:
            self.installation = installation

    def assert_originals(self):
        current = self.snapshot()
        assert canonical(current["operations"][:32]) == canonical(self.original_snapshot["operations"])
        assert set(map(tuple, self.original_snapshot["documents"])) <= set(map(tuple, current["documents"]))
        for name, table in self.original_snapshot["logical_tables"].items():
            if name.startswith(("workflow_source_", "workflow_party_", "workflow_agreement_")) or name == "workflow_customer_registrations":
                assert current["logical_tables"][name] == table, name
        assert (self.installation / "billing.json").read_bytes() == self.original_config
        assert canonical(self.source_snapshot()) == canonical(self.original_snapshot)
        assert (self.source_installation / "billing.json").read_bytes() == self.original_config

    def setup(self):
        raise AssertionError("No setup, publication or financial replay")

    def command(self, name, argv, expected=0, allow_unavailable=False):
        assert Path(argv[0]) == self.binary and str(argv[1]) == "workflow"
        route = str(argv[4])
        assert route in {"submit", "status", "read"}, argv
        if route == "submit":
            assert name == "correct-d-quantity" and name not in self.original_names
            assert Path(argv[5]).read_bytes() == self.correction_raw
        self.assert_originals()
        try:
            return super().command(name, argv, expected, allow_unavailable)
        finally:
            self.assert_originals()

    def perform(self, name, kind, payload, refusal=None):
        assert not self.args.diagnostic and not self.recovery_only
        assert name == "correct-d-quantity" and kind == "quantity.correct" and refusal is None
        request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": kind,
                   "key": self.key(name), "payload": payload}
        assert canonical(request) == self.correction_raw
        return super().perform(name, kind, payload, refusal)

    def preflight(self):
        assert self.correction_request["key"] == self.key("correct-d-quantity")
        for item in self.input_map["all_documents"]:
            assert sha(canonical(self.document(item["ref"]))) == item["body_sha256"]
        assert self.correction_request["payload"]["work_ref"] == self.d_ref
        assert self.d_body["key"]["source"] == self.alias and self.d_body["source_namespace"] == self.worker
        assert self.d_body["payload"]["facts"]["support:quantity"] == "1"
        assert self.b_body["payload"]["status"] == "failed"
        assert self.head("stage", self.agreement, self.chain, "support:stage") == ("3", self.terminal_ref)
        assert self.terminal_body["state"] == "closed" and self.terminal_body["first_work_ref"] == self.b_ref
        assert self.terminal_body["first_work_accepted_at"] == self.b_body["accepted_at"]
        for label in ("d_root_decision", "closure_decision"):
            item = self.input_map["named_inputs"][label]
            assert self.head("decision", item["body"]["decision_id"]) == ("1", item["ref"])
        trusted = now()
        # Stricter fixture eligibility observation; production verifies its own rights.
        for ref in self.accepted_grants():
            grant = self.document(ref)
            assert grant["starts_at"] <= trusted < grant["ends_at"]
            assert grant["principal_id"] == self.operator and self.customer in grant["customer_scope"]
            with self.connection() as db:
                head = db.execute("SELECT revision,grant_id,status FROM workflow_source_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND enrollment_id=?",
                    (self.store, *self.scope, grant["grant_id"])).fetchone()
            assert head == (int(grant["revision"]), ref["id"], "active")
        corrector = self.document(self.corrector_grant)
        assert "quantity.correct" in corrector["operations"] and "correct" in corrector["permissions"]
        assert "support:corrector" in corrector["source_slots"]
        terms = self.input_map["named_inputs"]["retail_slot_terms"]["body"]
        assert self.corrector_grant in terms["source_grant_refs"]
        assert terms["correction_source_slot"] == "support:corrector"
        assert self.correction_request["payload"]["dependent_reversals"][0]["authority_evidence_refs"] == [self.corrector_grant]
        assert self.input_map["original_window"]["correction_until"] > trusted
        self.book_totals("preserved-complete-two-member-close", 2 * ATOMS, 0)
        self.meta["current_fixture_authority_checked_at"] = trusted
        self.meta["original_window"] = self.input_map["original_window"]
        self.meta["input_map_checked_documents"] = len(self.input_map["all_documents"])
        self.save()
        if not self.args.diagnostic:
            for name in self.original_names:
                saved = self.commits[name]
                status = self.exact_status("preserved-" + name, saved["receipt"]["key"])
                assert status["status"] == "committed"
                assert status["receipt"] == saved["receipt"] and status["request_evidence"] == saved["request_evidence"]
            self.meta["preserved_original_statuses"] = len(self.original_names)
            self.save()

    def trace_once(self):
        key = self.correction_request["key"]
        before = self.exact_status("correct-d-quantity-before", key)
        assert before["status"] == "not_observed" and before["key"] == key
        path = self.root / "correct-d-quantity.request.json"
        path.write_bytes(self.correction_raw)
        try:
            result = self.workflow("correct-d-quantity", "submit", path, expected=None)
            self.meta["diagnostic_result"] = result
        finally:
            try:
                self.meta["diagnostic_final_status"] = self.exact_status("correct-d-quantity-final-status", key)
            except Exception as error:
                self.meta["diagnostic_final_status_error"] = repr(error)
            self.meta["status"] = "DIAGNOSTIC_STOPPED_NO_RETRY"
            self.save()

    def correction(self):
        result = self.submit("correct-d-quantity", "quantity.correct", self.correction_request["payload"])
        assert result["receipt"]["commit_sequence"] == "33"
        self.book_totals("original-d-correction-and-exact-dependent-inverse", 0, 0)
        refs = [ref for ref in result["receipt"]["result_refs"] if ref["kind"] == "workflow-action"]
        assert len(refs) == 1
        inverse = self.document(refs[0])
        assert inverse["inverse_of"] == self.old_action_ref and inverse["posting_role"] == "inverse"
        assert int(inverse["amount"]["atoms"]) == -int(self.old_action["amount"]["atoms"]) == -2 * ATOMS
        for field in ("book", "component_slot", "root_component_slot", "role_ref", "slot_terms_ref", "funding_leg", "basis_refs"):
            assert inverse[field] == self.old_action[field]
        assert self.document(self.d_ref) == self.d_body and self.document(self.b_ref) == self.b_body
        assert self.document(self.terminal_ref) == self.terminal_body
        assert self.head("stage", self.agreement, self.chain, "support:stage") == ("3", self.terminal_ref)
        self.meta["verified_exact_inverse"] = {"ref": refs[0], "inverse_of": self.old_action_ref,
            "amount": inverse["amount"], "immutable_stage_ref": self.terminal_ref}
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "pins", "source", "input-map", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--diagnostic", action="store_true")
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    candidate = json.loads(args.candidate.read_bytes())
    args.ledger = Path(candidate["binary_path"])
    args.case, args.resume, args.deadline_seconds, args.pending_finalize_check = "members", False, 0, False
    journey = None
    try:
        journey = MembersCorrection(args)
        journey.preflight()
        if args.diagnostic:
            journey.trace_once()
        else:
            journey.correction()
            journey.recover()
    except Exception as error:
        if journey is not None:
            journey.meta["status"] = "STOPPED_FIRST_SUBSTANTIVE_BLOCKER"
            journey.meta["error"] = repr(error)
        raise
    finally:
        original_error_active = sys.exc_info()[0] is not None
        final_errors = []
        try:
            if journey is not None:
                try:
                    journey.meta["final_snapshot"] = journey.snapshot()
                    journey.meta["final_snapshot_sha256"] = sha(canonical(journey.meta["final_snapshot"]))
                    journey.meta["final_configuration_sha256"] = sha((journey.installation / "billing.json").read_bytes())
                except Exception as error:
                    final_errors.append({"stage": "capture_actual_final_state", "error": repr(error)})
                try:
                    journey.assert_originals()
                    journey.meta["source_preserved"] = True
                except Exception as error:
                    journey.meta["source_preserved"] = False
                    final_errors.append({"stage": "preservation", "error": repr(error)})
        finally:
            try:
                verify_pins(pins)
            except Exception as error:
                final_errors.append({"stage": "final_pin_verification", "error": repr(error)})
            if journey is not None:
                if final_errors:
                    journey.meta["final_verification_errors"] = final_errors
                journey.save()
        if final_errors and not original_error_active:
            raise AssertionError(final_errors)


if __name__ == "__main__":
    main()
