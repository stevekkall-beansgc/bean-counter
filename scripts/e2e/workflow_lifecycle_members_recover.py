#!/usr/bin/env python3
"""Read/status-only recovery of the preserved 33-operation members history.

Read the exact refused original D Evaluation first, then every remaining
Action/Evaluation and all original receipts, separately and on a whole-copy
restore. No submission, retry, setup, helper, settings or financial replay.
"""
import argparse
import base64
import json
import shutil
import sys
from pathlib import Path

from workflow_lifecycle_finite import FiniteLifecycle, SETTINGS
from workflow_supported import canonical, sha

SOURCE_RUN_SHA = "7f82b4e7dfa16ecb5a3e7de688c9db72feda06ec5786377536c63355acf10354"
SOURCE_SNAPSHOT_SHA = "c2d26880408fbb3eb9b2544684738ec43d8f86350b5ac45e2956d9b767754a00"
CONFIG_SHA = "4f6afdb9326b3218feab3fdfcf23808225cff74859486b0d96a3d67eb04ab9ef"
READ_SHA = "94fc2bf83be74edfaa86f60c3cc3df21358fd7ee86cba5d0b87798132169cf50"
INPUTS_SHA = "c11fc83add305f3e836d3c99a7d3be1ffdab745aabf757bb8b41d737cb9fd5dc"


def verify_pins(pins):
    for group in ("files", "source_files", "product_files"):
        for item in pins[group]:
            raw = Path(item["path"]).read_bytes()
            assert sha(raw) == item["sha256"] and len(raw) == int(item["bytes"]), item["path"]


class MembersRecovery(FiniteLifecycle):
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
        map_raw = args.input_map.read_bytes()
        assert sha(map_raw) == INPUTS_SHA
        self.input_map = json.loads(map_raw)
        self.failed_raw = (self.source_root / "inputs/reopened-document-1.json").read_bytes()
        assert sha(self.failed_raw) == READ_SHA
        assert canonical(self.input_map["documents_in_read_order"][0]["request"]) == self.failed_raw
        super().__init__(args)
        self.store = json.loads(self.original_config)["store_id"]
        self.config_bytes = self.original_config
        self.recovery_only = True
        self.original_snapshot = self.source_snapshot()
        assert canonical(self.original_snapshot) == canonical(self.parent["final_snapshot"])
        shutil.copytree(self.source_installation, self.installation, copy_function=shutil.copy2)
        assert canonical(self.snapshot()) == canonical(self.original_snapshot)
        (self.root / "PARENT-RUN.json").write_bytes(parent_raw)
        (self.root / "RECOVERY-INPUTS.json").write_bytes(map_raw)
        (self.root / "original-refused-read.json").write_bytes(self.failed_raw)
        with self.connection() as db:
            receipts = [json.loads(b) for b, in db.execute("SELECT receipt_bytes FROM workflow_operations ORDER BY commit_sequence")]
        assert len(receipts) == len(self.input_map["operations"]) == 33
        for item in self.input_map["operations"]:
            assert receipts[item["sequence"] - 1] == item["receipt"]
            raw = Path(item["request_path"]).read_bytes()
            assert sha(raw) == item["raw_request_sha256"] == item["receipt"]["original_request_bytes_sha256"]
            assert json.loads(raw)["key"] == item["key"]
            (self.root / (item["name"] + ".request.json")).write_bytes(raw)
            result_raw = Path(item["committed_path"]).read_bytes()
            assert sha(result_raw) == item["committed_sha256"]
            (self.root / (item["name"] + ".committed.json")).write_bytes(result_raw)
        for item in self.input_map["documents_in_read_order"]:
            with self.connection() as db:
                ref = item["ref"]
                raw = db.execute("SELECT canonical_bytes FROM workflow_documents WHERE kind=? AND id=? AND digest=?",
                    (ref["kind"], ref["id"], ref["digest"])).fetchone()[0]
            assert sha(raw) == item["canonical_bytes_sha256"] and len(raw) == item["canonical_bytes_length"]
            assert json.loads(raw) == item["body"]
        self.meta["proof_scope"] = {
            "case": "members_reversal_recovery", "fresh_only": False,
            "preserved_operations": 33, "existing_operations_replayed": False,
            "allowed_routes": ["read", "status"], "settings": SETTINGS,
            "default_budget_qualification": False, "source_run_sha256": SOURCE_RUN_SHA,
            "source_snapshot_sha256": SOURCE_SNAPSHOT_SHA, "refused_read_sha256": READ_SHA,
            "reads_per_phase": 7, "statuses_per_phase": 33,
            "quantity_correction_qualified": False,
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

    def assert_preserved(self):
        assert canonical(self.snapshot()) == canonical(self.original_snapshot)
        assert (self.installation / "billing.json").read_bytes() == self.original_config
        assert canonical(self.source_snapshot()) == canonical(self.original_snapshot)
        assert (self.source_installation / "billing.json").read_bytes() == self.original_config

    def setup(self):
        raise AssertionError("Recovery cannot initialize or replay")

    def perform(self, *args, **kwargs):
        raise AssertionError("Recovery has no submission route")

    def command(self, name, argv, expected=0, allow_unavailable=False):
        assert Path(argv[0]) == self.binary and str(argv[1]) == "workflow"
        assert str(argv[4]) in {"read", "status"}, argv
        self.assert_preserved()
        try:
            return super().command(name, argv, expected, allow_unavailable)
        finally:
            self.assert_preserved()

    def recover(self):
        original = self.installation
        for phase in ("reopened", "whole-copy"):
            if phase == "whole-copy":
                self.installation = self.root / "restored-installation"
                shutil.copytree(original, self.installation, copy_function=shutil.copy2)
            try:
                self.assert_preserved()
                for index, item in enumerate(self.input_map["documents_in_read_order"]):
                    raw_request = canonical(item["request"])
                    assert sha(raw_request) == item["request_sha256"]
                    if index == 0:
                        assert raw_request == self.failed_raw
                    path = self.inputs / (phase + "-document-" + str(index) + ".json")
                    path.write_bytes(raw_request)
                    response = self.workflow(phase + "-document-" + str(index), "read", path)
                    raw = base64.b64decode(response["canonical_bytes_base64"], validate=True)
                    assert response["ref"] == item["ref"]
                    assert response["canonical_bytes_sha256"] == sha(raw) == item["canonical_bytes_sha256"]
                    assert len(raw) == item["canonical_bytes_length"] and raw == canonical(item["body"])
                for item in self.input_map["operations"]:
                    status = self.exact_status(phase + "-" + item["name"], item["key"])
                    assert status["status"] == "committed" and status["receipt"]["key"] == item["key"]
                    assert status["receipt"] == item["receipt"] and status["request_evidence"] == item["request_evidence"]
                self.meta.setdefault("completed_recovery_phases", []).append(phase)
                self.save()
            finally:
                self.assert_preserved()
                self.installation = original
                self.assert_preserved()
        self.meta["status"] = "PASS_MEMBERS_REVERSAL_ORIGINAL_RECOVERY"
        self.meta["recovery_counts_per_phase"] = {"exact_documents": 7, "exact_statuses": 33}
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "pins", "source", "input-map", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    candidate = json.loads(args.candidate.read_bytes())
    args.ledger = Path(candidate["binary_path"])
    args.case, args.resume, args.deadline_seconds, args.pending_finalize_check = "members", False, 0, False
    journey = None
    try:
        journey = MembersRecovery(args)
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
                    journey.assert_preserved()
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
