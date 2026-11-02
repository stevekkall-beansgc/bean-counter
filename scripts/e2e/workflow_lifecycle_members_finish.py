#!/usr/bin/env python3
"""One reviewed completion from the preserved 20-operation members installation.

One new customer-only acceptance identity; existing terms/customer assent stay exact.
No setup or publication replay, resource command,
financial retry, clock change, or fallback. Stop at the first unexpected result.
"""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_lifecycle_finite import FiniteLifecycle, SETTINGS
from workflow_supported import canonical, now, reference, sha, unique

SOURCE_RUN_SHA = "f3f65b17b39145b32ca9d0b7a8914570afc4f5b72d5fcb4fd4f15ffa72a04337"
SOURCE_SNAPSHOT_SHA = "ff534e6f1944127dc43c0953aaeaa1f5c607c482526bfe25ea70b1c4ac1574a4"
CONFIG_SHA = "4f6afdb9326b3218feab3fdfcf23808225cff74859486b0d96a3d67eb04ab9ef"
FAILED_REQUEST_SHA = "1aa8c9b8622b77de554b1db9640c93398c8f7b4be4ad157a63c598de73fb9d41"
EXPECTED_ACCEPTANCE_SHA = "c1334eaa7b2ce80ad343e4754a11f076bbf775d36a8533c865b946c921cee4c7"
ALLOWED = {
    "acceptance-customer-local": "agreement.accept", "activation": "agreement.activate",
    "stage-authorize": "invocation.authorize",
    "b-work-occurrence": "evidence.retain", "b-work": "work.accept",
    "close-missing-d-occurrence": "evidence.retain",
    "close-missing-d-success": "evidence.retain", "close-missing-d": "outcome.submit",
    "d-work-occurrence": "evidence.retain", "d-work": "work.accept",
    "close-complete-occurrence": "evidence.retain",
    "close-complete-success": "evidence.retain", "close-complete": "outcome.submit",
    "correct-d-quantity": "quantity.correct",
}


def verify_pins(manifest):
    for item in manifest["files"] + manifest["source_files"] + manifest["product_files"]:
        assert sha(Path(item["path"]).read_bytes()) == item["sha256"], item["path"]


