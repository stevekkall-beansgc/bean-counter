# Approved Phase 2 v0 outcome semantics

`Target::freeze` and `evaluate` implement the approved target-adjustment contract
in a pure, typed API. They do not extend canonical v1, any wire DTO or the Phase 1
acceptance/store port. `Operation::LinkedDiscount` is superseded and explicitly
rejects with `OUTCOME_TARGET_REQUIRED`; its old proposal fixtures are historical.
Existing chaining acquisition/closure pricing remains a separate legacy semantic
surface and cannot serve as the approved adjustment API. Any bundle containing a
cap rejects target registration with `OUTCOME_CAP_COMPOSITION`, even if its cap
has not fired. The frozen Phase 0/1 files and receipts are unchanged.

The coordinator must freeze the target in the **same transaction** that accepts
its successful, final base rating. No second rating or later policy attachment is
allowed. The target retains the original base Evaluation, policy, verified terms,
accepted time and booked retail basis. A target is exactly one immutable work
decision, never an adjustment, failed completion or full reversal. Zero-net
successful work is eligible, provided the rating is final. At evaluation the
complete locked base history must still contain that target and no reversal.
`reverse_base` additionally refuses a base reversal while monetary outcome results
are active. The coordinator must use that boundary wherever outcomes are enabled.

The request names one scope/target/agreement/family, source, occurrence time,
retained evidence and outcome code or authorized correction. There are no amount,
rate, basis or policy-version fields. Fixed signed integer Money and signed exact
rational percentages come only from the frozen policy. Every percentage uses the
original booked retail net after booking discounts, before tax/funding/payment or
any subsequent adjustments. Supplier costs are excluded; a separately authorized
supplier percentage rule also uses this explicitly declared retail basis. There
is no current-price lookup. Rational atom results round once, nearest with exact
half ties away from zero, using the bounded existing money implementation.

A claim's permanent structured key is scope (tenant/environment), agreement,
stable family, target. Policy versions, delivery IDs and rule labels cannot create
new eligibility. All predeclared families share the original target snapshot.
An ordinary identity or semantic duplicate returns the original decision index,
even after corrections, deadline expiry, or current price/role changes. Conflicting
facts fail. Zero amounts still consume the permanent claim and have explanations,
but no monetary postings. The coordinator owns durable aliases and receipt lookup.

Corrections require separate verified permission and the frozen correction source,
name the exact current revision, and use a separate frozen time window. A permitted
replacement appends the exact inverse of the current signed Money followed by its
new policy result in one immutable Decision. Full reversal means replacement None;
reinstatement is another explicitly permitted correction. Stale corrections fail.
Zero inverses/results have explanations but no zero-money postings. Any invalid
replacement or aggregate limit failure rejects the entire proposal. No partial
amount reversal or repricing is exposed.

Bounds inspect each family's latest active result. Sum absolute discounts
separately from premiums; premiums never buy discount capacity. Retail discounts
cannot exceed booked retail net. Each supplier has its own accepted binding,
actual nominated completion/invocation path, roles, discount capacity (that
supplier's booked net), and finite premium limit. Supplier premium limits must
fit the original held contingent capacity and binding exposure. Every used
binding has a finite nonnegative target premium ceiling, including a zero ceiling
for discount-only terms. Fail rather than clip. Original base and all outcome
postings remain immutable; total current net and atomic correction deltas remain
within integer bounds.

Occurrence, received_at and accepted_at are retained independently. Occurrence
must precede receipt and receipt must precede acceptance. Ordinary and correction
windows each freeze a starts_at (inclusive), occurs_before (exclusive), received_by
and accepted_by (inclusive). Base acceptance precedes any claim acceptance;
correction acceptance follows the current revision. Missing targets/history return
`WAITING_DEPENDENCIES` without reserving identity or claim. Replays use original
observations and frozen policy; current authentication is not rerun for replay.

`TargetVerification` requires finality, the exact policy document, verified binding
assents and any applicable supplier offer/payer delegation. `Verified` requires a
scoped authenticated principal/grant, explicit target/agreement/family/source
rights, separate read/submit/correct permissions, verified evidence and injected
times. These are **coordinator observations, not cryptographic authorization
capabilities**. The pure core checks their presence/consistency; it cannot establish
authentication, consent, document authenticity or that history is complete. Do not
map public ingress flags directly into them. The caller must authenticate and verify
assent/offer/delegation/link/evidence scope, current authority revisions and supplier
reservation rights under locks, retaining the actual evidence behind references.

For integration, retain every Target and full Decision (request, verification,
claim key/revision, original terms/roles, exact explanations and postings), plus the
base evaluation and its retained source authority, invocations, costs and receipt
time. Base replay also needs the original predecessor history. Freeze/review their
canonical encodings separately; no new IDs or serialized record family is invented
here. Lock target, family claims, all aggregate bounds, authority, base-reversal and
supplier reservations together; atomically persist inverse plus replacement and
compare the current revision. Do not replenish supplier reservations on reversal.
Supplier reservation consumption/release, authenticated receipt reads, aliases,
unknown-commit recovery and durable replay remain coordinator/persistence work.
Typed outputs cannot be appended using the existing Phase 1 port.

Validation is in testkit `phase2_outcomes`: live independent Python Fraction
calculations are compared after every submission, including rejection/duplicate,
full immutable journal, roles, postings, rational explanations, current revision,
policy version and all three times. Replay uses retained inputs. Legacy discrepancy
regressions use a second live runner with failed/reversed work, invalid supplier
paths, receipt ordering and historical reversal roles. These tests certify pure
semantics, not Phase 2 persistence, external authority, races, TLS or deployment.
