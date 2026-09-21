# Owner-approved delivery roadmap

**Phase 1 is complete and frozen. Phase 2+ is not started or authorized.**
Unchecked work preserves the owner's remaining plan; it is not authorization
to start another phase. Product roadmap phase numbers below remain distinct
from historical engineering labels such as “Phase 2 pure core.”

Authoritative semantic baseline:
`1e0ba3f886788c08f427d3aae1d916b341187e76` (semantic-freeze lane).
All frozen v1 contracts, IDs, fixtures and receipts remain immutable.

## Phase 1 — reconcile and freeze the canonical contract (complete)

Entry gate: approved semantic implementation and isolated canonical worktree;
no integration or persistence work in this phase.

- [x] Reconcile percentage basis, permanent claim identity and exact time bounds.
- [x] Freeze complete eligible family/binding/limit membership at base acceptance.
- [x] Retain original base evaluation and verified target/admission replay inputs.
- [x] Preserve zero claims, current-revision corrections and original receipts.
- [x] Add complete-rehash semantic attacks and independent byte/hash reconstruction.
- [x] Preserve this durable roadmap, including the later product scope.
- [x] Fresh-context independent canonical/semantic review passes; findings resolved.
- [x] Record owner-authorized freeze of the exact reviewed bytes in the immutable inventory.
- [x] Full offline suite, independent Python/Node reconstruction and frozen-v1 verification pass.

Exit gate satisfied: independent **PASS TO FREEZE** for exact reviewed commit
`429aa027a696a09cfeb8dc1eada8420e732dda6b`, against the semantic baseline above;
all material Phase 1 findings resolved and no v1 drift. The owner separately
authorized this bounded freeze after independent approval. See the
[freeze record](PHASE-1-FREEZE.md) and [immutable inventory](contracts/freeze.json).
The lane stops at its clean local freeze commit. No later implementation,
integration, merge, push, publication, deployment or spending is authorized.

## Phase 2 — integrate the completed lanes (not started)

Entry gate: Phase 1 independent approval and explicit integration ownership.

- [ ] Integrate the semantic freeze, revised canonical work, outbox hardening
      and CLI hardening in an isolated integration branch.
- [ ] Resolve cross-lane interfaces without weakening authority or compatibility.
- [ ] Run all combined offline checks and contract reconstruction.
- [ ] Run schema-upgrade tests, including pre-existing databases and rollback/
      unknown-outcome behavior where applicable; retain evidence.

Exit gate: one reviewed integration candidate, passing combined checks and
schema upgrades, with no silent v1 rewrite and no unresolved interface mismatch.
This gate does not establish Phase 3 outcome persistence.

## Phase 3 — persist one complete economic path (not started)

Entry gate: reviewed integrated contracts and explicit coordinator/store scope.

- [ ] Persist one final base-work → authorized outcome adjustment → authorized
      correction path atomically on SQLite and PostgreSQL.
- [ ] Freeze all eligible members with base acceptance; lock target, claims,
      binding aggregates, authorities, supplier capacity and base-reversal guards.
- [ ] Persist inverse plus replacement together, including accepted zero results.
- [ ] Preserve original receipt lookup, aliases, replay inputs and commit ambiguity.
- [ ] Prove crash/cancellation/write-boundary/race behavior on both real stores.

Exit gate: identical journals/receipts after reopen, complete-or-absent commits,
no duplicate economics, authoritative expected-current guards, and independent
both-store evidence. A happy-path receipt alone is insufficient.

## Phase 4 — isolated policy comparison (not started)

Entry gate: the persisted path and historical replay inputs are trustworthy.

- [ ] Compare the same retained activity under multiple candidate policies in an
      isolated, nonposting workspace.
- [ ] Keep historical replay (original terms), prospective policy change (future
      eligibility), and authorized correction (new economic history) distinct.
- [ ] Show comparable basis, assumptions, fees/rebates and explanations.
- [ ] Prove comparisons create no authoritative claims, postings or export intents.

