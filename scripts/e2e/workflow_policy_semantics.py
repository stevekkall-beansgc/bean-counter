#!/usr/bin/env python3
"""One exact converted-service policy journey with an independent local issuer.

Source setup and proof retention precede the Work window. No automatic submit
retry, source impersonation, SQL writes, or change to an accepted request.
"""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_attested import AttestedJourney
from workflow_attested_negatives import refuse
from workflow_supported import ATOMS, Journey, REPO, canonical, now, sha, unique

FIXTURES = REPO / "examples/workflow-policy-semantics"


class PolicyJourney(AttestedJourney):
    fixture_directory = FIXTURES
    quantity = "90"
    quantity_unit = "support:second"
    issuer_operations = {"main-quantity-proof", "main-quantity-replacement-proof"}

    def __init__(self, args):
        Journey.__init__(self, args)
        self.issuer = self.business["issuer_ceiling"]["source"]
        self.meta["policy_inputs"] = {"source_sha256": sha((FIXTURES / "source.json").read_bytes()),
            "business_sha256": sha((FIXTURES / "business.json").read_bytes()),
            "quantity": "90", "replacement": "60", "unit": self.quantity_unit,
            "normalized_unit": "support:minute", "rate": "0.1",
            "worker_source": self.source, "issuer_source": self.issuer}
        self.save()

    def source_documents(self):
        return Journey.source_documents(self)

    def accepted_context(self):
        return {"fact_schema_ref": self.fact_refs["support:context"],
                "facts": {"support:eligibility": "support:eligible"}}

    def accepted_grants(self):
        return sorted([self.grant, self.issuer_grant], key=canonical)

    def key(self, operation):
        key = super().key(operation)
        if operation in self.issuer_operations:
            key["source"] = self.issuer
        return key

    def setup_publication(self, auth, psub, roles):
        customer = unique(json.loads((self.root / "customer-register.committed.json").read_bytes()),
                          "workflow-customer")
        evidence = self.evidence("issuer-source-evidence", "ledger-workflow-evidence-retain/1",
            {"customer": self.customer}, {"source": self.issuer, "principal_id": self.operator}, "source_enrollment")
        enrolled = self.submit("issuer-enroll", "authority.change", {
            "schema": "ledger-workflow-source-enroll/2", "action": "source.enroll",
            "expected_revision": "0", "enrollment_id": "example-meter-source", "customer_refs": [customer],
            "ceiling": self.business["issuer_ceiling"], "evidence_ref": evidence})
        self.issuer_grant = unique(enrolled, "workflow-source-grant")
        self.issuer_enrollment = unique(enrolled, "workflow-source-enrollment")
        super().setup_publication(auth, psub, roles)

    def quantity_proof(self, prefix, label, value):
        result = self.submit(label, "evidence.retain", {
            "schema": "ledger-workflow-fact-attest/1", "attestation": {
                "schema": "ledger-workflow-fact-attestation/1", "attestation_id": label,
                "source": self.issuer, "evidence_kind": "meter_quantity", "subject": self.quantity_subject(prefix),
                "subject_schema_ref": self.fact_refs["support:work-facts"],
                "claims": [{"field": "support:quantity", "value": value}],
                "issued_at": now(), "supporting_evidence_refs": []}})
        assertion, verification = unique(result, "workflow-fact-attestation"), unique(result, "workflow-evidence-verification")
        self.meta.setdefault("quantity_proofs", {})[label] = {
            "attestation_ref": assertion, "verification_ref": verification,
            "value": value, "subject": self.quantity_subject(prefix)}
        self.save()
        return {"field": "support:quantity", "attestation_ref": assertion}

    def work(self, prefix):
        proof = self.quantity_proof(prefix, "main-quantity-proof", "90")
        self.replacement_proof = self.quantity_proof(prefix, "main-quantity-replacement-proof", "60")
        revoked = self.submit("issuer-revoke", "authority.change", {
            "schema": "ledger-workflow-source-revoke/2", "action": "source.revoke",
            "enrollment_ref": self.issuer_enrollment, "expected_revision": "1",
            "reason": "Synthetic issuer retired after issuing both authentic original proofs"})
        self.meta["historical_issuer"] = {"original_grant_ref": self.issuer_grant,
            "revoked_state_ref": unique(revoked, "workflow-source-state"),
            "worker_current_rights_remain_separate": True}
        payload = {"schema": "ledger-workflow-work/1", "external_event_id": prefix + "-delivery",
            "business_operation_id": prefix + "-service", "customer": self.customer,
            "chain": prefix + "-app-chain", "activity": "support:complete",
            "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:work-facts"], "binding_ref": self.binding_ref,
            "status": "succeeded", "facts": {"support:approved": True, "support:quantity": "90"},
            "fact_proofs": [proof], "links": [], "evidence_refs": [], "invocation_refs": [], "occurred_at": now()}
        request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "work.accept",
                   "key": self.key(prefix + "-work"), "payload": payload}
        (self.root / (prefix + "-work.request.json")).write_bytes(canonical(request))
        self.save()
        self.pre_work = self.preserve_fixture("pre-work-fixture")
        return self.work_result(prefix, self.submit(prefix + "-work", "work.accept", payload))

    def quantity_payload(self, work):
        return {"schema": "ledger-workflow-quantity-correction/2", **self.closure_inputs(work),
                "field": "support:quantity", "replacement_quantity": "60", "fact_proofs": [self.replacement_proof]}

    def proof_negatives(self):
        # Genuine issuer receipts and all sidecars are copied. These two refused
        # Work requests cannot create a second charge or mutate the main history.
        copied = copy.copy(self)
        copied.root = self.root / "proof-refusals"
        copied.root.mkdir(mode=0o700)
        copied.installation = copied.root / "installation"
        copied.inputs = copied.root / "inputs"
        shutil.copytree(self.pre_work / "installation", copied.installation, copy_function=shutil.copy2)
        shutil.copytree(self.pre_work / "inputs", copied.inputs, copy_function=shutil.copy2)
        copied.log = copied.root / "RUN.json"
        copied.meta = {"classification": "Private copy of genuine pre-Work retained proofs",
                       "fixture": str(self.pre_work), "commands": [], "checks": []}
        original = json.loads((self.pre_work / "main-work.request.json").read_bytes())
        for name in ["wrong-proof-value", "wrong-proof-subject"]:
            request = copy.deepcopy(original)
            request["key"]["operation_id"] = name
            if name == "wrong-proof-value":
                request["payload"]["facts"]["support:quantity"] = "60"
            else:
                request["payload"]["business_operation_id"] = "different-service"
            refuse(copied, name, request, "INTEGRITY_FAILURE")
        copied.meta["status"] = "two_genuine_proof_refusals_complete"
        copied.save()

    def sequence(self):
        work = self.work("main")
        self.total("main", 15 * ATOMS // 100)
        self.app_cost(work)
        self.total("main", 215 * ATOMS // 100)
        outcome = self.submit("main-success", "outcome.submit", self.outcome_payload(work, "support:success", "success"))
        work["outcome_claim_ref"] = unique(outcome, "workflow-outcome-claim")
        work["outcome_decision_id"] = self.document(unique(outcome, "workflow-decision"))["decision_id"]
        self.total("main", 2165 * ATOMS // 1000)
        payload = self.quantity_payload(work)
        (self.root / "main-quantity-correct.request.json").write_bytes(canonical({
            "schema": "ledger-operation/1", "contract": "workflow/1", "kind": "quantity.correct",
            "key": self.key("main-quantity-correct"), "payload": payload}))
        self.meta["work_selection"] = work
        self.save()
        self.preserve_fixture("pre-quantity-fixture")
        self.submit("main-quantity-correct", "quantity.correct", payload)
        self.total("main", ATOMS // 10)
        self.check_closed(work)
        self.reverse(work, "main-reverse")
        self.total("main", 0)
        operations = ["publication", "acceptance", "activation", "issuer-enroll", "issuer-revoke",
            "main-quantity-proof", "main-quantity-replacement-proof", "main-work", "main-observation",
            "main-final-evidence-1", "main-finalize", "main-success-outcome", "main-success-occurrence",
            "main-success", "main-quantity-correct", "main-reverse"]
        self.reopen_retry(operations)
        self.proof_negatives()
        self.meta["status"] = "policy_semantics_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.resume, args.deadline_seconds, args.pending_finalize_check = False, None, False
    journey = PolicyJourney(args)
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
