#!/usr/bin/env python3
"""One reviewed continuation from the preserved 15-operation members setup.

Only unpublished template DATA changes. No setup replay, resource command,
financial retry, clock change, or fallback. Stop at the first unexpected result.
"""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_lifecycle_finite import FiniteLifecycle, SETTINGS
from workflow_supported import canonical, now, reference, sha, unique

SOURCE_RUN_SHA = "2ea399547c586b11d8f9561ce91f8e85e5b0bf88546b4c53b948b121b1e83400"
SOURCE_SNAPSHOT_SHA = "636ce6896b9032ea1b799b14a866c09463be15e294aa463bd8a43d2fcabfd411"
CONFIG_SHA = "4f6afdb9326b3218feab3fdfcf23808225cff74859486b0d96a3d67eb04ab9ef"
CORRECTED_SOURCE_SHA = "3fbf5acc31789c9eb8039cfc6519f42733dd5ec4af57550d79f11f1dce163ba4"
ALLOWED = {
    "publication-authorized-product-v3": "policy.publish",
    "customer-assent-evidence-v3": "evidence.retain",
    "customer-assent-attestation-v3": "evidence.retain",
    "platform-assent-evidence-v3": "evidence.retain",
    "platform-assent-attestation-v3": "evidence.retain",
    "acceptance": "agreement.accept", "activation": "agreement.activate",
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


class MembersContinuation(FiniteLifecycle):
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
        assert sha(args.corrected_source.read_bytes()) == CORRECTED_SOURCE_SHA
        self.helper_attempts = set()
        super().__init__(args)
        self.store = json.loads(self.original_config)["store_id"]
        self.config_bytes = self.original_config
        self.original_snapshot = self.source_snapshot()
        assert canonical(self.original_snapshot) == canonical(self.parent["final_snapshot"])
        shutil.copytree(self.source_installation, self.installation, copy_function=shutil.copy2)
        shutil.copytree(self.source_root / "inputs", self.inputs, dirs_exist_ok=True, copy_function=shutil.copy2)
        assert canonical(self.snapshot()) == canonical(self.original_snapshot)
        (self.root / "PARENT-RUN.json").write_bytes(parent_raw)
        saved = []
        for path in self.source_root.glob("*.committed.json"):
            result = json.loads(path.read_bytes())
            saved.append((int(result["receipt"]["commit_sequence"]), path, result))
        assert [n for n, _, _ in sorted(saved)] == list(range(1, 16))
        with self.connection() as db:
            receipts = [json.loads(b) for b, in db.execute("SELECT receipt_bytes FROM workflow_operations ORDER BY commit_sequence")]
        assert len(receipts) == 15
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
            "resume_original_operations": 15, "existing_operations_replayed": False,
            "fixture_fix": "New initial no-share template identity; all components, rules, business and rights unchanged",
            "corrected_source_sha256": CORRECTED_SOURCE_SHA, "settings": SETTINGS,
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
        assert canonical(current["operations"][:15]) == canonical(self.original_snapshot["operations"])
        assert set(map(tuple, self.original_snapshot["documents"])) <= set(map(tuple, current["documents"]))
        assert (self.installation / "billing.json").read_bytes() == self.original_config
        assert canonical(self.source_snapshot()) == canonical(self.original_snapshot)
        assert (self.source_installation / "billing.json").read_bytes() == self.original_config

    def source_documents(self):
        return json.loads(self.args.corrected_source.read_bytes())["documents"]

    def setup(self):
        raise AssertionError("Committed setup must never be replayed")

    def command(self, name, argv, expected=0, allow_unavailable=False):
        executable = Path(argv[0])
        if executable == self.binary:
            assert list(map(str, argv[1:2])) == ["workflow"]
            route = str(argv[4])
            assert route in {"submit", "status", "read"}, argv
            if route == "submit":
                request = json.loads(Path(argv[5]).read_bytes())
                assert name in ALLOWED and request["kind"] == ALLOWED[name]
                assert request["key"] == self.key(name)
                assert name not in self.original_names
        else:
            allowed_helpers = {"prepare-publication": self.args.publication_helper, "prepare-terms": self.args.terms_helper}
            assert name in allowed_helpers and executable == allowed_helpers[name]
            assert name not in self.helper_attempts
            self.helper_attempts.add(name)
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
        auth = unique(self.commits["platform-authorize"], "workflow-authority")
        body = self.document(auth)
        assert body["status"] == "active"
        assert body["authorization"]["starts_at"] <= trusted < body["authorization"]["ends_at"]
        assert body["authorization"]["customer"] == self.customer
        assert body["authorization"]["agreement_id"] == self.agreement
        self.meta["preserved_setup_statuses"] = 15
        self.meta["current_fixture_authority_checked_at"] = trusted
        self.save()
        return auth

    def publish_and_accept(self, auth):
        self.write_source_documents()
        pub_path = self.inputs / "publication.request.json"
        assert not pub_path.exists()
        self.command("prepare-publication", [self.args.publication_helper, self.inputs,
            self.write("publication.key.json", self.key(self.publication_operation)), pub_path, "--operator-resources"])
        request = json.loads(pub_path.read_bytes())
        assert request["key"] == self.key(self.publication_operation)
        assert request["payload"]["root_policy_ref"] == json.loads(self.args.corrected_source.read_bytes())["root_policy_ref"]
        published = self.submit(self.publication_operation, "policy.publish", request["payload"])
        self.plan = self.document(unique(published, "workflow-compiled-plan"))
        sources = self.plan["source_refs"]
        self.product_ref = next(r for r in sources if r["kind"] == "workflow-product")
        self.policy_ref = next(r for r in sources if r["kind"] == "workflow-policy")
        self.policy = self.document(self.policy_ref)
        self.fact_refs = {self.document(r)["schema_id"]: r for r in sources if r["kind"] == "workflow-fact-schema"}
        template = next(r for r in sources if r["kind"] == "workflow-slot-template")
        assert template == self.policy["slot_template_ref"]
        role, catalog = [copy.deepcopy(self.business[n]) for n in ["role", "catalog"]]
        role_ref, catalog_ref = reference("workflow-roles", role), reference("workflow-slot-catalog", catalog)
        instantiation = {"schema": "ledger-workflow-slot-instantiation/1", "customer": self.customer,
            "agreement_id": self.agreement, "revision": "1", "catalog_ref": catalog_ref, "template_ref": template,
            "component_map": self.business["component_map"], "family_map": []}
        terms = {**copy.deepcopy(self.business["terms"]), "role_ref": role_ref, "source_grant_refs": self.accepted_grants()}
        binding = {**copy.deepcopy(self.business["binding"]),
            "accepted_context": {"fact_schema_ref": self.fact_refs["support:stage-context"], "facts": {}},
            "bindings": [{"slot": "support:retail", "binding_ref": reference("workflow-slot-terms", terms)}],
            "roles_refs": [role_ref], "slot_catalog_ref": catalog_ref,
            "slot_instantiation_ref": reference("workflow-slot-instantiation", instantiation)}
        psub = {"customer": self.customer, "agreement_id": self.agreement, "party": self.business["platform"],
            "acceptor": "synthetic-platform-representative"}
        self.finish_acceptance(binding, [role, catalog, instantiation, terms],
            unique(published, "workflow-publication"), auth, psub, ["bearer", "payer", "provider"], None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["candidate", "pins", "source", "corrected-source", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    verify_pins(pins)
    candidate = json.loads(args.candidate.read_bytes())
    args.ledger = Path(candidate["binary_path"])
    for arg, helper in [("publication_helper", "prepare_local_publication"), ("terms_helper", "prepare_local_terms"),
                        ("supplier_helper", "prepare_local_supplier_commitment")]:
        setattr(args, arg, Path(candidate["helpers"][helper]["path"]))
    args.case, args.resume, args.deadline_seconds, args.pending_finalize_check = "members", False, 0, False
    journey = None
    try:
        journey = MembersContinuation(args)
        auth = journey.preflight()
        journey.publish_and_accept(auth)
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