Exit gate: repeatable comparisons over identical activity, clear provenance and
no path from an exploratory result into a production posting without ordinary
explicit authorization and acceptance.

## Phase 5 — local workflow and one finance adapter (not started)

Entry gate: persisted semantics and nonposting comparison are reviewed.

- [ ] Expose the persisted base/outcome/correction path through the local CLI.
- [ ] Add one CSV finance adapter with explicit account/party/currency mapping.
- [ ] Preserve stable export identities, signed corrections and reconciliation
      evidence; export/delivery must not imply payment or settlement.
- [ ] Validate the end-to-end workflow with a realistic, consented or synthetic
      activity sample and finance review.

Exit gate: one understandable local workflow, reproducible CSV output and
corrections, documented limitations, and no duplicate export economics on retry.
No additional finance connectors are implied.

## Phase 6 — final independent release-to-main review (not started)

Entry gate: all preceding gates complete and review evidence collected.

- [ ] Fresh independent architecture and dependency-boundary review.
- [ ] Independent canonical bytes/IDs, authority and immutable-history review.
- [ ] Crash/race/unknown-commit review with both-store evidence.
- [ ] Outbox ordering, reconciliation, fencing and idempotency review.
- [ ] Resolve findings and rerun the affected combined checks.
- [ ] Obtain the owner's main-merge decision after the review gate passes.

Exit gate: explicit independent approval and owner-authorized main merge. This
roadmap does not authorize merge, push, publication, deployment or spending.

## Parallel $0 product validation (planned, separate from implementation)

- [ ] Use existing relationships and voluntarily supplied or synthetic examples
      to test the first workflow; no purchased panels, ads or paid infrastructure.
- [ ] Validate the buyer/operator, current finance handoff, dispute/correction
      frequency, authority source, and value of a reproducible audit trail.
- [ ] Compare the current manual workflow with a local demonstration; record
      evidence and objections instead of inferring demand from technical interest.
- [ ] Test whether fixed fees, percentage rebates and explicit corrections cover
      the first useful workflow before adding operators or integrations.
- [ ] Bring findings and scope changes back to the owner. Outreach/messages,
      production-data access and commitments require their own authorization.

Recommended first workflow to validate: an AI workflow/tool vendor records one
completed work item, an agreed business outcome adds a success fee or rebate,
and an authorized correction produces a finance-readable audit trail and CSV.
The likely first operator is the vendor's developer/technical founder working
with its finance counterpart; buyer fit remains a validation question.

Recommended defaults for that first demonstration: local SQLite, synthetic
sandbox data, one currency/scale, explicit agreement/roles/source permissions,
final base work, fixed fees before percentages, a finite premium ceiling, and
predeclared allowed codes and timing windows. Use the approved retail-net
percentage basis, inclusive receipt/acceptance deadlines and explicit current
revision on correction. These are workflow recommendations, not new implicit
policy defaults: actual amounts, deadlines and authority must be agreed and
pinned. Supplier economics stay explicit and separately authorized.

## Deferred capabilities and non-goals

- [ ] Revisit further adapters, UI/HTTP/SDK surfaces and broader platform support
      only after the first workflow and finance handoff are validated.
- [ ] Revisit compact replay encodings, multi-family atomic commands and wider
      pricing operators only with explicit contract and fixture review.

Deferred: live payment providers, multi-currency/FX, taxes, generalized accounting,
marketplace settlement, arbitrary retroactive rerating, running caps, automatic
supplier offsets, generalized distributed dispatch, cloud operations and hosted
product promises. Capped-stage/outcome composition is explicitly unsupported.

Non-goals: infer real assent or authorization from an event; rewrite prior
receipts; make policy comparison authoritative; make a pricing engine decide
legal enforceability; replace a finance system; or spend money to establish the
first product-validation signal. No additional scope starts merely because a
roadmap checkbox exists.
