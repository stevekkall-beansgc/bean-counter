#!/usr/bin/env python3
"""Bounded synthetic Unit journey using the maintained public workflow driver.

No automatic resubmit: preserve the first failure, original-key status and
quiescent installation. Build the candidate and helpers before running setup.
"""
import argparse
import json
import shutil
from pathlib import Path

from workflow_supported import Journey, REPO, canonical, sha, unique

FIXTURES = REPO / "examples/workflow-metered"
OPERATIONS = ["main-work", "main-finalize", "main-success", "main-quantity-correct", "main-reverse"]


class MeteredJourney(Journey):
    fixture_directory = FIXTURES
    quantity = "3"

    def __init__(self, args):
        super().__init__(args)
        variants = self.fixture_directory / "variants.json"
        self.parameters = json.loads(variants.read_bytes())[args.variant]
        self.meta["metered_inputs"] = {"variant": args.variant, **self.parameters,
                                       "variants_sha256": sha(variants.read_bytes())}
        self.save()

    def source_documents(self):
        documents = super().source_documents()
        policy = next(d for d in documents if d["schema"] == "ledger-workflow-policy/4")
        policy["policy_id"] = self.parameters["policy_id"]
        service = next(r for r in policy["rules"] if r["component_key"] == "support:service")
        assert service["formula"]["kind"] == "unit"
        service["formula"]["rate"] = self.parameters["rate"]
        return documents

    def command(self, name, argv, expected=0, allow_unavailable=False):
        return super().command(name, argv, expected, allow_unavailable=False)

    def submit(self, name, kind, payload):
        try:
            return super().submit(name, kind, payload)
        except Exception:
            path = self.root / (name + ".request.json")
            if path.exists():
                key = self.write(name + ".failure-key.json", json.loads(path.read_bytes())["key"])
                try:
                    status = self.workflow(name + "-failure-status", "status", key)
                    self.meta["failed_original_status"] = status
                except Exception as error:
                    self.meta["failed_original_status"] = {"status": "unknown", "error_type": type(error).__name__}
            self.save()
            raise

    def closure_inputs(self, work):
        root_id = self.document(work["root_decision_ref"])["decision_id"]
        revision, _ = self.head("decision", root_id)
        family_revision, family_ref = self.head("family", work["target_ref"]["kind"],
            work["target_ref"]["id"], work["target_ref"]["digest"], "support:quality")
        family = self.document(family_ref)
        closures = [] if family["state"] == "closed_by_root" else [{"family_state_ref": family_ref,
            "expected_revision": family_revision, "authority_evidence_refs": [self.grant]}]
        dependents = []
        ids = []
        if work.get("outcome_decision_id"):
            ids.append(work["outcome_decision_id"])
        if work.get("cost_claim_ref"):
            claim = work["cost_claim_ref"]
            _, cost = self.head("app_cost", claim["kind"], claim["id"], claim["digest"])
            ids.append(self.document(self.document(cost)["current_decision_ref"])["decision_id"])
        for decision_id in ids:
            rev, ref = self.head("decision", decision_id)
            if self.document(ref)["status"] != "reversed":
                dependents.append({"decision_ref": ref, "expected_revision": rev,
                                   "authority_evidence_refs": [self.grant]})
        return {"work_ref": work["work_ref"], "expected_revision": revision,
                "dependent_reversals": sorted(dependents, key=canonical), "family_closures": closures,
                "evidence_refs": []}

    def quantity_payload(self, work):
        return {"schema": "ledger-workflow-quantity-correction/2", **self.closure_inputs(work),
                "field": "support:quantity", "replacement_quantity": "2", "fact_proofs": []}

    def reverse(self, work, name):
        return self.submit(name, "work.reverse", {"schema": "ledger-workflow-work-reverse/2",
            **self.closure_inputs(work), "reason": "Synthetic reversal of remaining corrected service"})

    def preserve_fixture(self, name):
        fixture = self.root / name
        fixture.mkdir(mode=0o700)
        shutil.copytree(self.installation, fixture / "installation", copy_function=shutil.copy2)
        shutil.copytree(self.inputs, fixture / "inputs", copy_function=shutil.copy2)
        for pattern in ["*.request.json", "*.committed.json"]:
            for path in self.root.glob(pattern):
                shutil.copy2(path, fixture / path.name)
        shutil.copy2(self.log, fixture / "RUN.json")
        return fixture

    def check_closed(self, work):
        _, family = self.head("family", work["target_ref"]["kind"], work["target_ref"]["id"],
                              work["target_ref"]["digest"], "support:quality")
        assert self.document(family)["state"] == "closed_by_root"
        for field in ["outcome_decision_id", "cost_claim_ref"]:
            if field not in work:
                continue
            if field == "cost_claim_ref":
                claim = work[field]
                _, cost = self.head("app_cost", claim["kind"], claim["id"], claim["digest"])
                decision_id = self.document(self.document(cost)["current_decision_ref"])["decision_id"]
            else:
                decision_id = work[field]
            _, decision = self.head("decision", decision_id)
            body = self.document(decision)
            assert body["status"] == "reversed" and body["active_action_refs"] == []
        self.meta["checks"].append({"check": "quantity_causal_children_permanently_closed", "work_ref": work["work_ref"]})
        self.save()

    def sequence(self, pending=False):
        totals = [int(v) for v in self.parameters["totals_atoms"]]
        work = self.work("main")
        self.total("main", totals[0])
        self.app_cost(work, finalize=not pending)
        pending_outcome = self.outcome_payload(work, "support:success", "pending") if pending else None
        operations = ["main-work"]
        if not pending:
            self.total("main", totals[1])
            outcome = self.submit("main-success", "outcome.submit", self.outcome_payload(work, "support:success", "success"))
            work["outcome_claim_ref"] = unique(outcome, "workflow-outcome-claim")
            work["outcome_decision_id"] = self.document(unique(outcome, "workflow-decision"))["decision_id"]
            self.total("main", totals[2])
            operations += ["main-finalize", "main-success"]
        payload = self.quantity_payload(work)
        request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "quantity.correct",
                   "key": self.key("main-quantity-correct"), "payload": payload}
        (self.root / "main-quantity-correct.request.json").write_bytes(canonical(request))
        self.meta["work_selection"] = work
        self.save()
        self.preserve_fixture("pre-quantity-fixture")
        self.submit("main-quantity-correct", "quantity.correct", payload)
        self.total("main", totals[3])
        self.check_closed(work)
        operations.append("main-quantity-correct")
        if pending:
            before = self.snapshot()
            later = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "cost.finalize",
                     "key": self.key("pending-finalize-after-quantity"), "payload": {
                         "schema": "ledger-workflow-cost-finalize/2", "original_work_ref": work["work_ref"],
                         "component_slot": "support:model-retail", "observation_proof": work["observation"],
                         "final_cost_proof": work["final_evidence"], "evidence_refs": []}}
            response = self.workflow("pending-finalize-after-quantity", "submit", self.write("pending-finalize.request.json", later), expected=3)
            assert response["code"] == "TARGET_CLOSED" and self.snapshot() == before
            later.update({"kind": "outcome.submit", "key": self.key("pending-outcome-after-quantity"),
                          "payload": pending_outcome})
            response = self.workflow("pending-outcome-after-quantity", "submit", self.write("pending-outcome.request.json", later), expected=3)
            assert response["code"] == "TARGET_CLOSED" and self.snapshot() == before
            with self.connection() as db:
                for table in ["workflow_financial_outcome_claims", "workflow_app_cost_claims"]:
                    assert db.execute('SELECT COUNT(*) FROM "' + table + '"').fetchone()[0] == 0
        self.reverse(work, "main-reverse")
        self.total("main", 0)
        operations.append("main-reverse")
        self.reopen_retry(operations)
        self.meta["status"] = "metered_" + ("pending" if pending else self.args.variant) + "_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--variant", choices=["main", "fractional", "rounded-zero"], default="main")
    parser.add_argument("--pending", action="store_true")
    args = parser.parse_args()
    args.resume, args.deadline_seconds, args.pending_finalize_check = False, None, False
    journey = MeteredJourney(args)
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
