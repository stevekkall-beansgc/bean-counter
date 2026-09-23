#!/usr/bin/env python3
"""Exercise the interactive billing wizard through a real pseudo-terminal."""
import json
import os
import pty
import select
import stat
import subprocess
import sys
import tempfile
import time


def invoke(binary, destination, answers):
    master, slave = pty.openpty()
    try:
        process = subprocess.Popen(
            [binary, "billing", "setup", str(destination), "--json"],
            stdin=slave, stdout=slave, stderr=slave, close_fds=True,
        )
        os.close(slave)
        if answers:
            os.write(master, ("\n".join(answers) + "\n").encode())
        output = bytearray()
        deadline = time.monotonic() + 30
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([master], [], [], remaining)[0]:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                raise AssertionError(f"interactive setup timed out; transcript so far:\n{output.decode(errors='replace')}")
            try:
                chunk = os.read(master, 65536)
            except OSError:  # Linux PTY reports EIO when the child closes.
                break
            if not chunk:
                break
            output.extend(chunk)
        status = process.wait(timeout=30)
        text = output.decode(errors="replace")
        results = []
        decoder = json.JSONDecoder()
        for index, character in enumerate(text):
            if character != "{":
                continue
            try:
                value, _ = decoder.raw_decode(text[index:])
            except json.JSONDecodeError:
                continue
            if isinstance(value, dict) and value.get("status") in {"error", "initialized"}:
                results.append(value)
        if not results:
            raise AssertionError(f"CLI emitted no JSON result (exit {status}):\n{text}")
        return status, results[-1], text
    finally:
        try:
            os.close(slave)
        except OSError:
            pass
        os.close(master)


def answers_for_success():
    return [
        "synthetic-tenant", "local", "synthetic-customer", "synthetic-host",
        "synthetic-operator", "urn:example:product", "synthetic-agreement",
        "synthetic-authorized-acceptor", "0.02", "2026-09-22T00:00:00.000000Z",
        "2026-09-22T13:00:00.000000Z", "2036-10-01T00:00:00.000000Z",
        "2036-10-02T00:00:00.000000Z", "2036-10-03T00:00:00.000000Z",
        "2026-09-22T13:00:00.000000Z", "2036-10-15T00:00:00.000000Z",
        "2036-10-16T00:00:00.000000Z", "2036-10-17T00:00:00.000000Z",
        "delivery", "success", "0.98", "yes", "unsuccessful-by-cutoff",
        "-0.02", "no", "success,unsuccessful-by-cutoff", "0.98", "yes",
        "SYNTHETIC TEST ONLY; no actual customer assent.",
        "SYNTHETIC TEST ONLY; no real operator authority is asserted.",
        "SYNTHETIC TEST ONLY; these are demonstration finality terms.",
        "yes", "read, submit, correct", "CREATE",
    ]


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check-guided-billing-setup.py LEDGER_BINARY")
    binary = os.path.abspath(sys.argv[1])
    with tempfile.TemporaryDirectory(prefix="bean-counter-guided-pty-") as temporary:
        root = os.path.realpath(temporary)

        cancelled = os.path.join(root, "cancelled")
        status, result, _ = invoke(binary, cancelled, ["cancel"])
        assert status == 2 and result.get("code") == "CONFIG" and "cancelled" in result.get("message", "").lower(), result
        assert not os.path.lexists(cancelled), "cancellation created an installation"

        invalid = os.path.join(root, "invalid")
        invalid_answers = answers_for_success()[:8] + ["1.005"]
        status, result, _ = invoke(binary, invalid, invalid_answers)
        assert status != 0, result
        assert not os.path.lexists(invalid), "invalid exact-money input created an installation"

        existing = os.path.join(root, "existing")
        os.mkdir(existing, 0o700)
        sentinel = os.path.join(existing, "keep")
        with open(sentinel, "w", encoding="utf-8") as stream:
            stream.write("unchanged")
        status, result, _ = invoke(binary, existing, [])
        assert status == 2 and result.get("code") == "BILLING_DIRECTORY_EXISTS", result
        assert open(sentinel, encoding="utf-8").read() == "unchanged"

        destination = os.path.join(root, "guided-install")
        status, result, transcript = invoke(binary, destination, answers_for_success())
        assert status == 0 and result.get("status") == "initialized", result
        assert result.get("setup_config_retained") is True, result
        config_path = os.path.join(destination, "setup.json")
        assert os.path.isfile(os.path.join(destination, ".ledger", "local.db"))
        setup = json.load(open(config_path, encoding="utf-8"))
        assert setup["price"] == "0.02", setup["price"]
        assert setup["permissions"] == ["read", "submit", "correct"], setup["permissions"]
        assert setup["assent_evidence"].startswith("SYNTHETIC TEST ONLY"), setup
        assert stat.S_IMODE(os.stat(config_path).st_mode) == 0o600
        if os.name == "posix":
            assert stat.S_IMODE(os.stat(destination).st_mode) == 0o700
        assert "Type CREATE" in transcript

        no_correction = os.path.join(root, "no-correction")
        no_correction_answers = answers_for_success()
        no_correction_answers[31] = "no"
        no_correction_answers[32] = "read, submit"
        status, result, _ = invoke(binary, no_correction, no_correction_answers)
        assert status == 0 and result.get("setup_config_retained") is True, result
        no_correction_setup = json.load(open(os.path.join(no_correction, "setup.json"), encoding="utf-8"))
        assert no_correction_setup["permissions"] == ["read", "submit"]
        print("guided setup PTY checks passed: no JSON input, exact money, private config, explicit CREATE, cancellation/invalid/existing-path no-write")


if __name__ == "__main__":
    main()
