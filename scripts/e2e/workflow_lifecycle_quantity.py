#!/usr/bin/env python3
"""One fresh, priced D quantity correction with its actual Stage inverse.

The preserved fixed-only history is recovery evidence only. A new immutable
Unit policy, agreement, chain and business identities precede all new Work.
No retry, resource calibration, old submission, or supplier extension.
"""
import argparse
import json
import sys
from pathlib import Path

from workflow_lifecycle_finite import FiniteLifecycle, SETTINGS
from workflow_stage import StageJourney
from workflow_supported import ATOMS, REPO, canonical, now, reference, sha, unique

FIXTURE = REPO / "examples/workflow-lifecycle-quantity"
RECOVERED_SNAPSHOT = "c2d26880408fbb3eb9b2544684738ec43d8f86350b5ac45e2956d9b767754a00"
RECOVERED_CONFIG = "4f6afdb9326b3218feab3fdfcf23808225cff74859486b0d96a3d67eb04ab9ef"


def verify_pins(pins):
    for group in ("files", "source_files", "product_files"):
        for item in pins[group]:
            raw = Path(item["path"]).read_bytes()
            assert sha(raw) == item["sha256"] and len(raw) == int(item["bytes"]), item["path"]


def check_gate(args, pins):
    verify_pins(pins)
    candidate = json.loads(args.candidate.read_bytes())
    recovery = json.loads(args.recovery_run.read_bytes())
    assert candidate["build_exit"] == 0
    args.ledger = Path(candidate["binary_path"])
    assert sha(args.ledger.read_bytes()) == candidate["binary_sha256"] == recovery["binary_sha256"]
    assert recovery["status"] == "PASS_MEMBERS_REVERSAL_ORIGINAL_RECOVERY"
    assert recovery["completed_recovery_phases"] == ["reopened", "whole-copy"]
    assert recovery["recovery_counts_per_phase"] == {"exact_documents": 7, "exact_statuses": 33}
    assert recovery["source_preserved"] is True
    assert recovery["final_snapshot_sha256"] == RECOVERED_SNAPSHOT
    assert recovery["final_configuration_sha256"] == RECOVERED_CONFIG
    assert len(recovery["final_snapshot"]["operations"]) == 33
    assert len(recovery["final_snapshot"]["documents"]) == 130
    assert len(recovery["final_snapshot"]["heads"]) == 7
    capture = Path(candidate["capture"]).resolve()
    for helper in (args.publication_helper, args.terms_helper):
        assert helper.resolve().is_relative_to(capture), helper
        assert helper.is_file()
    paths = {Path(item["path"]).resolve() for group in ("files", "source_files", "product_files") for item in pins[group]}
    for required in (Path(__file__), args.candidate, args.recovery_run, args.ledger,
                     args.publication_helper, args.terms_helper,
                     FIXTURE / "source.json", FIXTURE / "business.json", FIXTURE / "ORACLE.json"):
        assert required.resolve() in paths, required
    assert not args.output.exists(), "Fresh proof requires a new output directory"
    return candidate


