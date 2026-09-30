# Fixed post-hoc adjustments plan

**Status:** proposed bounded follow-on scope. This plan is not part of the completed M0–M5 contracts and does not itself amend the canonical roadmap or authorize implementation.

## Goal

Add one simple way to record explicit fixed-amount post-hoc adjustments. The first configured policy use is a fixed charge for a named, already-logged failed work record. Other fee types remain operator-entered fixed adjustments; Bean Counter does not calculate fee formulas or extend chained billing.

The [canonical billing roadmap](../billing-roadmap.md) remains authoritative. Before implementation, the owner and Agency must place the approved scope and exit criteria in the roadmap and assign the implementation task. Keep this as a later billing-lifecycle addition; it does not reopen M4/M5 or rename M6 (external collection).

## Current gap

M5 supports append-only usage quantity corrections and linked post-close adjustments. Its ad hoc statement command presents retained adjustments; it does not create an arbitrary fee amount. M4's failed-work path does not charge for failed work, and the roadmap says explicit failed-work pricing is deferred. Therefore this work needs one new retained adjustment-creation path plus a small fixed-failure policy. Reuse the existing M5 period, statement, fiscal, export, identity, and writer-lock machinery.

## Proposed simplest behavior

1. **One fixed failure policy.** An authorized operator can set a fixed USD amount for failed work within the existing customer/source scope. The policy is versioned and starts disabled; changing it creates a new version. Do not add policy inheritance, tiers, percentages, minimums, or a general calculator.
2. **Explicit issuance against a logged failure.** Applying the policy names one retained failure record and its policy version. Do not scan history, create background charges, or issue fees implicitly from a billing chain. The resulting immutable adjustment retains the failure reference, amount, policy version, operator reason/evidence, and acceptance time.
3. **One fixed manual adjustment mechanism for every other fee.** For milestones, extra usage, minimums, percentages, tiers, and similar items, an operator explicitly supplies the final amount, reason, and evidence. No fee-specific arithmetic or chained adjustment is added.
4. **Exactly-once presentation.** Assign a newly accepted adjustment by its ledger acceptance time. It may appear once on the ordinary statement for an open customer period or on an explicitly issued ad hoc statement. If its ordinary period has closed, it belongs to a later open period unless an operator issues the permitted ad hoc statement. Closed statements remain immutable.
5. **Corrections append history.** Never edit or delete an accepted adjustment. Any reversal/correction must be another linked adjustment with explicit authority and reason; it must not rewrite a closed statement or the original failure.

### Defaults to confirm before freezing the contract

These are implementation recommendations, not already-frozen M5 rules:

- One fee per retained failed-work record; a retry of the same request is the same failure and cannot generate another fee.
- The operator explicitly issues the fee for a selected failure. Policy activation does not retroactively sweep old failures, and there is no automatic issuance job.
- The first policy is scoped to the same customer/source as the referenced work. Use USD and the existing exact integer money representation.
- A failed record must be a retained unsuccessful work result, not a rejected request that never became a ledger record. Identify the exact record family and reference in the CLI/API contract before code.

BEANA should present these defaults to the owner before freezing contract bytes or adding the scope to the canonical roadmap. If the desired charge trigger or failure identity differs, stop and amend this plan first.

## Work sequence, persona, model, and manager

| Step | Work | Persona / owner | Model | Manager session |
| --- | --- | --- | --- | --- |
| 1. Scope gate | Confirm the defaults above; amend the canonical roadmap with the new scope, milestone placement, and acceptance boundary; obtain the Agency task assignment. | BEANA, product/work coordinator | OpenAI Sol | OpenAI Sol |
| 2. Contract design | Define the minimum policy, adjustment, failure-link, identity/retry, authority, period-assignment, statement-claim, and reversal records. Keep existing M5 records and CLI/JSON families compatible. | BEANA, contract owner | OpenAI Sol | OpenAI Sol |
| 3. Bounded implementation | Add policy configuration, explicit fixed adjustment creation, exact retry/conflict behavior, validation, migration, and standard/ad hoc statement presentation. Implement no formula engine or automatic sweep. | BEANA, implementation owner | OpenAI Luna in a local repository session | OpenAI Sol |
| 4. Independent review and release | Review economic invariants, authority, idempotency, migration/recovery, statement exactly-once behavior, and exported values. Fix findings, qualify the exact profile, and release through Bean gates. | Astra, independent technical reviewer; Navy Bean, release operator | OpenAI Astra review; OpenAI Luna for release validation | OpenAI Sol |

Project-specific repository content stays in local OpenAI sessions. Do not send source, private roadmap text, or repository evidence to the remote Zen free lane; the repository's governed route accepts only synthetic or published-public envelopes.

## Acceptance criteria

- An authorized operator can configure and version the fixed-failure policy within its documented customer/source scope; invalid amount, missing authority, unsupported currency, and stale/conflicting change refuse without mutation.
- An operator can issue one adjustment for a named retained failure. The record pins the exact failure, policy version, amount, acceptance time, reason/evidence, and retry identity. Replaying the exact command returns its original result; conflicting ID reuse refuses; repeated issuance cannot charge the same failure twice.
- An operator can create an explicit fixed-amount adjustment for another fee reason without invoking a fee formula, changing a chain, or mutating the original work.
- Each adjustment can be presented once through an ordinary open-period statement or an explicit ad hoc statement. After close, the standard statement is immutable. Totals and finance export reconcile exactly to retained adjustment records.
- Corrections use linked append-only records. Migration preflight, refusal, rollback/unknown-commit recovery, and whole-installation backup restore preserve prior M0–M5 history and identities.
- Tests exercise wrong customer/source, unauthorized use, unknown commit and exact retry, identity conflict, duplicate failure issuance, period close races, standard/ad hoc double-presentation refusal, and exact export reconciliation.
- The canonical roadmap is updated, independent review is recorded, exact-commit QA/CI and Bean release gates pass, and release notes state that these are explicit fixed adjustments with no fee formula engine.

## Explicit exclusions

No percentage/tier/minimum/milestone calculator; no automatic billing-chain event or background failure sweep; no retroactive bulk charging; no tax, invoice-legal status, payment collection, Stripe adapter, universal currency, or hosted/multi-business mode; no general rules engine or arbitrary adjustment plug-in framework.

## Effort

Medium: approximately 4–7 engineering days after the scope gate, plus contract review, migration/recovery qualification, independent review, and release checks. Estimate assumes the existing M5 append-only adjustment and statement machinery can be reused without changing its economic invariants.

## Source references

- [Canonical billing roadmap](../billing-roadmap.md), including M4 failed-work and deferred-fee boundaries.
- [M5 decision register](../m5-decision-register.md) and [M5 qualification](../m5-qualification.md), for immutable statements, adjustment presentation, acceptance-time assignment, and qualified source-built behavior.
- [Current local SQLite requirements](../../CURRENT-REQUIREMENTS.md), for supported scope and release acceptance.
