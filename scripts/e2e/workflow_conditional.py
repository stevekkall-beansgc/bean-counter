#!/usr/bin/env python3
"""One bounded conditional service with an independent app expense.

The accepted enum context is immutable. A nonmatching service retains its Work
root and app dependency but has no service target or outcome family. The driver
never automatically resubmits a failed operation.
"""
import argparse
import json
import shutil
from pathlib import Path

from workflow_attested import AttestedJourney
from workflow_attested_negatives import refuse
from workflow_supported import ATOMS, REPO, canonical, sha, unique

FIXTURES = REPO / "examples/workflow-conditional"
ADMIN_OPERATIONS = ["main-quantity-proof", "main-proof-source-narrow", "main-quantity-replacement-proof"]


class ConditionalJourney(AttestedJourney):
    fixture_directory = FIXTURES

    def __init__(self, args):
        super().__init__(args)
        context = self.business["conditional_context"]
        self.matches = args.context == "matching"
        self.context_value = context["matching_value" if self.matches else "nonmatching_value"]
        self.meta["conditional_inputs"] = {"context": args.context, "field": context["field"],
            "value": self.context_value, "service_condition_only": True, "independent_app": True,
            "source_sha256": sha((FIXTURES / "source.json").read_bytes()),
            "business_sha256": sha((FIXTURES / "business.json").read_bytes())}
        self.save()

    def submit(self, name, kind, payload):
        if name == "acceptance":
            self.write("acceptance.request.json", {"schema": "ledger-operation/1", "contract": "workflow/1",
                "kind": kind, "key": self.key(name), "payload": payload})
            self.save()
            self.preserve_fixture("pre-acceptance-fixture")
        return super().submit(name, kind, payload)

    def accepted_context(self):
        return {"fact_schema_ref": self.fact_refs["support:context"],
                "facts": {self.business["conditional_context"]["field"]: self.context_value}}

    def work_result(self, prefix, result):
        if self.matches:
            return super().work_result(prefix, result)
        kinds = [r["kind"] for r in result["receipt"]["result_refs"]]
        assert "workflow-target" not in kinds and "workflow-family-state" not in kinds
        return {"prefix": prefix, "receipt": result, "work_ref": unique(result, "workflow-work"),
                "root_decision_ref": unique(result, "workflow-decision")}

    def family_closures(self, work):
        if self.matches:
            return super().family_closures(work)
        assert "target_ref" not in work
        return []

    def check_closed(self, work):
        if self.matches:
            return super().check_closed(work)
        assert "target_ref" not in work
        if work.get("cost_claim_ref"):
            claim = work["cost_claim_ref"]
            _, state = self.head("app_cost", claim["kind"], claim["id"], claim["digest"])
            _, decision = self.head("decision", self.document(self.document(state)["current_decision_ref"])["decision_id"])
            body = self.document(decision)
            assert body["status"] == "reversed" and body["active_action_refs"] == []
        with self.connection() as db:
            assert db.execute("SELECT COUNT(*) FROM workflow_financial_outcome_claims").fetchone()[0] == 0
        self.meta["checks"].append({"check": "nonmatching_service_has_no_family_and_app_dependency_closed",
                                    "work_ref": work["work_ref"]})
        self.save()

    def sequence(self, pending=False):
        if self.matches:
            # Existing attested sequence, with only the accepted-context input changed.
            super().sequence()
            return
        work = self.work("main")
        self.total("main", 0)
        self.app_cost(work, finalize=not pending)
        operations = ["main-work"]
        if not pending:
            self.total("main", 2 * ATOMS)
            operations.append("main-finalize")
        payload = self.quantity_payload(work)
        request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "quantity.correct",
                   "key": self.key("main-quantity-correct"), "payload": payload}
        (self.root / "main-quantity-correct.request.json").write_bytes(canonical(request))
        self.meta["work_selection"] = work
        self.save()
        self.preserve_fixture("pre-quantity-fixture")
        self.submit("main-quantity-correct", "quantity.correct", payload)
        self.total("main", 0)
        self.check_closed(work)
        operations.append("main-quantity-correct")
        if pending:
            request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "cost.finalize",
                "key": self.key("pending-finalize-after-quantity"), "payload": {
                    "schema": "ledger-workflow-cost-finalize/2", "original_work_ref": work["work_ref"],
                    "component_slot": "support:model-retail", "observation_proof": work["observation"],
                    "final_cost_proof": work["final_evidence"], "evidence_refs": []}}
            refuse(self, "pending-finalize-after-quantity", request, "TARGET_CLOSED")
            with self.connection() as db:
                assert db.execute("SELECT COUNT(*) FROM workflow_app_cost_claims").fetchone()[0] == 0
        self.reverse(work, "main-reverse")
        self.total("main", 0)
        operations.append("main-reverse")
        self.reopen_retry(operations + ADMIN_OPERATIONS)
        self.meta["status"] = "conditional_nonmatching_" + ("pending" if pending else "finalized") + "_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--context", choices=["matching", "nonmatching"], required=True)
    parser.add_argument("--pending", action="store_true")
    args = parser.parse_args()
    if args.pending and args.context != "nonmatching":
        parser.error("--pending is the bounded nonmatching app-dependency case")
    args.resume, args.variant, args.deadline_seconds, args.pending_finalize_check = False, "main", None, False
    journey = ConditionalJourney(args)
    try:
        journey.setup()
        journey.sequence(args.pending)
    except Exception:
        journey.meta["status"] = "STOPPED_FIRST_FAILURE"
        journey.save()
        if journey.installation.exists():
            shutil.copytree(journey.installation, journey.root / "failed-quiescent-installation", copy_function=shutil.copy2)
        raise


if __name__ == "__main__":
    main()