class MembersCompletion(FiniteLifecycle):
    source_filename = "source-initial-template.json"

    def __init__(self, args):
        self.source_root = args.source.resolve()
        parent_raw = (self.source_root / "RUN.json").read_bytes()
        assert sha(parent_raw) == SOURCE_RUN_SHA
        self.parent = json.loads(parent_raw)
        assert self.parent["final_snapshot_sha256"] == SOURCE_SNAPSHOT_SHA
        assert sha(canonical(self.parent["final_snapshot"])) == SOURCE_SNAPSHOT_SHA
        self.source_installation = self.source_root / "installation"
        self.original_config = (self.source_installation / "billing.json").read_bytes()
        assert sha(self.original_config) == CONFIG_SHA
        assert json.loads(self.original_config)["resources"] == SETTINGS
        failed_raw = (self.source_root / "acceptance.request.json").read_bytes()
        assert sha(failed_raw) == FAILED_REQUEST_SHA
        self.failed_acceptance = json.loads(failed_raw)
        super().__init__(args)
        self.store = json.loads(self.original_config)["store_id"]
        self.config_bytes = self.original_config
        self.original_snapshot = self.source_snapshot()
        assert canonical(self.original_snapshot) == canonical(self.parent["final_snapshot"])
        shutil.copytree(self.source_installation, self.installation, copy_function=shutil.copy2)
        shutil.copytree(self.source_root / "inputs", self.inputs, dirs_exist_ok=True, copy_function=shutil.copy2)
        assert canonical(self.snapshot()) == canonical(self.original_snapshot)
        (self.root / "PARENT-RUN.json").write_bytes(parent_raw)
        (self.root / "original-failed-acceptance.request.json").write_bytes(failed_raw)
        saved = []
        for path in self.source_root.glob("*.committed.json"):
            result = json.loads(path.read_bytes())
            saved.append((int(result["receipt"]["commit_sequence"]), path, result))
        assert [n for n, _, _ in sorted(saved)] == list(range(1, 21))
        with self.connection() as db:
            receipts = [json.loads(b) for b, in db.execute("SELECT receipt_bytes FROM workflow_operations ORDER BY commit_sequence")]
        assert len(receipts) == 20
        for sequence, path, result in sorted(saved):
            name = path.name.removesuffix(".committed.json")
            raw = (self.source_root / (name + ".request.json")).read_bytes()
            request = json.loads(raw)
            assert result["receipt"] == receipts[sequence - 1]
            assert result["receipt"]["key"] == request["key"]
            assert result["receipt"]["original_request_bytes_sha256"] == sha(raw)
            assert request["key"]["logical_store_id"] == self.store
            (self.root / path.name).write_bytes(path.read_bytes())
            (self.root / (name + ".request.json")).write_bytes(raw)
            self.commits[name] = result
            self.attempted.add(canonical(request["key"]))
        self.original_names = tuple(self.commits)
        self.grant = unique(self.commits["source-enroll"], "workflow-source-grant")
        self.enrollment = unique(self.commits["source-enroll"], "workflow-source-enrollment")
        for label in ["worker", "corrector", "closer", "alias"]:
            setattr(self, label + "_grant", unique(self.commits[label + "-enroll"], "workflow-source-grant"))
            setattr(self, label + "_enrollment", unique(self.commits[label + "-enroll"], "workflow-source-enrollment"))
        self.meta["proof_scope"] = {"case": "members", "fresh_only": False,
            "resume_original_operations": 20, "existing_operations_replayed": False,
            "fixture_fix": "Customer-only no-share Stage consent, exact original terms/customer assent; only unrelated platform assent and authority omitted from new request",
            "original_failed_request_sha256": FAILED_REQUEST_SHA, "settings": SETTINGS,
            "default_budget_qualification": False, "source_run_sha256": SOURCE_RUN_SHA,
            "source_snapshot_sha256": SOURCE_SNAPSHOT_SHA}
        self.meta["configuration_sha256"] = CONFIG_SHA
        self.save()

    def source_snapshot(self):
        installation = self.installation
        self.installation = self.source_installation
        try:
            return self.snapshot()
        finally:
            self.installation = installation

    def assert_originals(self):
        current = self.snapshot()
        assert canonical(current["operations"][:20]) == canonical(self.original_snapshot["operations"])
        assert set(map(tuple, self.original_snapshot["documents"])) <= set(map(tuple, current["documents"]))
        assert (self.installation / "billing.json").read_bytes() == self.original_config
        assert canonical(self.source_snapshot()) == canonical(self.original_snapshot)
        assert (self.source_installation / "billing.json").read_bytes() == self.original_config

    def setup(self):
        raise AssertionError("Committed setup must never be replayed")

    def command(self, name, argv, expected=0, allow_unavailable=False):
        assert Path(argv[0]) == self.binary and str(argv[1]) == "workflow"
        route = str(argv[4])
        assert route in {"submit", "status", "read"}, argv
        if route == "submit":
            request = json.loads(Path(argv[5]).read_bytes())
            assert name in ALLOWED and request["kind"] == ALLOWED[name]
            assert request["key"] == self.key(name)
            assert name not in self.original_names
            assert request["key"] != self.failed_acceptance["key"]
        self.assert_originals()
        try:
            return super().command(name, argv, expected, allow_unavailable)
        finally:
            self.assert_originals()

    def perform(self, name, kind, payload, refusal=None):
        assert name in ALLOWED and kind == ALLOWED[name]
        return super().perform(name, kind, payload, refusal)

    def preflight(self):
        for name in self.original_names:
            saved = self.commits[name]
            status = self.exact_status("preserved-" + name, saved["receipt"]["key"])
            assert status["status"] == "committed"
            assert status["receipt"] == saved["receipt"]
            assert status["request_evidence"] == saved["request_evidence"]
        trusted = now()
        for ref in self.accepted_grants():
            grant = self.document(ref)
            assert grant["starts_at"] <= trusted < grant["ends_at"]
            assert grant["principal_id"] == self.operator and self.customer in grant["customer_scope"]
            with self.connection() as db:
                head = db.execute("SELECT revision,grant_id,status FROM workflow_source_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND enrollment_id=?",
                    (self.store, *self.scope, grant["grant_id"])).fetchone()
            assert head == (int(grant["revision"]), ref["id"], "active")
        status = self.exact_status("original-failed-acceptance", self.failed_acceptance["key"])
        assert status["status"] == "not_observed" and status["key"] == self.failed_acceptance["key"]
        with self.connection() as db:
            assert db.execute("SELECT count(*) FROM workflow_documents WHERE kind IN ('workflow-accepted-agreement','workflow-binding')").fetchone()[0] == 0
        self.meta["preserved_original_statuses"] = 20
        self.meta["old_acceptance_status"] = status
        self.meta["current_fixture_authority_checked_at"] = trusted
        self.save()

    def accept_customer_only(self):
        original = self.failed_acceptance["payload"]
        payload = copy.deepcopy(original)
        customer_assent = next(d for d in payload["documents"] if d["schema"] == "ledger-workflow-assent/1")
        party_assents = [d for d in payload["documents"] if d["schema"] == "ledger-workflow-party-assent/2"]
        assert len(party_assents) == 1
        removed_assent = reference("workflow-assent", party_assents[0])
        removed_authority = party_assents[0]["consent"]["authorization_ref"]
        payload["documents"] = [d for d in payload["documents"] if d["schema"] != "ledger-workflow-party-assent/2"]
        for target in [payload, payload["binding_inputs"]]:
            assert target["assent_refs"].count(removed_assent) == 1
            target["assent_refs"] = [r for r in target["assent_refs"] if r != removed_assent]
            assert target["assent_refs"] == [reference("workflow-assent", customer_assent)]
            assert target["authority_refs"].count(removed_authority) == 1
            target["authority_refs"] = [r for r in target["authority_refs"] if r != removed_authority]
            assert target["authority_refs"] == self.accepted_grants()
        terms = json.loads((self.inputs / "acceptance-terms.json").read_bytes())
        identity = json.loads((self.inputs / "acceptance-terms.identity.json").read_bytes())
        projected = copy.deepcopy(payload["binding_inputs"])
        projected.pop("assent_refs")
        projected.pop("authority_refs")
        assert projected == terms["binding_inputs"]
        assert reference("workflow-acceptance-terms", terms) == identity["reference"]
        assert customer_assent["terms_digest"] == identity["terms_digest"]
        # Exact equality after restoring only the deliberately omitted fields.
        restored = copy.deepcopy(payload)
        restored["documents"] = original["documents"]
        for target, old in [(restored, original), (restored["binding_inputs"], original["binding_inputs"])]:
            target["assent_refs"] = old["assent_refs"]
            target["authority_refs"] = old["authority_refs"]
        assert restored == original
        proposed = {"schema": "ledger-operation/1", "contract": "workflow/1", "kind": "agreement.accept",
            "key": self.key("acceptance-customer-local"), "payload": payload}
        assert sha(canonical(proposed)) == EXPECTED_ACCEPTANCE_SHA
        self.meta["acceptance_input_delta"] = {"removed_assent_ref": removed_assent,
            "removed_authority_ref": removed_authority, "customer_assent_ref": reference("workflow-assent", customer_assent),
            "terms_ref": identity["reference"], "terms_digest": identity["terms_digest"],
            "old_key": self.failed_acceptance["key"], "new_key": self.key("acceptance-customer-local")}
        self.save()
        published = self.commits[self.publication_operation]
        self.plan = self.document(unique(published, "workflow-compiled-plan"))
        sources = self.plan["source_refs"]
        self.product_ref = next(r for r in sources if r["kind"] == "workflow-product")
        self.policy_ref = next(r for r in sources if r["kind"] == "workflow-policy")
        self.policy = self.document(self.policy_ref)
        self.fact_refs = {self.document(r)["schema_id"]: r for r in sources if r["kind"] == "workflow-fact-schema"}
        assert payload["publication_ref"] == unique(published, "workflow-publication")
        accepted = self.submit("acceptance-customer-local", "agreement.accept", payload)
        self.binding_ref = self.document(unique(accepted, "workflow-accepted-agreement"))["binding_ref"]
        self.binding = self.document(self.binding_ref)
        self.submit("activation", "agreement.activate", {"schema": "ledger-workflow-activation/1", "agreement_id": self.agreement,
            "expected_revision": "0", "accepted_binding_ref": self.binding_ref, "evidence_refs": [], "policy_ref": self.policy_ref,
            "product_ref": self.product_ref, "fact_schema_refs": self.binding["fact_schema_refs"],
            "compiled_plan_ref": self.binding["compiled_plan_ref"], "effective": {"kind": "at_commit"}})
        self.meta["setup_complete_before_work_at"] = now()
        self.save()



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["candidate", "pins", "source", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    candidate = json.loads(args.candidate.read_bytes())
    args.ledger = Path(candidate["binary_path"])
    args.case, args.resume, args.deadline_seconds, args.pending_finalize_check = "members", False, 0, False
    journey = None
    try:
        journey = MembersCompletion(args)
        journey.preflight()
        journey.accept_customer_only()
        journey.members_sequence()
        journey.recover()
    except Exception as error:
        if journey is not None:
            journey.meta["status"] = "STOPPED_FIRST_SUBSTANTIVE_BLOCKER"
            journey.meta["error"] = repr(error)
        raise
    finally:
        if journey is not None:
            journey.assert_originals()
            journey.meta["final_snapshot"] = journey.snapshot()
            journey.meta["final_snapshot_sha256"] = sha(canonical(journey.meta["final_snapshot"]))
            journey.meta["final_configuration_sha256"] = sha(journey.config_bytes)
            journey.meta["source_preserved"] = True
            journey.save()
        verify_pins(pins)


if __name__ == "__main__":
    main()
