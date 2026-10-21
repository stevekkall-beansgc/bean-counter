#!/usr/bin/env python3
"""Bounded original-identity recovery in the maintained synthetic driver."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "workflow_supported", Path(__file__).with_name("e2e") / "workflow_supported.py")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class AvailabilityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        root = Path(self.temporary.name)
        self.journey = RUNNER.Journey.__new__(RUNNER.Journey)
        self.journey.root = root
        self.journey.inputs = root / "inputs"
        self.journey.inputs.mkdir()
        self.journey.installation = root / "installation"
        self.journey.binary = root / "ledger"
        self.journey.binary.write_bytes(b"synthetic executable")
        self.journey.log = root / "RUN.json"
        self.journey.meta = {"commands": [], "checks": []}
        self.journey.executable_hashes = {}
        self.journey.store = "synthetic-store"
        self.journey.scope = ["synthetic", "local"]
        self.journey.source = "urn:example:work"
        self.journey.snapshot = lambda: {"tables": {"synthetic": ["unchanged"]}}
        self.request = root / "example.request.json"
        self.request.write_bytes(RUNNER.canonical({"schema": "ledger-operation/1", "contract": "workflow/1",
            "kind": "work.reverse", "key": self.journey.key("example"), "payload": {"unchanged": True}}))
        self.raw = self.request.read_bytes()
        self.receipt = {"key": self.journey.key("example"), "original_request_bytes_sha256": RUNNER.sha(self.raw)}
        self.committed = {"schema": "ledger-operation-result/1", "status": "committed", "receipt": self.receipt,
                          "request_evidence": {"synthetic": True}}
        self.absent = {"schema": "ledger-operation-status/1", "status": "not_observed", "key": self.journey.key("example")}
        self.unavailable = {"schema": "ledger-cli/1", "status": "error", "code": "UNAVAILABLE", "message": "synthetic unavailable"}
        self.calls = []

    def tearDown(self):
        self.temporary.cleanup()

    def fake(self, responses):
        responses = iter(responses)
        def run(argv, **kwargs):
            self.calls.append([str(v) for v in argv])
            if "submit" in argv:
                self.assertEqual(Path(argv[argv.index("submit") + 1]).read_bytes(), self.raw)
            code, value = next(responses)
            return subprocess.CompletedProcess(argv, code, RUNNER.canonical(value), b"retained stderr")
        return run

    def submit(self):
        return self.journey.submit("example", "work.reverse", {"ignored because exact saved request exists": True})

    def test_committed_status_resolves_lost_reply_without_second_submit(self):
        status = dict(self.committed, schema="ledger-operation-status/1")
        with patch.object(RUNNER.subprocess, "run", side_effect=self.fake([(7, self.unavailable), (0, status)])):
            self.assertEqual(self.submit(), self.committed)
        self.assertEqual([a[4] for a in self.calls], ["submit", "status"])
        self.assertEqual(self.journey.meta["commands"][0]["exit"], 7)
        self.assertTrue((self.journey.root / "000-example.stdout").exists())

    def test_explicit_absence_allows_only_one_identical_resubmit(self):
        with patch.object(RUNNER.subprocess, "run", side_effect=self.fake([(7, self.unavailable), (0, self.absent), (0, self.committed)])):
            self.assertEqual(self.submit(), self.committed)
        self.assertEqual([a[4] for a in self.calls], ["submit", "status", "submit"])
        self.assertEqual(self.calls[0][5], self.calls[2][5])
        self.assertEqual(self.request.read_bytes(), self.raw)

    def test_second_unavailable_stops_without_another_status_or_submit(self):
        with patch.object(RUNNER.subprocess, "run", side_effect=self.fake([(7, self.unavailable), (0, self.absent), (7, self.unavailable)])):
            with self.assertRaises(RuntimeError):
                self.submit()
        self.assertEqual(len(self.calls), 3)
        self.assertFalse((self.journey.root / "example.committed.json").exists())

    def test_other_errors_and_negative_invocations_never_recover(self):
        for code, value in [(3, dict(self.unavailable, code="INTEGRITY_FAILURE")), (7, dict(self.unavailable, code="OTHER"))]:
            with self.subTest(code=code, value=value), patch.object(RUNNER.subprocess, "run", side_effect=self.fake([(code, value)])):
                with self.assertRaises(RuntimeError):
                    self.submit()
        with patch.object(RUNNER.subprocess, "run", side_effect=self.fake([(7, self.unavailable)])):
            with self.assertRaises(RuntimeError):
                self.journey.workflow("negative", "submit", self.request, expected=3)
        self.assertEqual(len(self.calls), 3)

    def test_status_error_malformed_or_wrong_original_stops(self):
        status = dict(self.committed, schema="ledger-operation-status/1")
        status["receipt"] = dict(self.receipt, original_request_bytes_sha256="wrong")
        for code, value in [(7, self.unavailable), (0, {}), (0, status), (0, dict(self.absent, key={}))]:
            before = len(self.calls)
            with self.subTest(value=value), patch.object(RUNNER.subprocess, "run", side_effect=self.fake([(7, self.unavailable), (code, value)])):
                with self.assertRaises((RuntimeError, AssertionError)):
                    self.submit()
            self.assertEqual(len(self.calls) - before, 2)


if __name__ == "__main__":
    unittest.main()
