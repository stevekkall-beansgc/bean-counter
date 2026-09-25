#!/usr/bin/env python3
"""Process-level E2E: actual candidate CLI, SQLite reopen, worker and abrupt exits.

No production module imports, test framework, mocked store or pricing evaluator.
The tiny ASCII-only digest oracle follows the written candidate contract.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "contracts/candidates/admission-v1"
PROFILE = "admission/1-candidate.1"
PRODUCTS = ["Amber", "Birch", "Cedar", "Delta", "Elm"]

def digest(domain, value):
    body = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(("ledgerlab/" + domain + "/" + PROFILE + "\0").encode() + body).hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--candidate-ledger", type=Path, required=True)
    parser.add_argument("--default-ledger", type=Path, required=True)
    args = parser.parse_args()
    binary = args.ledger.resolve()
    candidate_binary = args.candidate_ledger.resolve()
    default_binary = args.default_ledger.resolve()
    os.umask(0o077)
    scratch = ROOT / "work/zen-charge-pilot-e2e"
    scratch.mkdir(parents=True, exist_ok=True)
    count = 0
    with tempfile.TemporaryDirectory(dir=scratch) as tmp:
        root = Path(tmp)
        serial = 0

        def environment(at=None, crash=None):
            env = dict(os.environ)
            env.pop("LEDGER_ZEN_CANDIDATE_CRASH", None)
            env.pop("LEDGER_ZEN_E2E_TIME_US", None)
            env.pop("LEDGER_ZEN_E2E_PAUSE_DIR", None)
            if crash:
                env["LEDGER_ZEN_CANDIDATE_CRASH"] = crash
            if at is not None:
                env["LEDGER_ZEN_E2E_TIME_US"] = str(at)
            return env

        def call(op, store, payload=None, expected=0, crash=None, at=None, raw_input=None, executable=None):
            nonlocal serial, count
            serial += 1
            argv = [str(executable or binary), "zen-charge-candidate", op, str(store), "--json"]
            if payload is not None or raw_input is not None:
                f = root / f"request-{serial}.json"
                f.write_bytes(raw_input if raw_input is not None else json.dumps(payload).encode())
                argv.append(str(f))
            result = subprocess.run(argv, capture_output=True, text=True, env=environment(at, crash), timeout=20)
            assert result.returncode == expected, (argv, result.returncode, result.stdout, result.stderr)
            count += 1
            return json.loads(result.stdout) if result.stdout.strip() else None

        def fresh(name, setup=None):
            store = root / name
            setup = setup or json.loads((FIXTURES / "setup.json").read_text())
            initial = call("init", store, setup)
            assert initial["net_atoms"] == "0" and initial["slot_owner"] is None
            return store, initial

        def command(binding, op, artifact=None):
            c = {"schema": PROFILE, "operation": op, "binding": copy.deepcopy(binding),
                 "evidence": {"delivery_id": "d", "attempt_id": "a", "session_id": "s", "model": "synthetic/worker", "outcome_id": "o"}}
            if artifact is not None:
                c["artifact"] = artifact
            return c

        def statement(store, atoms):
            s = call("statement", store)
            assert s["complete"] is True and s["net_atoms"] == str(atoms)
            assert s["payment_collected"] is False and s["provider_cost_accounted"] is False
            return s

        def verify_receipt(response, kind, atoms):
            r = response["receipt"]
            assert r["body"]["kind"] == kind and r["body"]["atoms"] == str(atoms)
            assert digest("receipt", r["body"]) == r["id"]
            return r

        def race(store, commands, allowed, at=None):
            """Synchronize independent processes before they exec the real CLI.

            Real owner-lock contention may return 7. Never count refusal as a
            completed semantic attempt; the caller reconciles each contender.
            """
            nonlocal serial, count
            serial += 1
            directory = root / f"race-{serial}"
            directory.mkdir()
            gate = directory / "go"
            launcher = (
                "import os,sys,time\nfrom pathlib import Path\n"
                "Path(sys.argv[1]).touch()\n"
                "while not Path(sys.argv[2]).exists(): time.sleep(0.005)\n"
                "os.execv(sys.argv[3],sys.argv[3:])\n"
            )
            processes = []
            try:
                for index, cmd in enumerate(commands):
                    request = directory / f"request-{index}.json"
                    request.write_text(json.dumps(cmd))
                    ready = directory / f"ready-{index}"
                    argv = [sys.executable, "-c", launcher, str(ready), str(gate), str(binary),
                            "zen-charge-candidate", "submit", str(store), str(request), "--json"]
                    processes.append(subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                                      text=True, env=environment(at)))
                deadline = time.monotonic() + 10
                while not all((directory / f"ready-{i}").exists() for i in range(len(commands))):
                    assert time.monotonic() < deadline, "race launch barrier timed out"
                    time.sleep(0.005)
                gate.touch()
                results = []
                for p in processes:
                    stdout, stderr = p.communicate(timeout=20)
                    assert p.returncode in allowed, (p.returncode, stdout, stderr)
                    count += 1
                    results.append((p.returncode, json.loads(stdout)))
                return results
            finally:
                for p in processes:
                    if p.poll() is None:
                        p.kill()
                        p.communicate()

        # Default build must reject the candidate route and retain basic CLI behavior.
        version = subprocess.run([str(default_binary), "--version"], capture_output=True, text=True,
                                 env=environment(1, "before_commit"), timeout=20)
        assert version.returncode == 0 and version.stdout.startswith("ledger ")
        help_result = subprocess.run([str(default_binary), "--help"], capture_output=True, text=True, timeout=20)
        assert help_result.returncode == 0 and "billing" in help_result.stdout
        disabled_store = root / "disabled"
        call("init", disabled_store, json.loads((FIXTURES / "setup.json").read_text()),
             executable=default_binary, expected=2, at=1, crash="before_commit")
        assert not disabled_store.exists()
        # Exercise the existing released-profile command path in a fresh synthetic store.
        legacy_setup = json.loads((ROOT / "examples/billing/setup.json").read_text())
        legacy_setup["assent_evidence"] = "SYNTHETIC E2E ONLY; no customer assent."
        legacy_setup["operator_attestation"] = "SYNTHETIC E2E ONLY; no real authorization."
        legacy_setup["finality_attestation"] = "SYNTHETIC E2E ONLY; fixed fixture work."
        for window in ["ordinary", "corrections"]:
            legacy_setup["outcome_policy"]["families"][0][window].update(
                occurs_before="2096-01-01T00:00:00.000000Z",
                received_by="2096-01-02T00:00:00.000000Z",
                accepted_by="2096-01-03T00:00:00.000000Z")
        legacy_setup_file = root / "legacy-setup.json"
        legacy_setup_file.write_text(json.dumps(legacy_setup))
        legacy_store = root / "feature-off-billing"

        def legacy(arguments):
            nonlocal count
            result = subprocess.run([str(default_binary), "billing", *arguments, "--json"],
                                    capture_output=True, text=True, timeout=20, env=environment())
            assert result.returncode == 0, (arguments, result.stdout, result.stderr)
            count += 1
            return json.loads(result.stdout)

        legacy(["init", str(legacy_store), "--setup", str(legacy_setup_file)])
        original = legacy(["--directory", str(legacy_store), "accept", str(ROOT / "examples/billing/event.json")])
        retry = legacy(["--directory", str(legacy_store), "accept", str(ROOT / "examples/billing/event.json")])
        assert original["status"] == "accepted" and retry["status"] == "duplicate"
        assert original["receipt"] == retry["receipt"]
        old_statement = legacy(["--directory", str(legacy_store), "statement", "--customer", "customer-1"])
        assert old_statement["complete"] is True and old_statement["net_atoms"] == "250"
        assert old_statement["payment_collected"] is False
        # The ordinary candidate ignores both private test-hook environment variables.
        ordinary_store = root / "ordinary-candidate"
        normal = call("init", ordinary_store, json.loads((FIXTURES / "setup.json").read_text()), executable=candidate_binary)
        start_us = time.time_ns() // 1000
        response = call("submit", ordinary_store, command(normal["orders"][0]["binding"], "reserve"),
                        executable=candidate_binary, at=1, crash="before_commit")
        end_us = time.time_ns() // 1000
        assert start_us <= int(response["receipt"]["body"]["accepted_at_us"]) <= end_us

        # Cancel the public API waiter while its detached SQLite worker is
        # paused with an uncommitted append. A second process must remain locked;
        # after release, replay must recover the one committed receipt.
        cancel_store, cancel_initial = fresh("caller-cancelled-api")
        bounded_api = call("e2e-bounded-api-input", cancel_store)
        assert bounded_api["status"] == "oversized-api-input-rejected", bounded_api
        cancel_command = command(cancel_initial["orders"][0]["binding"], "reserve")
        cancel_request = root / "caller-cancelled-reserve.json"
        cancel_request.write_text(json.dumps(cancel_command))
        pause_dir = root / "caller-cancelled-pause"
        pause_dir.mkdir()
        cancel_env = environment()
        cancel_env["LEDGER_ZEN_E2E_PAUSE_DIR"] = str(pause_dir)
        cancel_process = subprocess.Popen(
            [str(binary), "zen-charge-candidate", "e2e-cancel-submit", str(cancel_store),
             str(cancel_request), "--json"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=cancel_env)
        count += 1
        try:
            deadline = time.monotonic() + 20
            while not (pause_dir / "cancelled").exists():
                assert cancel_process.poll() is None, "cancellation probe exited before reaching its barrier"
                assert time.monotonic() < deadline, "cancellation probe barrier timed out"
                time.sleep(0.005)
            assert (pause_dir / "ready").exists()
            busy = call("statement", cancel_store, expected=7)
            assert busy["code"] == "UNAVAILABLE", busy
            (pause_dir / "release").touch()
            stdout, stderr = cancel_process.communicate(timeout=20)
            assert cancel_process.returncode == 0, (cancel_process.returncode, stdout, stderr)
            assert json.loads(stdout)["status"] == "caller-cancelled", stdout
        finally:
            (pause_dir / "release").touch()
            if cancel_process.poll() is None:
                cancel_process.kill()
                cancel_process.communicate()
        after_cancel = statement(cancel_store, 0)
        assert after_cancel["slot_owner"] == cancel_initial["orders"][0]["binding"]["order_id"]
        saved_reservation = after_cancel["orders"][0]["receipts"]["reserve"]
        recovered = call("submit", cancel_store, cancel_command)
        assert recovered["status"] == "duplicate" and recovered["receipt"] == saved_reservation
        assert statement(cancel_store, 0) == after_cancel

        # Compare complete implementation receipts to checked-in fixed expected values.
        # No production imports, regenerated goldens or runtime-derived expected IDs.
        golden = json.loads((FIXTURES / "expected.json").read_text())
        s, i = fresh("fixed-canonical-receipts")
        binding = i["orders"][0]["binding"]
        for op in ["reserve", "admit", "outcome"]:
            expected_receipt = golden["receipts"][op]
            result = call("submit", s, command(binding, op, PRODUCTS if op == "outcome" else None),
                          at=expected_receipt["body"]["accepted_at_us"])
            assert result["receipt"] == expected_receipt
        reopened = statement(s, int(golden["completed_atoms"]))
        assert reopened["orders"][0]["receipts"] == golden["receipts"]
        # Read retained response bytes in a separate SQLite inspection process.
        inspector = (
            "import json,sqlite3,sys\n"
            "with sqlite3.connect(sys.argv[1]) as c:\n"
            " print(json.dumps([r[0].hex() for r in c.execute('SELECT response FROM candidate_entries ORDER BY seq')]))\n"
        )
        inspection = subprocess.run([sys.executable, "-c", inspector, str(s / "local.db")],
                                    capture_output=True, text=True, timeout=20, check=True)
        expected_bytes = [json.dumps({"candidate": True, "status": "accepted", "receipt": golden["receipts"][op]},
                                    sort_keys=True, separators=(",", ":")).encode().hex()
                          for op in ["reserve", "admit", "outcome"]]
        assert json.loads(inspection.stdout) == expected_bytes

        store, initial = fresh("happy")
        expected_bindings = json.loads((FIXTURES / "bindings.json").read_text())
        assert [o["binding"] for o in initial["orders"]] == expected_bindings
        b, second = expected_bindings
        call("submit", store, command(b, "admit"), expected=3)
        statement(store, 0)
        reserve = verify_receipt(call("submit", store, command(b, "reserve")), "reserve", 0)
        statement(store, 0)
        call("submit", store, command(second, "reserve"), expected=3)
        admit = verify_receipt(call("submit", store, command(b, "admit")), "admit", 1)
        assert admit["body"]["reservation_receipt"] == reserve["id"]
        statement(store, 1)
        # Real child executor; no fake ledger or imported production functions.
        worker = subprocess.run([sys.executable, str(ROOT / "examples/zen-charge-pilot/fixture_worker.py")],
                                input=initial["deliverable_input"], capture_output=True, text=True, timeout=10)
        assert worker.returncode == 0
        artifact = json.loads(worker.stdout)
        assert artifact == PRODUCTS
        # Work alone does not charge B. The host must submit it for authorized acceptance.
        statement(store, 1)
        call("submit", store, command(b, "outcome", ["Done"]), expected=3)
        statement(store, 1)
        outcome = verify_receipt(call("submit", store, command(b, "outcome", artifact)), "outcome", 499)
        assert outcome["body"]["admission_receipt"] == admit["id"]
        assert outcome["body"]["artifact_hash"] == digest("artifact", artifact)
        assert call("submit", store, command(b, "fail"), expected=3)["code"] == "TERMINAL"
        baseline = statement(store, 500)
        for op, original in [("reserve", reserve), ("admit", admit), ("outcome", outcome)]:
            for rename in [False, True]:
                cmd = command(b, op, artifact if op == "outcome" else None)
                if rename:
                    cmd["evidence"] = {k: "mutated-" + v for k, v in cmd["evidence"].items()}
                retry = call("submit", store, cmd)
                assert retry["status"] == "duplicate" and retry["receipt"] == original
        for field in b:
            cmd = command(b, "outcome", artifact)
            cmd["binding"][field] = "mutated-" + cmd["binding"][field]
            call("submit", store, cmd, expected=6 if field == "order_id" else 4)
        renamed_purchase = command(b, "outcome", artifact)
        renamed_purchase["binding"] = {k: "forged-" + v for k, v in b.items()}
        renamed_purchase["evidence"] = {k: "forged-" + v for k, v in renamed_purchase["evidence"].items()}
        call("submit", store, renamed_purchase, expected=6)
        assert statement(store, 500) == baseline
        injected = command(b, "outcome", artifact)
        injected["operation_id"] = "new-economic-id"
        call("submit", store, injected, expected=3)
        asserted_success = command(b, "outcome", artifact)
        asserted_success["verified"] = True
        call("submit", store, asserted_success, expected=3)
        call("submit", store, command(b, "outcome", list(reversed(artifact))), expected=3)
        forged = command(second, "reserve")
        forged["binding"]["authorization_id"] = b["authorization_id"]
        call("submit", store, forged, expected=4)
        call("submit", store, command(second, "reserve"))
        call("submit", store, command(second, "admit"))
        call("submit", store, command(second, "outcome", artifact))
        statement(store, 1000)

        for admitted in [False, True]:
            s, i = fresh("failed-" + str(admitted))
            binding = i["orders"][0]["binding"]
            call("submit", s, command(binding, "reserve"))
            if admitted:
                paid = call("submit", s, command(binding, "admit"))
            failed = call("submit", s, command(binding, "fail"))
            assert call("submit", s, command(binding, "fail"))["receipt"] == failed["receipt"]
            call("submit", s, command(binding, "outcome", artifact), expected=3)
            if admitted:
                assert call("submit", s, command(binding, "admit"))["receipt"] == paid["receipt"]
            assert statement(s, int(admitted))["slot_owner"] is None

        # Kill actual ledger processes at transaction boundaries and reopen with new processes.
        for op, before_atoms, after_atoms in [("reserve", 0, 0), ("admit", 0, 1), ("outcome", 1, 500)]:
            for point, exit_code in [("before_commit", 91), ("after_commit", 92)]:
                s, i = fresh(op + "-" + point)
                binding = i["orders"][0]["binding"]
                if op != "reserve":
                    call("submit", s, command(binding, "reserve"))
                if op == "outcome":
                    call("submit", s, command(binding, "admit"))
                cmd = command(binding, op, artifact if op == "outcome" else None)
                call("submit", s, cmd, expected=exit_code, crash=point)
                before_retry = statement(s, before_atoms if point == "before_commit" else after_atoms)
                retry = call("submit", s, cmd)
                assert retry["status"] == ("accepted" if point == "before_commit" else "duplicate")
                if point == "after_commit":
                    assert retry["receipt"] == before_retry["orders"][0]["receipts"][op]
                statement(s, after_atoms)

        # Competing admissions mutate every subordinate label, without changing the order.
        s, i = fresh("race")
        binding = i["orders"][0]["binding"]
        call("submit", s, command(binding, "reserve"))
        cmd = command(binding, "admit")
        contenders = []
        for index in range(4):
            contender = copy.deepcopy(cmd)
            contender["evidence"] = {k: f"renamed-{index}-{v}" for k, v in cmd["evidence"].items()}
            contenders.append(contender)
        results = race(s, contenders, (0, 7))
        assert sum(code == 0 and r["status"] == "accepted" for code, r in results) == 1
        receipt = call("submit", s, cmd)["receipt"]
        for contender in contenders:
            retry = call("submit", s, contender)
            assert retry["status"] == "duplicate" and retry["receipt"] == receipt
        before = statement(s, 1)
        # These are concurrently replaying an already committed operation.
        results = race(s, contenders, (0, 7))
        for (code, result), contender in zip(results, contenders):
            if code == 7:
                result = call("submit", s, contender)
            assert result["status"] == "duplicate" and result["receipt"] == receipt
        assert statement(s, 1) == before

        # Different orders race for the single free slot. Only one may reserve it.
        s, i = fresh("different-order-reservation-race")
        contenders = [command(o["binding"], "reserve") for o in i["orders"]]
        results = race(s, contenders, (0, 3, 7))
        winners = [index for index, (code, r) in enumerate(results) if code == 0 and r["status"] == "accepted"]
        assert len(winners) == 1
        winner = winners[0]
        for index, cmd in enumerate(contenders):
            r = call("submit", s, cmd, expected=0 if index == winner else 3)
            if index == winner:
                assert r["status"] == "duplicate" and r["receipt"] == results[index][1]["receipt"]
            else:
                assert r["code"] == "SLOT_UNAVAILABLE"
        reserved = statement(s, 0)
        assert reserved["slot_owner"] == contenders[winner]["binding"]["order_id"]
        assert sum(o["phase"] == "reserved" for o in reserved["orders"]) == 1

        # Outcome and failure race on the same admitted order: exactly one terminal decision.
        s, i = fresh("outcome-failure-race")
        binding = i["orders"][0]["binding"]
        call("submit", s, command(binding, "reserve"))
        call("submit", s, command(binding, "admit"))
        contenders = [command(binding, "outcome", artifact), command(binding, "fail")]
        results = race(s, contenders, (0, 3, 7))
        winners = [index for index, (code, r) in enumerate(results) if code == 0 and r["status"] == "accepted"]
        assert len(winners) == 1
        winner = winners[0]
        for index, cmd in enumerate(contenders):
            r = call("submit", s, cmd, expected=0 if index == winner else 3)
            if index == winner:
                assert r["status"] == "duplicate" and r["receipt"] == results[index][1]["receipt"]
            else:
                assert r["code"] == ("TERMINAL" if cmd["operation"] == "fail" else "NOT_ADMITTED")
        final = statement(s, 500 if winner == 0 else 1)
        assert final["slot_owner"] is None
        assert final["orders"][0]["phase"] == ("completed" if winner == 0 else "failed")
        receipts = final["orders"][0]["receipts"]
        assert ("outcome" in receipts) != ("fail" in receipts)

        expired = json.loads((FIXTURES / "setup.json").read_text())
        expired.update(authorized_at_us="1", admit_before_us="2", outcome_before_us="3")
        s, i = fresh("expired", expired)
        call("submit", s, command(i["orders"][0]["binding"], "reserve"), expected=3)
        statement(s, 0)

        # Exact times exist only in the separate hooks build; normal CLI has no time input.
        window_setup = json.loads((FIXTURES / "setup.json").read_text())
        window_setup.update(authorized_at_us="100", admit_before_us="200", outcome_before_us="300")
        for op, times in [("reserve", [99, 100, 199, 200, 201]),
                          ("admit", [99, 100, 199, 200, 201]),
                          ("outcome", [109, 110, 299, 300, 301])]:
            for at in times:
                s, i = fresh(f"boundary-{op}-{at}", window_setup)
                binding = i["orders"][0]["binding"]
                if op != "reserve":
                    call("submit", s, command(binding, "reserve"), at=100)
                if op == "outcome":
                    call("submit", s, command(binding, "admit"), at=110)
                lower, upper = (110, 300) if op == "outcome" else (100, 200)
                eligible = lower <= at < upper
                result = call("submit", s, command(binding, op, artifact if op == "outcome" else None),
                              at=at, expected=0 if eligible else 3)
                if not eligible:
                    assert result["code"] == ("CLOCK" if at < lower else "WINDOW")
                amount = (500 if eligible else 1) if op == "outcome" else (1 if eligible and op == "admit" else 0)
                statement(s, amount)

        s, i = fresh("late-ordinary-outcome", window_setup)
        binding = i["orders"][0]["binding"]
        reserve = call("submit", s, command(binding, "reserve"), at=100)["receipt"]
        admit = call("submit", s, command(binding, "admit"), at=110)["receipt"]
        before = statement(s, 1)
        result = call("submit", s, command(binding, "outcome", artifact), at=301, expected=3)
        assert result["code"] == "WINDOW" and statement(s, 1) == before
        for op, receipt in [("reserve", reserve), ("admit", admit)]:
            r = call("submit", s, command(binding, op), at=301)
            assert r["status"] == "duplicate" and r["receipt"] == receipt
        # Expiry never prevents explicit terminal release; it does not refund A.
        call("submit", s, command(binding, "fail"), at=301)
        assert statement(s, 1)["slot_owner"] is None

        s, i = fresh("receipt-replay-after-expiry-and-clock-rollback", window_setup)
        binding = i["orders"][0]["binding"]
        receipts = {}
        for op, at in [("reserve", 100), ("admit", 110), ("outcome", 120)]:
            receipts[op] = call("submit", s, command(binding, op, artifact if op == "outcome" else None), at=at)["receipt"]
        before = statement(s, 500)
        for at in [301, 0]:
            for op, receipt in receipts.items():
                cmd = command(binding, op, artifact if op == "outcome" else None)
                cmd["evidence"] = {k: "late-" + v for k, v in cmd["evidence"].items()}
                r = call("submit", s, cmd, at=at)
                assert r["status"] == "duplicate" and r["receipt"] == receipt
            assert statement(s, 500) == before
        # Clock floor covers other newly authorized orders, not only the active job.
        second_binding = i["orders"][1]["binding"]
        r = call("submit", s, command(second_binding, "reserve"), at=119, expected=3)
        assert r["code"] == "CLOCK" and statement(s, 500) == before
        call("submit", s, command(second_binding, "reserve"), at=120)

        # Malformed transport/claims are refused by the actual process parser.
        s, i = fresh("invalid-wire")
        binding = i["orders"][0]["binding"]
        cmd = command(binding, "reserve")
        encoded = json.dumps(cmd).encode()
        null_binding = copy.deepcopy(cmd)
        null_binding["binding"]["target"] = None
        for raw in [b'{"schema":', b'{"schema":"' + PROFILE.encode() + b'",' + encoded[1:],
                    b'null', json.dumps(null_binding).encode(), b'\xff']:
            call("submit", s, raw_input=raw, expected=3)
            statement(s, 0)
        injected_time = dict(cmd, accepted_at_us="100")
        call("submit", s, injected_time, executable=candidate_binary, expected=3)

        base_setup = json.loads((FIXTURES / "setup.json").read_text())
        for duplicate in ["authorization_id", "order_id"]:
            invalid = copy.deepcopy(base_setup)
            invalid["authorizations"][1][duplicate] = invalid["authorizations"][0][duplicate]
            destination = root / ("duplicate-" + duplicate)
            result = call("init", destination, invalid, expected=3)
            assert result["code"] == "AUTHORIZATION" and not destination.exists()
        for field in ["admission_atoms", "outcome_atoms"]:
            for value in ["0", "10001", "-1", "01"]:
                invalid = copy.deepcopy(base_setup)
                invalid[field] = value
                destination = root / f"bad-price-{field}-{value}"
                call("init", destination, invalid, expected=3)
                assert not destination.exists()
        for bound in ["1", "10000"]:
            prices = dict(base_setup, admission_atoms=bound, outcome_atoms=bound)
            s, i = fresh("accepted-price-bound-" + bound, prices)
            binding = i["orders"][0]["binding"]
            for op in ["reserve", "admit", "outcome"]:
                call("submit", s, command(binding, op, artifact if op == "outcome" else None))
            statement(s, 2 * int(bound))

        # Corrupt only quiescent throwaway stores via a separate SQLite process.
        # This deliberately bypasses triggers, modeling privileged disk/history damage;
        # no live owner or production store is touched.
        corruptor = (
            "import json,sqlite3,sys\n"
            "with sqlite3.connect(sys.argv[1]) as c:\n"
            " c.execute('DROP TRIGGER entries_no_update')\n"
            " mode=sys.argv[2]\n"
            " if mode=='response':\n"
            "  c.execute('UPDATE candidate_entries SET response=? WHERE seq=2',(b\"{}\",))\n"
            " elif mode=='binding':\n"
            "  v=json.loads(c.execute('SELECT command FROM candidate_entries WHERE seq=2').fetchone()[0])\n"
            "  v['binding']['target']='sha256:forged'\n"
            "  c.execute('UPDATE candidate_entries SET command=? WHERE seq=2',(json.dumps(v,sort_keys=True,separators=(',',':')).encode(),))\n"
            " elif mode=='sequence':\n"
            "  c.execute('UPDATE candidate_entries SET seq=3 WHERE seq=2')\n"
            " print(c.execute('SELECT COUNT(*) FROM candidate_entries').fetchone()[0])\n"
        )
        for attack in ["response", "binding", "sequence"]:
            s, i = fresh("corruption-" + attack)
            binding = i["orders"][0]["binding"]
            call("submit", s, command(binding, "reserve"))
            call("submit", s, command(binding, "admit"))
            corruption = subprocess.run([sys.executable, "-c", corruptor, str(s / "local.db"), attack],
                                        capture_output=True, text=True, timeout=20, check=True)
            assert corruption.stdout.strip() == "2"
            result = call("statement", s, expected=9)
            assert result["code"] == "INTEGRITY"
            result = call("submit", s, command(binding, "outcome", artifact), expected=9)
            assert result["code"] == "INTEGRITY"
            counter = subprocess.run([sys.executable, "-c",
                "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); print(c.execute('SELECT COUNT(*) FROM candidate_entries').fetchone()[0]); c.close()",
                str(s / "local.db")], capture_output=True, text=True, timeout=20, check=True)
            assert counter.stdout.strip() == "2"
        print(json.dumps({"status": "passed", "candidate": True, "cli_process_checks": count,
                          "live_zen": False, "power_loss_tested": False, "reviewed_freeze": False,
                          "clock_hooks": "separate-e2e-build", "fixed_receipt_fixture_compared": True,
                          "feature_off_runtime_checked": True}))

if __name__ == "__main__":
    main()
