#!/usr/bin/env python3
"""One public synthetic local workflow journey; no SQL writes or live charges.

Build the CLI with scripts/build_workflow.py --local-execution
--operator-resources, and build the two existing prepare_local_* examples from
that captured source. Pass those executables explicitly. Every CLI request,
response and actual argv is retained. A failure stops this invocation; --resume
is an explicit operator action and never changes an accepted request.
"""
from pathlib import Path
import argparse, base64, copy, datetime as dt, hashlib, json, sqlite3, subprocess, sys

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / "examples/workflow-supported"
ATOMS = 10**18

def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()

def now():
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def reference(kind, body):
    digest = sha(b"bean-counter/workflow-document/1\0" + canonical([body["schema"], body]))
    return {"kind": kind, "id": "wdoc_" + digest, "digest": "sha256:" + digest}

def replace(value, before, after):
    if value == before:
        return after
    if isinstance(value, dict):
        return {k: replace(v, before, after) for k, v in value.items()}
    if isinstance(value, list):
        return [replace(v, before, after) for v in value]
    return value

def unique(result, kind):
    rows = [r for r in result["receipt"]["result_refs"] if r["kind"] == kind]
    assert len(rows) == 1, (kind, rows)
    return rows[0]

def proof(result):
    return {"attestation_ref": unique(result, "workflow-fact-attestation"),
            "verification_ref": unique(result, "workflow-evidence-verification")}

def material(text):
    raw = text.encode()
    return {"media_type": "text/plain;charset=utf-8", "content_base64": base64.b64encode(raw).decode(),
            "content_sha256": sha(raw)}

