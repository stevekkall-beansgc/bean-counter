#!/usr/bin/env python3
"""One synthetic Stage consent/share history; stop on the first unexpected result.

All authority and consent are retained through actual public operations. The
commitment helper only derives proposed DATA; it cannot authorize or bill.
"""
import argparse
import base64
import copy
import json
import shutil
from pathlib import Path

from workflow_metered import MeteredJourney
from workflow_supported import ATOMS, Journey, REPO, canonical, material, now, reference, sha, unique
from workflow_work_lifecycle import LifecycleJourney


class StageJourney(MeteredJourney):
    fixture_directory = REPO / "examples/workflow-stage"
    reopen_retry = LifecycleJourney.reopen_retry

    def __init__(self, args):
        Journey.__init__(self, args)
        self.worker = self.business["worker_source"]
        self.alias = self.business["alias_source"]
        self.corrector = self.business["corrector_source"]
        self.closer = self.business["closer_source"]
        self.chain = "stage-chain"
        self.business_id = "stage-member"

    def key(self, operation):
        key = Journey.key(self, operation)
        if operation in {"work-occurrence", "member-work", "invocation-completed"}:
            key["source"] = self.worker
        elif operation == "shared-work-alias":
            key["source"] = self.alias
        elif operation in {"closure-occurrence", "closure-success", "stage-close"}:
            key["source"] = self.closer
        elif operation == "member-reverse":
            key["source"] = self.corrector
        return key

    def snapshot(self):
        value = Journey.snapshot(self)
        value["billing_configuration_sha256"] = sha((self.installation / "billing.json").read_bytes())
        return value

    def write_source_documents(self):
        names = {"support:stage-context": "context-facts.json",
                 "support:stage-work-facts": "work-facts.json",
                 "support:stage-close-facts": "outcome-facts.json"}
        for document in self.source_documents():
            name = names.get(document.get("schema_id")) or {
                "ledger-workflow-product/1": "product.json",
                "ledger-workflow-slot-template/1": "slot-template.json",
                "ledger-workflow-policy/4": "policy.json"}[document["schema"]]
            self.write(name, document)

    def accepted_grants(self):
        return sorted([self.grant, self.worker_grant, self.alias_grant,
                       self.corrector_grant, self.closer_grant], key=canonical)

    def setup_publication(self, auth, psub, roles):
        customer = unique(json.loads((self.root / "customer-register.committed.json").read_bytes()), "workflow-customer")
        for label in ["worker", "corrector", "closer", "alias"]:
            ceiling = self.business[label + "_ceiling"]
            ev = self.evidence(label + "-source-evidence", "ledger-workflow-evidence-retain/1",
                {"customer": self.customer}, {"source": ceiling["source"], "principal_id": self.operator}, "source_enrollment")
            payload = {"schema": "ledger-workflow-source-enroll/3" if label == "alias" else "ledger-workflow-source-enroll/2",
                "action": "source.enroll", "expected_revision": "0", "enrollment_id": "stage-" + label,
                "customer_refs": [customer], "ceiling": ceiling, "evidence_ref": ev}
            if label == "alias":
                payload["namespace_owner_enrollment_ref"] = self.worker_enrollment
            result = self.submit(label + "-enroll", "authority.change", payload)
            setattr(self, label + "_grant", unique(result, "workflow-source-grant"))
            setattr(self, label + "_enrollment", unique(result, "workflow-source-enrollment"))
        self.write_source_documents()
        pub_path = self.inputs / "publication.request.json"
        self.command("prepare-publication", [self.args.publication_helper, self.inputs,
            self.write("publication.key.json", self.key("publication")), pub_path, "--operator-resources"])
        pub_payload = json.loads(pub_path.read_bytes())["payload"]
        published = self.submit("publication", "policy.publish", pub_payload)
        publication = unique(published, "workflow-publication")
        self.plan = self.document(unique(published, "workflow-compiled-plan"))
        source_refs = self.plan["source_refs"]
        self.product_ref = next(r for r in source_refs if r["kind"] == "workflow-product")
        self.policy_ref = next(r for r in source_refs if r["kind"] == "workflow-policy")
        template_ref = next(r for r in source_refs if r["kind"] == "workflow-slot-template")
        self.policy = self.document(self.policy_ref)
        self.fact_refs = {self.document(r)["schema_id"]: r for r in source_refs if r["kind"] == "workflow-fact-schema"}
        account = self.business["supplier_inputs"]["provider_account_identity"]
        ownership = {"statement": "local_owner_controls_provider_accounts", "principal_id": self.operator,
            "logical_store_id": self.store, "provider_account_identities": [account]}
        content = canonical(ownership)
        funding = self.submit("supplier-owner-evidence", "evidence.retain", {
            "schema": "ledger-workflow-evidence-retain/1", "purpose": "funding",
            "subject": {"customer": self.customer, "agreement_id": self.agreement},
            "metadata": {"resource_owner": self.business["platform"], "payer": self.business["platform"], "mode": "platform"},
            "media_type": "application/json", "content_base64": base64.b64encode(content).decode(), "content_sha256": sha(content)})
        funding_ref = unique(funding, "workflow-evidence")
        enrolled = self.submit("supplier-account-enroll", "authority.change", {"schema": "ledger-cost-authority-enroll/1",
            "authority_id": "stage-supplier-owner", "provider_account_identities": [account], "evidence_refs": [funding_ref]})
        cost_refs = [r for r in enrolled["receipt"]["result_refs"]
                     if self.document(r)["schema"] == "ledger-cost-authority/1"]
        assert len(cost_refs) == 1
        cost_authority = cost_refs[0]
        role, supplier_role, catalog = [copy.deepcopy(self.business[n]) for n in ["role", "supplier_role", "catalog"]]
        role_ref, supplier_role_ref = reference("workflow-roles", role), reference("workflow-roles", supplier_role)
        catalog_ref = reference("workflow-slot-catalog", catalog)
        instantiation = {"schema": "ledger-workflow-slot-instantiation/1", "customer": self.customer,
            "agreement_id": self.agreement, "revision": "1", "catalog_ref": catalog_ref, "template_ref": template_ref,
            "component_map": self.business["component_map"], "family_map": self.business["family_map"]}
        retail = {**copy.deepcopy(self.business["terms"]), "role_ref": role_ref, "source_grant_refs": self.accepted_grants()}
        retail_ref = reference("workflow-slot-terms", retail)
        binding = {**copy.deepcopy(self.business["binding"]),
            "accepted_context": {"fact_schema_ref": self.fact_refs["support:stage-context"], "facts": {}},
            "bindings": [{"slot": "support:retail", "binding_ref": retail_ref}],
            "roles_refs": sorted([role_ref, supplier_role_ref], key=canonical), "slot_catalog_ref": catalog_ref,
            "slot_instantiation_ref": reference("workflow-slot-instantiation", instantiation)}
        next(leg for leg in binding["funding_legs"] if leg["name"] == "support:provider")["evidence_refs"] = [funding_ref]
        quantity = {"activity": "support:prepare", "field": "support:quantity", "unit": "support:item", "maximum": "1"}
        supplier = {"scope": self.scope, "customer": self.customer, "agreement_id": self.agreement,
            "slot": "support:supplier", "book": "supplier", "role_slot": "support:supplier-role",
            "role_ref": supplier_role_ref, "funding_leg": "support:provider",
            "allowed_activities": ["support:close", "support:prepare"], "quantity_ceilings": [quantity],
            "exposure_ceiling": self.business["supplier_inputs"]["maximum_exposure"],
            "source_grant_refs": self.accepted_grants(), "correction_source_slot": "support:supplier-authorizer",
            "cost_authority_ref": cost_authority, "provider_account_identity": account,
            "consumption_basis": "owned_obligation_high_water"}
        documents = [role, supplier_role, catalog, instantiation, retail]
        kinds = ["workflow-roles", "workflow-roles", "workflow-slot-catalog", "workflow-slot-instantiation", "workflow-slot-terms"]
        records = [{"reference": reference(kind, body), "body": body} for kind, body in zip(kinds, documents)]
        preview = {"publication_payload": pub_payload, "key": self.key("supplier-commitment-preview"),
            "binding_inputs": binding, "supplier_terms_projection": supplier, "records": records}
        self.command("prepare-supplier-commitment", [self.args.supplier_helper,
            self.write("supplier-commitment.input.json", preview), self.inputs, "--operator-resources"])
        commitment = json.loads((self.inputs / "supplier-commitment.json").read_bytes())
        commitment_ref = json.loads((self.inputs / "supplier-commitment.identity.json").read_bytes())["reference"]
        assert commitment_ref == reference("workflow-offer", commitment)
        # Use the actual normalized projection, never a hand-built replacement.
        supplier = commitment["acceptance"]["terms"]
        offer = {"schema": "ledger-workflow-offer/4", "agreement_id": self.agreement, "version": "1",
            "provider": self.business["provider"], "valuation_mode": "fixed_unit", "role_refs": [supplier_role_ref],
            "activity_names": supplier["allowed_activities"], "unit_ceilings": [quantity],
            "exposure_ceiling": supplier["exposure_ceiling"], "allowed_component_slots": ["support:share"],
            "source_slots": ["support:worker"], "starts_at": "2000-01-01T00:00:00.000000Z",
            "ends_at": "2099-01-01T00:00:00.000000Z", "evidence_refs": [funding_ref], "commitment_ref": commitment_ref}
        offer_ref = reference("workflow-offer", offer)
        supplier = {**supplier, "schema": "ledger-workflow-slot-binding-terms/8", "offer_ref": offer_ref, "commitment_ref": commitment_ref}
        supplier_ref = reference("workflow-slot-terms", supplier)
        provider_auth, provider_assent = self.provider_consent(commitment_ref, offer_ref, supplier_ref)
        binding["bindings"].append({"slot": "support:supplier", "binding_ref": supplier_ref})
        documents += [commitment, offer, supplier, provider_assent]
        self.finish_acceptance(binding, documents, publication, auth, psub, roles, provider_auth)

    def provider_consent(self, commitment, offer, terms):
        subject = {"customer": self.customer, "agreement_id": self.agreement,
                   "party": self.business["provider"], "acceptor": "synthetic-provider-representative"}
        authorization = {**subject, "scope": self.scope, "authorization_id": "stage-provider-authority", "roles": ["provider"],
            "starts_at": "2000-01-01T00:00:00.000000Z", "ends_at": "2099-01-01T00:00:00.000000Z"}
        ev = self.evidence("provider-authority-evidence", "ledger-workflow-party-evidence-retain/1", subject,
                           {"purpose": "party_authorization", "authorization": authorization})
        att = self.evidence("provider-authority-attestation", "ledger-workflow-party-evidence-retain/1", subject,
            {"purpose": "party_operator_attestation", "statement": "operator_verified_party_acceptor_authority", "target_evidence_ref": ev})
        result = self.submit("provider-authorize", "authority.change", {"schema": "ledger-workflow-party-authorize/1",
            "action": "party.authorize", "expected_revision": "0", "authorization": authorization,
            "evidence_ref": ev, "operator_attestation_ref": att})
        auth = unique(result, "workflow-authority")
        consent = {**subject, "scope": self.scope, "roles": ["provider"], "authorization_ref": auth,
            "basis": {"kind": "supplier_commitment", "commitment_ref": commitment,
                      "offer_ref": offer, "supplier_terms_ref": terms}, "accepted_at": now()}
        ev = self.evidence("provider-consent-evidence", "ledger-workflow-party-evidence-retain/3", subject,
                           {"purpose": "party_consent", "consent": consent})
        att = self.evidence("provider-consent-attestation", "ledger-workflow-party-evidence-retain/3", subject,
            {"purpose": "party_operator_attestation", "statement": "operator_verified_party_exact_consent", "target_evidence_ref": ev})
        return auth, {"schema": "ledger-workflow-party-assent/3", "consent": consent,
            "mode": "real", "evidence_ref": ev, "operator": self.operator, "operator_attestation_ref": att}

    def finish_acceptance(self, binding, documents, publication, auth, platform_subject, roles, provider_auth):
        terms = {"schema": "ledger-workflow-acceptance-terms/2", "scope": self.scope, "customer": self.customer,
                 "agreement_id": self.agreement, "version": "1", "publication_ref": publication, "binding_inputs": binding}
        self.command("prepare-terms", [self.args.terms_helper, self.inputs / "context-facts.json",
            self.write("acceptance-terms.raw.json", terms), self.inputs])
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
        consent = {**platform_subject, "scope": self.scope, "roles": roles, "authorization_ref": auth,
            "basis": {"kind": "acceptance_terms", "terms_ref": identity["reference"], "terms_digest": digest}, "accepted_at": accepted_at}
        ev = self.evidence("platform-assent-evidence", "ledger-workflow-party-evidence-retain/2", platform_subject,
                           {"purpose": "party_consent", "consent": consent})
        att = self.evidence("platform-assent-attestation", "ledger-workflow-party-evidence-retain/2", platform_subject,
            {"purpose": "party_operator_attestation", "statement": "operator_verified_party_exact_consent", "target_evidence_ref": ev})
        platform_assent = {"schema": "ledger-workflow-party-assent/2", "consent": consent,
            "mode": "real", "evidence_ref": ev, "operator": self.operator, "operator_attestation_ref": att}
        documents += [customer_assent, platform_assent]
        assents = sorted([reference("workflow-assent", d) for d in documents if d["schema"] in
                          {"ledger-workflow-assent/1", "ledger-workflow-party-assent/2", "ledger-workflow-party-assent/3"}], key=canonical)
        authorities = sorted([*self.accepted_grants(), auth, provider_auth], key=canonical)
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

    def field_proof(self, name, subject, schema_ref, field, value, kind, source):
        assert self.key(name)["source"] == source
        result = self.submit(name, "evidence.retain", {"schema": "ledger-workflow-fact-attest/1", "attestation": {
            "schema": "ledger-workflow-fact-attestation/1", "attestation_id": name, "source": source,
            "evidence_kind": kind, "subject": subject, "subject_schema_ref": schema_ref,
            "claims": [{"field": field, "value": value}], "issued_at": now(), "supporting_evidence_refs": []}})
        verification = unique(result, "workflow-evidence-verification")
        attestation = unique(result, "workflow-fact-attestation")
        self.meta.setdefault("field_proofs", {})[name] = {"attestation_ref": attestation,
            "verification_ref": verification, "subject": subject, "field": field, "value": value}
        self.save()
        return {"field": field, "attestation_ref": attestation}

    def book_totals(self, label, retail, supplier):
        # Actions are authenticated through their actual producing Decision,
        # never counted once per alias receipt mentioning the same result.
        with self.connection() as db:
            rows = [(json.loads(b), {"kind": "workflow-decision", "id": i, "digest": d})
                    for i, d, b in db.execute("SELECT id,digest,canonical_bytes FROM workflow_documents WHERE kind='workflow-decision'")]
        actions = {}
        for decision, decision_ref in rows:
            for ref in decision["actions_appended"]:
                identity = canonical(ref)
                assert identity not in actions
                action = self.document(ref)
                assert action["decision_id"] == decision["decision_id"] and action["revision"] == decision["revision"]
                assert action["operation_key"] == decision["operation_key"]
                assert action["amount"]["currency"] == "USD" and action["amount"]["scale"] == 18
                with self.connection() as db:
                    receipts = [json.loads(b) for b, in db.execute("SELECT receipt_bytes FROM workflow_operations")]
                producers = [r for r in receipts if r["key"] == action["operation_key"] and ref in r["result_refs"]
                             and decision_ref in r["result_refs"]]
                assert len(producers) == 1
                actions[identity] = action
        with self.connection() as db:
            census = {canonical({"kind": "workflow-action", "id": i, "digest": d})
                      for i, d in db.execute("SELECT id,digest FROM workflow_documents WHERE kind='workflow-action'")}
        assert census == set(actions), "Action without its exact producing Decision"
        total = {book: sum(int(a["amount"]["atoms"]) for a in actions.values() if a["book"] == book)
                 for book in ["retail", "supplier"]}
        assert total == {"retail": retail, "supplier": supplier}, (label, total)
        self.meta["checks"].append({"check": "actual_producer_unique_action_books", "step": label, "atoms": total})
        self.save()
        return actions

    def invocation_state(self, original_ref):
        original = self.document(original_ref)
        revision, current_ref = self.head("invocation", original["invocation_id"])
        current = self.document(current_ref)
        assert current["revision"] == revision and current["invocation_id"] == original["invocation_id"]
        values = {n: int(current[n]["atoms"]) for n in ["authorized", "consumed", "held", "released"]}
        assert values["authorized"] == values["consumed"] + values["held"] + values["released"]
        return current_ref, current, values

    def sequence(self):
        authorized = self.submit("stage-authorize", "invocation.authorize", {
            "schema": "ledger-workflow-stage-authorize/1", "expected_chain_revision": "0",
            "stage_instance": {"customer": self.customer, "chain": self.chain, "agreement_id": self.agreement,
                "binding_ref": self.binding_ref, "stage_name": "support:stage", "authority_refs": [self.grant],
                "expected": [{"template_input_key": "support:member", "source": self.worker,
                    "business_operation_id": self.business_id, "activity": "support:prepare", "component_slots": ["support:prior"]}]},
            "invocation_inputs": [{"schema": "ledger-workflow-invocation-authorize/1", "agreement_id": self.agreement,
                "binding_ref": self.binding_ref, "business_operation_id": self.business_id, "chain_id": self.chain,
                "activity": "support:prepare", "supplier_slot": "support:supplier", "source_slot": "support:worker",
                "unit": "support:item", "maximum_quantity": "1", "maximum_exposure": self.business["supplier_inputs"]["maximum_exposure"],
                "start_before": "2099-01-01T00:00:00.000000Z", "evidence_refs": [self.worker_grant]}]})
        stage_ref = unique(authorized, "workflow-stage-instance")
        original_invocation = unique(authorized, "workflow-invocation")
        _, chain = self.head("chain", self.customer, self.chain)
        assert self.document(chain)["admitted_events"] == "0"
        occurred = now()
        subject = {"kind": "work", "logical_store_id": self.store, "scope": self.scope,
            "canonical_source_namespace": self.worker, "business_operation_id": self.business_id,
            "customer": self.customer, "activity": "support:prepare", "product_ref": self.product_ref, "binding_ref": self.binding_ref}
        occurrence = self.field_proof("work-occurrence", subject, self.fact_refs["support:stage-work-facts"],
                                      "support:occurred", occurred, "occurrence", self.worker)
        payload = {"schema": "ledger-workflow-work/1", "external_event_id": "stage-member-delivery",
            "business_operation_id": self.business_id, "customer": self.customer, "chain": self.chain,
            "activity": "support:prepare", "product_ref": self.product_ref, "policy_ref": self.policy_ref,
            "fact_schema_ref": self.fact_refs["support:stage-work-facts"], "binding_ref": self.binding_ref,
            "status": "succeeded", "facts": {"support:quantity": "1", "support:occurred": occurred},
            "fact_proofs": [occurrence], "links": [], "evidence_refs": [], "invocation_refs": [original_invocation],
            "occurred_at": occurred, "stage_instance_ref": stage_ref}
        worked = self.submit("member-work", "work.accept", payload)
        work_ref, work_decision = unique(worked, "workflow-work"), unique(worked, "workflow-decision")
        self.book_totals("member", 135 * ATOMS // 100, 0)
        before = self.snapshot()
        alias_payload = {**payload, "external_event_id": "shared-reporter-delivery"}
        alias = self.submit("shared-work-alias", "work.accept", alias_payload)
        assert alias["receipt"]["result_kind"] == "semantic_alias"
        assert alias["receipt"]["key"] == self.key("shared-work-alias")
        assert alias["receipt"]["result_refs"] == worked["receipt"]["result_refs"]
        after = self.snapshot()
        assert len(after["operations"]) == len(before["operations"]) + 1 and after["heads"] == before["heads"]
        self.book_totals("alias-original-source-pricing", 135 * ATOMS // 100, 0)
        inv_ref, inv, _ = self.invocation_state(original_invocation)
        self.submit("invocation-completed", "invocation.close", {"schema": "ledger-workflow-invocation-close/1",
            "invocation_ref": inv_ref, "expected_revision": inv["revision"], "reason": "completed", "evidence_refs": [self.worker_grant]})
        _, closed_inv, closed_values = self.invocation_state(original_invocation)
        assert closed_inv["state"] == "closed_completed" and closed_values["held"] >= 50 * ATOMS // 100
        closed_at = now()
        close_subject = {"kind": "stage", "logical_store_id": self.store, "scope": self.scope,
            "stage_instance_ref": stage_ref, "business_operation_id": "stage-close-business",
            "activity": "support:close", "binding_ref": self.binding_ref}
        proofs = [self.field_proof("closure-occurrence", close_subject, self.fact_refs["support:stage-close-facts"],
                                  "support:occurred", closed_at, "occurrence", self.closer),
                  self.field_proof("closure-success", close_subject, self.fact_refs["support:stage-close-facts"],
                                  "support:success", True, "outcome", self.closer)]
        closed = self.submit("stage-close", "outcome.submit", {"schema": "ledger-workflow-stage-close/1",
            "external_event_id": "stage-close-delivery", "business_operation_id": "stage-close-business",
            "customer": self.customer, "chain": self.chain, "activity": "support:close", "product_ref": self.product_ref,
            "policy_ref": self.policy_ref, "fact_schema_ref": self.fact_refs["support:stage-close-facts"],
            "binding_ref": self.binding_ref, "facts": {"support:occurred": closed_at, "support:success": True},
            "fact_proofs": sorted(proofs, key=canonical), "links": [{"slot": "support:closes", "predecessor": work_ref}],
            "evidence_refs": [], "occurred_at": closed_at, "stage": "support:stage", "stage_instance_ref": stage_ref})
        priced_actions = self.book_totals("stage-cap-share", 315 * ATOMS // 100, 45 * ATOMS // 100)
        claim_ref = unique(closed, "workflow-stage-closure-claim")
        close_decision = unique(closed, "workflow-decision")
        decision = self.document(close_decision)
        assert decision["subject_ref"] == claim_ref
        actions = {canonical(r): self.document(r) for r in decision["actions_appended"]}
        shares = [r for r in decision["actions_appended"] if actions[canonical(r)]["book"] == "supplier"]
        assert len(shares) == 1 and int(actions[canonical(shares[0])]["amount"]["atoms"]) == 45 * ATOMS // 100
        allocation_refs = [r for r in closed["receipt"]["result_refs"] if r["kind"] == "workflow-allocation"]
        allocations = [self.document(r) for r in allocation_refs]
        assert len(allocations) == 2 and {a["partition"] for a in allocations} == {"supplier_share", "host_remainder"}
        assert {int(a["amount"]["atoms"]) for a in allocations} == {45 * ATOMS // 100, 135 * ATOMS // 100}
        assert sum(int(a["amount"]["atoms"]) for a in allocations) == 180 * ATOMS // 100
        for a in allocations:
            assert a["allocation_parent"] == shares[0]
            assert sum(int(self.document(r)["amount"]["atoms"]) for r in a["inputs"]) == 180 * ATOMS // 100
        _, consumed, consumed_values = self.invocation_state(original_invocation)
        assert consumed["state"] == "closed_completed" and consumed_values["consumed"] == 45 * ATOMS // 100
        _, stage_head = self.head("stage", self.agreement, self.chain, "support:stage")
        terminal_stage = self.document(stage_head)
        assert terminal_stage["state"] == "closed" and terminal_stage["closure_claim_ref"] == claim_ref
        actual_work = self.document(work_ref)
        assert terminal_stage["first_work_ref"] == work_ref and terminal_stage["first_work_accepted_at"] == actual_work["accepted_at"]
        claim = self.document(claim_ref)
        assert claim["first_work_ref"] == claim["anchor_work_ref"] == work_ref
        assert claim["accepted_at"] <= terminal_stage["activation_deadline"]
        assert len(claim["anchor_link_refs"]) == 1
        anchor = self.document(claim["anchor_link_refs"][0])
        assert anchor["child"] == stage_ref and anchor["predecessor"] == work_ref
        assert anchor["operation_key"] == claim["operation_key"] == self.key("stage-close")
        root = self.document(work_decision)
        reversed_result = self.submit("member-reverse", "work.reverse", {"schema": "ledger-workflow-work-reverse/2",
            "work_ref": work_ref, "expected_revision": root["revision"], "family_closures": [],
            "dependent_reversals": [{"decision_ref": close_decision, "expected_revision": decision["revision"],
                "authority_evidence_refs": sorted([self.corrector_grant, self.closer_grant], key=canonical)}],
            "evidence_refs": [self.corrector_grant], "reason": "Synthetic causal Stage terminal proof"})
        final_actions = self.book_totals("stored-atom-causal-inverse", 0, 0)
        inverse_refs = [r for r in reversed_result["receipt"]["result_refs"] if r["kind"] == "workflow-action"]
        inverses = [self.document(r) for r in inverse_refs]
        assert len(inverses) == len(priced_actions)
        for original_key, original in priced_actions.items():
            assert final_actions[original_key] == original
            matches = [v for v in inverses if canonical(v.get("inverse_of")) == original_key]
            assert len(matches) == 1 and int(matches[0]["amount"]["atoms"]) == -int(original["amount"]["atoms"])
            for field in ["book", "component_slot", "root_component_slot", "role_ref", "slot_terms_ref", "funding_leg", "basis_refs"]:
                assert matches[0][field] == original[field], ("stored inverse changed", field)
        inverse_allocations = [self.document(r) for r in reversed_result["receipt"]["result_refs"] if r["kind"] == "workflow-allocation"]
        assert len(inverse_allocations) == len(allocations)
        for reference, original in zip(allocation_refs, allocations):
            matches = [v for v in inverse_allocations if v.get("reverses") == reference]
            assert len(matches) == 1
            inverse = matches[0]
            assert int(inverse["amount"]["atoms"]) == -int(original["amount"]["atoms"])
            for field in ["partition", "recipient", "stage_instance_ref"]:
                assert inverse[field] == original[field]
        _, stage_head = self.head("stage", self.agreement, self.chain, "support:stage")
        assert self.document(stage_head)["closure_claim_ref"] == claim_ref and self.document(stage_head)["state"] == "closed"
        for original_decision in [work_decision, close_decision]:
            initial = self.document(original_decision)
            _, current_decision = self.head("decision", initial["decision_id"])
            assert self.document(current_decision)["status"] == "reversed"
            assert not self.document(current_decision)["active_action_refs"]
        _, _, final_values = self.invocation_state(original_invocation)
        assert final_values["consumed"] == consumed_values["consumed"], "inverse replenished consumed supplier exposure"
        self.reopen_retry(["alias-enroll", "supplier-account-enroll", "provider-consent-evidence", "publication",
            "acceptance", "activation", "stage-authorize", "work-occurrence", "member-work", "shared-work-alias",
            "invocation-completed", "stage-close", "member-reverse"])
        restored_root = self.root / "restored-final-history"
        shutil.copytree(self.root / "installation", restored_root / "installation", copy_function=shutil.copy2)
        shutil.copytree(self.inputs, restored_root / "inputs", copy_function=shutil.copy2)
        for name in ["shared-work-alias", "stage-close"]:
            for suffix in ["request.json", "committed.json"]:
                shutil.copy2(self.root / (name + "." + suffix), restored_root / (name + "." + suffix))
        restored = copy.copy(self)
        restored.root, restored.installation, restored.inputs = restored_root, restored_root / "installation", restored_root / "inputs"
        restored.log = restored_root / "RESTORE-RUN.json"
        restored.meta = {"classification": "Whole quiescent installation copy; exact original recovery only", "commands": [], "checks": []}
        restored.reopen_retry(["shared-work-alias", "stage-close"])
        restored.save()
        self.meta["status"] = "stage_cap_share_originals_complete"
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["ledger", "publication-helper", "terms-helper", "supplier-helper", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.set_defaults(resume=False, deadline_seconds=0, pending_finalize_check=False)
    args = parser.parse_args()
    journey = StageJourney(args)
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
