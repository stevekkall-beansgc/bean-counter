#!/usr/bin/env python3
"""Focused observer/driver regressions; these do not qualify a financial bill."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).with_name("e2e")))
import workflow_metered as metered
import workflow_supported as supported


def ref(name, kind="workflow-decision"):
    return {"kind": kind, "id": name, "digest": "sha256:" + name}


class MeteredDriverTests(unittest.TestCase):
    def test_fixed_defaults_and_source_files_are_preserved(self):
        self.assertEqual(supported.Journey.quantity, "20")
        self.assertEqual(supported.Journey.fixture_directory, supported.FIXTURES)
        self.assertEqual(metered.MeteredJourney.quantity, "3")
        original = supported.Journey.__new__(supported.Journey).source_documents()
        policy = next(d for d in original if d["schema"] == "ledger-workflow-policy/4")
        self.assertEqual(policy["rules"][0]["formula"], {"kind": "fixed", "amount": "1"})

    def test_variant_changes_are_fresh_input_data_not_shared_mutation(self):
        journey = metered.MeteredJourney.__new__(metered.MeteredJourney)
        journey.parameters = {"rate": "0.125", "policy_id": "renamed:fractional-service-policy"}
        documents = journey.source_documents()
        policy = next(d for d in documents if d["schema"] == "ledger-workflow-policy/4")
        self.assertEqual(policy["policy_id"], journey.parameters["policy_id"])
        self.assertEqual(policy["rules"][0]["formula"]["rate"], "0.125")
        original = json.loads((metered.FIXTURES / "source.json").read_bytes())
        policy = next(d for d in original["documents"] if d["schema"] == "ledger-workflow-policy/4")
        self.assertEqual(policy["rules"][0]["formula"]["rate"], "1")

    def closure_journey(self, closed):
        journey = metered.MeteredJourney.__new__(metered.MeteredJourney)
        root, family, outcome, app, cost, claim = [ref(n) for n in ["root", "family", "outcome", "app", "cost", "claim"]]
        records = {"root": {"decision_id": "root-id"}, "family": {"state": "closed_by_root" if closed else "claimed"},
                   "cost": {"current_decision_ref": app}, "app": {"decision_id": "app-id", "status": "reversed" if closed else "active"},
                   "outcome": {"status": "reversed" if closed else "active"}}
        journey.document = lambda r: records[r["id"]]
        def head(kind, *keys):
            if kind == "family":
                return "2", family
            if kind == "app_cost":
                return "2", cost
            return "2", {"root-id": root, "app-id": app, "outcome-id": outcome}[keys[0]]
        journey.head, journey.grant = head, ref("grant")
        work = {"work_ref": ref("work", "workflow-work"), "root_decision_ref": root,
                "target_ref": ref("target"), "outcome_decision_id": "outcome-id", "cost_claim_ref": claim}
        return journey, work

    def test_quantity_selects_exact_active_children_and_family(self):
        journey, work = self.closure_journey(False)
        payload = journey.quantity_payload(work)
        self.assertEqual(payload["replacement_quantity"], "2")
        self.assertEqual({r["decision_ref"]["id"] for r in payload["dependent_reversals"]}, {"app", "outcome"})
        self.assertEqual(payload["family_closures"][0]["family_state_ref"]["id"], "family")

    def test_later_root_reverse_does_not_reverse_terminal_children_twice(self):
        journey, work = self.closure_journey(True)
        selected = journey.closure_inputs(work)
        self.assertEqual(selected["dependent_reversals"], [])
        self.assertEqual(selected["family_closures"], [])

    def test_unavailable_preserves_original_status_without_resubmitting(self):
        with tempfile.TemporaryDirectory() as directory:
            journey = metered.MeteredJourney.__new__(metered.MeteredJourney)
            journey.root = Path(directory)
            journey.inputs = journey.root / "inputs"
            journey.inputs.mkdir()
            journey.log = journey.root / "RUN.json"
            journey.meta = {"commands": [], "checks": []}
            journey.binary = journey.root / "ledger"
            journey.binary.write_bytes(b"synthetic executable")
            journey.installation = journey.root / "installation"
            journey.executable_hashes = {}
            journey.store, journey.scope, journey.source = "synthetic", ["synthetic", "local"], "urn:example:work"
            absent = {"schema": "ledger-operation-status/1", "status": "not_observed", "key": journey.key("quantity")}
            responses = [(7, {"schema": "ledger-cli/1", "status": "error", "code": "UNAVAILABLE"}), (0, absent)]
            calls = []
            def run(argv, **kwargs):
                calls.append(argv)
                code, value = responses.pop(0)
                return subprocess.CompletedProcess(argv, code, supported.canonical(value), b"")
            with patch.object(supported.subprocess, "run", side_effect=run):
                with self.assertRaises(RuntimeError):
                    journey.submit("quantity", "quantity.correct", {"schema": "synthetic-only"})
            self.assertEqual([call[4] for call in calls], ["submit", "status"])
            self.assertEqual(journey.meta["failed_original_status"], absent)
            self.assertFalse((journey.root / "quantity.committed.json").exists())


if __name__ == "__main__":
    unittest.main()
