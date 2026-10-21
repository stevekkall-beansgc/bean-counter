#!/usr/bin/env python3
"""One public synthetic local workflow journey; no SQL writes or live charges.

Build the CLI with scripts/build_workflow.py --local-execution
--operator-resources, and build the two existing prepare_local_* examples from
that captured source. Pass those executables explicitly. Every CLI request,
response and actual argv is retained. Positive submits may resolve one exact
UNAVAILABLE result through original-key status and at most one identical retry.
Every other failure stops this invocation; --resume
is an explicit operator action and never changes an accepted request.
"""
from pathlib import Path
from contextlib import contextmanager
import argparse, base64, copy, datetime as dt, hashlib, json, shutil, sqlite3, subprocess, sys, time

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / "examples/workflow-supported"
ATOMS = 10**18
MAIN_OPERATIONS = ["main-work", "main-finalize", "main-success", "main-none", "main-discount", "main-cost-correct", "main-reverse"]

def source_commit():
    # Evidence metadata only; it grants no workflow authority.
    if (REPO / ".git").exists():
        return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    info = REPO / "BUILD-INFO.txt"
    if info.is_file():
        return dict(line.split("=", 1) for line in info.read_text().splitlines() if "=" in line)["source_commit"]
    captured = REPO.parent / "build-inputs.json"
    if captured.is_file():
        return json.loads(captured.read_bytes())["source"]["base_commit"]
    raise RuntimeError("Missing source checkout or package/capture build identity")

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
            "source_commit": source_commit(),
            "installation": str(self.installation), "commands": [], "checks": []}
        # Pin each actual executable used across an explicit resumed run;
        # the initial binary field remains the original observation.
        self.executable_hashes = {}
        for command in self.meta["commands"]:
            executable = Path(command["argv"][0])
            if executable.is_file():
                digest = self.executable_hashes.setdefault(str(executable), sha(executable.read_bytes()))
                command.setdefault("executable_sha256", digest)
        build = self.binary.parents[3] if len(self.binary.parents) > 3 else self.binary.parent
        execution = {"path": str(self.binary), "sha256": sha(self.binary.read_bytes()),
                     "runner_commit": source_commit(),
                     "fixtures": {name: sha((FIXTURES / name).read_bytes()) for name in ["source.json", "business.json"]}}
        package_info = self.binary.parent / "BUILD-INFO.txt"
        if package_info.is_file():
            fields = dict(line.split("=", 1) for line in package_info.read_text().splitlines() if "=" in line)
            execution["binary_source_commit"] = fields["source_commit"]
            execution["binary_build_info_sha256"] = sha(package_info.read_bytes())
        for name in ["inventory.json", "build-inputs.json", "qualified.json"]:
            if (build / name).is_file():
                execution[name] = {"path": str(build / name), "sha256": sha((build / name).read_bytes())}
        if execution not in self.meta.setdefault("executions", []):
            self.meta["executions"].append(execution)
        self.save()

    def save(self):
        self.log.write_text(json.dumps(self.meta, indent=2) + "\n")

    def write(self, name, value):
        path = self.inputs / name
        path.write_bytes(canonical(value))
        return path

    def command(self, name, argv, expected=0, allow_unavailable=False):
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
        executable = Path(argv[0])
        if executable.is_file():
            entry["executable_sha256"] = self.executable_hashes.setdefault(str(executable), sha(executable.read_bytes()))
        self.meta["commands"].append(entry)
        self.save()
        print(name, result.returncode, flush=True)
        if result.returncode != expected:
            if allow_unavailable and expected == 0 and result.returncode == 7:
                value = json.loads(result.stdout)
                if (isinstance(value, dict) and value.get("schema") == "ledger-cli/1"
                        and value.get("status") == "error" and value.get("code") == "UNAVAILABLE"):
                    return value
            raise RuntimeError(f"{name}: exit {result.returncode}, expected {expected}: {result.stdout.decode(errors='replace')}")
        return json.loads(result.stdout) if result.stdout.strip() else {}

    def workflow(self, name, *args, expected=0, allow_unavailable=False):
        return self.command(name, [self.binary, "workflow", "--directory", self.installation, *args, "--json"],
                            expected, allow_unavailable)

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
        original_bytes = request.read_bytes()
        result = self.workflow(name, "submit", request, allow_unavailable=True)
        if self.meta["commands"][-1]["exit"] == 7:
            key = json.loads(original_bytes)["key"]
            key_path = self.write(name + ".availability-key.json", key)
            before = self.snapshot()
            (self.root / (name + ".availability-before.json")).write_bytes(canonical(before))
            status = self.workflow(name + "-availability-status", "status", key_path)
            after = self.snapshot()
            (self.root / (name + ".availability-after.json")).write_bytes(canonical(after))
            assert after == before, "original status changed logical state"
            assert isinstance(status, dict) and status.get("schema") == "ledger-operation-status/1", status
            assert request.read_bytes() == original_bytes, "saved request changed during availability recovery"
            if status.get("status") == "committed":
                assert set(status) == {"schema", "status", "receipt", "request_evidence"}, status
                result = dict(status, schema="ledger-operation-result/1")
                recovery = "original_committed_receipt"
            else:
                assert set(status) == {"schema", "status", "key"} and status["status"] == "not_observed", status
                assert status["key"] == key, status
                # This second submit is deliberately strict: another failure stops.
                result = self.workflow(name + "-availability-retry", "submit", request)
                recovery = "one_identical_submit_after_explicit_absence"
            assert result.get("schema") == "ledger-operation-result/1" and result.get("status") == "committed", result
            assert result["receipt"]["key"] == key, result
            assert result["receipt"]["original_request_bytes_sha256"] == sha(original_bytes), result
            self.meta["checks"].append({"check": "bounded_original_identity_availability_recovery", "operation": name,
                                       "recovery": recovery, "status_no_append": True,
                                       "original_request_bytes_sha256": sha(original_bytes)})
            self.save()
        saved.write_bytes(canonical(result))
        return result

    @contextmanager
    def connection(self):
        # sqlite3.Connection.__exit__ ends a transaction but does not close the
        # reader. Bound every readonly reader to this observation, including
        # failures, before the next separate-process workflow command.
        connection = sqlite3.connect("file:" + str(self.installation / ".ledger/local.db") + "?mode=ro", uri=True)
        try:
            yield connection
        finally:
            connection.close()

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
            tables = {}
            for name, in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"):
                cursor = db.execute('SELECT * FROM "' + name.replace('"', '""') + '"')
                rows = [canonical([{"bytes": len(v), "sha256": sha(v)} if isinstance(v, bytes) else v for v in row]) for row in cursor]
                tables[name] = {"columns": [c[0] for c in cursor.description], "rows": len(rows),
                                "sha256": sha(canonical(sorted(sha(row) for row in rows)))}
        return {"operations": operations, "documents": documents, "heads": heads, "logical_tables": tables}

    def total(self, prefix, expected):
        # Select immutable postings through the original Work's canonical
        # subjects and Decision.actions_appended, never operation-name prefixes.
        request = json.loads((self.root / (prefix + "-work.request.json")).read_bytes())
        receipt = json.loads((self.root / (prefix + "-work.committed.json")).read_bytes())["receipt"]
        assert receipt["key"] == request["key"]
        assert receipt["original_request_bytes_sha256"] == sha((self.root / (prefix + "-work.request.json")).read_bytes())
        work = unique({"receipt": receipt}, "workflow-work")
        binding = request["payload"]["binding_ref"]
        key = request["key"]
        def same_scope(value):
            return value["logical_store_id"] == key["logical_store_id"] and value["scope"] == key["scope"]
        with self.connection() as db:
            row = db.execute("SELECT receipt_bytes FROM workflow_operations WHERE logical_store_id=? AND tenant=? AND environment=? AND source=? AND operation_id=?",
                             (key["logical_store_id"], *key["scope"], key["source"], key["operation_id"])).fetchone()
            assert row and json.loads(row[0]) == receipt, "original Work receipt association"
            decisions = [json.loads(b) for b, in db.execute("SELECT canonical_bytes FROM workflow_documents WHERE kind='workflow-decision' ORDER BY id")]
        selected = {}
        for decision in decisions:
            subject = decision["subject_ref"]
            if subject["kind"] == "workflow-work":
                subject_work = subject
            elif subject["kind"] == "workflow-outcome-claim":
                claim = self.document(subject)
                assert claim["schema"] == "ledger-workflow-outcome-claim/1"
                target = self.document(claim["target_ref"])
                assert target["schema"] == "ledger-workflow-component-target/1"
                subject_work = target["work_ref"]
                if subject_work == work:
                    assert target["binding_ref"] == binding and claim["original_payload"]["binding_ref"] == binding
                    assert claim["original_payload"]["target_ref"] == claim["target_ref"]
                    assert target["scope"] == claim["scope"] == key["scope"]
            elif subject["kind"] == "workflow-cost":
                claim = self.document(subject)
                assert claim["schema"] == "ledger-workflow-app-cost-claim/1", "unsupported cost subject"
                subject_work = claim["subject"]["original_work_ref"]
                if subject_work == work:
                    assert claim["subject"]["original_binding_ref"] == binding and same_scope(claim["subject"])
            else:
                raise AssertionError("unsupported decision subject " + subject["kind"])
            if subject_work != work:
                continue
            assert decision["original_terms_ref"] == binding and same_scope(decision["operation_key"])
            for ref in decision["actions_appended"]:
                assert ref["kind"] == "workflow-action"
                action = self.document(ref)
                assert action["decision_id"] == decision["decision_id"] and action["revision"] == decision["revision"]
                assert same_scope(action["operation_key"])
                identity = canonical(ref).decode()
                assert identity not in selected, "action appended by multiple decisions"
                selected[identity] = action
        actions = list(selected.values())
        assert actions and all(v["book"] == "retail" and v["amount"]["currency"] == "USD" and v["amount"]["scale"] == 18 for v in actions)
        for action in actions:
            if "inverse_of" in action:
                original = selected[canonical(action["inverse_of"]).decode()]
                assert int(action["amount"]["atoms"]) == -int(original["amount"]["atoms"])
                assert action["decision_id"] == original["decision_id"]
                for field in ["basis_refs", "funding_leg", "slot_terms_ref", "role_ref", "component_slot", "root_component_slot", "book"]:
                    assert action[field] == original[field], ("inverse changed", field)
        total = sum(int(v["amount"]["atoms"]) for v in actions)
        assert total == expected, (prefix, total, expected)
        self.meta["checks"].append({"check": "immutable_retail_sum", "prefix": prefix, "work_ref": work,
                                    "atoms": str(total), "actions": actions})
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
        if not any(c["name"] == "enable" and c["exit"] == 0 for c in self.meta["commands"]):
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

    def app_cost(self, w, finalize=True):
        prefix = w["prefix"]
        observation = self.submit(prefix + "-observation", "evidence.retain", {"schema": "ledger-workflow-app-observe/1",
            "attestation": {"schema": "ledger-workflow-app-observation/1", "attestation_id": prefix + "-observation", "source": self.source,
                "evidence_kind": "app_usage", "subject": self.app_subject(w), "usage_field": "support:quantity", "quantity": "20",
                "unit": "support:request", "observed_at": self.document(w["work_ref"])["payload"]["occurred_at"],
                "provider_key": {"provider": "synthetic-provider", "account": "synthetic-account", "request_id": prefix + "-provider-call"},
                "material": material("Synthetic same-call usage")}})
        w["observation"] = proof(observation)
        final = self.final_evidence(w, 1, 2 * ATOMS)
        w["final_evidence"] = final
        if not finalize:
            return
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

    def main_total(self, operation, expected):
        # Explicit resume may already include later immutable actions. Earlier
        # totals remain in RUN.json; check only the latest completed milestone.
        order = MAIN_OPERATIONS
        if not any((self.root / (name + ".committed.json")).exists() for name in order[order.index(operation) + 1:]):
            self.total("main", expected)

    def main_sequence(self):
        w = self.work("main")
        self.main_total("main-work", ATOMS)
        self.app_cost(w)
        self.main_total("main-finalize", 3 * ATOMS)
        outcome = self.submit("main-success", "outcome.submit", self.outcome_payload(w, "support:success", "success"))
        self.main_total("main-success", 325 * ATOMS // 100)
        w["outcome_claim_ref"] = unique(outcome, "workflow-outcome-claim")
        w["outcome_decision_id"] = self.document(unique(outcome, "workflow-decision"))["decision_id"]
        revision, _ = self.head("decision", w["outcome_decision_id"])
        self.submit("main-none", "outcome.correct", {"schema": "ledger-workflow-outcome-correction/1",
            "original_claim_ref": w["outcome_claim_ref"], "expected_revision": revision,
            "replacement": {"kind": "none", "evidence_refs": [], "reason": "Synthetic correction to no adjustment"}})
        self.main_total("main-none", 3 * ATOMS)
        revision, _ = self.head("decision", w["outcome_decision_id"])
        self.submit("main-discount", "outcome.correct", {"schema": "ledger-workflow-outcome-correction/1",
            "original_claim_ref": w["outcome_claim_ref"], "expected_revision": revision,
            "replacement": self.outcome_payload(w, "support:discount", "discount")})
        self.main_total("main-discount", 280 * ATOMS // 100)
        revision, _ = self.head("app_cost", w["cost_claim_ref"]["kind"], w["cost_claim_ref"]["id"], w["cost_claim_ref"]["digest"])
        saved_correction = self.root / "main-cost-correct.committed.json"
        final = (json.loads((self.root / "main-cost-correct.request.json").read_bytes())["payload"]["replacement_final_cost_proof"]
                 if saved_correction.exists() else self.final_evidence(w, int(revision) + 1, 160 * ATOMS // 100))
        self.submit("main-cost-correct", "cost.correct", {"schema": "ledger-workflow-cost-correct/2",
            "original_cost_claim_ref": w["cost_claim_ref"], "expected_revision": revision,
            "replacement_final_cost_proof": final, "dependent_reversals": [], "evidence_refs": []})
        self.main_total("main-cost-correct", 240 * ATOMS // 100)
        fixture = self.root / "pre-reverse-fixture"
        if not fixture.exists() and not (self.root / "main-reverse.committed.json").exists():
            fixture.mkdir(mode=0o700)
            shutil.copytree(self.installation, fixture / "installation", copy_function=shutil.copy2)
            shutil.copytree(self.inputs, fixture / "inputs", copy_function=shutil.copy2)
            for pattern in ["*.request.json", "*.committed.json"]:
                for path in self.root.glob(pattern):
                    shutil.copy2(path, fixture / path.name)
            shutil.copy2(self.log, fixture / "PARENT-RUN.json")
        self.reverse(w, "main-reverse")
        self.main_total("main-reverse", 0)
        self.reopen_retry(MAIN_OPERATIONS)
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
            decision = self.document(cost_ref)["current_decision_ref"]
            body = self.document(decision)
            rev, ref = self.head("decision", body["decision_id"])
            dependents.append({"decision_ref": ref, "expected_revision": rev, "authority_evidence_refs": [self.grant]})
        return self.submit(name, "work.reverse", {"schema": "ledger-workflow-work-reverse/2", "work_ref": w["work_ref"],
            "expected_revision": revision, "reason": "Synthetic authorized complete causal reversal", "evidence_refs": [],
            "dependent_reversals": sorted(dependents, key=canonical), "family_closures": [{"family_state_ref": family_ref,
                "expected_revision": family_revision, "authority_evidence_refs": [self.grant]}]})

    def pending_reverse(self):
        w = self.work("pending")
        if not (self.root / "pending-reverse.committed.json").exists():
            self.total("pending", ATOMS)
        if self.args.pending_finalize_check:
            self.app_cost(w, finalize=False)
        self.reverse(w, "pending-reverse")
        self.total("pending", 0)
        if self.args.pending_finalize_check:
            before = self.snapshot()
            request = self.write("pending-post-close-finalize.request.json", {"schema": "ledger-operation/1", "contract": "workflow/1",
                "kind": "cost.finalize", "key": self.key("pending-post-close-finalize"), "payload": {
                    "schema": "ledger-workflow-cost-finalize/2", "original_work_ref": w["work_ref"], "component_slot": "support:model-retail",
                    "observation_proof": w["observation"], "final_cost_proof": w["final_evidence"], "evidence_refs": []}})
            response = self.workflow("pending-post-close-finalize", "submit", request, expected=3)
            assert response["code"] == "TARGET_CLOSED", response
            assert self.snapshot() == before
            self.meta["checks"].append({"check": "N7-unconsumed-app-proof-after-root-reversal", "code": response["code"], "no_append": True})
        self.reopen_retry(["pending-work", "pending-reverse"])

    def close_family(self, deadline=False):
        prefix = "deadline" if deadline else "no-success"
        w = self.work(prefix)
        revision, family_ref = self.head("family", w["target_ref"]["kind"], w["target_ref"]["id"], w["target_ref"]["digest"], "support:quality")
        payload = {"schema": "ledger-workflow-outcome-close/2", "target_ref": w["target_ref"],
                   "family_slot": "support:quality", "expected_revision": revision,
                   "reason": "deadline" if deadline else "no_success", "evidence_refs": []}
        if deadline:
            accepted_by = self.document(family_ref)["window"]["accepted_by"]
            cutoff = dt.datetime.fromisoformat(accepted_by.replace("Z", "+00:00"))
            wait = max(0, (cutoff - dt.datetime.now(dt.timezone.utc)).total_seconds()) + .05
            assert wait <= 30, "deadline case requires its separately accepted short window"
            time.sleep(wait)
            self.meta["checks"].append({"check": "actual_deadline_wait", "accepted_by": accepted_by, "after_wait": now()})
            self.save()
        else:
            result = self.submit(prefix + "-proof", "evidence.retain", {"schema": "ledger-workflow-family-close-attest/1",
                "attestation": {"schema": "ledger-workflow-family-close-attestation/1", "attestation_id": prefix + "-proof",
                    "source": self.source, "evidence_kind": "outcome", "purpose": "family_no_success", "assertion": "no_success",
                    "subject": {"logical_store_id": self.store, "scope": self.scope, "original_binding_ref": self.binding_ref,
                        "target_ref": w["target_ref"], "family_slot": "support:quality", "expected_family_revision": revision},
                    "issued_at": now(), "supporting_evidence_refs": []}})
            payload["close_proof"] = proof(result)
        self.submit(prefix + "-close", "outcome.close", payload)
        self.total(prefix, ATOMS)
        self.reopen_retry([prefix + "-work", prefix + "-close"])

    def reopen_retry(self, operations):
        before = self.snapshot()
        (self.root / "status-retry-before.json").write_bytes(canonical(before))
        for name in operations:
            key = self.write(name + ".key.json", self.key(name))
            original = json.loads((self.root / (name + ".committed.json")).read_bytes())["receipt"]
            assert original["key"] == self.key(name)
            assert original["original_request_bytes_sha256"] == sha((self.root / (name + ".request.json")).read_bytes())
            status = self.workflow(name + "-status", "status", key)
            retried = self.workflow(name + "-retry", "submit", self.root / (name + ".request.json"))
            assert status["receipt"] == original == retried["receipt"]
        after = self.snapshot()
        (self.root / "status-retry-after.json").write_bytes(canonical(after))
        assert after == before, "status or original retry appended or changed data"
        self.meta["checks"].append({"check": "public_separate_process_status_and_retry", "operations": operations, "no_append": True})
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--publication-helper", type=Path, required=True)
    parser.add_argument("--terms-helper", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--case", choices=["main", "pending", "no-success", "deadline", "restore", "verify-main"], default="main")
    parser.add_argument("--deadline-seconds", type=int, default=0)
    parser.add_argument("--pending-finalize-check", action="store_true")
    parser.add_argument("--verify-from", type=Path, help="verify saved main reconciliation/status/retries only, without new financial work")
    parser.add_argument("--restore-from", type=Path)
    parser.add_argument("--restore-operations", nargs="+")
    args = parser.parse_args()
    if args.pending_finalize_check and args.case != "pending":
        parser.error("--pending-finalize-check requires the pending case")
    if args.case == "deadline":
        args.deadline_seconds = args.deadline_seconds or 5
    elif args.deadline_seconds:
        parser.error("--deadline-seconds is only valid for the separate deadline case")
    if args.case == "restore":
        if not args.restore_from or not args.restore_operations or args.output.exists():
            parser.error("restore requires --restore-from, --restore-operations and a new output")
        # The orchestrator has closed every source process. Copy the WHOLE
        # installation, including any quiescent WAL, and verify stable bytes.
        source_files = [args.restore_from / "installation/.ledger" / n for n in ["local.db", "local.db-wal"]]
        source_hashes = {str(p): sha(p.read_bytes()) for p in source_files if p.exists()}
        args.output.mkdir(mode=0o700, parents=True)
        shutil.copytree(args.restore_from / "installation", args.output / "installation", copy_function=shutil.copy2)
        assert source_hashes == {str(p): sha(p.read_bytes()) for p in source_files if p.exists()}, "source changed during copy"
        for name in args.restore_operations:
            for suffix in [".request.json", ".committed.json"]:
                shutil.copy2(args.restore_from / (name + suffix), args.output / (name + suffix))
        args.resume = True
    elif args.restore_from or args.restore_operations:
        parser.error("restore arguments require --case restore")
    if args.case == "verify-main":
        if not args.verify_from or not args.resume or args.output.exists():
            parser.error("verify-main requires --verify-from, --resume and a new output directory")
        args.output.mkdir(mode=0o700, parents=True)
        for name in MAIN_OPERATIONS:
            for suffix in [".request.json", ".committed.json"]:
                shutil.copy2(args.verify_from / (name + suffix), args.output / (name + suffix))
    elif args.verify_from:
        parser.error("--verify-from requires --case verify-main")
    journey = Journey(args)
    try:
        if args.case == "verify-main":
            original = json.loads((args.output / "main-work.request.json").read_bytes())
            journey.store, journey.scope, journey.source = (original["key"][k] for k in ["logical_store_id", "scope", "source"])
            journey.installation = (args.verify_from / "installation").resolve()
            journey.meta["installation"] = str(journey.installation)
            journey.meta["original_run"] = {"path": str((args.verify_from / "RUN.json").resolve()),
                                            "sha256": sha((args.verify_from / "RUN.json").read_bytes())}
            journey.total("main", 0)
            journey.reopen_retry(MAIN_OPERATIONS)
            journey.meta["status"] = "saved_main_reconciliation_and_retry_complete"
            journey.save()
            return
        if args.case == "restore":
            original = json.loads((args.output / (args.restore_operations[0] + ".request.json")).read_bytes())
            journey.store, journey.scope, journey.source = (original["key"][k] for k in ["logical_store_id", "scope", "source"])
            restored = journey.snapshot()
            copied = journey.installation
            journey.installation = args.restore_from / "installation"
            assert journey.snapshot() == restored, "whole-installation copy differs from quiescent source"
            journey.installation = copied
            journey.reopen_retry(args.restore_operations)
            journey.meta["restore_source"] = str(args.restore_from.resolve())
            journey.meta["restore_source_files"] = source_hashes
            journey.meta["status"] = "whole_installation_restore_complete"
            journey.save()
            return
        journey.setup()
        if args.case == "main":
            journey.main_sequence()
        elif args.case == "pending":
            journey.pending_reverse()
        else:
            journey.close_family(deadline=args.case == "deadline")
        journey.meta["status"] = args.case + "_sequence_complete"
        journey.save()
    except Exception as error:
        journey.meta["status"] = "stopped"
        journey.meta["error"] = str(error)
        journey.save()
        raise

if __name__ == "__main__":
    main()