class QuantityLifecycle(FiniteLifecycle):
    fixture_directory = FIXTURE
    source_filename = "source.json"
    publication_operation = "publication-stage-quantity"

    def __init__(self, args):
        # FiniteLifecycle's initializer chooses the old case directory. Keep its
        # strict command/identity methods, but initialize this separate fixture.
        self.attempted, self.commits = set(), {}
        self.config_bytes, self.recovery_only = None, False
        StageJourney.__init__(self, args)
        self.fixture_identity = self.business["fixture_identity"]
        self.chain = self.fixture_identity["chain"]
        self.oracle = json.loads((FIXTURE / "ORACLE.json").read_bytes())
        assert sha((FIXTURE / "source.json").read_bytes()) == self.oracle["source_sha256"]
        assert sha((FIXTURE / "business.json").read_bytes()) == self.oracle["business_sha256"]
        self.original_definitions = self.source_documents()
        assert len(self.original_definitions) == len(self.oracle["document_refs"]) == 6
        kinds = {"ledger-workflow-fact-schema/1": "workflow-fact-schema",
                 "ledger-workflow-product/1": "workflow-product",
                 "ledger-workflow-slot-template/1": "workflow-slot-template",
                 "ledger-workflow-policy/4": "workflow-policy"}
        for document, expected in zip(self.original_definitions, self.oracle["document_refs"]):
            assert reference(kinds[document["schema"]], document) == expected["ref"]
            assert sha(canonical(document)) == expected["body_sha256"]
        assert self.agreement == self.oracle["identity"]["agreement"] == "stage-quantity-support"
        assert self.fixture_identity["operation_prefix"] == "finite-quantity-"
        assert self.fixture_identity["new_initial_agreement"] is True
        assert self.worker != self.alias != self.corrector
        for leg in self.business["binding"]["funding_legs"]:
            assert leg["mode"] == "customer_local" and leg["payer"] == leg["resource_owner"] == self.customer
        self.meta["proof_scope"] = {"case": "quantity", "fresh_only": True,
            "existing_histories_replayed": False, "settings": SETTINGS,
            "default_budget_qualification": False, "supplier_extension": False,
            "read_authority": "Existing local-owner path; enrolled scoped-source reads remain unqualified",
            "oracle_sha256": sha((FIXTURE / "ORACLE.json").read_bytes()),
            "fixture_identity": self.fixture_identity,
            "prior_read_recovery_run": str(args.recovery_run),
            "prior_read_recovery_run_sha256": sha(args.recovery_run.read_bytes())}
        self.save()

    def finish_acceptance(self, binding, documents, publication, auth, platform_subject, roles, provider_auth):
        del auth, platform_subject, roles
        assert provider_auth is None
        terms = {"schema": "ledger-workflow-acceptance-terms/2", "scope": self.scope,
            "customer": self.customer, "agreement_id": self.agreement, "version": "1",
            "publication_ref": publication, "binding_inputs": binding}
        self.command("prepare-terms", [self.args.terms_helper, self.inputs / "context-facts.json",
            self.write("acceptance-terms.raw.json", terms), self.inputs])
        normalized = json.loads((self.inputs / "acceptance-terms.json").read_bytes())
        identity = json.loads((self.inputs / "acceptance-terms.identity.json").read_bytes())
        digest, accepted_at = identity["terms_digest"], now()
        subject = {"customer": self.customer, "agreement_id": self.agreement}
        evidence = self.evidence("customer-assent-evidence-v3", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "accepted_at": accepted_at}, "assent")
        attestation = self.evidence("customer-assent-attestation-v3", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "assent_evidence_ref": evidence,
             "statement": "operator_verified_exact_terms_acceptance"}, "operator_attestation")
        assent = {"schema": "ledger-workflow-assent/1", "scope": self.scope, "customer": self.customer,
            "agreement_id": self.agreement, "terms_digest": digest, "mode": "real", "acceptor": self.operator,
            "accepted_at": accepted_at, "evidence_ref": evidence, "operator": self.operator,
            "operator_attestation_ref": attestation}
        documents = [*documents, assent]
        assents = [reference("workflow-assent", assent)]
        authorities = self.accepted_grants()
        binding = {**normalized["binding_inputs"], "assent_refs": assents, "authority_refs": authorities}
        assert self.policy_ref == self.oracle["new_policy_ref"]
        assert self.policy["policy_id"] == "policy:stage-quantity-original-d" and self.policy["version"] == "1"
        accepted = self.submit("acceptance", "agreement.accept", {"schema": "ledger-workflow-agreement-accept/9",
            "agreement_id": self.agreement, "expected_revision": "0", "publication_ref": publication,
            "slot_catalog_ref": binding["slot_catalog_ref"], "slot_instantiation_ref": binding["slot_instantiation_ref"],
            "binding_inputs": binding, "assent_refs": assents, "authority_refs": authorities, "documents": documents})
        self.binding_ref = self.document(unique(accepted, "workflow-accepted-agreement"))["binding_ref"]
        self.binding = self.document(self.binding_ref)
        assert self.binding["assent_refs"] == assents and self.binding["authority_refs"] == authorities
        self.submit("activation", "agreement.activate", {"schema": "ledger-workflow-activation/1",
            "agreement_id": self.agreement, "expected_revision": "0", "accepted_binding_ref": self.binding_ref,
            "evidence_refs": [], "policy_ref": self.policy_ref, "product_ref": self.product_ref,
            "fact_schema_refs": self.binding["fact_schema_refs"], "compiled_plan_ref": self.binding["compiled_plan_ref"],
            "effective": {"kind": "at_commit"}})
        self.meta["setup_complete_before_work_at"] = now()
        self.save()

    def authorize_stage(self, supplier):
        assert not supplier
        members = [("support:member-b", self.worker, self.fixture_identity["b_business_operation_id"]),
                   ("support:member-d", self.alias, self.fixture_identity["d_business_operation_id"])]
        return self.submit("stage-authorize", "invocation.authorize", {
            "schema": "ledger-workflow-stage-authorize/1", "expected_chain_revision": "0",
            "stage_instance": {"customer": self.customer, "chain": self.chain, "agreement_id": self.agreement,
                "binding_ref": self.binding_ref, "stage_name": "support:stage", "authority_refs": [self.grant],
                "expected": [{"template_input_key": template, "source": source, "business_operation_id": business,
                    "activity": "support:prepare", "component_slots": ["support:prior"]}
                    for template, source, business in members]}, "invocation_inputs": []})

    def close_payload(self, name, stage, anchor):
        occurred = now()
        business = self.fixture_identity["close_business_operation_id"]
        subject = {"kind": "stage", "logical_store_id": self.store, "scope": self.scope,
            "stage_instance_ref": stage, "business_operation_id": business,
            "activity": "support:close", "binding_ref": self.binding_ref}
        proofs = [self.field_proof(name + "-occurrence", subject, self.fact_refs["support:stage-close-facts"],
                    "support:occurred", occurred, "occurrence", self.closer),
                  self.field_proof(name + "-success", subject, self.fact_refs["support:stage-close-facts"],
                    "support:success", True, "outcome", self.closer)]
        return {"schema": "ledger-workflow-stage-close/1", "external_event_id": name,
            "business_operation_id": business, "customer": self.customer, "chain": self.chain,
            "activity": "support:close", "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:stage-close-facts"], "binding_ref": self.binding_ref,
            "facts": {"support:occurred": occurred, "support:success": True}, "fact_proofs": sorted(proofs, key=canonical),
            "links": [{"slot": "support:closes", "predecessor": anchor}], "evidence_refs": [],
            "occurred_at": occurred, "stage": "support:stage", "stage_instance_ref": stage}

    def current_rights(self):
        trusted = now()
        for ref in self.accepted_grants():
            grant = self.document(ref)
            assert grant["starts_at"] <= trusted < grant["ends_at"]
            assert grant["principal_id"] == self.operator and self.customer in grant["customer_scope"]
            with self.connection() as db:
                row = db.execute("SELECT revision,grant_id,status FROM workflow_source_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND enrollment_id=?",
                    (self.store, *self.scope, grant["grant_id"])).fetchone()
            assert row == (int(grant["revision"]), ref["id"], "active")
        corrector = self.document(self.corrector_grant)
        assert "quantity.correct" in corrector["operations"] and "correct" in corrector["permissions"]
        assert "support:corrector" in corrector["source_slots"]
        self.meta["current_fixture_authority_checked_at"] = trusted
        self.save()

    def original_priced_action(self, result, expected_atoms, component):
        ref = unique(result, "workflow-action")
        action = self.document(ref)
        decision_ref = unique(result, "workflow-decision")
        decision = self.document(decision_ref)
        assert action["amount"] == {"atoms": str(expected_atoms), "currency": "USD", "scale": 18}
        assert action["posting_role"] == "original" and action["book"] == "retail"
        assert action["component_slot"] == action["root_component_slot"] == component
        assert "inverse_of" not in action and action["operation_key"] == result["receipt"]["key"]
        assert decision["actions_appended"] == decision["active_action_refs"] == [ref]
        assert decision["revision"] == "1" and decision["original_terms_ref"] == self.binding_ref
        assert action["decision_id"] == decision["decision_id"] and action["revision"] == decision["revision"]
        evaluation = self.document(decision["evaluation_ref"])
        assert decision["evaluation_ref"] in result["receipt"]["result_refs"]
        assert evaluation["subject_ref"] == decision["subject_ref"]
        assert evaluation["policy_ref"] == self.policy_ref and evaluation["binding_ref"] == self.binding_ref
        posted = [step for step in evaluation["steps"] if "action_ref" in step]
        assert len(posted) == 1 and posted[0]["action_ref"] == ref
        assert posted[0]["disposition"] == "posted" and posted[0]["rounded"] == action["amount"]
        return ref, action, decision_ref, decision

    def quantity_sequence(self):
        self.current_rights()
        stage = unique(self.authorize_stage(False), "workflow-stage-instance")
        first = self.submit("b-work", "work.accept", self.work_payload("b-work",
            self.fixture_identity["b_business_operation_id"], self.worker, "failed", stage))
        b_ref = unique(first, "workflow-work")
        b_body = self.document(b_ref)
        b_decision = self.document(unique(first, "workflow-decision"))
        assert b_decision["actions_appended"] == b_decision["active_action_refs"] == []
        _, started_ref = self.head("stage", self.agreement, self.chain, "support:stage")
        started = self.document(started_ref)
        assert started["state"] == "started" and started["first_work_ref"] == b_ref
        assert started["first_work_accepted_at"] == b_body["accepted_at"]
        self.book_totals("failed-B-zero", 0, 0)
        second = self.submit("d-work", "work.accept", self.work_payload("d-work",
            self.fixture_identity["d_business_operation_id"], self.alias, "succeeded", stage))
        d_ref = unique(second, "workflow-work")
        d_body = self.document(d_ref)
        assert d_body["key"]["source"] == self.alias and d_body["source_namespace"] == self.worker
        assert d_body["payload"]["facts"]["support:quantity"] == "1"
        root_ref, root_action, root_decision_ref, root_decision = self.original_priced_action(second, ATOMS, "support:prior")
        assert self.head("stage", self.agreement, self.chain, "support:stage")[1] == started_ref
        self.book_totals("original-D-Unit-one", ATOMS, 0)
        assert now() < started["activation_deadline"]
        closed = self.submit("close-complete", "outcome.submit", self.close_payload("close-complete", stage, b_ref))
        stage_ref, stage_action, close_ref, close_decision = self.original_priced_action(closed, 2 * ATOMS, "support:premium")
        self.book_totals("root-one-plus-stage-two", 3 * ATOMS, 0)
        terminal_head = self.head("stage", self.agreement, self.chain, "support:stage")
        terminal = self.document(terminal_head[1])
        assert terminal_head[0] == "3" and terminal["state"] == "closed"
        for field in ("first_work_ref", "first_work_accepted_at", "activation_deadline"):
            assert terminal[field] == started[field]
        closure_ref = unique(closed, "workflow-stage-closure-claim")
        closure = self.document(closure_ref)
        assert closure["stage_instance_ref"] == stage and closure["first_work_ref"] == b_ref
        assert now() < closure["window"]["correction_until"]
        evaluation = self.document(close_decision["evaluation_ref"])
        cap = [step for step in evaluation["steps"] if step["component_slot"] == "support:cap"]
        assert len(cap) == 1 and cap[0]["disposition"] == "zero" and int(cap[0]["rounded"]["atoms"]) == 0
        assert "action_ref" not in cap[0]
        b_heads = {kind: self.head(kind, b_decision["decision_id"]) for kind in ("decision", "capacity")}
        assert self.head("decision", root_decision["decision_id"]) == ("1", root_decision_ref)
        assert self.head("decision", close_decision["decision_id"]) == ("1", close_ref)
        self.current_rights()
        terms = self.document(root_action["slot_terms_ref"])
        assert terms["correction_source_slot"] == "support:corrector" and self.corrector_grant in terms["source_grant_refs"]
        result = self.submit("correct-d-quantity", "quantity.correct", {
            "schema": "ledger-workflow-quantity-correction/2", "work_ref": d_ref, "expected_revision": "1",
            "field": "support:quantity", "replacement_quantity": "0", "fact_proofs": [], "family_closures": [],
            "dependent_reversals": [{"decision_ref": close_ref, "expected_revision": "1",
                                     "authority_evidence_refs": [self.corrector_grant]}],
            "evidence_refs": [self.corrector_grant]})
        refs = [ref for ref in result["receipt"]["result_refs"] if ref["kind"] == "workflow-action"]
        assert len(refs) == 2 and len({canonical(ref) for ref in refs}) == 2
        expected = {canonical(root_ref): (root_ref, root_action, -ATOMS),
                    canonical(stage_ref): (stage_ref, stage_action, -2 * ATOMS)}
        inverse_by_original = {}
        for ref in refs:
            inverse = self.document(ref)
            old_ref, old, amount = expected[canonical(inverse["inverse_of"])]
            assert canonical(old_ref) not in inverse_by_original
            assert inverse["posting_role"] == "inverse" and inverse["amount"] == {"atoms": str(amount), "currency": "USD", "scale": 18}
            assert inverse["operation_key"] == result["receipt"]["key"] and inverse["revision"] == "2"
            for field in ("book", "component_slot", "root_component_slot", "role_ref", "slot_terms_ref", "funding_leg", "basis_refs"):
                assert inverse[field] == old[field]
            inverse_by_original[canonical(old_ref)] = ref
        assert set(inverse_by_original) == set(expected)
        for old_ref, old_decision, transition, status, old_action_ref in (
            (root_decision_ref, root_decision, "quantity_correct", "active", root_ref),
            (close_ref, close_decision, "dependent_reverse", "reversed", stage_ref)):
            revision, current_ref = self.head("decision", old_decision["decision_id"])
            decision = self.document(current_ref)
            inverse_ref = inverse_by_original[canonical(old_action_ref)]
            assert revision == decision["revision"] == "2" and decision["prior_ref"] == old_ref
            assert decision["transition"] == transition and decision["status"] == status
            assert decision["active_action_refs"] == [] and decision["actions_appended"] == [inverse_ref]
            assert decision["actions_appended"] == sorted(decision["actions_appended"], key=canonical)
            assert decision["original_terms_ref"] == self.binding_ref
            assert decision["operation_key"] == result["receipt"]["key"] and current_ref in result["receipt"]["result_refs"]
            evaluation = self.document(decision["evaluation_ref"])
            assert decision["evaluation_ref"] in result["receipt"]["result_refs"]
            action_steps = [step for step in evaluation["steps"] if "action_ref" in step]
            assert len(action_steps) == 1 and action_steps[0]["action_ref"] == inverse_ref
            assert action_steps[0]["disposition"] == "inverse"
            assert action_steps[0]["rounded"] == self.document(inverse_ref)["amount"]
            assert evaluation["policy_ref"] == self.policy_ref and evaluation["binding_ref"] == self.binding_ref
        self.book_totals("exact-minus-one-minus-two-no-replacement", 0, 0)
        assert self.document(b_ref) == b_body and self.document(d_ref) == d_body
        assert self.document(root_ref) == root_action and self.document(stage_ref) == stage_action
        assert self.head("stage", self.agreement, self.chain, "support:stage") == terminal_head
        assert self.document(terminal_head[1]) == terminal and self.document(closure_ref) == closure
        for kind, expected_head in b_heads.items():
            assert self.head(kind, b_decision["decision_id"]) == expected_head
        with self.connection() as db:
            assert db.execute("SELECT COUNT(*) FROM workflow_documents WHERE kind IN ('workflow-invocation','workflow-cost','workflow-allocation')").fetchone()[0] == 0
        self.meta["verified_quantity_oracle"] = {"original_D": d_ref, "B": b_ref, "stage": stage,
            "closed_stage_ref": terminal_head[1], "original_root_action": root_ref,
            "original_stage_action": stage_ref, "inverse_refs": refs,
            "retail_atoms": [str(ATOMS), str(2 * ATOMS), str(-ATOMS), str(-2 * ATOMS)],
            "total_atoms": "0", "supplier_atoms": "0", "original_source": self.alias,
            "canonical_namespace": self.worker, "current_corrector": self.corrector,
            "new_policy_ref": self.policy_ref, "binding_ref": self.binding_ref}
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "pins", "recovery-run", "publication-helper", "terms-helper", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.set_defaults(case="quantity", resume=False, deadline_seconds=0, pending_finalize_check=False)
    args = parser.parse_args()
    pins = json.loads(args.pins.read_bytes())
    check_gate(args, pins)
    journey = None
    try:
        journey = QuantityLifecycle(args)
        journey.setup()
        journey.quantity_sequence()
        journey.recover()
        assert journey.meta["status"] == "PASS_FINITE_QUANTITY"
    except Exception as error:
        if journey is not None:
            journey.meta["status"] = "STOPPED_FIRST_SUBSTANTIVE_BLOCKER"
            journey.meta["error"] = repr(error)
        raise
    finally:
        active_exception = sys.exc_info()[0] is not None
        preservation_errors = []
        # Always attempt every evidence/preservation step. Never mask an active
        # submission/read refusal with a later snapshot, pin, or metadata error.
        if journey is not None:
            try:
                if journey.can_snapshot():
                    snapshot = journey.snapshot()
                    journey.meta["final_snapshot"] = snapshot
                    journey.meta["final_snapshot_sha256"] = sha(canonical(snapshot))
                elif (journey.installation / ".ledger/local.db").exists():
                    with journey.connection() as db:
                        tables = {}
                        for name, in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"):
                            rows = db.execute('SELECT * FROM "' + name.replace('"', '""') + '"').fetchall()
                            values = [canonical([{"bytes": len(v), "sha256": sha(v)} if isinstance(v, bytes) else v for v in row]) for row in rows]
                            tables[name] = {"rows": len(rows), "sha256": sha(b"\n".join(sorted(values)))}
                    journey.meta["final_preworkflow_snapshot"] = tables
            except Exception as error:
                preservation_errors.append({"phase": "final_snapshot", "error": repr(error)})
            try:
                config = journey.installation / "billing.json"
                if config.exists():
                    journey.meta["final_configuration_sha256"] = sha(config.read_bytes())
                    if journey.config_bytes is not None:
                        assert config.read_bytes() == journey.config_bytes
            except Exception as error:
                preservation_errors.append({"phase": "configuration", "error": repr(error)})
        try:
            verify_pins(pins)
        except Exception as error:
            preservation_errors.append({"phase": "final_pins", "error": repr(error)})
        if journey is not None:
            journey.meta["preservation_errors"] = preservation_errors
            if preservation_errors:
                journey.meta["status"] = "STOPPED_PRESERVATION_FAILURE"
            try:
                journey.save()
            except Exception as error:
                preservation_errors.append({"phase": "final_metadata", "error": repr(error)})
        if preservation_errors:
            print(json.dumps({"preservation_errors": preservation_errors}), file=sys.stderr, flush=True)
            if not active_exception:
                raise RuntimeError("Final evidence or preservation check failed")


if __name__ == "__main__":
    main()
