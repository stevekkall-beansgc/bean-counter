#!/usr/bin/env python3
"""Temporary phase telemetry; never a replacement compliance qualification."""

import datetime
import hashlib
from pathlib import Path
import runpy
import subprocess
import sys
import time


PHASES = {
    ("sh", "scripts/ci-setup.sh"): "SETUP",
    ("sh", "scripts/check-local-billing.sh"): "UNIT",
    ("sh", "scripts/check-local-billing-e2e.sh"): "E2E",
}


def main():
    checker = Path(sys.argv[1])
    # Same checker identity; v0.4.19 adapts it only for repo=agents.
    # Its bean-counter branch executes this raw v0.4.4 source unchanged.
    expected = "69ebc139280b5c3a1d7490399c6534be72ef2b76f4569ab3427c49996d87762f"
    source = checker.read_text().rstrip("\n")
    if hashlib.sha256(source.encode()).hexdigest() != expected:
        raise RuntimeError("Released diagnostic checker source changed")
    manifest = Path("qa-kit/manifest.json").read_bytes()
    if hashlib.sha256(manifest).hexdigest() != "e802ab8825c1785302bbbbe0f0c753b424f8128075d7baeee39382ab3d1a3cfd":
        raise RuntimeError("Released diagnostic manifest changed")

    original_run = subprocess.run

    def observed_run(*args, **kwargs):
        command = args[0] if args else kwargs.get("args")
        phase = PHASES.get(tuple(command), "OTHER") if isinstance(command, (list, tuple)) else "OTHER"
        started = time.monotonic()
        utc = datetime.datetime.now(datetime.timezone.utc).isoformat()
        print(f"DIAGNOSTIC {phase} START utc={utc}", flush=True)
        try:
            result = original_run(*args, **kwargs)
        except BaseException as error:
            outcome = "timeout" if isinstance(error, subprocess.TimeoutExpired) else "exception"
            print(f"DIAGNOSTIC {phase} END elapsed={time.monotonic() - started:.3f}s outcome={outcome}", flush=True)
            raise
        print(f"DIAGNOSTIC {phase} END elapsed={time.monotonic() - started:.3f}s exit={result.returncode}", flush=True)
        return result

    subprocess.run = observed_run
    sys.argv = sys.argv[1:]
    try:
        runpy.run_path(str(checker), run_name="__main__")
    finally:
        subprocess.run = original_run


if __name__ == "__main__":
    main()
