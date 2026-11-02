#!/usr/bin/env python3
"""Finite W23 supplier Invocation and no-share fixed-membership proof.

Fresh synthetic installations only. Existing Stage63 and Component90 histories
are evidence inputs, never replayed. One attempt per new identity; exact-key
status on any uncertain result, then stop. No resource ladder or auto retry.
"""
import argparse
import base64
import copy
import datetime as dt
import json
import shutil
import subprocess
import time
from pathlib import Path

from workflow_stage import StageJourney
from workflow_supported import ATOMS, REPO, canonical, now, reference, sha, unique

SETTINGS = {
    "schema": "ledger-local-resource-settings/1", "semantic_work": None,
    "compile_work": 1048576, "resolved_records": 4096, "resolved_bytes": 8388608,
    "new_body_bytes": 8388608, "arena_bytes": 67108864,
    "retained_operations": 100000, "retained_bytes": 268435456,
    "attempt_timeout_ms": 60000,
}


class FiniteLifecycle(StageJourney):
    source_filename = "source.json"

    def __init__(self, args):
        self.fixture_directory = REPO / "examples" / ("workflow-lifecycle-invocation" if args.case == "invocation" else "workflow-lifecycle-members")
        self.attempted = set()
        self.commits = {}
        self.config_bytes = None
        self.recovery_only = False
        super().__init__(args)
        self.meta["proof_scope"] = {"case": args.case, "fresh_only": True,
            "settings": SETTINGS, "default_budget_qualification": False,
            "existing_histories_replayed": False,
            "invocation_coverage": "Embedded authorization to Work/PRESENT; standalone authorization to unused expiry. Standalone authorization to Work is separately unqualified."}
        self.save()

    def write(self, name, value):
        if name == "resources.json":
            value = SETTINGS
        return super().write(name, value)

    def key(self, operation):
        key = super().key(operation)
        if operation.startswith(("b-", "unused-")):
            key["source"] = self.worker
        elif operation.startswith("d-"):
            key["source"] = self.alias
        elif operation.startswith("close-"):
            key["source"] = self.closer
        elif operation.startswith("correct-"):
            key["source"] = self.corrector
        key["operation_id"] = "finite-" + self.args.case + "-" + operation
        return key

    def can_snapshot(self):
        if not (self.installation / ".ledger/local.db").exists():
            return False
        with self.connection() as db:
            return db.execute("SELECT 1 FROM sqlite_master WHERE name='workflow_operations'").fetchone() is not None

    def command(self, name, argv, expected=0, allow_unavailable=False):
        del allow_unavailable
        argv = [str(v) for v in argv]
        if self.recovery_only:
            assert argv[1] == "workflow" and argv[4] in {"read", "status"}, argv
        if "resources" in argv:
            assert self.config_bytes is None and name in {"resources-set", "resources-show"}
        index = len(self.meta["commands"])
        stem = self.root / (f"{index:03d}-" + name)
        before = self.snapshot() if self.can_snapshot() else None
        if before is not None:
            stem.with_suffix(".before.json").write_bytes(canonical(before))
        if self.config_bytes is not None:
            assert (self.installation / "billing.json").read_bytes() == self.config_bytes
        entry = {"name": name, "argv": argv, "started_at": now(),
                 "executable_sha256": sha(Path(argv[0]).read_bytes())}
        out = err = b""
        try:
            process = subprocess.run(argv, capture_output=True, timeout=120)
            out, err = process.stdout, process.stderr
            entry["exit"] = process.returncode
        except subprocess.TimeoutExpired as error:
            out, err = error.stdout or b"", error.stderr or b""
            entry["exit"] = None
            entry["outcome"] = "unknown_outer_process_timeout_no_retry"
            raise
        finally:
            stem.with_suffix(".stdout").write_bytes(out)
            stem.with_suffix(".stderr").write_bytes(err)
            entry.update({"completed_at": now(), "stdout": stem.with_suffix(".stdout").name,
                          "stderr": stem.with_suffix(".stderr").name,
                          "stdout_sha256": sha(out), "stderr_sha256": sha(err)})
            if self.can_snapshot():
                after = self.snapshot()
                stem.with_suffix(".after.json").write_bytes(canonical(after))
                entry["after_snapshot_sha256"] = sha(canonical(after))
                if before is not None:
                    entry["before_snapshot_sha256"] = sha(canonical(before))
                    if argv[1] == "workflow" and argv[4] in {"status", "read"}:
                        entry["read_only_snapshot_equal"] = canonical(after) == canonical(before)
            self.meta["commands"].append(entry)
            self.save()
            assert entry.get("read_only_snapshot_equal", True), "Read changed logical state"
            if self.config_bytes is not None:
                assert (self.installation / "billing.json").read_bytes() == self.config_bytes
        print(self.args.case, name, entry["exit"], flush=True)
        if expected is not None and entry["exit"] != expected:
            raise RuntimeError(f"{name}: exit {entry['exit']}; {out.decode(errors='replace')}")
        value = json.loads(out) if out.strip() else {}
        if name == "resources-show":
            assert value["resources"] == SETTINGS
            self.config_bytes = (self.installation / "billing.json").read_bytes()
            assert json.loads(self.config_bytes)["resources"] == SETTINGS
            self.meta["configuration_sha256"] = sha(self.config_bytes)
            self.save()
        return value

    def exact_status(self, name, key):
        return self.workflow(name, "status", self.write(name + ".key.json", key))

    def perform(self, name, kind, payload, refusal=None):
        assert not self.recovery_only
        key = self.key(name)
        identity = canonical(key)
        assert identity not in self.attempted and name not in self.commits
        path = self.root / (name + ".request.json")
        assert not path.exists()
        raw = canonical({"schema": "ledger-operation/1", "contract": "workflow/1",
                         "kind": kind, "key": key, "payload": payload})
        path.write_bytes(raw)
        before_status = self.exact_status(name + "-before", key)
        assert before_status["status"] == "not_observed" and before_status["key"] == key
        before = self.snapshot()
        self.attempted.add(identity)
        try:
            result = self.workflow(name, "submit", path, expected=None if refusal else 0)
        except Exception:
            failure = {"name": name, "key": key, "raw_sha256": sha(raw), "outcome": "unknown_until_status"}
            try:
                failure["status"] = self.exact_status(name + "-failure-status", key)
            except Exception as error:
                failure["status_error"] = repr(error)
            self.meta["first_failure"] = failure
            self.save()
            raise
        if refusal:
            assert result.get("status") == "error" and result.get("code") == refusal, result
            assert canonical(self.snapshot()) == canonical(before)
            status = self.exact_status(name + "-refused-status", key)
            assert status["status"] == "not_observed" and status["key"] == key
            self.meta["checks"].append({"check": name, "refusal": refusal, "no_append": True})
        else:
            assert result["status"] == "committed" and result["receipt"]["key"] == key
            assert result["receipt"]["original_request_bytes_sha256"] == sha(raw)
            (self.root / (name + ".committed.json")).write_bytes(canonical(result))
            self.commits[name] = result
        self.save()
        return result

    def submit(self, name, kind, payload):
        return self.perform(name, kind, payload)

    def negative(self, name, kind, payload, code="INTEGRITY_FAILURE"):
        return self.perform(name, kind, payload, code)

    def setup_publication(self, auth, psub, roles):
        if self.args.case == "invocation":
            return super().setup_publication(auth, psub, roles)
        customer = unique(self.commits["customer-register"], "workflow-customer")
        for label in ["worker", "corrector", "closer", "alias"]:
            ceiling = self.business[label + "_ceiling"]
            ev = self.evidence(label + "-source-evidence", "ledger-workflow-evidence-retain/1",
                {"customer": self.customer}, {"source": ceiling["source"], "principal_id": self.operator}, "source_enrollment")
            payload = {"schema": "ledger-workflow-source-enroll/3" if label == "alias" else "ledger-workflow-source-enroll/2",
                "action": "source.enroll", "expected_revision": "0", "enrollment_id": "finite-" + label,
                "customer_refs": [customer], "ceiling": ceiling, "evidence_ref": ev}
            if label == "alias":
                payload["namespace_owner_enrollment_ref"] = self.worker_enrollment
            result = self.submit(label + "-enroll", "authority.change", payload)
            setattr(self, label + "_grant", unique(result, "workflow-source-grant"))
            setattr(self, label + "_enrollment", unique(result, "workflow-source-enrollment"))
        self.write_source_documents()
        pub_path = self.inputs / "publication.request.json"
        self.command("prepare-publication", [self.args.publication_helper, self.inputs,
            self.write("publication.key.json", self.key(self.publication_operation)), pub_path, "--operator-resources"])
        published = self.submit(self.publication_operation, "policy.publish", json.loads(pub_path.read_bytes())["payload"])
        self.plan = self.document(unique(published, "workflow-compiled-plan"))
        sources = self.plan["source_refs"]
        self.product_ref = next(r for r in sources if r["kind"] == "workflow-product")
        self.policy_ref = next(r for r in sources if r["kind"] == "workflow-policy")
        self.policy = self.document(self.policy_ref)
        self.fact_refs = {self.document(r)["schema_id"]: r for r in sources if r["kind"] == "workflow-fact-schema"}
        template = next(r for r in sources if r["kind"] == "workflow-slot-template")
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
        self.finish_acceptance(binding, [role, catalog, instantiation, terms],
                               unique(published, "workflow-publication"), auth, psub, roles, None)

    def finish_acceptance(self, binding, documents, publication, auth, platform_subject, roles, provider_auth):
        terms = {"schema": "ledger-workflow-acceptance-terms/2", "scope": self.scope, "customer": self.customer,
                 "agreement_id": self.agreement, "version": "1", "publication_ref": publication, "binding_inputs": binding}
        self.command("prepare-terms", [self.args.terms_helper, self.inputs / "context-facts.json",
            self.write("acceptance-terms.raw.json", terms), self.inputs])
        normalized = json.loads((self.inputs / "acceptance-terms.json").read_bytes())
        identity = json.loads((self.inputs / "acceptance-terms.identity.json").read_bytes())
        digest, accepted_at = identity["terms_digest"], now()
        subject = {"customer": self.customer, "agreement_id": self.agreement}
        ev = self.evidence("customer-assent-evidence-v3", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "accepted_at": accepted_at}, "assent")
        att = self.evidence("customer-assent-attestation-v3", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "assent_evidence_ref": ev,
             "statement": "operator_verified_exact_terms_acceptance"}, "operator_attestation")
        customer_assent = {"schema": "ledger-workflow-assent/1", "scope": self.scope, "customer": self.customer,
            "agreement_id": self.agreement, "terms_digest": digest, "mode": "real", "acceptor": self.operator,
            "accepted_at": accepted_at, "evidence_ref": ev, "operator": self.operator, "operator_attestation_ref": att}
        consent = {**platform_subject, "scope": self.scope, "roles": roles, "authorization_ref": auth,
            "basis": {"kind": "acceptance_terms", "terms_ref": identity["reference"], "terms_digest": digest}, "accepted_at": accepted_at}
        ev = self.evidence("platform-assent-evidence-v3", "ledger-workflow-party-evidence-retain/2", platform_subject,
                           {"purpose": "party_consent", "consent": consent})
        att = self.evidence("platform-assent-attestation-v3", "ledger-workflow-party-evidence-retain/2", platform_subject,
            {"purpose": "party_operator_attestation", "statement": "operator_verified_party_exact_consent", "target_evidence_ref": ev})
        platform_assent = {"schema": "ledger-workflow-party-assent/2", "consent": consent,
            "mode": "real", "evidence_ref": ev, "operator": self.operator, "operator_attestation_ref": att}
        documents += [customer_assent, platform_assent]
        assents = sorted([reference("workflow-assent", d) for d in documents if d["schema"] in
                          {"ledger-workflow-assent/1", "ledger-workflow-party-assent/2", "ledger-workflow-party-assent/3"}], key=canonical)
        authorities = sorted([*self.accepted_grants(), auth] + ([provider_auth] if provider_auth else []), key=canonical)
        binding = {**normalized["binding_inputs"], "assent_refs": assents, "authority_refs": authorities}
        accepted = self.submit("acceptance", "agreement.accept", {"schema": "ledger-workflow-agreement-accept/9",
            "agreement_id": self.agreement, "expected_revision": "0", "publication_ref": publication,
            "slot_catalog_ref": binding["slot_catalog_ref"], "slot_instantiation_ref": binding["slot_instantiation_ref"],
            "binding_inputs": binding, "assent_refs": assents, "authority_refs": authorities, "documents": documents})
        self.binding_ref = self.document(unique(accepted, "workflow-accepted-agreement"))["binding_ref"]
        self.binding = self.document(self.binding_ref)
        self.submit("activation", "agreement.activate", {"schema": "ledger-workflow-activation/1", "agreement_id": self.agreement,
            "expected_revision": "0", "accepted_binding_ref": self.binding_ref, "evidence_refs": [], "policy_ref": self.policy_ref,
            "product_ref": self.product_ref, "fact_schema_refs": self.binding["fact_schema_refs"],
            "compiled_plan_ref": self.binding["compiled_plan_ref"], "effective": {"kind": "at_commit"}})
        self.meta["setup_complete_before_work_at"] = now()
        self.save()

    def invocation_payload(self, business, deadline):
        return {"schema": "ledger-workflow-invocation-authorize/1", "agreement_id": self.agreement,
            "binding_ref": self.binding_ref, "business_operation_id": business, "chain_id": self.chain,
            "activity": "support:prepare", "supplier_slot": "support:supplier", "source_slot": "support:worker",
            "unit": "support:item", "maximum_quantity": "1",
            "maximum_exposure": self.business["supplier_inputs"]["maximum_exposure"],
            "start_before": deadline, "evidence_refs": [self.worker_grant]}

    def authorize_stage(self, supplier):
        members = [("support:member", self.worker, "member-b")] if supplier else [
            ("support:member-b", self.worker, "member-b"), ("support:member-d", self.alias, "member-d")]
        return self.submit("stage-authorize", "invocation.authorize", {
            "schema": "ledger-workflow-stage-authorize/1", "expected_chain_revision": "0",
            "stage_instance": {"customer": self.customer, "chain": self.chain, "agreement_id": self.agreement,
                "binding_ref": self.binding_ref, "stage_name": "support:stage", "authority_refs": [self.grant],
                "expected": [{"template_input_key": template, "source": source, "business_operation_id": business,
                    "activity": "support:prepare", "component_slots": ["support:prior"]} for template, source, business in members]},
            "invocation_inputs": [self.invocation_payload("member-b", "2099-01-01T00:00:00.000000Z")] if supplier else []})

    def work_payload(self, name, business, source, status, stage=None, invocation=None):
        occurred = now()
        subject = {"kind": "work", "logical_store_id": self.store, "scope": self.scope,
            "canonical_source_namespace": self.worker, "business_operation_id": business,
            "customer": self.customer, "activity": "support:prepare", "product_ref": self.product_ref,
            "binding_ref": self.binding_ref}
        proof = self.field_proof(name + "-occurrence", subject, self.fact_refs["support:stage-work-facts"],
                                 "support:occurred", occurred, "occurrence", source)
        facts = {"support:quantity": "1", "support:occurred": occurred}
        if invocation is not None:
            facts["support:invocation"] = invocation
        payload = {"schema": "ledger-workflow-work/1", "external_event_id": name,
            "business_operation_id": business, "customer": self.customer, "chain": self.chain,
            "activity": "support:prepare", "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:stage-work-facts"], "binding_ref": self.binding_ref,
            "status": status, "facts": facts, "fact_proofs": [proof], "links": [], "evidence_refs": [],
            "invocation_refs": [invocation] if invocation is not None else [], "occurred_at": occurred}
        if stage is not None:
            payload["stage_instance_ref"] = stage
        return payload

    def close_payload(self, name, stage, anchor):
        occurred = now()
        subject = {"kind": "stage", "logical_store_id": self.store, "scope": self.scope,
            "stage_instance_ref": stage, "business_operation_id": "close-business",
            "activity": "support:close", "binding_ref": self.binding_ref}
        proofs = [self.field_proof(name + "-occurrence", subject, self.fact_refs["support:stage-close-facts"],
                    "support:occurred", occurred, "occurrence", self.closer),
                  self.field_proof(name + "-success", subject, self.fact_refs["support:stage-close-facts"],
                    "support:success", True, "outcome", self.closer)]
        return {"schema": "ledger-workflow-stage-close/1", "external_event_id": name,
            "business_operation_id": "close-business", "customer": self.customer, "chain": self.chain,
            "activity": "support:close", "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:stage-close-facts"], "binding_ref": self.binding_ref,
            "facts": {"support:occurred": occurred, "support:success": True}, "fact_proofs": sorted(proofs, key=canonical),
            "links": [{"slot": "support:closes", "predecessor": anchor}], "evidence_refs": [],
            "occurred_at": occurred, "stage": "support:stage", "stage_instance_ref": stage}

    def close_invocation(self, name, original, reason):
        ref, current, values = self.invocation_state(original)
        result = self.submit(name, "invocation.close", {"schema": "ledger-workflow-invocation-close/1",
            "invocation_ref": ref, "expected_revision": current["revision"], "reason": reason,
            "evidence_refs": [self.worker_grant]})
        _, terminal, after = self.invocation_state(original)
        assert after["authorized"] == values["authorized"]
        assert after["consumed"] == values["consumed"]
        if reason == "completed":
            assert terminal["state"] == "closed_completed" and after["held"] == values["held"]
        else:
            assert terminal["state"] == "closed_unused" and after["held"] == after["consumed"] == 0
            assert after["released"] == values["held"] and not terminal["cost_consumption_refs"]
        return result

    def invocation_sequence(self):
        auth = self.authorize_stage(True)
        stage, invocation = unique(auth, "workflow-stage-instance"), unique(auth, "workflow-invocation")
        # Actual future boundary, then natural elapsed time only. No clock edit.
        deadline = dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=90)
        unused = unique(self.submit("unused-authorize", "invocation.authorize",
            self.invocation_payload("unused-member", deadline.strftime("%Y-%m-%dT%H:%M:%S.%fZ"))), "workflow-invocation")
        payload = self.work_payload("b-work", "member-b", self.worker, "succeeded", stage, invocation)
        wrong = copy.deepcopy(payload)
        wrong["facts"]["support:invocation"] = unused
        self.negative("b-wrong-invocation", "work.accept", wrong)
        worked = self.submit("b-work", "work.accept", payload)
        work = unique(worked, "workflow-work")
        original = self.document(work)
        assert original["payload"]["facts"]["support:invocation"] == invocation
        assert original["payload"]["invocation_refs"] == [invocation]
        self.book_totals("present-invocation-original-work", 135 * ATOMS // 100, 0)
        self.close_invocation("b-invocation-close", invocation, "completed")
        # An existing closed_completed hold still pays its original obligation.
        closed = self.submit("close-complete", "outcome.submit", self.close_payload("close-complete", stage, work))
        self.book_totals("closed-invocation-original-obligation", 315 * ATOMS // 100, 45 * ATOMS // 100)
        _, consumed, values = self.invocation_state(invocation)
        assert consumed["state"] == "closed_completed" and values == {
            "authorized": ATOMS, "consumed": 45 * ATOMS // 100,
            "held": 55 * ATOMS // 100, "released": 0}
        allocations = [self.document(r) for r in closed["receipt"]["result_refs"] if r["kind"] == "workflow-allocation"]
        assert sorted(int(a["amount"]["atoms"]) for a in allocations) == [45 * ATOMS // 100, 135 * ATOMS // 100]
        remaining = (deadline - dt.datetime.now(dt.timezone.utc)).total_seconds()
        if remaining > 0:
            self.meta["natural_expiry_wait_seconds"] = remaining
            self.save()
            print("Waiting for the original Invocation expiry", flush=True)
            while remaining > 0:
                time.sleep(min(remaining, 1))
                remaining = (deadline - dt.datetime.now(dt.timezone.utc)).total_seconds()
        retained_stage_invocation = self.invocation_state(invocation)
        no_work_before = self.snapshot()
        self.close_invocation("unused-close", unused, "unused_expired")
        assert self.invocation_state(invocation) == retained_stage_invocation
        after = self.snapshot()
        for table in ["workflow_financial_work_claims", "workflow_financial_outcome_claims"]:
            assert after["logical_tables"][table] == no_work_before["logical_tables"][table]
        refused = self.work_payload("unused-late-work", "unused-member", self.worker, "succeeded", invocation=unused)
        self.negative("unused-late-work", "work.accept", refused)
        self.book_totals("unused-close-no-obligation", 315 * ATOMS // 100, 45 * ATOMS // 100)

    def members_sequence(self):
        stage = unique(self.authorize_stage(False), "workflow-stage-instance")
        first = self.submit("b-work", "work.accept",
            self.work_payload("b-work", "member-b", self.worker, "failed", stage))
        b = unique(first, "workflow-work")
        _, started_ref = self.head("stage", self.agreement, self.chain, "support:stage")
        started = self.document(started_ref)
        assert started["state"] == "started" and started["first_work_ref"] == b
        assert started["first_work_accepted_at"] == self.document(b)["accepted_at"]
        self.book_totals("failed-first-member", 0, 0)
        self.negative("close-missing-d", "outcome.submit", self.close_payload("close-missing-d", stage, b))
        second = self.submit("d-work", "work.accept",
            self.work_payload("d-work", "member-d", self.alias, "succeeded", stage))
        d = unique(second, "workflow-work")
        d_body = self.document(d)
        assert d_body["key"]["source"] == self.alias and d_body["source_namespace"] == self.worker
        assert self.head("stage", self.agreement, self.chain, "support:stage")[1] == started_ref
        self.book_totals("new-d-source-not-b-price", 0, 0)
        payload = self.close_payload("close-complete", stage, b)
        shutil.copytree(self.installation, self.root / "close-precommit-installation", copy_function=shutil.copy2)
        closed = self.submit("close-complete", "outcome.submit", payload)
        self.book_totals("complete-fixed-members", 2 * ATOMS, 0)
        _, terminal_ref = self.head("stage", self.agreement, self.chain, "support:stage")
        terminal = self.document(terminal_ref)
        assert terminal["state"] == "closed"
        for field in ["first_work_ref", "first_work_accepted_at", "activation_deadline"]:
            assert terminal[field] == started[field]
        close_decision = unique(closed, "workflow-decision")
        before_actions = self.book_totals("before-original-d-correction", 2 * ATOMS, 0)
        correction = self.submit("correct-d-quantity", "quantity.correct", {
            "schema": "ledger-workflow-quantity-correction/2", "work_ref": d, "expected_revision": "1",
            "field": "support:quantity", "replacement_quantity": "0", "fact_proofs": [],
            "family_closures": [], "dependent_reversals": [{"decision_ref": close_decision,
                "expected_revision": "1", "authority_evidence_refs": [self.corrector_grant]}],
            "evidence_refs": [self.corrector_grant]})
        self.book_totals("original-d-correction-and-exact-dependent-inverse", 0, 0)
        inverses = [self.document(r) for r in correction["receipt"]["result_refs"] if r["kind"] == "workflow-action"]
        assert len(inverses) == len(before_actions) == 1
        old_key, old = next(iter(before_actions.items()))
        inverse = inverses[0]
        assert canonical(inverse["inverse_of"]) == old_key
        assert int(inverse["amount"]["atoms"]) == -int(old["amount"]["atoms"])
        for field in ["book", "component_slot", "root_component_slot", "role_ref", "slot_terms_ref", "funding_leg", "basis_refs"]:
            assert inverse[field] == old[field]
        assert self.document(d) == d_body
        assert self.head("stage", self.agreement, self.chain, "support:stage")[1] == terminal_ref
        self.meta["fixed_member_originals"] = {"failed_b": b, "new_d": d,
            "first_work_accepted_at": started["first_work_accepted_at"], "deadline": started["activation_deadline"]}
        self.save()

    def recover(self):
        baseline = self.snapshot()
        original = self.installation
        reads = {}
        for result in self.commits.values():
            for ref in result["receipt"]["result_refs"]:
                if ref["kind"] in {"workflow-action", "workflow-evaluation"}:
                    reads[canonical(ref)] = (ref, canonical(self.document(ref)))
        self.recovery_only = True
        for phase in ["reopened", "whole-copy"]:
            if phase == "whole-copy":
                self.installation = self.root / "restored-installation"
                shutil.copytree(original, self.installation, copy_function=shutil.copy2)
            try:
                assert canonical(self.snapshot()) == canonical(baseline)
                for name, saved in self.commits.items():
                    raw = (self.root / (name + ".request.json")).read_bytes()
                    status = self.exact_status(phase + "-" + name, saved["receipt"]["key"])
                    assert status["status"] == "committed"
                    assert status["receipt"] == saved["receipt"]
                    assert status["request_evidence"] == saved["request_evidence"]
                    assert status["receipt"]["original_request_bytes_sha256"] == sha(raw)
                for index, (ref, raw) in enumerate(reads.values()):
                    request = {"schema": "ledger-read/1", "kind": "policy.document",
                               "payload": {"ref": ref, "representation": "canonical_bytes"}}
                    response = self.workflow(phase + "-document-" + str(index), "read",
                        self.write(phase + "-document-" + str(index) + ".json", request))
                    assert response["ref"] == ref and response["canonical_bytes_sha256"] == sha(raw)
                    assert base64.b64decode(response["canonical_bytes_base64"], validate=True) == raw
                self.meta.setdefault("completed_recovery_phases", []).append(phase)
                self.save()
            finally:
                assert canonical(self.snapshot()) == canonical(baseline)
                self.installation = original
                assert canonical(self.snapshot()) == canonical(baseline)
        self.meta["status"] = "PASS_FINITE_" + self.args.case.upper()
        self.meta["recovery_counts_per_phase"] = {"exact_statuses": len(self.commits), "exact_documents": len(reads)}
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["ledger", "publication-helper", "terms-helper", "supplier-helper", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--case", choices=["invocation", "members"], required=True)
    parser.set_defaults(resume=False, deadline_seconds=0, pending_finalize_check=False)
    args = parser.parse_args()
    journey = FiniteLifecycle(args)
    try:
        journey.setup()
        if args.case == "invocation":
            journey.invocation_sequence()
        else:
            journey.members_sequence()
        journey.recover()
    except Exception as error:
        journey.meta["status"] = "STOPPED_FIRST_SUBSTANTIVE_BLOCKER"
        journey.meta["error"] = repr(error)
        raise
    finally:
        if journey.can_snapshot():
            journey.meta["final_snapshot"] = journey.snapshot()
            journey.meta["final_snapshot_sha256"] = sha(canonical(journey.meta["final_snapshot"]))
            journey.meta["final_configuration_sha256"] = sha((journey.installation / "billing.json").read_bytes())
        journey.save()


if __name__ == "__main__":
    main()
