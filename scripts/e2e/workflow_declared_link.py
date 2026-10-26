#!/usr/bin/env python3
"""Finite synthetic declared Work-link history; never automatically resubmit.

The root keeps its original temporal window. A separate zero-price report is
an explicit immutable selector witness, with no report-induced money cascade.
"""
import argparse
import json
import shutil
from pathlib import Path

from workflow_attested import AttestedJourney
from workflow_attested_negatives import refuse
from workflow_supported import ATOMS, REPO, canonical, now, sha, unique

FIXTURES = REPO / "examples/workflow-declared-link"


class DeclaredLinkJourney(AttestedJourney):
    fixture_directory = FIXTURES
    outcome_claim_schema = "ledger-workflow-outcome-claim/2"

    def accepted_context(self):
        context = self.business["conditional_context"]
        return {"fact_schema_ref": self.fact_refs["support:context"],
                "facts": {context["field"]: context["matching_value"]}}

    def submit(self, name, kind, payload):
        # Freeze raw bytes before any pre-operation fixture copy. The parent
        # preserves an existing original file rather than reserializing it.
        request = self.root / (name + ".request.json")
        if not request.exists():
            request.write_bytes(canonical({"schema": "ledger-operation/1", "contract": "workflow/1",
                "kind": kind, "key": self.key(name), "payload": payload}))
        if name == "acceptance":
            self.save()
            self.preserve_fixture("pre-acceptance-fixture")
        before = self.snapshot()
        (self.root / (name + ".before.json")).write_bytes(canonical(before))
        try:
            return super().submit(name, kind, payload)
        finally:
            (self.root / (name + ".after.json")).write_bytes(canonical(self.snapshot()))

    def report(self, work, label):
        root_payload = self.document(work["work_ref"])["payload"]
        result = self.submit(label + "-work", "work.accept", {
            "schema": "ledger-workflow-work/1", "external_event_id": label + "-delivery",
            "business_operation_id": label + "-service", "customer": self.customer,
            "chain": root_payload["chain"], "activity": "support:report",
            "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:report-facts"], "binding_ref": self.binding_ref,
            "status": "succeeded", "facts": {}, "fact_proofs": [],
            "links": [{"slot": "support:reported-root", "predecessor": work["work_ref"]}],
            "evidence_refs": [], "invocation_refs": [], "occurred_at": now()})
        kinds = {r["kind"] for r in result["receipt"]["result_refs"]}
        assert not {"workflow-action", "workflow-target", "workflow-family-state", "workflow-dependency"} & kinds
        report_ref, decision_ref = unique(result, "workflow-work"), unique(result, "workflow-decision")
        report, decision = self.document(report_ref), self.document(decision_ref)
        assert report["key"] == result["receipt"]["key"] and report["payload"]["binding_ref"] == self.binding_ref
        assert report["payload"]["links"] == [{"slot": "support:reported-root", "predecessor": work["work_ref"]}]
        assert decision["subject_ref"] == report_ref and decision["kind"] == "work"
        assert decision["transition"] == "initial" and decision["status"] == "active" and decision["revision"] == "1"
        assert decision["active_action_refs"] == decision["actions_appended"] == []
        claim = self.document(unique(result, "workflow-work-claim"))
        assert claim["work_ref"] == report_ref and claim["scope"] == self.scope
        capacity = self.document(unique(result, "workflow-correction-capacity"))
        assert capacity["root_ref"] == decision_ref and capacity["reserved_dependency_refs"] == []
        link_ref = unique(result, "workflow-link")
        link = self.document(link_ref)
        assert link["child"] == report_ref and link["predecessor"] == work["work_ref"]
        assert link["slot"] == "support:reported-root" and link["binding_ref"] == self.binding_ref
        self.total(label, 0)
        self.check_chain(work)
        return {"prefix": label, "work_ref": report_ref, "root_decision_ref": decision_ref,
                "link_ref": link_ref, "original_work": report, "original_link": link}

    def outcome_payload(self, work, code, label):
        payload = super().outcome_payload(work, code, label)
        payload["schema"] = "ledger-workflow-outcome/2"
        payload["selector_start_work_ref"] = work["report"]["work_ref"]
        return payload

    def record_outcome(self, work, name, code):
        result = self.submit(name, "outcome.submit", self.outcome_payload(work, code, name))
        work["outcome_claim_ref"] = unique(result, "workflow-outcome-claim")
        claim = self.document(work["outcome_claim_ref"])
        assert claim["schema"] == "ledger-workflow-outcome-claim/2"
        assert claim["target_ref"] == work["target_ref"]
        assert claim["original_payload"]["selector_start_work_ref"] == work["report"]["work_ref"]
        link = self.document(unique(result, "workflow-link"))
        assert link["child"] == work["outcome_claim_ref"] and link["predecessor"] == work["report"]["work_ref"]
        assert link["slot"] == "support:assesses"
        work["outcome_decision_id"] = self.document(unique(result, "workflow-decision"))["decision_id"]
        _, family_ref = self.head("family", work["target_ref"]["kind"], work["target_ref"]["id"],
                                  work["target_ref"]["digest"], "support:quality")
        assert self.document(family_ref)["anchor_work_ref"] == work["work_ref"]
        return result

    def correct_outcome(self, work, name, code=None):
        revision, _ = self.head("decision", work["outcome_decision_id"])
        replacement = ({"kind": "none", "evidence_refs": [], "reason": "Synthetic independent original outcome correction"}
                       if code is None else self.outcome_payload(work, code, name))
        return self.submit(name, "outcome.correct", {"schema": "ledger-workflow-outcome-correction/2",
            "original_claim_ref": work["outcome_claim_ref"], "expected_revision": revision,
            "replacement": replacement})

    def reverse_report(self, report, name):
        decision_id = self.document(report["root_decision_ref"])["decision_id"]
        revision, _ = self.head("decision", decision_id)
        result = self.submit(name, "work.reverse", {"schema": "ledger-workflow-work-reverse/2",
            "work_ref": report["work_ref"], "expected_revision": revision, "dependent_reversals": [],
            "family_closures": [], "evidence_refs": [], "reason": "Synthetic report-only zero-root reversal"})
        assert not any(r["kind"] == "workflow-action" for r in result["receipt"]["result_refs"])
        _, current = self.head("decision", decision_id)
        assert self.document(current)["status"] == "reversed"
        report["terminal_ref"] = current
        self.check_report(report)
        return result

    def check_report(self, report):
        assert self.document(report["work_ref"]) == report["original_work"]
        assert self.document(report["link_ref"]) == report["original_link"]
        if "terminal_ref" in report:
            decision_id = self.document(report["root_decision_ref"])["decision_id"]
            assert self.head("decision", decision_id)[1] == report["terminal_ref"]

    def check_chain(self, work):
        payload = self.document(work["work_ref"])["payload"]
        refs = []
        with self.connection() as db:
            rows = db.execute("SELECT record_kind,record_id,record_digest,predecessor_kind,predecessor_id,predecessor_digest FROM workflow_financial_links WHERE logical_store_id=? AND tenant=? AND environment=? AND binding_kind=? AND binding_id=? AND binding_digest=?",
                (self.store, *self.scope, self.binding_ref["kind"], self.binding_ref["id"], self.binding_ref["digest"])).fetchall()
        for kind, identity, digest, pkind, pid, pdigest in rows:
            predecessor = self.document({"kind": pkind, "id": pid, "digest": pdigest})
            assert predecessor["schema"] == "ledger-workflow-work-record/1"
            if (predecessor["payload"]["customer"], predecessor["payload"]["chain"]) == (self.customer, payload["chain"]):
                refs.append({"kind": kind, "id": identity, "digest": digest})
        assert len({canonical(r) for r in refs}) == len(refs)
        _, head = self.head("chain", self.customer, payload["chain"])
        expected = "sha256:" + sha(b"bean-counter/workflow-link-set/1\0" + canonical(sorted(refs, key=canonical)))
        assert self.document(head)["links_root"] == expected
        self.meta["checks"].append({"check": "complete_immutable_chain_link_set", "links": len(refs), "links_root": expected})
        self.save()

    def reopen_retry(self, operations):
        before = self.snapshot()
        (self.root / "status-retry-before.json").write_bytes(canonical(before))
        for name in operations:
            raw = (self.root / (name + ".request.json")).read_bytes()
            expected = json.loads((self.root / (name + ".committed.json")).read_bytes())
            assert expected["receipt"]["original_request_bytes_sha256"] == sha(raw)
            status = self.workflow(name + "-status", "status", self.write(name + ".key.json", self.key(name)))
            retried = self.workflow(name + "-retry", "submit", self.root / (name + ".request.json"))
            assert status["schema"] == "ledger-operation-status/1" and status["status"] == "committed"
            assert retried["schema"] == "ledger-operation-result/1" and retried["duplicate_kind"] == "identity"
            assert status["receipt"] == expected["receipt"] == retried["receipt"]
            assert status["request_evidence"] == expected["request_evidence"] == retried["request_evidence"]
        after = self.snapshot()
        (self.root / "status-retry-after.json").write_bytes(canonical(after))
        assert after == before
        self.meta["checks"].append({"check": "exact_status_identity_receipt_evidence_and_no_append", "operations": operations})
        self.save()

    def sequence(self, case):
        work = self.work("main")
        self.total("main", 3 * ATOMS)
        self.preserve_fixture("post-work-fixture")
        self.app_cost(work, finalize=case != "preclaim-pending")
        self.total("main", (3 if case == "preclaim-pending" else 5) * ATOMS)
        report = self.report(work, "main-report-1")
        work["report"] = report
        # A second unselected report proves child cardinality is not a hidden
        # one-report-per-root/chain rule and the full chain set is retained.
        other = self.report(work, "main-report-2")
        if case == "main":
            no_quantity = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "quantity.correct",
                "key": self.key("main-report-quantity-refusal"), "payload": {
                    "schema": "ledger-workflow-quantity-correction/2", "work_ref": report["work_ref"],
                    "expected_revision": "1", "field": "support:quantity", "replacement_quantity": "2",
                    "fact_proofs": [], "dependent_reversals": [], "family_closures": [], "evidence_refs": []}}
            # The frozen report schema has no field with this name; the
            # existing root-correction field guard returns Integrity.
            refuse(self, "main-report-quantity-refusal", no_quantity, "INTEGRITY_FAILURE")
        if case == "preclaim-pending":
            self.reverse_report(report, "main-report-reverse")
            payload = self.outcome_payload(work, "support:success", "refused")
            request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "outcome.submit",
                       "key": self.key("main-reversed-report-outcome"), "payload": payload}
            # declared_links::current_work rejects this exact terminal B root.
            refuse(self, "main-reversed-report-outcome", request, "TARGET_CLOSED")
        else:
            code = "support:none" if case == "claimed-none" else "support:success"
            self.record_outcome(work, "main-success", code)
            changed = json.loads((self.root / "main-success.request.json").read_bytes())
            changed["payload"]["selector_start_work_ref"] = other["work_ref"]
            refuse(self, "main-selected-report-conflict", changed, "IDENTITY_CONFLICT")
            self.total("main", 5 * ATOMS if case == "claimed-none" else 53 * ATOMS // 10)
            self.reverse_report(report, "main-report-reverse")
            self.total("main", 5 * ATOMS if case == "claimed-none" else 53 * ATOMS // 10)
            if case == "main":
                self.correct_outcome(work, "main-outcome-none")
                self.total("main", 5 * ATOMS)
            self.correct_outcome(work, "main-outcome-premium", "support:success")
            self.total("main", 53 * ATOMS // 10)
        payload = self.quantity_payload(work)
        (self.root / "main-quantity-correct.request.json").write_bytes(canonical({
            "schema": "ledger-operation/1", "contract": "workflow/1", "kind": "quantity.correct",
            "key": self.key("main-quantity-correct"), "payload": payload}))
        self.meta["work_selection"] = work
        self.save()
        self.preserve_fixture("pre-quantity-fixture")
        self.submit("main-quantity-correct", "quantity.correct", payload)
        self.total("main", 2 * ATOMS)
        self.check_closed(work)
        self.check_report(report)
        self.check_report(other)
        self.check_chain(work)
        if case == "preclaim-pending":
            late = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "cost.finalize",
                    "key": self.key("late-finalize"), "payload": {
                        "schema": "ledger-workflow-cost-finalize/2", "original_work_ref": work["work_ref"],
                        "component_slot": "support:model-retail", "observation_proof": work["observation"],
                        "final_cost_proof": work["final_evidence"], "evidence_refs": []}}
            refuse(self, "late-finalize", late, "TARGET_CLOSED")
            late.update(kind="outcome.submit", key=self.key("late-outcome"), payload=dict(request["payload"]))
            late["payload"]["selector_start_work_ref"] = other["work_ref"]
            refuse(self, "late-outcome", late, "TARGET_CLOSED")
        self.reverse(work, "main-reverse")
        self.total("main", 0)
        self.reverse_report(other, "main-other-report-reverse")
        self.total("main", 0)
        self.check_report(report)
        self.check_chain(work)
        operations = sorted(p.name.removesuffix(".committed.json") for p in self.root.glob("*.committed.json"))
        self.reopen_retry(operations)
        self.meta["status"] = "declared_work_link_" + case + "_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["ledger", "publication-helper", "terms-helper", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--case", choices=["main", "claimed-none", "preclaim-pending"], default="main")
    args = parser.parse_args()
    args.resume, args.variant, args.deadline_seconds, args.pending_finalize_check = False, "main", None, False
    journey = DeclaredLinkJourney(args)
    try:
        journey.setup()
        journey.sequence(args.case)
    except Exception:
        journey.meta["status"] = "STOPPED_FIRST_FAILURE"
        journey.save()
        if journey.installation.exists():
            shutil.copytree(journey.installation, journey.root / "failed-quiescent-installation", copy_function=shutil.copy2)
        raise


if __name__ == "__main__":
    main()
