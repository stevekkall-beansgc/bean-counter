# Candidate ADR: reconcile canonical target outcomes with approved semantics

Status: **proposed; not frozen; fresh-context independent review pending**.
Profile `2-candidate.2` supersedes candidate `.1` in `c95cae9`.
Semantic authority: `1e0ba3f886788c08f427d3aae1d916b341187e76`.

The earlier candidate diverged from the approved implementation: it used supplier
net as a percentage denominator, added book to claim eligibility, treated report
and correction deadlines as exclusive, omitted a distinct acceptance deadline,
and did not bind the complete eligible set to base acceptance. Reconciliation
changes these choices explicitly; no v1 bytes or production code change.

Decisions encoded for review:

- Use original final retail net for every percentage, including supplier rules.
  Signed ratios encode percentage points (`-10/1` is minus ten percent).
- Permanent key is scope/agreement/stable-family/target. Resolve posting book,
  binding, payer, roles and policy through complete frozen target membership.
- Freeze all families, bindings and finite limits at base acceptance, including
  unclaimed families. Bind base evaluation, target verification and original
  receipt to an immutable base-acceptance root checked independently of the
  journal being audited.
- Retain original base replay inputs and separate verified admission decisions.
  Replaying historical admission uses recorded observations, not current auth.
- Use distinct ordinary and correction four-endpoint windows: inclusive start,
  exclusive occurrence end, inclusive receipt and acceptance deadlines. Clock,
  base acceptance and previous-revision ordering use non-strict comparisons.
- Encode full reversal separately from a zero-valued allowed code. Keep ordered
  inverse/result explanations even for zero, exact inverse-plus-replacement
  postings and optimistic revision guards. Never reset the permanent claim.
- Aggregate by target/binding; keep supplier discount capacity, held contingent
  capacity and binding exposure separate from the retail percentage basis.
- Reject target registration for any cap-containing base bundle. No accepted
  target receipt is fabricated for the rejected registration example.
- Require fully rehashed negative histories to pass independent structural/hash
  validation before semantic rejection. Original base acceptance is a required
  external trust anchor; replacing it and all history cannot be detected by
  self-consistent hashes alone.

Canonical profile changes are intentionally versioned in the candidate hash
domain. Existing v1 contracts and compatibility declarations stay untouched.
The complete field dictionary, ordering, IDs and membership are in
[the reconciled design](../../design/CANONICAL-RECORDS-V2-CANDIDATE.md).

Open reviewer questions concern the proposed historical base-material codec,
v1-to-target reference mapping without receipt rewriting, authentic durable
base-root retrieval, supplier reservation observation completeness and combined
canonical-to-typed equivalence. These require a separate fresh-context reviewer
and the later integration/persistence gates in `ROADMAP.md`.

Neither these checks nor the author's self-review certify a freeze. No merge,
push, deployment, outbox/CLI/core change or persistence implementation is part
of the current Phase 1 assignment.
