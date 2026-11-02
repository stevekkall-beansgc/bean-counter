#!/usr/bin/env python3
"""One observation-only exact original D Evaluation read on a copied store."""
import argparse
import json
import sys
from pathlib import Path

from workflow_lifecycle_members_recover import MembersRecovery, READ_SHA, verify_pins
from workflow_supported import canonical, sha


class OriginalDReadTrace(MembersRecovery):
    def command(self, name, argv, expected=0, allow_unavailable=False):
        assert name == "original-d-evaluation" and str(argv[4]) == "read"
        assert Path(argv[5]).read_bytes() == self.failed_raw
        assert not self.meta["commands"], "Only one diagnostic read is allowed"
        return super().command(name, argv, expected, allow_unavailable)

    def trace(self):
        path = self.inputs / "original-d-evaluation.json"
        path.write_bytes(self.failed_raw)
        assert sha(path.read_bytes()) == READ_SHA
        self.meta["proof_scope"].update({"mode": "one_read_diagnostic", "reads_per_phase": None,
            "statuses_per_phase": 0, "production_timing_qualification": False})
        self.save()
        self.meta["diagnostic_result"] = self.workflow("original-d-evaluation", "read", path, expected=None)
        self.meta["status"] = "DIAGNOSTIC_STOPPED_AFTER_ONE_READ"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "pins", "source", "input-map", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    args.ledger = Path(json.loads(args.candidate.read_bytes())["binary_path"])
    args.case, args.resume, args.deadline_seconds, args.pending_finalize_check = "members", False, 0, False
    journey = None
    try:
        journey = OriginalDReadTrace(args)
        journey.trace()
    except Exception as error:
        if journey is not None:
            journey.meta["status"] = "DIAGNOSTIC_STOPPED_WITH_ERROR"
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