class Journey:
    def __init__(self, args):
        self.args = args
        self.root = args.output.resolve()
        if self.root.exists() and not args.resume:
            raise SystemExit("Output exists; use a new directory or explicitly --resume the same installation")
        self.root.mkdir(parents=True, exist_ok=True)
        self.installation = self.root / "installation"
        self.inputs = self.root / "inputs"
        self.inputs.mkdir(exist_ok=True)
        self.business = json.loads((FIXTURES / "business.json").read_bytes())
        self.operator, self.source = self.business["operator"], self.business["source"]
        self.customer, self.agreement = self.business["customer"], self.business["agreement"]
        self.scope = self.business["scope"]
        self.store = "workflow-example-" + sha(str(self.root).encode())[:16]
        self.binary = args.ledger.resolve()
        self.log = self.root / "RUN.json"
        self.meta = json.loads(self.log.read_bytes()) if self.log.exists() else {
            "classification": "Public synthetic example; no live customer, invoice or payment",
            "binary": str(self.binary), "binary_sha256": sha(self.binary.read_bytes()),
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
            "installation": str(self.installation), "commands": [], "checks": []}
        self.save()

    def save(self):
        self.log.write_text(json.dumps(self.meta, indent=2) + "\n")

    def write(self, name, value):
        path = self.inputs / name
        path.write_bytes(canonical(value))
        return path

    def command(self, name, argv, expected=0):
        index = len(self.meta["commands"])
        stem = self.root / (f"{index:03d}-" + name)
        started = now()
        result = subprocess.run([str(v) for v in argv], capture_output=True, timeout=120)
        stem.with_suffix(".stdout").write_bytes(result.stdout)
        stem.with_suffix(".stderr").write_bytes(result.stderr)
        entry = {"name": name, "argv": [str(v) for v in argv], "started_at": started,
                 "completed_at": now(), "exit": result.returncode,
                 "stdout": stem.with_suffix(".stdout").name, "stdout_sha256": sha(result.stdout),
                 "stderr": stem.with_suffix(".stderr").name, "stderr_sha256": sha(result.stderr)}
        self.meta["commands"].append(entry)
        self.save()
        print(name, result.returncode, flush=True)
        if result.returncode != expected:
            raise RuntimeError(f"{name}: exit {result.returncode}, expected {expected}: {result.stdout.decode(errors='replace')}")
        return json.loads(result.stdout) if result.stdout.strip() else {}

    def workflow(self, name, *args, expected=0):
        return self.command(name, [self.binary, "workflow", "--directory", self.installation, *args, "--json"], expected)

    def key(self, operation):
        return {"logical_store_id": self.store, "scope": self.scope, "source": self.source, "operation_id": operation}

    def submit(self, name, kind, payload):
        request = self.root / (name + ".request.json")
        if not request.exists():
            request.write_bytes(canonical({"schema": "ledger-operation/1", "contract": "workflow/1",
                                           "kind": kind, "key": self.key(name), "payload": payload}))
        saved = self.root / (name + ".committed.json")
        if saved.exists():
            return json.loads(saved.read_bytes())
        result = self.workflow(name, "submit", request)
        saved.write_bytes(canonical(result))
        return result

    def connection(self):
        return sqlite3.connect("file:" + str(self.installation / ".ledger/local.db") + "?mode=ro", uri=True)

    def document(self, ref):
        with self.connection() as db:
            row = db.execute("SELECT canonical_bytes FROM workflow_documents WHERE kind=? AND id=? AND digest=?",
                             (ref["kind"], ref["id"], ref["digest"])).fetchone()
        assert row is not None, ref
        raw = bytes(row[0])
        (self.root / (ref["id"] + ".json")).write_bytes(raw)
        return json.loads(raw)

    def head(self, kind, *keys):
        keys = list(keys) + [""] * (4 - len(keys))
        with self.connection() as db:
            row = db.execute("SELECT revision,record_kind,record_id,record_digest FROM workflow_financial_heads "
                             "WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? "
                             "AND key_a=? AND key_b=? AND key_c=? AND key_d=?",
                             (self.store, *self.scope, kind, *keys)).fetchone()
        assert row is not None, (kind, keys)
        return str(row[0]), {"kind": row[1], "id": row[2], "digest": row[3]}

    def snapshot(self):
        with self.connection() as db:
            operations = [(i, q, sha(r), sha(o)) for i, q, r, o in db.execute(
                "SELECT operation_id,commit_sequence,receipt_bytes,operation_bytes FROM workflow_operations ORDER BY commit_sequence")]
            documents = [(i, sha(b)) for i, b in db.execute("SELECT id,canonical_bytes FROM workflow_documents ORDER BY id")]
            heads = db.execute("SELECT * FROM workflow_financial_heads ORDER BY head_kind,key_a,key_b,key_c,key_d").fetchall()
        return {"operations": operations, "documents": documents, "heads": heads}

    def total(self, prefix, expected):
        with self.connection() as db:
            actions = [json.loads(b) for b, in db.execute("SELECT canonical_bytes FROM workflow_documents WHERE kind='workflow-action' ORDER BY id")]
        selected = [v for v in actions if v["operation_key"]["operation_id"].startswith(prefix + "-")]
        assert all(v["book"] == "retail" for v in selected), "unexpected supplier/AP action"
        total = sum(int(v["amount"]["atoms"]) for v in selected)
        assert total == expected, (prefix, total, expected)
        self.meta["checks"].append({"check": "immutable_retail_sum", "prefix": prefix, "atoms": str(total),
                                    "actions": selected})
        self.save()

    def evidence(self, name, schema, subject, metadata, purpose=None):
        payload = {"schema": schema, "subject": subject, "metadata": metadata,
                   **material("Synthetic local workflow example: " + name)}
        if purpose:
            payload["purpose"] = purpose
        return unique(self.submit(name, "evidence.retain", payload), "workflow-evidence")

    def setup(self):
        setup = json.loads((REPO / "examples/billing/setup.json").read_bytes())
        for old, new in [(setup["store_id"], self.store), (setup["operator"], self.operator),
                         (setup["source"], self.source), (setup["customer"], self.customer)]:
            setup = replace(setup, old, new)
        setup.update({"scope": self.scope, "host": self.business["platform"], "price": "1", "accepted_at": now(),
                      "acceptor": self.operator, "assent_evidence": "Synthetic bootstrap only; actual workflow fullterms assent is retained separately.",
                      "operator_attestation": "Synthetic example operator; no live customer authorization.",
                      "finality_attestation": "No work is submitted under this legacy bootstrap agreement."})
        if not self.installation.exists():
            self.command("initialize", [self.binary, "billing", "init", self.installation, "--setup", self.write("setup.json", setup), "--json"])
        self.workflow("enable", "enable")
        resources = {"schema": "ledger-local-resource-settings/1", **{k: None for k in [
            "semantic_work", "compile_work", "resolved_records", "resolved_bytes", "new_body_bytes", "arena_bytes",
            "retained_operations", "retained_bytes", "attempt_timeout_ms"]}}
        self.workflow("resources-set", "resources", "set", self.write("resources.json", resources))
        self.workflow("resources-show", "resources", "show")
        ev = self.evidence("customer-evidence", "ledger-workflow-evidence-retain/1", {"customer": self.customer},
                           {"acceptor": self.operator}, "customer_registration")
        customer = unique(self.submit("customer-register", "authority.change", {"schema": "ledger-workflow-customer-register/1",
            "action": "customer.register", "expected_revision": "0", "customer": self.customer,
            "acceptor": self.operator, "registration_evidence_ref": ev}), "workflow-customer")
        ev = self.evidence("source-evidence", "ledger-workflow-evidence-retain/1", {"customer": self.customer},
                           {"source": self.source, "principal_id": self.operator}, "source_enrollment")
        enrollment = self.submit("source-enroll", "authority.change", {"schema": "ledger-workflow-source-enroll/1",
            "action": "source.enroll", "expected_revision": "0", "enrollment_id": "example-workflow-source",
            "customer_refs": [customer], "ceiling": self.business["source_ceiling"], "evidence_ref": ev})
        self.grant = unique(enrollment, "workflow-source-grant")
        self.enrollment = unique(enrollment, "workflow-source-enrollment")
        psub = {"customer": self.customer, "agreement_id": self.agreement, "party": self.business["platform"],
                "acceptor": "synthetic-platform-representative"}
        roles = ["bearer", "payer", "provider"]
        authorization = {**psub, "scope": self.scope, "authorization_id": "example-platform-authority", "roles": roles,
                         "starts_at": "2000-01-01T00:00:00.000000Z", "ends_at": "2099-01-01T00:00:00.000000Z"}
        ev = self.evidence("platform-authority-evidence", "ledger-workflow-party-evidence-retain/1", psub,
                           {"purpose": "party_authorization", "authorization": authorization})
        att = self.evidence("platform-authority-attestation", "ledger-workflow-party-evidence-retain/1", psub,
                            {"purpose": "party_operator_attestation", "statement": "operator_verified_party_acceptor_authority", "target_evidence_ref": ev})
        auth = unique(self.submit("platform-authorize", "authority.change", {"schema": "ledger-workflow-party-authorize/1",
            "expected_revision": "0", "authorization": authorization, "evidence_ref": ev, "operator_attestation_ref": att}), "workflow-authority")
        documents = json.loads((FIXTURES / "source.json").read_bytes())["documents"]
        for d in documents:
            if d["schema"] == "ledger-workflow-policy/4" and self.args.deadline_seconds:
                d["families"][0]["window"].update({"occurrence_us": str(self.args.deadline_seconds * 1_000_000),
                    "receipt_grace_us": "1000000", "acceptance_grace_us": "1000000"})
            name = {"support:context": "context-facts.json", "support:work-facts": "work-facts.json",
                    "support:assessment-facts": "outcome-facts.json"}.get(d.get("schema_id"))
            name = name or {"ledger-workflow-product/1": "product.json", "ledger-workflow-slot-template/1": "slot-template.json",
                            "ledger-workflow-policy/4": "policy.json"}.get(d["schema"])
            self.write(name, d)
        pub_input = self.inputs / "publication.request.json"
        self.command("prepare-publication", [self.args.publication_helper, self.inputs,
            self.write("publication.key.json", self.key("publication")), pub_input, "--operator-resources"])
        pub_request = json.loads(pub_input.read_bytes())
        publication = self.submit("publication", "policy.publish", pub_request["payload"])
        pub_ref = unique(publication, "workflow-publication")
        self.plan = self.document(unique(publication, "workflow-compiled-plan"))
        def source_ref(kind):
            refs = [r for r in self.plan["source_refs"] if r["kind"] == kind]
            assert len(refs) == 1, (kind, refs)
            return refs[0]
        self.product_ref, self.policy_ref = source_ref("workflow-product"), source_ref("workflow-policy")
        self.policy = self.document(self.policy_ref)
        self.fact_refs = {}
        for r in self.plan["source_refs"]:
            if r["kind"] == "workflow-fact-schema":
                self.fact_refs[self.document(r)["schema_id"]] = r
        role, catalog = self.business["role"], self.business["catalog"]
        role_ref, catalog_ref = reference("workflow-roles", role), reference("workflow-slot-catalog", catalog)
        instantiation = {"schema": "ledger-workflow-slot-instantiation/1", "customer": self.customer, "agreement_id": self.agreement,
            "revision": "1", "catalog_ref": catalog_ref, "template_ref": source_ref("workflow-slot-template"),
            "component_map": [{"slot_id": n, "template_key": n} for n in ["support:model-retail", "support:service"]],
            "family_map": [{"slot_id": "support:quality", "template_key": "support:quality"}]}
        terms = {**self.business["terms"], "source_grant_refs": [self.grant], "role_ref": role_ref,
                 "app_cost_terms": self.policy["app_delayed_costs"]}
        terms_ref = reference("workflow-slot-terms", terms)
        binding = {**self.business["binding"], "accepted_context": {"fact_schema_ref": self.fact_refs["support:context"], "facts": {}},
            "bindings": [{"slot": "support:retail", "binding_ref": terms_ref}], "roles_refs": [role_ref],
            "slot_catalog_ref": catalog_ref, "slot_instantiation_ref": reference("workflow-slot-instantiation", instantiation)}
        raw_terms = {"schema": "ledger-workflow-acceptance-terms/2", "scope": self.scope, "customer": self.customer,
            "agreement_id": self.agreement, "version": "1", "publication_ref": pub_ref, "binding_inputs": binding}
        self.command("prepare-terms", [self.args.terms_helper, self.inputs / "context-facts.json",
            self.write("acceptance-terms.raw.json", raw_terms), self.inputs])
        normalized = json.loads((self.inputs / "acceptance-terms.json").read_bytes())
        identity = json.loads((self.inputs / "acceptance-terms.identity.json").read_bytes())
        digest, accepted_at = identity["terms_digest"], now()
        subject = {"customer": self.customer, "agreement_id": self.agreement}
        ev = self.evidence("customer-assent-evidence", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "accepted_at": accepted_at}, "assent")
        att = self.evidence("customer-assent-attestation", "ledger-workflow-evidence-retain/1", subject,
            {"terms_digest": digest, "acceptor": self.operator, "assent_evidence_ref": ev,
             "statement": "operator_verified_exact_terms_acceptance"}, "operator_attestation")
        customer_assent = {"schema": "ledger-workflow-assent/1", "scope": self.scope, "customer": self.customer,
            "agreement_id": self.agreement, "terms_digest": digest, "mode": "real", "acceptor": self.operator,
            "accepted_at": accepted_at, "evidence_ref": ev, "operator": self.operator, "operator_attestation_ref": att}
        consent = {**psub, "scope": self.scope, "roles": roles, "authorization_ref": auth,
            "basis": {"kind": "acceptance_terms", "terms_ref": identity["reference"], "terms_digest": digest}, "accepted_at": accepted_at}
        ev = self.evidence("platform-assent-evidence", "ledger-workflow-party-evidence-retain/2", psub,
                           {"purpose": "party_consent", "consent": consent})
        att = self.evidence("platform-assent-attestation", "ledger-workflow-party-evidence-retain/2", psub,
            {"purpose": "party_operator_attestation", "target_evidence_ref": ev, "statement": "operator_verified_party_exact_consent"})
        platform_assent = {"schema": "ledger-workflow-party-assent/2", "consent": consent, "mode": "real", "evidence_ref": ev,
            "operator": self.operator, "operator_attestation_ref": att}
        assents = sorted([reference("workflow-assent", customer_assent), reference("workflow-assent", platform_assent)], key=canonical)
        binding = {**normalized["binding_inputs"], "assent_refs": assents, "authority_refs": sorted([self.grant, auth], key=canonical)}
        accepted = self.submit("acceptance", "agreement.accept", {"schema": "ledger-workflow-agreement-accept/9", "agreement_id": self.agreement,
            "expected_revision": "0", "publication_ref": pub_ref, "slot_catalog_ref": catalog_ref,
            "slot_instantiation_ref": binding["slot_instantiation_ref"], "binding_inputs": binding,
            "assent_refs": assents, "authority_refs": binding["authority_refs"],
            "documents": [role, terms, catalog, instantiation, customer_assent, platform_assent]})
        self.binding_ref = self.document(unique(accepted, "workflow-accepted-agreement"))["binding_ref"]
        self.binding = self.document(self.binding_ref)
        self.submit("activation", "agreement.activate", {"schema": "ledger-workflow-activation/1", "agreement_id": self.agreement,
            "expected_revision": "0", "accepted_binding_ref": self.binding_ref, "evidence_refs": [], "policy_ref": self.policy_ref,
            "product_ref": self.product_ref, "fact_schema_refs": self.binding["fact_schema_refs"], "compiled_plan_ref": self.binding["compiled_plan_ref"],
            "effective": {"kind": "at_commit"}})
        self.meta["setup_complete_before_work_at"] = now()
        self.save()

    def work(self, prefix):
        result = self.submit(prefix + "-work", "work.accept", {"schema": "ledger-workflow-work/1", "external_event_id": prefix + "-delivery",
            "business_operation_id": prefix + "-service", "customer": self.customer, "chain": prefix + "-app-chain",
            "activity": "support:complete", "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:work-facts"], "binding_ref": self.binding_ref, "status": "succeeded",
            "facts": {"support:quantity": "20"}, "fact_proofs": [], "links": [], "evidence_refs": [], "invocation_refs": [], "occurred_at": now()})
        return {"prefix": prefix, "receipt": result, "work_ref": unique(result, "workflow-work"),
                "target_ref": unique(result, "workflow-target"), "root_decision_ref": unique(result, "workflow-decision")}

    def app_subject(self, w):
        return {"logical_store_id": self.store, "scope": self.scope, "customer": self.customer, "agreement_id": self.agreement,
            "original_work_ref": w["work_ref"], "original_binding_ref": self.binding_ref,
            "component_slot": "support:model-retail", "component_key": "support:model-retail",
            "app_call": {"source_namespace": self.source, "call_id": w["prefix"] + "-call"}}

    def app_cost(self, w):
        prefix = w["prefix"]
        observation = self.submit(prefix + "-observation", "evidence.retain", {"schema": "ledger-workflow-app-observe/1",
            "attestation": {"schema": "ledger-workflow-app-observation/1", "attestation_id": prefix + "-observation", "source": self.source,
                "evidence_kind": "app_usage", "subject": self.app_subject(w), "usage_field": "support:quantity", "quantity": "20",
                "unit": "support:request", "observed_at": self.document(w["work_ref"])["payload"]["occurred_at"],
                "provider_key": {"provider": "synthetic-provider", "account": "synthetic-account", "request_id": prefix + "-provider-call"},
                "material": material("Synthetic same-call usage")}})
        w["observation"] = proof(observation)
        final = self.final_evidence(w, 1, 2 * ATOMS)
        result = self.submit(prefix + "-finalize", "cost.finalize", {"schema": "ledger-workflow-cost-finalize/2",
            "original_work_ref": w["work_ref"], "component_slot": "support:model-retail", "observation_proof": w["observation"],
            "final_cost_proof": final, "evidence_refs": []})
        w["cost_claim_ref"] = unique(result, "workflow-cost") if len([r for r in result['receipt']['result_refs'] if r['kind']=='workflow-cost']) == 1 else None
        # Select the exact claim from the emitted schema, not a largest/latest Ref.
        for r in result["receipt"]["result_refs"]:
            if r["kind"] == "workflow-cost" and self.document(r)["schema"] == "ledger-workflow-app-cost-claim/1":
                w["cost_claim_ref"] = r
        assert w["cost_claim_ref"]
        w["final_evidence"] = final
        w["cost_result"] = result

    def final_evidence(self, w, revision, atoms):
        attestation = {"schema": "ledger-workflow-app-final-cost/1", "attestation_id": w["prefix"] + f"-final-evidence-{revision}",
            "source": self.source, "evidence_kind": "app_final_cost", "subject": self.app_subject(w),
            "observation_ref": w["observation"]["attestation_ref"], "cost_revision": str(revision),
            "finality": "final_attributable_amount", "amount": {"currency": "USD", "scale": 18, "atoms": str(atoms)},
            "quantity_comparison": {"kind": "comparable", "quantity": "20", "unit": "support:request"},
            "issued_at": now(), "material": material("Synthetic exact same-call attributable final cost")}
        if revision > 1:
            attestation["prior_final_evidence_ref"] = w["final_evidence"]["attestation_ref"]
        return proof(self.submit(w["prefix"] + f"-final-evidence-{revision}", "evidence.retain",
            {"schema": "ledger-workflow-app-cost-attest/1", "attestation": attestation}))

    def outcome_payload(self, w, code, label):
        clock = self.root / (w["prefix"] + "-outcome-time.json")
        if not clock.exists():
            clock.write_bytes(canonical({"occurred_at": now()}))
        occurred = json.loads(clock.read_bytes())["occurred_at"]
        token = w["prefix"] + "-assessment"
        subject = {"kind": "outcome", "logical_store_id": self.store, "scope": self.scope, "target_ref": w["target_ref"],
            "family_slot": "support:quality", "business_outcome_token": token, "activity": "support:assess", "binding_ref": self.binding_ref}
        facts = []
        for field, value, purpose in [("support:code", code, "outcome"), ("support:occurred", occurred, "occurrence")]:
            name = w["prefix"] + "-" + label + "-" + purpose
            result = self.submit(name, "evidence.retain", {"schema": "ledger-workflow-fact-attest/1",
                "attestation": {"schema": "ledger-workflow-fact-attestation/1", "attestation_id": name, "source": self.source,
                    "evidence_kind": purpose, "subject": subject, "subject_schema_ref": self.fact_refs["support:assessment-facts"],
                    "claims": [{"field": field, "value": value}], "issued_at": now(), "supporting_evidence_refs": []}})
            facts.append({"field": field, "attestation_ref": proof(result)["attestation_ref"]})
        return {"schema": "ledger-workflow-outcome/1", "target_ref": w["target_ref"], "family_slot": "support:quality",
            "business_outcome_token": token, "activity": "support:assess", "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:assessment-facts"], "binding_ref": self.binding_ref,
            "facts": {"support:code": code, "support:occurred": occurred}, "fact_proofs": facts, "evidence_refs": [], "occurred_at": occurred}

    def main_sequence(self):
        w = self.work("main")
        self.total("main", ATOMS)
        self.app_cost(w)
        self.total("main", 3 * ATOMS)
        outcome = self.submit("main-success", "outcome.submit", self.outcome_payload(w, "support:success", "success"))
        self.total("main", 325 * ATOMS // 100)
        w["outcome_claim_ref"] = unique(outcome, "workflow-outcome-claim")
        w["outcome_decision_id"] = self.document(unique(outcome, "workflow-decision"))["decision_id"]
        revision, _ = self.head("decision", w["outcome_decision_id"])
        self.submit("main-none", "outcome.correct", {"schema": "ledger-workflow-outcome-correction/1",
            "original_claim_ref": w["outcome_claim_ref"], "expected_revision": revision,
            "replacement": {"kind": "none", "evidence_refs": [], "reason": "Synthetic correction to no adjustment"}})
        self.total("main", 3 * ATOMS)
        revision, _ = self.head("decision", w["outcome_decision_id"])
        self.submit("main-discount", "outcome.correct", {"schema": "ledger-workflow-outcome-correction/1",
            "original_claim_ref": w["outcome_claim_ref"], "expected_revision": revision,
            "replacement": self.outcome_payload(w, "support:discount", "discount")})
        self.total("main", 280 * ATOMS // 100)
        revision, _ = self.head("app_cost", w["cost_claim_ref"]["kind"], w["cost_claim_ref"]["id"], w["cost_claim_ref"]["digest"])
        final = self.final_evidence(w, int(revision) + 1, 160 * ATOMS // 100)
        self.submit("main-cost-correct", "cost.correct", {"schema": "ledger-workflow-cost-correct/2",
            "original_cost_claim_ref": w["cost_claim_ref"], "expected_revision": revision,
            "replacement_final_cost_proof": final, "dependent_reversals": [], "evidence_refs": []})
        self.total("main", 240 * ATOMS // 100)
        self.reverse(w, "main-reverse")
        self.total("main", 0)
        self.reopen_retry(["main-work", "main-finalize", "main-success", "main-none", "main-discount", "main-cost-correct", "main-reverse"])
        return w

    def reverse(self, w, name):
        root = self.document(w["root_decision_ref"])["decision_id"]
        revision, _ = self.head("decision", root)
        family_revision, family_ref = self.head("family", w["target_ref"]["kind"], w["target_ref"]["id"], w["target_ref"]["digest"], "support:quality")
        dependents = []
        if w.get("outcome_decision_id"):
            rev, ref = self.head("decision", w["outcome_decision_id"])
            dependents.append({"decision_ref": ref, "expected_revision": rev, "authority_evidence_refs": [self.grant]})
        if w.get("cost_claim_ref"):
            _, cost_ref = self.head("app_cost", w["cost_claim_ref"]["kind"], w["cost_claim_ref"]["id"], w["cost_claim_ref"]["digest"])
            decision = self.document(cost_ref)["decision_ref"]
            body = self.document(decision)
            rev, ref = self.head("decision", body["decision_id"])
            dependents.append({"decision_ref": ref, "expected_revision": rev, "authority_evidence_refs": [self.grant]})
        return self.submit(name, "work.reverse", {"schema": "ledger-workflow-work-reverse/2", "work_ref": w["work_ref"],
            "expected_revision": revision, "reason": "Synthetic authorized complete causal reversal", "evidence_refs": [],
            "dependent_reversals": sorted(dependents, key=canonical), "family_closures": [{"family_state_ref": family_ref,
                "expected_revision": family_revision, "authority_evidence_refs": [self.grant]}]})

    def reopen_retry(self, operations):
        before = self.snapshot()
        for name in operations:
            key = self.write(name + ".key.json", self.key(name))
            original = json.loads((self.root / (name + ".committed.json")).read_bytes())["receipt"]
            status = self.workflow(name + "-status", "status", key)
            retried = self.workflow(name + "-retry", "submit", self.root / (name + ".request.json"))
            assert status["receipt"] == original == retried["receipt"]
        assert self.snapshot() == before, "status or original retry appended or changed data"
        self.meta["checks"].append({"check": "public_separate_process_status_and_retry", "operations": operations, "no_append": True})
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--deadline-seconds", type=int, default=0)
    args = parser.parse_args()
    journey = Journey(args)
    try:
        journey.setup()
        journey.main_sequence()
        journey.meta["status"] = "main_sequence_complete"
        journey.save()
    except Exception as error:
        journey.meta["status"] = "stopped"
        journey.meta["error"] = str(error)
        journey.save()
        raise

if __name__ == "__main__":
    main()
