#!/usr/bin/env python3
"""One synthetic attested integer quantity and its complete correction path.

Build/setup precedes live financial windows. The inherited metered driver never
automatically resubmits and preserves the exact request/status on failure.
"""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_metered import MeteredJourney
from workflow_supported import Journey, REPO, canonical, now, sha, unique

FIXTURES = REPO / "examples/workflow-attested"
OPERATIONS = ["main-quantity-proof", "main-proof-source-narrow", "main-work",
              "main-finalize", "main-success", "main-quantity-replacement-proof",
              "main-quantity-correct", "main-reverse"]


class AttestedJourney(MeteredJourney):
    fixture_directory = FIXTURES

    def __init__(self, args):
        Journey.__init__(self, args)
        self.parameters = json.loads((REPO / "examples/workflow-metered/variants.json").read_bytes())["main"]
        self.meta["attested_inputs"] = {
            "source_sha256": sha((self.fixture_directory / "source.json").read_bytes()),
            "business_sha256": sha((self.fixture_directory / "business.json").read_bytes()),
            "field": "support:quantity", "source_slot": "support:worker",
            "evidence_kind": "meter_quantity", "quantity": "3", "replacement": "2"}
        self.save()

    def quantity_subject(self, prefix):
        return {"kind": "work", "logical_store_id": self.store, "scope": self.scope,
                "canonical_source_namespace": self.source, "business_operation_id": prefix + "-service",
                "customer": self.customer, "activity": "support:complete",
                "product_ref": self.product_ref, "binding_ref": self.binding_ref}

    def quantity_proof(self, prefix, label, value):
        result = self.submit(label, "evidence.retain", {
            "schema": "ledger-workflow-fact-attest/1", "attestation": {
                "schema": "ledger-workflow-fact-attestation/1", "attestation_id": label,
                "source": self.source, "evidence_kind": "meter_quantity",
                "subject": self.quantity_subject(prefix),
                "subject_schema_ref": self.fact_refs["support:work-facts"],
                "claims": [{"field": "support:quantity", "value": value}],
                "issued_at": now(), "supporting_evidence_refs": []}})
        # A genuine retention response supplies both records. The Work request
        # names the assertion only; its verifier is acquired from that receipt.
        verification = unique(result, "workflow-evidence-verification")
        attestation = unique(result, "workflow-fact-attestation")
        self.meta.setdefault("quantity_proofs", {})[label] = {
            "attestation_ref": attestation, "verification_ref": verification,
            "value": value, "subject": self.quantity_subject(prefix)}
        self.save()
        return {"field": "support:quantity", "attestation_ref": attestation}

    def work(self, prefix):
        proof = self.quantity_proof(prefix, prefix + "-quantity-proof", self.quantity)
        ceiling = copy.deepcopy(self.business["source_ceiling"])
        ceiling["operations"].remove("agreement.revoke")
        restriction = self.submit(prefix + "-proof-source-narrow", "authority.change", {
            "schema": "ledger-workflow-source-restrict/1", "action": "source.restrict",
            "enrollment_ref": self.enrollment, "expected_revision": "1", "replacement_ceiling": ceiling})
        current_grant = unique(restriction, "workflow-source-grant")
        current_state = unique(restriction, "workflow-source-state")
        self.meta["proof_grant_narrowing"] = {
            "retention_grant_ref": self.grant, "current_grant_ref": current_grant,
            "current_state_ref": current_state, "removed_operation": "agreement.revoke"}
        payload = {"schema": "ledger-workflow-work/1", "external_event_id": prefix + "-delivery",
                   "business_operation_id": prefix + "-service", "customer": self.customer,
                   "chain": prefix + "-app-chain", "activity": "support:complete",
                   "product_ref": self.product_ref, "policy_ref": self.policy_ref,
                   "fact_schema_ref": self.fact_refs["support:work-facts"], "binding_ref": self.binding_ref,
                   "status": "succeeded", "facts": {"support:quantity": self.quantity},
                   "fact_proofs": [proof], "links": [], "evidence_refs": [], "invocation_refs": [],
                   "occurred_at": now()}
        (self.root / (prefix + "-work.request.json")).write_bytes(canonical({
            "schema": "ledger-operation/1", "contract": "workflow/1", "kind": "work.accept",
            "key": self.key(prefix + "-work"), "payload": payload}))
        self.save()
        self.preserve_fixture("pre-work-fixture")
        result = self.submit(prefix + "-work", "work.accept", payload)
        return self.work_result(prefix, result)

    def work_result(self, prefix, result):
        return {"prefix": prefix, "receipt": result, "work_ref": unique(result, "workflow-work"),
                "target_ref": unique(result, "workflow-target"),
                "root_decision_ref": unique(result, "workflow-decision")}

    def quantity_payload(self, work):
        proof = self.quantity_proof(work["prefix"], work["prefix"] + "-quantity-replacement-proof", "2")
        return {"schema": "ledger-workflow-quantity-correction/2", **self.closure_inputs(work),
                "field": "support:quantity", "replacement_quantity": "2", "fact_proofs": [proof]}

    def sequence(self):
        super().sequence()
        self.reopen_retry(["main-quantity-proof", "main-proof-source-narrow", "main-quantity-replacement-proof"])
        self.meta["status"] = "attested_work_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.resume, args.variant, args.deadline_seconds, args.pending_finalize_check = False, "main", None, False
    journey = AttestedJourney(args)
    try:
        journey.setup()
        journey.sequence()
    except Exception:
        journey.meta["status"] = "STOPPED_FIRST_FAILURE"
        journey.save()
        if journey.installation.exists():
            shutil.copytree(journey.installation, journey.root / "failed-quiescent-installation", copy_function=shutil.copy2)
        raise


if __name__ == "__main__":
    main()
