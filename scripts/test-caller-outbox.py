#!/usr/bin/env python3
"""Real local-engine regressions for the Python and Node caller examples.

LEDGER_BINARY selects an installed or source-built CLI. Transport probes wrap
that real CLI to discard an acknowledgement or damage explanation output;
they do not manufacture accepted receipts or alter the billing database.
"""

import datetime as dt
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
LEDGER = Path(os.environ["LEDGER_BINARY"]).resolve(strict=True)
NODE = shutil.which("node")
if NODE is None:
    raise SystemExit("Node.js is required to verify the Node caller example")


def timestamp(value):
    return value.isoformat(timespec="microseconds").replace("+00:00", "Z")


class CallerOutbox(unittest.TestCase):
    def fixture(self, profile):
        temporary = tempfile.TemporaryDirectory(prefix="bean-counter-caller-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        examples = ROOT / "examples/billing" / ("usage" if profile == "usage" else "")
        setup = json.loads((examples / "setup.json").read_text())
        event = json.loads((examples / "event.json").read_text())
        now = dt.datetime.now(dt.timezone.utc)
        start = now - dt.timedelta(days=1)
        setup["accepted_at"] = timestamp(start)
        for window, offset in [("ordinary", 30), ("corrections", 60)]:
            bounds = setup["outcome_policy"]["families"][0][window]
            bounds["starts_at"] = timestamp(start)
            bounds["occurs_before"] = timestamp(now + dt.timedelta(days=offset))
            bounds["received_by"] = timestamp(now + dt.timedelta(days=offset + 1))
            bounds["accepted_by"] = timestamp(now + dt.timedelta(days=offset + 2))
        event["occurred_at"] = timestamp(start)
        (root / "setup.json").write_text(json.dumps(setup))
        (root / "event.json").write_text(json.dumps(event))
        self.cli(root, "init", "store", "--setup", "setup.json")
        return root, setup, event

    def cli(self, root, *args):
        result = subprocess.run([str(LEDGER), "billing", *args, "--json"],
                                cwd=root, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(result.stdout)

    def statement(self, root, setup):
        return self.cli(root, "--directory", "store", "statement", "--customer", setup["customer"])

    def invoke(self, language, root, setup, *, binary=LEDGER, outbox="outbox"):
        command = ([sys.executable, str(ROOT / "examples/integration/billing_outbox.py")]
                   if language == "python" else [NODE, str(ROOT / "examples/integration/billing_outbox.mjs")])
        return subprocess.run(command + [str(binary), str(root / "store"), setup["customer"],
                                        setup["source"], str(root / "event.json"), str(root / outbox)],
                              cwd=root, capture_output=True, text=True, timeout=30)

    def proxy(self, root, mode):
        path = root / "transport-probe"
        path.write_text("#!" + sys.executable + "\n" +
                        "import json,subprocess,sys\n" +
                        f"real={str(LEDGER)!r}\nmode={mode!r}\n" +
                        "args=sys.argv[1:]\np=subprocess.run([real,*args],capture_output=True,text=True)\n" +
                        "code=p.returncode\nraw=p.stdout\n" +
                        "if 'accept' in args and code==0 and mode=='drop-ack':\n"
                        " raw=json.dumps({'status':'unknown'});code=8\n" +
                        "if 'explain' in args and code==0:\n"
                        " x=json.loads(raw)\n"
                        " if mode=='unknown-schema': x['schema']='ledger-billing-statement/999'\n"
                        " if mode=='period-schema': x['schema']='ledger-billing-statement/4'\n"
                        " if mode=='incomplete': x['complete']=False\n"
                        " if mode=='wrong-receipt': x['entries'][0]['receipt']['id']='wrong-receipt'\n"
                        " if mode=='wrong-operation':\n"
                        "  for record in x['entries'][0]['records']:\n"
                        "   if record['kind']=='event': record['body']['data']['operation_id']='wrong-operation'\n"
                        " if mode=='explain-failed': code=3\n"
                        " raw=json.dumps(x)\n"
                        "sys.stdout.write(raw)\nsys.stderr.write(p.stderr)\nsys.exit(code)\n")
        path.chmod(0o700)
        return path

    def acknowledged(self, language, root, setup, outbox="outbox"):
        result = self.invoke(language, root, setup, outbox=outbox)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        receipt = json.loads((root / outbox / "receipt.json").read_text())
        self.assertEqual((root / outbox / "request.json").read_bytes(), (root / "event.json").read_bytes())
        return receipt

    def exact_retry(self, language, profile):
        root, setup, _ = self.fixture(profile)
        first = self.acknowledged(language, root, setup)
        before = self.statement(root, setup)
        self.assertEqual(before["schema"], "ledger-billing-statement/3" if profile == "usage" else "ledger-billing-statement/2")
        self.assertEqual(before["net_atoms"], "25000000000000" if profile == "usage" else "250")
        self.assertEqual(len(before["entries"]), 1)
        self.assertEqual(self.acknowledged(language, root, setup), first)
        self.assertEqual(self.statement(root, setup), before)

    def lost_ack_restart(self, language, profile):
        root, setup, _ = self.fixture(profile)
        result = self.invoke(language, root, setup, binary=self.proxy(root, "drop-ack"))
        self.assertEqual(result.returncode, 8, result.stdout + result.stderr)
        self.assertFalse((root / "outbox/receipt.json").exists())
        self.assertEqual((root / "outbox/request.json").read_bytes(), (root / "event.json").read_bytes())
        before = self.statement(root, setup)
        receipt = self.acknowledged(language, root, setup)
        self.assertEqual(receipt, before["entries"][0]["receipt"])
        self.assertEqual(self.statement(root, setup), before)

    def identity_conflict(self, language, profile):
        root, setup, event = self.fixture(profile)
        self.acknowledged(language, root, setup)
        before = self.statement(root, setup)
        saved = (root / "outbox/request.json").read_bytes()
        event["operation_id"] = "different-operation-same-delivery"
        (root / "event.json").write_text(json.dumps(event))
        for outbox in ["outbox", "new-caller-outbox"]:
            result = self.invoke(language, root, setup, outbox=outbox)
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertEqual(self.statement(root, setup), before)
        self.assertEqual((root / "outbox/request.json").read_bytes(), saved)
        self.assertFalse((root / "new-caller-outbox/receipt.json").exists())

    def explanation_refusal(self, language, profile):
        for mode in ["unknown-schema", "period-schema", "incomplete", "wrong-receipt", "wrong-operation", "explain-failed"]:
            with self.subTest(mode=mode):
                root, setup, _ = self.fixture(profile)
                result = self.invoke(language, root, setup, binary=self.proxy(root, mode))
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertFalse((root / "outbox/receipt.json").exists())
                self.assertEqual((root / "outbox/request.json").read_bytes(), (root / "event.json").read_bytes())
                before = self.statement(root, setup)
                receipt = self.acknowledged(language, root, setup)
                self.assertEqual(receipt, before["entries"][0]["receipt"])
                self.assertEqual(self.statement(root, setup), before)

    def test_two_product_callers_share_installation_without_cross_customer_effects(self):
        root, report_setup, report_event = self.fixture("fixed")
        report_receipt = self.acknowledged("python", root, report_setup, "report-outbox")
        report_statement = self.statement(root, report_setup)
        _, draft_setup, draft_event = self.fixture("usage")
        for key in ["scope", "store_id", "operator", "host"]:
            draft_setup[key] = report_setup[key]
        registration = {
            "schema": "ledger-billing-registration/2", "customer": draft_setup["customer"],
            "source": draft_setup["source"], "change_id": "register-drafting-product",
            "expected_revision": "0", "effective_at": draft_setup["accepted_at"], "setup": draft_setup,
        }
        (root / "registration.json").write_text(json.dumps(registration))
        self.cli(root, "--directory", "store", "agreement", "--customer", draft_setup["customer"],
                 "--source", draft_setup["source"], "registration.json")
        (root / "event.json").write_text(json.dumps(draft_event))
        draft_receipt = self.acknowledged("node", root, draft_setup, "draft-outbox")
        draft_statement = self.statement(root, draft_setup)
        self.assertEqual(draft_statement["net_atoms"], "25000000000000")
        self.assertEqual(len(draft_statement["entries"]), 1)
        self.assertNotEqual(report_receipt["body"]["target"], draft_receipt["body"]["target"])
        self.assertEqual(self.statement(root, report_setup), report_statement)
        self.assertEqual(self.acknowledged("node", root, draft_setup, "draft-outbox"), draft_receipt)
        (root / "event.json").write_text(json.dumps(report_event))
        self.assertEqual(self.acknowledged("python", root, report_setup, "report-outbox"), report_receipt)
        self.assertEqual(self.statement(root, report_setup), report_statement)
        self.assertEqual(self.statement(root, draft_setup), draft_statement)
        refused = subprocess.run(
            [str(LEDGER), "billing", "--directory", "store", "explain", "--customer",
             report_setup["customer"], draft_receipt["body"]["target"], "--json"],
            cwd=root, capture_output=True, text=True, timeout=30)
        self.assertNotEqual(refused.returncode, 0)
        self.assertNotIn(draft_receipt["id"], refused.stdout)


for language in ["python", "node"]:
    for profile in ["fixed", "usage"]:
        for scenario in ["exact_retry", "lost_ack_restart", "identity_conflict", "explanation_refusal"]:
            def case(self, language=language, profile=profile, scenario=scenario):
                getattr(self, scenario)(language, profile)
            setattr(CallerOutbox, f"test_{language}_{profile}_{scenario}", case)

if __name__ == "__main__":
    unittest.main(verbosity=2)
