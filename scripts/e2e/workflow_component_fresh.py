#!/usr/bin/env python3
"""One bounded synthetic component/family history using the public SDK CLI.

Starts from an explicitly supplied quiescent installation copy. Every existing
key is read-only, and each new request is submitted at most once per invocation.
Source, assent and financial inputs require the packet's independent review.
"""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_supported import Journey, REPO, canonical, now, reference, sha, unique

FIXTURES = REPO / "examples/workflow-component-corrected"


class ComponentJourney(Journey):
    fixture_directory = FIXTURES
    outcome_claim_schema = "ledger-workflow-outcome-claim/2"

    def __init__(self, args):
        super().__init__(args)
        self.baseline = args.baseline.resolve()
        self.attempted = set()
        self.committed = {}
        self.operations = []
        self.actor = self.source
        self.grants = {}
        self.enrollments = {}
        config = json.loads((self.baseline / "billing.json").read_bytes())
        assert config["scope"] == self.scope
        self.store = config["store_id"]
        if not self.installation.exists():
            shutil.copytree(self.baseline, self.installation, copy_function=shutil.copy2)
        self.original = self.installation
        self.installation = self.baseline
        self.baseline_snapshot = self.snapshot()
        self.installation = self.original
        retained = self.retained_operations()
        self.prefix = retained[:37]
        for row in retained:
            self.committed[canonical(row["receipt"]["key"])] = row["receipt"]
        assert len(self.prefix) == 37, "This packet starts only from the preserved main37 baseline"
        if args.resume_configuration:
            checkpoint = json.loads((self.inputs / "configuration.checkpoint.json").read_bytes())
            assert canonical(checkpoint["snapshot"]) == canonical(self.snapshot()), "Configuration changed since checkpoint"
            assert checkpoint["retained"] == retained, "Configuration receipts changed since checkpoint"
            for name, value in checkpoint["state"].items():
                setattr(self, name, value)
            for row in retained[-1:]:
                result = self.status("resume-" + str(row["sequence"]), row["receipt"]["key"])
                assert result["status"] == "committed" and result["receipt"] == row["receipt"]
        elif args.resume_partial:
            assert self.prefix == self.meta["protected_prefix"]
            for row in retained[37:]:
                key = row["receipt"]["key"]
                result = self.status("partial-resume-" + str(row["sequence"]), key)
                assert result["status"] == "committed" and result["receipt"] == row["receipt"]
                name = key["operation_id"].removeprefix("component-")
                saved = json.loads((self.root / (name + ".committed.json")).read_bytes())
                assert saved["receipt"] == result["receipt"] and saved["request_evidence"] == result["request_evidence"]
        self.meta["protected_prefix"] = self.prefix
        self.meta["baseline"] = str(self.baseline)
        self.save()

    def snapshot(self):
        result = super().snapshot()
        result["billing_configuration_sha256"] = sha((self.installation / "billing.json").read_bytes())
        return result

    def retained_operations(self):
        with self.connection() as db:
            return [{"sequence": seq, "receipt": json.loads(receipt),
                     "receipt_sha256": sha(receipt), "operation_sha256": sha(operation)}
                    for seq, receipt, operation in db.execute(
                        "SELECT commit_sequence,receipt_bytes,operation_bytes FROM workflow_operations ORDER BY commit_sequence")]

    def key(self, operation):
        return {"logical_store_id": self.store, "scope": self.scope,
                "source": self.actor, "operation_id": "component-fresh-" + operation}

    def workflow(self, name, *args, **kwargs):
        if args and args[0] == "submit":
            request = json.loads(Path(args[1]).read_bytes())
            identity = canonical(request["key"])
            assert identity not in self.committed, "Committed keys are STATUS-only"
            assert identity not in self.attempted, "No automatic request retry"
            self.attempted.add(identity)
        return super().workflow(name, *args, **kwargs)

    def status(self, name, key):
        before = self.snapshot()
        result = self.workflow(name + "-status", "status", self.write(name + ".key.json", key))
        assert self.snapshot() == before, "STATUS appended state"
        return result

    def submit(self, name, kind, payload):
        request = self.root / (name + ".request.json")
        proposed = canonical({"schema": "ledger-operation/1", "contract": "workflow/1",
                              "kind": kind, "key": self.key(name), "payload": payload})
        if request.exists():
            assert request.read_bytes() == proposed, "A saved request is immutable"
        else:
            request.write_bytes(proposed)
        key = json.loads(proposed)["key"]
        saved = self.root / (name + ".committed.json")
        status = self.status(name + "-before", key)
        if status["status"] == "committed":
            assert saved.exists(), "Unexpected committed identity requires explicit reconciliation"
            result = json.loads(saved.read_bytes())
            assert status["receipt"] == result["receipt"]
            assert status["request_evidence"] == result["request_evidence"]
            self.committed[canonical(key)] = status["receipt"]
            return result
        assert status["status"] == "not_observed" and status["key"] == key
        assert canonical(key) not in self.committed
        before = self.snapshot()
        self.write(name + ".before.json", before)
        try:
            result = self.workflow(name, "submit", request)
        except Exception:
            self.preserve_failure(name, key)
            raise
        assert result["schema"] == "ledger-operation-result/1" and result["status"] == "committed"
        assert result["receipt"]["key"] == key
        assert result["receipt"]["original_request_bytes_sha256"] == sha(proposed)
        saved.write_bytes(canonical(result))
        self.committed[canonical(key)] = result["receipt"]
        self.operations.append(name)
        checkpoint_path = self.inputs / "configuration.checkpoint.json"
        if checkpoint_path.exists():
            checkpoint = json.loads(checkpoint_path.read_bytes())
            checkpoint["snapshot"] = self.snapshot()
            checkpoint["retained"] = self.retained_operations()
            checkpoint_path.write_bytes(canonical(checkpoint))
        return result

    def preserve_failure(self, name, key):
        failure = {"key": key, "raw_sha256": sha((self.root / (name + ".request.json")).read_bytes())}
        try:
            failure["status"] = self.status(name + "-failure", key)
        except Exception as error:
            failure["status_unavailable"] = type(error).__name__
        try:
            self.write(name + ".after-failure.json", self.snapshot())
        finally:
            backup = self.root / (name + "-failed-quiescent-installation")
            if not backup.exists():
                shutil.copytree(self.installation, backup, copy_function=shutil.copy2)
            self.write(name + ".failure.json", failure)

    def as_source(self, actor, name, kind, payload):
        old = self.actor
        self.actor = self.business["sources"].get(actor, actor)
        try:
            saved = self.root / (name + ".request.json")
            if canonical(self.key(name)) in self.committed:
                prior = json.loads(saved.read_bytes())
                assert prior["key"] == self.key(name) and prior["kind"] == kind
                payload = prior["payload"]
            return self.submit(name, kind, payload)
        finally:
            self.actor = old

    def accepted_grants(self):
        return sorted({canonical(v): v for v in self.grants.values()}.values(), key=canonical)

    def setup(self):
        self.workflow("enable-component-copy", "enable")
        # The existing installation's operator limits are preserved verbatim.
        self.workflow("component-resources-observe", "resources", "show")
        evidence = self.evidence("customer-evidence", "ledger-workflow-evidence-retain/1",
            {"customer": self.customer}, {"acceptor": self.operator}, "customer_registration")
        customer = unique(self.submit("customer-register", "authority.change", {
            "schema": "ledger-workflow-customer-register/1", "action": "customer.register",
            "expected_revision": "0", "customer": self.customer, "acceptor": self.operator,
            "registration_evidence_ref": evidence}), "workflow-customer")
        for label, ceiling in self.business["ceilings"].items():
            evidence = self.evidence(label + "-source-evidence", "ledger-workflow-evidence-retain/1",
                {"customer": self.customer}, {"source": ceiling["source"], "principal_id": self.operator},
                "source_enrollment")
            payload = {"schema": "ledger-workflow-source-enroll/3" if label == "d" else "ledger-workflow-source-enroll/2",
                "action": "source.enroll", "expected_revision": "0", "enrollment_id": "component-source-" + label,
                "customer_refs": [customer], "ceiling": ceiling, "evidence_ref": evidence}
            if label == "d":
                payload["namespace_owner_enrollment_ref"] = self.enrollments["b"]
            result = self.submit(label + "-source-enroll", "authority.change", payload)
            self.grants[label] = unique(result, "workflow-source-grant")
            self.enrollments[label] = unique(result, "workflow-source-enrollment")
        self.grants["c"] = self.grants["c2"]
        self.grants["f"] = self.grants["g"] = self.grants["a2"]
        self.setup_publication()
        with self.connection() as db:
            assert db.execute("SELECT COUNT(*) FROM workflow_financial_heads").fetchone()[0] == 0
            for kind in ["workflow-work", "workflow-action", "workflow-decision", "workflow-target",
                         "workflow-family-state", "workflow-chain-state", "workflow-link", "workflow-invocation"]:
                assert db.execute("SELECT COUNT(*) FROM workflow_documents WHERE kind=?", (kind,)).fetchone()[0] == 0
        self.meta["configuration_zero_financial_state"] = True
        state = {name: getattr(self, name) for name in ["grants", "enrollments", "plan", "product_ref",
                 "policy_ref", "policy", "fact_refs", "binding_ref", "binding"]}
        self.write("configuration.checkpoint.json", {"state": state, "snapshot": self.snapshot(),
                   "retained": self.retained_operations()})
        self.save()

    def write_source_documents(self):
        names = {"component:context": "context-facts.json", "component:work-facts": "work-facts.json",
                 "component:assessment-facts": "outcome-facts.json", "component:report-facts": "report-facts.json"}
        native = {"ledger-workflow-product/1": "product.json", "ledger-workflow-slot-template/1": "slot-template.json",
                  "ledger-workflow-policy/4": "policy.json"}
        written = set()
        for document in self.source_documents():
            name = names.get(document.get("schema_id"), native.get(document["schema"]))
            assert name is not None and name not in written
            written.add(name)
            self.write(name, document)
        assert written == set(names.values()) | set(native.values())

    def setup_publication(self):
        self.write_source_documents()
        pub_path = self.inputs / "publication.request.json"
        self.command("prepare-component-publication", [self.args.publication_helper, self.inputs,
            self.write("publication.key.json", self.key("publication-corrected-v2")), pub_path, "--operator-resources"])
        payload = json.loads(pub_path.read_bytes())["payload"]
        result = self.submit("publication-corrected-v2", "policy.publish", payload)
        publication = unique(result, "workflow-publication")
        self.plan = self.document(unique(result, "workflow-compiled-plan"))
        def selected(kind):
            refs = [r for r in self.plan["source_refs"] if r["kind"] == kind]
            assert len(refs) == 1
            return refs[0]
        self.product_ref, self.policy_ref = selected("workflow-product"), selected("workflow-policy")
        self.policy = self.document(self.policy_ref)
        self.fact_refs = {self.document(r)["schema_id"]: r for r in self.plan["source_refs"]
                          if r["kind"] == "workflow-fact-schema"}
        role, catalog = copy.deepcopy(self.business["role"]), copy.deepcopy(self.business["catalog"])
        role_ref, catalog_ref = reference("workflow-roles", role), reference("workflow-slot-catalog", catalog)
        instantiation = {"schema": "ledger-workflow-slot-instantiation/1", "customer": self.customer,
            "agreement_id": self.agreement, "revision": "1", "catalog_ref": catalog_ref,
            "template_ref": selected("workflow-slot-template"), "component_map": self.business["component_map"],
            "family_map": self.business["family_map"]}
        terms = [{**copy.deepcopy(t), "role_ref": role_ref, "source_grant_refs": self.accepted_grants()}
                 for t in self.business["terms"]]
        binding = {**copy.deepcopy(self.business["binding"]),
            "accepted_context": {"fact_schema_ref": self.fact_refs["component:context"], "facts": {}},
            "bindings": [{"slot": t["slot"], "binding_ref": reference("workflow-slot-terms", t)} for t in terms],
            "roles_refs": [role_ref], "slot_catalog_ref": catalog_ref,
            "slot_instantiation_ref": reference("workflow-slot-instantiation", instantiation)}
        raw_terms = {"schema": "ledger-workflow-acceptance-terms/2", "scope": self.scope,
            "customer": self.customer, "agreement_id": self.agreement, "version": "1",
            "publication_ref": publication, "binding_inputs": binding}
        self.command("prepare-component-terms", [self.args.terms_helper, self.inputs / "context-facts.json",
            self.write("acceptance-terms.raw.json", raw_terms), self.inputs, pub_path])
        normalized = json.loads((self.inputs / "acceptance-terms.json").read_bytes())
        identity = json.loads((self.inputs / "acceptance-terms.identity.json").read_bytes())
        digest = identity["terms_digest"]
        at = self.meta.setdefault("acceptance_time_corrected", now())
        self.save()
        subject = {"customer": self.customer, "agreement_id": self.agreement}
        evidence = self.evidence("corrected-customer-assent-evidence", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "accepted_at": at}, "assent")
        attestation = self.evidence("corrected-customer-assent-attestation", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "assent_evidence_ref": evidence,
             "statement": "operator_verified_exact_terms_acceptance"}, "operator_attestation")
        assent = {"schema": "ledger-workflow-assent/1", "scope": self.scope, **subject,
            "terms_digest": digest, "mode": "real", "acceptor": self.operator, "accepted_at": at,
            "evidence_ref": evidence, "operator": self.operator, "operator_attestation_ref": attestation}
        assents = [reference("workflow-assent", assent)]
        binding = {**normalized["binding_inputs"], "assent_refs": assents, "authority_refs": self.accepted_grants()}
        accepted = self.submit("acceptance-corrected", "agreement.accept", {
            "schema": "ledger-workflow-agreement-accept/9", "agreement_id": self.agreement,
            "expected_revision": "0", "publication_ref": publication, "slot_catalog_ref": catalog_ref,
            "slot_instantiation_ref": binding["slot_instantiation_ref"], "binding_inputs": binding,
            "assent_refs": assents, "authority_refs": self.accepted_grants(),
            "documents": [role, catalog, instantiation, *terms, assent]})
        self.binding_ref = self.document(unique(accepted, "workflow-accepted-agreement"))["binding_ref"]
        self.binding = self.document(self.binding_ref)
        self.submit("activation-corrected", "agreement.activate", {
            "schema": "ledger-workflow-activation/1", "agreement_id": self.agreement,
            "expected_revision": "0", "accepted_binding_ref": self.binding_ref, "evidence_refs": [],
            "policy_ref": self.policy_ref, "product_ref": self.product_ref,
            "fact_schema_refs": self.binding["fact_schema_refs"], "compiled_plan_ref": self.binding["compiled_plan_ref"],
            "effective": {"kind": "at_commit"}})

    def refusal(self, actor, name, kind, payload, code):
        old = self.actor
        self.actor = self.business["sources"].get(actor, actor)
        try:
            request = {"schema": "ledger-operation/1", "contract": "workflow/1",
                       "kind": kind, "key": self.key(name), "payload": payload}
            path = self.root / (name + ".request.json")
            completed = any(row.get("check") == name and row.get("code") == code and row.get("no_append")
                            for row in self.meta["checks"])
            if completed:
                prior = json.loads(path.read_bytes())
                assert prior["key"] == request["key"] and prior["kind"] == kind
                request["payload"] = prior["payload"]
                assert path.read_bytes() == canonical(request), "Completed refusal request changed"
                status = self.status(name + "-resume", request["key"])
                assert status["status"] == "not_observed" and status["key"] == request["key"]
                assert json.loads((self.inputs / (name + ".before.json")).read_bytes()) == json.loads((self.inputs / (name + ".after.json")).read_bytes())
                return
            if path.exists():
                assert path.read_bytes() == canonical(request), "Saved refusal request changed"
                failure = json.loads((self.inputs / (name + ".failure.json")).read_bytes())
                assert failure["status"]["status"] == "not_observed" and failure["key"] == request["key"]
            else:
                path.write_bytes(canonical(request))
            status = self.status(name + "-before", request["key"])
            assert status["status"] == "not_observed"
            before = self.snapshot()
            self.write(name + ".before.json", before)
            try:
                response = self.workflow(name, "submit", path, expected=4 if code == "IDENTITY_CONFLICT" else 3)
                after = self.snapshot()
                self.write(name + ".after.json", after)
                assert response["code"] == code, (name, response, code)
                assert after == before
                status = self.status(name + "-after", request["key"])
                assert status["status"] == "not_observed" and status["key"] == request["key"]
                self.meta["checks"].append({"check": name, "code": code, "no_append": True})
                self.save()
            except Exception:
                self.preserve_failure(name, request["key"])
                raise
        finally:
            self.actor = old

    def observation(self):
        saved = self.root / "observation.request.json"
        issued = json.loads(saved.read_bytes())["payload"]["attestation"]["issued_at"] if saved.exists() else now()
        subject = {"kind": "work", "logical_store_id": self.store, "scope": self.scope,
            "canonical_source_namespace": self.business["sources"]["b"], "business_operation_id": "component-fresh-A",
            "customer": self.customer, "activity": "support:complete", "product_ref": self.product_ref,
            "binding_ref": self.binding_ref}
        result = self.as_source("observer", "observation", "evidence.retain", {
            "schema": "ledger-workflow-cost-observation-attest/1", "attestation": {
                "schema": "ledger-workflow-cost-observation-evidence/1", "attestation_id": "component-fresh-observation",
                "source": self.business["sources"]["observer"], "evidence_kind": "cost_observation",
                "evidence_slot": "support:observer", "subject": subject,
                "amount": {"currency": "USD", "scale": 18, "atoms": "120000000000000000"},
                "final": True, "issued_at": issued, "supporting_evidence_refs": []}})
        return sorted([unique(result, "workflow-fact-attestation"),
                       unique(result, "workflow-evidence-verification")], key=canonical)

    def work(self, label, actor, activity, *, links=None, evidence=None, quantity=None):
        report = activity in ["support:report-b", "support:report-c"]
        saved_request = self.root / (label + "-work.request.json")
        occurred_at = json.loads(saved_request.read_bytes())["payload"]["occurred_at"] if saved_request.exists() else now()
        payload = {"schema": "ledger-workflow-work/1", "external_event_id": "component-fresh-" + label + "-delivery",
            "business_operation_id": "component-fresh-" + label, "customer": self.customer, "chain": "component-fresh-chain",
            "activity": activity, "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["component:report-facts" if report else "component:work-facts"],
            "binding_ref": self.binding_ref, "status": "succeeded", "facts": {} if report else {"support:quantity": quantity},
            "fact_proofs": [], "links": links or [], "evidence_refs": evidence or [], "invocation_refs": [],
            "occurred_at": occurred_at}
        result = self.as_source(actor, label + "-work", "work.accept", payload)
        work = {"work_ref": unique(result, "workflow-work"), "result": result,
                "root_decision_ref": unique(result, "workflow-decision"), "payload": payload}
        targets = [r for r in result["receipt"]["result_refs"] if r["kind"] == "workflow-target"]
        assert len(targets) <= 1
        if targets:
            work["target_ref"] = targets[0]
        if report:
            assert not {"workflow-action", "workflow-target", "workflow-family-state", "workflow-dependency"} & {
                r["kind"] for r in result["receipt"]["result_refs"]}
        return work

    def outcome_payload(self, work, family, code, label, start, *, corrector=False):
        activity, slot = "support:assess-" + family, "support:family-" + family
        token = "component-fresh-outcome-" + family
        occurred = self.meta.setdefault("outcome_times", {}).setdefault(family, now())
        subject = {"kind": "outcome", "logical_store_id": self.store, "scope": self.scope,
            "target_ref": work["target_ref"], "family_slot": slot, "business_outcome_token": token,
            "activity": activity, "binding_ref": self.binding_ref}
        proofs = []
        for field, value, purpose in [("support:code", code, "outcome"), ("support:occurred", occurred, "occurrence")]:
            name = label + "-" + purpose
            saved = self.root / (name + ".request.json")
            issued = json.loads(saved.read_bytes())["payload"]["attestation"]["issued_at"] if saved.exists() else now()
            result = self.as_source(family, name, "evidence.retain", {
                "schema": "ledger-workflow-fact-attest/1", "attestation": {
                    "schema": "ledger-workflow-fact-attestation/1", "attestation_id": "component-fresh-" + name,
                    "source": self.business["sources"][family], "evidence_kind": purpose, "subject": subject,
                    "subject_schema_ref": self.fact_refs["component:assessment-facts"],
                    "claims": [{"field": field, "value": value}], "issued_at": issued, "supporting_evidence_refs": []}})
            proofs.append({"field": field, "attestation_ref": unique(result, "workflow-fact-attestation")})
        return {"schema": "ledger-workflow-outcome/2", "target_ref": work["target_ref"], "family_slot": slot,
            "business_outcome_token": token, "activity": activity, "product_ref": self.product_ref,
            "policy_ref": self.policy_ref, "fact_schema_ref": self.fact_refs["component:assessment-facts"],
            "binding_ref": self.binding_ref, "selector_start_work_ref": start["work_ref"],
            "facts": {"support:code": code, "support:occurred": occurred}, "fact_proofs": proofs,
            "evidence_refs": [], "occurred_at": occurred}

    def closure_inputs(self, work):
        root_id = self.document(work["root_decision_ref"])["decision_id"]
        revision, _ = self.head("decision", root_id)
        families, dependents = [], []
        if "target_ref" in work:
            target = work["target_ref"]
            for label in ["f", "g"]:
                rev, ref = self.head("family", target["kind"], target["id"], target["digest"], "support:family-" + label)
                family = self.document(ref)
                if family["state"] == "closed_by_root":
                    continue
                families.append({"family_state_ref": ref, "expected_revision": rev,
                                 "authority_evidence_refs": [self.grants["c"]]})
                if "claim_ref" in family:
                    decision_id = self.document(family["decision_ref"])["decision_id"]
                    drev, dref = self.head("decision", decision_id)
                    assert dref == family["decision_ref"]
                    if self.document(dref)["status"] != "reversed":
                        dependents.append({"decision_ref": dref, "expected_revision": drev,
                                           "authority_evidence_refs": [self.grants["c"]]})
        return {"work_ref": work["work_ref"], "expected_revision": revision,
            "dependent_reversals": sorted(dependents, key=canonical), "family_closures": sorted(families, key=canonical),
            "evidence_refs": [self.grants["c"]]}

    def quantity(self, work):
        return {"schema": "ledger-workflow-quantity-correction/2", **self.closure_inputs(work),
                "field": "support:quantity", "replacement_quantity": "1", "fact_proofs": []}

    def amount(self, label, expected_retail, expected_observation):
        previous = [row for row in self.meta["checks"] if row.get("check") == "component_immutable_posting_oracle" and row.get("phase") == label]
        if previous:
            assert len(previous) == 1 and previous[0]["retail_atoms"] == str(expected_retail)
            assert previous[0]["informational_atoms"] == str(expected_observation)
            return
        with self.connection() as db:
            decisions = [json.loads(body) for body, in db.execute(
                "SELECT canonical_bytes FROM workflow_documents WHERE kind='workflow-decision'")]
        actions = {}
        for decision in decisions:
            if decision.get("original_terms_ref") != self.binding_ref:
                continue
            for ref in decision["actions_appended"]:
                key = canonical(ref)
                assert key not in actions
                action = self.document(ref)
                assert action["decision_id"] == decision["decision_id"] and action["revision"] == decision["revision"]
                assert action["amount"]["currency"] == "USD" and action["amount"]["scale"] == 18
                actions[key] = action
        totals = {book: sum(int(a["amount"]["atoms"]) for a in actions.values() if a["book"] == book)
                  for book in ["retail", "cost_observation"]}
        assert totals == {"retail": expected_retail, "cost_observation": expected_observation}, (label, totals)
        assert all(a["book"] in totals for a in actions.values())
        for action in actions.values():
            if "inverse_of" in action:
                original = actions[canonical(action["inverse_of"])]
                assert int(action["amount"]["atoms"]) == -int(original["amount"]["atoms"])
                for field in ["basis_refs", "funding_leg", "slot_terms_ref", "role_ref", "component_slot", "root_component_slot", "book"]:
                    assert action[field] == original[field]
        self.meta["checks"].append({"check": "component_immutable_posting_oracle", "phase": label,
            "retail_atoms": str(expected_retail), "informational_atoms": str(expected_observation), "actions": list(actions.values())})
        self.save()

    def recover(self, names):
        before = self.snapshot()
        for name in names:
            request = json.loads((self.root / (name + ".request.json")).read_bytes())
            expected = json.loads((self.root / (name + ".committed.json")).read_bytes())
            status = self.status(name + "-recovery", request["key"])
            assert status["status"] == "committed" and status["receipt"] == expected["receipt"]
            assert status["request_evidence"] == expected["request_evidence"]
        assert self.snapshot() == before

    def sequence(self):
        atoms = 10**18
        tool = self.work("Tool", "d", "support:tool", quantity="2")
        tool_alias = {**copy.deepcopy(tool["payload"]), "external_event_id": "component-fresh-Tool-alias-B"}
        before_alias = self.snapshot()
        original_results = tool["result"]["receipt"]["result_refs"]
        original_bodies = {canonical(ref): canonical(self.document(ref)) for ref in original_results}
        alias_key = {**self.key("Tool-alias-B"), "source": self.business["sources"]["b"]}
        alias_already_committed = canonical(alias_key) in self.committed
        result = self.as_source("b", "Tool-alias-B", "work.accept", tool_alias)
        assert result["receipt"]["result_kind"] == "semantic_alias"
        assert result["receipt"]["result_refs"] == original_results
        after_alias = self.snapshot()
        assert after_alias["heads"] == before_alias["heads"]
        assert len(after_alias["operations"]) == len(before_alias["operations"]) + (0 if alias_already_committed else 1)
        assert {canonical(ref): canonical(self.document(ref)) for ref in original_results} == original_bodies
        observation = self.observation()
        work = self.work("A", "b", "support:complete", evidence=observation, quantity="2")
        assert "support:optional-approved" not in work["payload"]["facts"]
        evaluation = self.document(self.document(work["root_decision_ref"])["evaluation_ref"])
        optional = [step for step in evaluation["steps"] if step["component_slot"] == "support:optional"]
        assert len(optional) == 1 and optional[0]["predicate"] == "unknown"
        assert optional[0]["disposition"] == "predicate_not_true" and "action_ref" not in optional[0]
        self.meta["checks"].append({"check": "optional_Missing_is_unknown_without_Action", "step": optional[0]})
        self.amount("A_and_Tool", 130 * atoms // 100, 12 * atoms // 100)
        b = self.work("B", "b", "support:report-b")
        c = self.work("C", "b", "support:report-c", links=[{"slot": "support:reported-b", "predecessor": b["work_ref"]}])
        first_f = self.outcome_payload(work, "f", "support:premium25", "F-original", c)
        self.refusal("f", "F-wait-before-late-link", "outcome.submit", first_f, "MISSING_DEPENDENCY")
        chain_revision, _ = self.head("chain", self.customer, "component-fresh-chain")
        saved_link = self.root / "B-late-link.request.json"
        if saved_link.exists():
            chain_revision = json.loads(saved_link.read_bytes())["payload"]["expected_chain_revision"]
        link = {"schema": "ledger-workflow-link-assert/1", "child": b["work_ref"],
            "product_ref": self.product_ref, "binding_ref": self.binding_ref,
            "link": {"slot": "support:reported-a", "predecessor": work["work_ref"]},
            "expected_chain_revision": chain_revision, "evidence_refs": [self.grants["b"]]}
        linked = self.as_source("b", "B-late-link", "link.assert", link)
        original_link = unique(linked, "workflow-link")
        assert linked["receipt"]["result_kind"] == "administrative"
        chain_before = self.head("chain", self.customer, "component-fresh-chain")
        link["expected_chain_revision"] = chain_before[0]
        saved_alias = self.root / "B-late-link-alias.request.json"
        if saved_alias.exists():
            link["expected_chain_revision"] = json.loads(saved_alias.read_bytes())["payload"]["expected_chain_revision"]
        aliased = self.as_source("b", "B-late-link-alias", "link.assert", link)
        assert unique(aliased, "workflow-link") == original_link
        assert self.head("chain", self.customer, "component-fresh-chain") == chain_before
        assert not {"workflow-chain-state", "workflow-correction-capacity", "workflow-action", "workflow-target"} & {
            ref["kind"] for ref in aliased["receipt"]["result_refs"]}
        f = self.as_source("f", "F-original-v2", "outcome.submit", first_f)
        g_payload = self.outcome_payload(work, "g", "support:none", "G-original-none", c)
        g = self.as_source("g", "G-original-none", "outcome.submit", g_payload)
        assert not any(ref["kind"] == "workflow-action" for ref in g["receipt"]["result_refs"])
        assert not any(ref["kind"] == "workflow-target" for ref in f["receipt"]["result_refs"] + g["receipt"]["result_refs"])
        omitted = self.quantity(work)
        omitted["family_closures"] = [row for row in omitted["family_closures"]
            if self.document(row["family_state_ref"])["family_slot"] != "support:family-g"]
        self.refusal("c", "omitted-NONE-family-closure", "quantity.correct", omitted, "INTEGRITY_FAILURE")
        g_claim = unique(g, "workflow-outcome-claim")
        g_decision = self.document(unique(g, "workflow-decision"))["decision_id"]
        revision, _ = self.head("decision", g_decision)
        self.as_source("c", "G-discount10", "outcome.correct", {
            "schema": "ledger-workflow-outcome-correction/2", "original_claim_ref": g_claim,
            "expected_revision": revision, "replacement": self.outcome_payload(work, "g", "support:discount10", "G-discount10", c, corrector=True)})
        self.amount("initial_1.42", 142 * atoms // 100, 12 * atoms // 100)
        alternate = self.work("C-alternate", "b", "support:report-c", links=[{"slot": "support:reported-b", "predecessor": b["work_ref"]}])
        changed = {**copy.deepcopy(first_f), "selector_start_work_ref": alternate["work_ref"]}
        self.meta["changed_path_control_pending"] = True  # Preserved failed request; complete only after reviewed fix.
        self.save()
        self.as_source("c", "C-reverse-unpriced-report", "work.reverse", {
            "schema": "ledger-workflow-work-reverse/2", **self.closure_inputs(c),
            "reason": "Reverse only unpriced report eligibility; original selector remains immutable"})
        f_claim = unique(f, "workflow-outcome-claim")
        f_decision = self.document(unique(f, "workflow-decision"))["decision_id"]
        revision, _ = self.head("decision", f_decision)
        target = work["target_ref"]
        prior_g_checks = [row for row in self.meta["checks"] if row.get("check") == "G_fullRef_and_Decision_unchanged_by_F_correction"]
        if not prior_g_checks:
            g_before = self.head("family", target["kind"], target["id"], target["digest"], "support:family-g")
            g_body = self.document(g_before[1])
            g_decision_before = self.head("decision", self.document(g_body["decision_ref"])["decision_id"])
        self.as_source("c", "F-premium10", "outcome.correct", {
            "schema": "ledger-workflow-outcome-correction/2", "original_claim_ref": f_claim,
            "expected_revision": revision, "replacement": self.outcome_payload(work, "f", "support:premium10", "F-premium10", c, corrector=True)})
        if not prior_g_checks:
            assert self.head("family", target["kind"], target["id"], target["digest"], "support:family-g") == g_before
            assert self.document(g_before[1]) == g_body
            assert self.head("decision", self.document(g_body["decision_ref"])["decision_id"]) == g_decision_before
            self.meta["checks"].append({"check": "G_fullRef_and_Decision_unchanged_by_F_correction", "family": g_before, "decision": g_decision_before})
            self.save()
        else:
            assert len(prior_g_checks) == 1
        self.amount("F_1.30_original_path", 130 * atoms // 100, 12 * atoms // 100)
        corrected = self.as_source("c", "A-quantity1", "quantity.correct", self.quantity(work))
        assert not any(ref["kind"] == "workflow-target" for ref in corrected["receipt"]["result_refs"])
        self.amount("A_quantity1_0.90", 90 * atoms // 100, 12 * atoms // 100)
        for family in ["support:family-f", "support:family-g"]:
            target = work["target_ref"]
            _, ref = self.head("family", target["kind"], target["id"], target["digest"], family)
            assert self.document(ref)["state"] == "closed_by_root"
        self.as_source("c", "A-reverse", "work.reverse", {
            "schema": "ledger-workflow-work-reverse/2", **self.closure_inputs(work),
            "reason": "Exact remaining original retail and nonpayable observation inverse"})
        self.amount("A_reversed_0.30", 30 * atoms // 100, 0)
        rebirth = {**copy.deepcopy(work["payload"]), "external_event_id": "component-fresh-A-changed-delivery",
                   "facts": {"support:quantity": "3"}}
        self.refusal("b", "closed-A-no-Target-rebirth", "work.accept", rebirth, "IDENTITY_CONFLICT")
        self.as_source("c", "Tool-original-D-quantity1", "quantity.correct", self.quantity(tool))
        self.amount("D_original_rate_postfix_0.15", 15 * atoms // 100, 0)
        selected = ["acceptance-corrected", "activation-corrected", "observation", "Tool-work", "Tool-alias-B", "B-late-link",
                    "B-late-link-alias", "F-original-v2", "G-original-none", "G-discount10", "F-premium10",
                    "A-quantity1", "A-reverse", "Tool-original-D-quantity1"]
        self.recover(selected)
        fixture = self.root / "restored-complete-installation"
        shutil.copytree(self.installation, fixture, copy_function=shutil.copy2)
        active = self.installation
        self.installation = fixture
        try:
            self.recover(selected)
        finally:
            self.installation = active
        self.meta["status"] = "component_financial_and_status_reopen_restore_complete_owning_fold_pending"
        self.save()

    def assert_preserved(self):
        assert self.retained_operations()[:len(self.prefix)] == self.prefix
        installation = self.installation
        try:
            self.installation = self.baseline
            assert self.snapshot() == self.baseline_snapshot, "Original main37 changed"
        finally:
            self.installation = installation
        self.meta["original_main37_unchanged"] = True
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--configuration-only", action="store_true")
    parser.add_argument("--resume-configuration", action="store_true")
    parser.add_argument("--resume-partial", action="store_true")
    args = parser.parse_args()
    args.resume, args.deadline_seconds = args.resume_configuration or args.resume_partial, None
    assert not (args.configuration_only and args.resume_configuration)
    assert not (args.resume_configuration and args.resume_partial)
    journey = ComponentJourney(args)
    try:
        if not args.resume_configuration:
            journey.setup()
        if args.configuration_only:
            journey.meta["status"] = "configuration_complete_financial_story_not_executed"
        else:
            journey.sequence()
    finally:
        journey.assert_preserved()
        journey.save()


if __name__ == "__main__":
    main()
