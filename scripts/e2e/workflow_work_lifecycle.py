#!/usr/bin/env python3
"""One finite failed/Reference/rounded-zero history; first unexpected result stops."""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_attested_negatives import refuse
from workflow_metered import MeteredJourney
from workflow_supported import ATOMS, Journey, REPO, canonical, now, replace, sha, unique

FIXTURES = REPO / "examples/workflow-work-lifecycle"


class LifecycleJourney(MeteredJourney):
    fixture_directory = FIXTURES

    def __init__(self, args):
        Journey.__init__(self, args)
        self.worker = self.business["worker_source"]
        self.corrector = self.business["corrector_source"]
        self.dependent = self.business["dependent_source"]
        self.key_prefix = ""

    def source_documents(self):
        return Journey.source_documents(self)

    def snapshot(self):
        value = super().snapshot()
        value["billing_configuration_sha256"] = sha((self.installation / "billing.json").read_bytes())
        return value

    def key(self, operation):
        key = super().key(operation)
        if operation in {"fixed-work", "failed-work", "rounded-work", "failed-as-success", "wrong-binding-work"}:
            key["source"] = self.worker
        elif operation in {"failed-quantity-correct", "failed-reverse", "rounded-reverse", "revoked-dependent-closure"}:
            key["source"] = self.corrector
        key["operation_id"] = self.key_prefix + key["operation_id"]
        return key

    def accepted_context(self):
        return {"fact_schema_ref": self.fact_refs["support:context"],
                "facts": {"support:eligibility": "support:eligible"}}

    def accepted_grants(self):
        return sorted([self.grant, self.worker_grant, self.corrector_grant, self.dependent_grant], key=canonical)

    def setup_publication(self, auth, psub, roles):
        customer = unique(json.loads((self.root / "customer-register.committed.json").read_bytes()), "workflow-customer")
        for label in ["worker", "corrector", "dependent"]:
            ceiling = self.business[label + "_ceiling"]
            evidence = self.evidence(label + "-source-evidence", "ledger-workflow-evidence-retain/1",
                {"customer": self.customer}, {"source": ceiling["source"], "principal_id": self.operator}, "source_enrollment")
            result = self.submit(label + "-enroll", "authority.change", {
                "schema": "ledger-workflow-source-enroll/2", "action": "source.enroll", "expected_revision": "0",
                "enrollment_id": "lifecycle-" + label, "customer_refs": [customer],
                "ceiling": ceiling, "evidence_ref": evidence})
            setattr(self, label + "_grant", unique(result, "workflow-source-grant"))
            setattr(self, label + "_enrollment", unique(result, "workflow-source-enrollment"))
        Journey.setup_publication(self, auth, psub, roles)

    def accepted_only(self, auth, psub, roles):
        Journey.setup_publication(self, auth, psub, roles)

    def refusal(self, name, request, code):
        refuse(self, name, request, code)
        before = self.snapshot()
        (self.root / (name + ".absence-before.json")).write_bytes(canonical(before))
        status = self.workflow(name + "-absence-status", "status", self.write(name + ".absence-key.json", request["key"]))
        assert status["schema"] == "ledger-operation-status/1" and status["status"] == "not_observed"
        assert status["key"] == request["key"]
        after = self.snapshot()
        (self.root / (name + ".absence-after.json")).write_bytes(canonical(after))
        assert after == before
        self.meta["checks"].append({"check": name + "-exact-key-absent", "no_append": True})
        self.save()

    def wrong_binding_refusal(self, endpoint):
        other = copy.copy(self)
        other.root = self.root / "other-binding"
        other.root.mkdir(mode=0o700)
        other.inputs = other.root / "inputs"
        other.inputs.mkdir()
        other.log = other.root / "RUN.json"
        other.meta = {"classification": "Second empty Binding in the same installation; no admitted Work or fee",
                      "commands": [], "checks": []}
        other.agreement = self.agreement + "-other"
        other.business = replace(copy.deepcopy(self.business), self.agreement, other.agreement)
        other.key_prefix = "other-"
        subject = {"customer": other.customer, "agreement_id": other.agreement,
                   "party": other.business["platform"], "acceptor": "synthetic-platform-representative"}
        roles = ["bearer", "payer", "provider"]
        authorization = {**subject, "scope": other.scope, "authorization_id": "other-platform-authority", "roles": roles,
                         "starts_at": "2000-01-01T00:00:00.000000Z", "ends_at": "2099-01-01T00:00:00.000000Z"}
        evidence = other.evidence("platform-authority-evidence", "ledger-workflow-party-evidence-retain/1", subject,
                                  {"purpose": "party_authorization", "authorization": authorization})
        attestation = other.evidence("platform-authority-attestation", "ledger-workflow-party-evidence-retain/1", subject,
            {"purpose": "party_operator_attestation", "statement": "operator_verified_party_acceptor_authority", "target_evidence_ref": evidence})
        auth = unique(other.submit("platform-authorize", "authority.change", {"schema": "ledger-workflow-party-authorize/1",
            "expected_revision": "0", "authorization": authorization, "evidence_ref": evidence,
            "operator_attestation_ref": attestation}), "workflow-authority")
        other.accepted_only(auth, subject, roles)
        payload = other.completion_payload("wrong-binding", "succeeded", "0.1", endpoint)
        other.refusal("wrong-binding-work", {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "work.accept",
            "key": other.key("wrong-binding-work"), "payload": payload}, "INTEGRITY_FAILURE")
        other.meta["status"] = "genuine_wrong_binding_reference_refusal_complete"
        other.save()
        self.unchanged_fixed("wrong-binding-refusal")

    def completion_payload(self, prefix, status, quantity, endpoint=None):
        facts = {"support:quantity": quantity}
        if endpoint is not None:
            facts["support:reference"] = endpoint
        return {"schema": "ledger-workflow-work/1", "external_event_id": prefix + "-delivery",
            "business_operation_id": prefix + "-service", "customer": self.customer, "chain": prefix + "-chain",
            "activity": "support:fixed" if prefix == "fixed" else "support:complete",
            "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:work-facts"], "binding_ref": self.binding_ref,
            "status": status, "facts": facts, "fact_proofs": [], "links": [], "evidence_refs": [],
            "invocation_refs": [], "occurred_at": now()}

    def complete(self, prefix, status, quantity, endpoint=None):
        result = self.submit(prefix + "-work", "work.accept", self.completion_payload(prefix, status, quantity, endpoint))
        value = {"prefix": prefix, "receipt": result, "work_ref": unique(result, "workflow-work"),
                 "root_decision_ref": unique(result, "workflow-decision")}
        targets = [r for r in result["receipt"]["result_refs"] if r["kind"] == "workflow-target"]
        assert len(targets) <= 1
        if targets:
            value["target_ref"] = targets[0]
        return value

    def closure_inputs(self, work):
        root_id = self.document(work["root_decision_ref"])["decision_id"]
        revision, _ = self.head("decision", root_id)
        families = []
        if "target_ref" in work:
            target = work["target_ref"]
            rev, state = self.head("family", target["kind"], target["id"], target["digest"], "support:quality")
            if self.document(state)["state"] != "closed_by_root":
                families = [{"family_state_ref": state, "expected_revision": rev,
                             "authority_evidence_refs": [self.dependent_grant]}]
        return {"work_ref": work["work_ref"], "expected_revision": revision, "dependent_reversals": [],
                "family_closures": families,
                "evidence_refs": sorted([self.corrector_grant] + ([self.dependent_grant] if families else []), key=canonical)}

    def pin_fixed(self, fixed):
        decision = self.document(fixed["root_decision_ref"])
        self.fixed = fixed
        self.fixed_decision_bytes = canonical(decision)
        assert len(decision["active_action_refs"]) == 1
        self.fixed_action = decision["active_action_refs"][0]
        self.fixed_action_bytes = canonical(self.document(self.fixed_action))
        action = self.document(self.fixed_action)
        assert action["book"] == "retail" and action["amount"] == {"currency": "USD", "scale": 18, "atoms": str(2 * ATOMS)}

    def unchanged_fixed(self, label):
        root = self.document(self.fixed["root_decision_ref"])
        rev, current = self.head("decision", root["decision_id"])
        assert rev == "1" and current == self.fixed["root_decision_ref"]
        assert canonical(self.document(current)) == self.fixed_decision_bytes
        assert canonical(self.document(self.fixed_action)) == self.fixed_action_bytes
        with self.connection() as db:
            actions = [json.loads(b) for b, in db.execute("SELECT canonical_bytes FROM workflow_documents WHERE kind='workflow-action'")]
        assert actions == [self.document(self.fixed_action)], label
        self.meta["checks"].append({"check": "independent_fixed_unchanged", "step": label,
                                    "action_ref": self.fixed_action, "total_atoms": str(2 * ATOMS)})
        self.save()

    def root_state(self, work, revision, status):
        initial = self.document(work["root_decision_ref"])
        rev, reference = self.head("decision", initial["decision_id"])
        value = self.document(reference)
        assert rev == str(revision) and value["status"] == status and not value["active_action_refs"]
        assert value["subject_ref"] == work["work_ref"]
        assert not value["actions_appended"]
        return value

    def failed_state(self, work, revision, status):
        current = self.root_state(work, revision, status)
        assert "target_ref" not in work and self.document(work["work_ref"])["payload"]["status"] == "failed"
        if revision < 3:
            evaluation = self.document(current["evaluation_ref"])
            assert any(step["component_slot"] == "support:service" and step["disposition"] == "failed_work"
                       and step["rational_numerator"] == "0" and step["rational_denominator"] == "1"
                       for step in evaluation["steps"])
        root_id = self.document(work["root_decision_ref"])["decision_id"]
        _, capacity = self.head("capacity", root_id)
        capacity = self.document(capacity)
        assert not capacity["reserved_dependency_refs"] and not capacity["retirement_refs"]
        self.unchanged_fixed("failed-revision-" + str(revision))

    def copied_dependent_refusal(self, work, payload):
        fixture = self.preserve_fixture("pre-rounded-close")
        copied = copy.copy(self)
        copied.root = self.root / "dependent-refusal"
        copied.root.mkdir(mode=0o700)
        copied.installation, copied.inputs = copied.root / "installation", copied.root / "inputs"
        shutil.copytree(fixture / "installation", copied.installation, copy_function=shutil.copy2)
        shutil.copytree(fixture / "inputs", copied.inputs, copy_function=shutil.copy2)
        copied.log = copied.root / "RUN.json"
        copied.meta = {"classification": "Private whole pre-closure copy; legitimate D revoke then one refused C closure",
                       "fixture": str(fixture), "commands": [], "checks": []}
        copied.submit("dependent-copy-revoke", "authority.change", {
            "schema": "ledger-workflow-source-revoke/2", "action": "source.revoke",
            "enrollment_ref": self.dependent_enrollment, "expected_revision": "1", "reason": "Finite current dependent authority refusal"})
        request = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "work.reverse",
                   "key": copied.key("revoked-dependent-closure"), "payload": payload}
        copied.refusal("revoked-dependent-closure", request, "AUTHORITY_REQUIRED")
        copied.meta["status"] = "dependent_current_authority_refusal_complete"
        copied.save()

    def reopen_retry(self, operations):
        for name in operations:
            before = self.snapshot()
            (self.root / (name + ".recovery-before.json")).write_bytes(canonical(before))
            saved = json.loads((self.root / (name + ".committed.json")).read_bytes())
            raw = (self.root / (name + ".request.json")).read_bytes()
            request = json.loads(raw)
            assert request["key"] == self.key(name) == saved["receipt"]["key"]
            assert saved["receipt"]["original_request_bytes_sha256"] == sha(raw)
            try:
                status = self.workflow(name + "-status", "status", self.write(name + ".key.json", request["key"]))
                assert status["schema"] == "ledger-operation-status/1" and status["status"] == "committed"
                assert status["receipt"] == saved["receipt"] and status["request_evidence"] == saved["request_evidence"]
                # A verified known commit is mandatory before this ONE original
                # query. Missing/unknown/mismatched status never submits raw.
                retry = self.workflow(name + "-retry", "submit", self.root / (name + ".request.json"))
                assert retry["status"] == "committed" and retry["duplicate_kind"] == "identity"
                assert retry["receipt"] == saved["receipt"] and retry["request_evidence"] == saved["request_evidence"]
            finally:
                after = self.snapshot()
                (self.root / (name + ".recovery-after.json")).write_bytes(canonical(after))
            assert after == before, name + " original recovery changed retained state"
            self.meta["checks"].append({"check": "exact_original_status_then_identity", "operation": name,
                                        "no_append": True, "all_logical_tables_and_configuration_equal": True})
            self.save()

    def restored_originals(self):
        fixture = self.preserve_fixture("restored-final-history")
        restored = copy.copy(self)
        restored.root, restored.installation = fixture, fixture / "installation"
        restored.inputs, restored.log = fixture / "inputs", fixture / "RESTORE-RUN.json"
        restored.meta = {"classification": "Whole quiescent installation copy; two exact original recovery pairs",
                         "commands": [], "checks": [], "source_history": str(self.root)}
        restored.reopen_retry(["failed-quantity-correct", "rounded-reverse"])
        restored.meta["status"] = "restored_exact_originals_complete"
        restored.save()

    def sequence(self):
        fixed = self.complete("fixed", "succeeded", "1")
        self.pin_fixed(fixed)
        self.unchanged_fixed("initial")
        self.wrong_binding_refusal(fixed["work_ref"])
        failed = self.complete("failed", "failed", "3", fixed["work_ref"])
        self.failed_state(failed, 1, "active")
        self.submit("failed-quantity-correct", "quantity.correct", {
            "schema": "ledger-workflow-quantity-correction/2", **self.closure_inputs(failed),
            "field": "support:quantity", "replacement_quantity": "2", "fact_proofs": []})
        self.failed_state(failed, 2, "active")
        self.reverse(failed, "failed-reverse")
        self.failed_state(failed, 3, "reversed")
        changed = json.loads((self.root / "failed-work.request.json").read_bytes())
        changed["key"] = self.key("failed-as-success")
        changed["payload"]["status"] = "succeeded"
        self.refusal("failed-as-success", changed, "IDENTITY_CONFLICT")
        rounded = self.complete("rounded", "succeeded", "0.1", fixed["work_ref"])
        rounded_root = self.root_state(rounded, 1, "active")
        evaluation = self.document(rounded_root["evaluation_ref"])
        service = [step for step in evaluation["steps"] if step["component_slot"] == "support:service"]
        assert len(service) == 1 and service[0]["disposition"] == "zero"
        assert service[0]["rational_numerator"] == "1" and service[0]["rational_denominator"] == "10"
        root_id = self.document(rounded["root_decision_ref"])["decision_id"]
        _, capacity = self.head("capacity", root_id)
        dependencies = [self.document(r) for r in self.document(capacity)["reserved_dependency_refs"]]
        assert sorted(d["kind"] for d in dependencies) == ["delayed_cost", "outcome_family"]
        target = rounded["target_ref"]
        _, prior_ref = self.head("family", target["kind"], target["id"], target["digest"], "support:quality")
        prior = self.document(prior_ref)
        assert "claim_ref" not in prior
        self.unchanged_fixed("rounded-zero-promises")
        payload = {"schema": "ledger-workflow-work-reverse/2", **self.closure_inputs(rounded),
                   "reason": "Close actual rounded-zero completion promises with separate dependent authority"}
        self.copied_dependent_refusal(rounded, payload)
        self.submit("rounded-reverse", "work.reverse", payload)
        self.root_state(rounded, 2, "reversed")
        _, closed_ref = self.head("family", target["kind"], target["id"], target["digest"], "support:quality")
        closed = self.document(closed_ref)
        assert closed["state"] == "closed_by_root" and "claim_ref" not in closed
        for field in ["target_ref", "window"]:
            assert closed[field] == prior[field]
        assert closed["prior_ref"] == prior_ref
        cause = closed["causal_closure"]
        assert cause["root_decision_ref"] == rounded["root_decision_ref"] and cause["root_expected_revision"] == "1"
        authority = self.document(cause["authority_ref"])
        assert authority["source"] == self.dependent and authority["operation"] == "work.reverse"
        _, capacity = self.head("capacity", root_id)
        capacity = self.document(capacity)
        assert len(capacity["retirement_refs"]) == 1
        retirement = self.document(capacity["retirement_refs"][0])
        assert retirement["operation_key"] == self.key("rounded-reverse")
        assert self.document(retirement["dependency_ref"])["kind"] == "delayed_cost"
        pending_authority = self.document(retirement["authority_ref"])
        assert pending_authority["source"] == self.dependent and pending_authority["operation"] == "cost.correct"
        assert [self.document(r)["kind"] for r in capacity["reserved_dependency_refs"]] == ["outcome_family"]
        with self.connection() as db:
            for table in ["workflow_financial_outcome_claims", "workflow_app_cost_claims"]:
                assert db.execute('SELECT COUNT(*) FROM "' + table + '"').fetchone()[0] == 0
            assert db.execute("SELECT COUNT(*) FROM workflow_financial_work_claims").fetchone()[0] == 3
        self.unchanged_fixed("closed-rounded-promises")
        self.reopen_retry(["acceptance", "activation", "worker-enroll", "corrector-enroll", "dependent-enroll",
                           "fixed-work", "failed-work", "failed-quantity-correct", "failed-reverse", "rounded-work", "rounded-reverse"])
        self.restored_originals()
        self.meta["status"] = "lifecycle_finite_main_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["ledger", "publication-helper", "terms-helper", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.set_defaults(resume=False, deadline_seconds=0, pending_finalize_check=False)
    args = parser.parse_args()
    journey = LifecycleJourney(args)
    try:
        journey.setup()
        journey.sequence()
    except Exception:
        journey.meta["status"] = "STOPPED_FIRST_UNEXPECTED_RESULT"
        raise
    finally:
        if (journey.installation / ".ledger/local.db").exists():
            backup = journey.root / "final-quiescent-installation"
            shutil.copytree(journey.installation, backup, copy_function=shutil.copy2)
            journey.meta["final_quiescent_backup"] = str(backup)
        journey.save()


if __name__ == "__main__":
    main()
