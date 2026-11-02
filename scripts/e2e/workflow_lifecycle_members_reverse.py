#!/usr/bin/env python3
"""One authorized fresh-key D Work reversal, then exact original recovery.

The refused quantity correction remains untouched and status-only. This is a
separate reversal proof; it does not qualify monetary quantity correction.
"""
import argparse
import json
import sys
from pathlib import Path

from workflow_lifecycle_finite import FiniteLifecycle
from workflow_lifecycle_members_correct import MembersCorrection, verify_pins
from workflow_supported import ATOMS, canonical, now, sha


class MembersReversal(MembersCorrection):
    def __init__(self, args):
        super().__init__(args)
        old = self.correction_request["payload"]
        self.reverse_request = {
            "schema": "ledger-operation/1", "contract": "workflow/1", "kind": "work.reverse",
            "key": self.key("reverse-d-work"),
            "payload": {"schema": "ledger-workflow-work-reverse/2", "work_ref": old["work_ref"],
                "expected_revision": old["expected_revision"], "family_closures": old["family_closures"],
                "dependent_reversals": old["dependent_reversals"], "evidence_refs": old["evidence_refs"],
                "reason": "Synthetic original D causal Stage reversal"}}
        proposed = args.proposed_request.read_bytes()
        self.reverse_raw = canonical(self.reverse_request)
        assert self.reverse_raw == proposed
        assert self.reverse_request["key"] != self.correction_request["key"]
        self.b_decision = self.input_map["named_inputs"]["b_root_decision"]["body"]
        self.b_decision_head = self.head("decision", self.b_decision["decision_id"])
        self.b_capacity_head = self.head("capacity", self.b_decision["decision_id"])
        self.meta["proof_scope"].update({
            "mode": "authorized_work_reversal", "new_reversal_key": self.reverse_request["key"],
            "new_reversal_request_sha256": sha(self.reverse_raw),
            "old_quantity_correction_status_only": True, "quantity_correction_qualified": False})
        self.save()

    def key(self, name):
        key = super().key(name)
        if name == "reverse-d-work":
            key["source"] = self.corrector
        return key

    def command(self, name, argv, expected=0, allow_unavailable=False):
        assert Path(argv[0]) == self.binary and str(argv[1]) == "workflow"
        route = str(argv[4])
        assert route in {"submit", "status", "read"}, argv
        if route == "submit":
            assert name == "reverse-d-work" and name not in self.original_names
            assert Path(argv[5]).read_bytes() == self.reverse_raw
        self.assert_originals()
        try:
            return FiniteLifecycle.command(self, name, argv, expected, allow_unavailable)
        finally:
            self.assert_originals()

    def perform(self, name, kind, payload, refusal=None):
        assert name == "reverse-d-work" and kind == "work.reverse" and refusal is None
        request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": kind,
                   "key": self.key(name), "payload": payload}
        assert canonical(request) == self.reverse_raw
        return FiniteLifecycle.perform(self, name, kind, payload, refusal)

    def preflight(self):
        super().preflight()
        old = self.exact_status("preserved-refused-quantity", self.correction_request["key"])
        assert old["status"] == "not_observed" and old["key"] == self.correction_request["key"]
        grant = self.document(self.corrector_grant)
        assert "work.reverse" in grant["operations"] and "correct" in grant["permissions"]
        trusted = now()
        assert grant["starts_at"] <= trusted < grant["ends_at"]
        assert trusted < self.input_map["original_window"]["correction_until"]
        self.meta["original_refused_quantity_status"] = old
        self.meta["reversal_fixture_preflight_at"] = trusted
        self.save()

    def reverse(self):
        result = self.submit("reverse-d-work", "work.reverse", self.reverse_request["payload"])
        assert result["receipt"]["commit_sequence"] == "33"
        self.book_totals("original-d-work-reversal-and-exact-dependent-inverse", 0, 0)
        actions = [ref for ref in result["receipt"]["result_refs"] if ref["kind"] == "workflow-action"]
        assert len(actions) == 1
        inverse = self.document(actions[0])
        assert inverse["inverse_of"] == self.old_action_ref and inverse["posting_role"] == "inverse"
        assert int(inverse["amount"]["atoms"]) == -int(self.old_action["amount"]["atoms"]) == -2 * ATOMS
        for field in ("book", "component_slot", "root_component_slot", "role_ref", "slot_terms_ref", "funding_leg", "basis_refs"):
            assert inverse[field] == self.old_action[field]
        for name in ("d_root_decision", "closure_decision"):
            original = self.input_map["named_inputs"][name]["body"]
            revision, ref = self.head("decision", original["decision_id"])
            current = self.document(ref)
            assert revision == current["revision"] == "2"
            assert current["status"] == "reversed" and current["active_action_refs"] == []
            assert current["operation_key"] == self.reverse_request["key"]
            assert ref in result["receipt"]["result_refs"]
        assert self.document(self.d_ref) == self.d_body and self.document(self.b_ref) == self.b_body
        assert self.document(self.terminal_ref) == self.terminal_body
        assert self.head("stage", self.agreement, self.chain, "support:stage") == ("3", self.terminal_ref)
        assert self.head("decision", self.b_decision["decision_id"]) == self.b_decision_head
        assert self.head("capacity", self.b_decision["decision_id"]) == self.b_capacity_head
        self.meta["verified_exact_inverse"] = {"ref": actions[0], "inverse_of": self.old_action_ref,
            "amount": inverse["amount"], "immutable_stage_ref": self.terminal_ref,
            "original_B_decision_and_capacity_unchanged": True}
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "pins", "source", "input-map", "proposed-request", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    candidate = json.loads(args.candidate.read_bytes())
    args.ledger = Path(candidate["binary_path"])
    args.case, args.resume, args.deadline_seconds, args.pending_finalize_check = "members", False, 0, False
    args.diagnostic = False
    journey = None
    try:
        journey = MembersReversal(args)
        journey.preflight()
        journey.reverse()
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
