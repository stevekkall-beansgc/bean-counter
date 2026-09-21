# Independent successor review — reservation settlement

**Verdict: PASS for the bounded additive candidate review. R1 is closed.**
No remaining material findings were identified in this successor's reviewed
identity/replay changes or their interaction with the independent lifecycle
expectations. This verdict is not a freeze, implementation authorization,
real-store conformance result, or approval to merge/release.

Exact candidate: `d311d9622b527863c79fd5e5820f5b890ab2c6b1`.
Predecessor reviewed: `557a4a6de2a518ad1670935849fef680cac460f7`.
Candidate clone:
`/Users/stephenkall/Documents/Codex/2026-09-21/ledger-lab-p3-coordinator/work/ledger-lab`.
Review used a local Git archive of the exact successor SHA.
Candidate review-manifest SHA-256:
`90d1c6b73d6692f6c366735bfc2188d9e8c50d5038c7fb6f5629c85ffa3d8b22`.

The independent baseline remains
`6ae64d14fac96068caa3e0d91a70f5e6ba23143d`; its numeric oracle was not edited.
The prior REQUEST CHANGES verdict still applies to `557a4a6`, not this successor.

## R1 resolution

Both Python and Node now keep separate registries for:

- previously accepted scoped source/external delivery identities;
- scoped `(kind,id)` identities of newly appended records;
- reference identities keyed by scope/kind/id, without content hash in the key.

New accepted steps cannot reuse a delivery label, even with identical command
bytes or a renamed operation kind. Re-appended immutable IDs reject. Repeated
references to the same identity/hash remain valid, while a different hash for
that identity rejects. Replay construction uses the identity-keyed registry.

The original three reviewer-owned, fully rehashed histories were rerun unchanged:
post-hoc reusing ordinary identity, closure reusing ordinary identity, and no-op
closure reusing a previous closure identity. Each now rejects in Python as
`DELIVERY_IDENTITY_REUSED`. Node verifies row integrity first and rejects all
three. These histories contain 63 total records; each includes the same two
conflicting IDs that previously passed. The additional candidate regressions
cover reference-hash conflicts, transition-ID reuse, registration reuse and an
identical closure retry incorrectly appended as a new accepted step.

## Non-appending lookup and no-op review

The 13 new lookup cases exercise original ordinary, post-hoc, closure and no-op
closure results after later decisions; changed request/ingress conflicts;
read-denied nondisclosure; durable lookup; proven absence; and inconclusive
absence remaining `outcome_unknown`. Lookup registry contents remain identical.
Original settlement receipt bytes and original synthetic economic receipt
references are compared exactly. Read permission is checked before disclosure;
current write permissions/timing and stale guards do not replace identity retry.

The helper is a pure fixture lookup, not the production transaction resolver.
`absence_confirmed` remains a trusted test observation: live transaction/primary
proof must come from later adapter tests. Current write/time annotations in the
case file are deliberately not inputs to identity lookup, rather than proof of
runtime authorization call ordering. Full original economic receipt bytes are
outside this package's synthetic boundary, as the successor README now states.
Semantic alias handling still belongs to the existing v2/coordinator bridge.
These are declared implementation gates, not unresolved R1 defects.

## Verification and independent comparison

- Standalone candidate checks pass: 12 histories, 44 steps, 112 records; 47
  packaged adversaries, including 46 rehashed cases; nine strict JSON negatives;
  13 non-appending lookup cases in Python and Node.
- Reviewer-owned serialization/ID/content hash reconstruction matches all 112
  positive records and the 63 records in the three original R1 attack histories.
- The unchanged independent lifecycle comparison passes for all 10 history
  variants: 25 accepted literal operations plus 10 registration checkpoints.
  Rejected/duplicate operations are not disguised as accepted canonical steps.
- Direct Git-object comparisons prove positive numeric histories, closed schema
  and canonical vectors are byte-identical to `557a4a6`.
- All 159 frozen files and the freeze registry remain byte-identical to exact
  Phase 2 base `6194376a053b8a27887a9b09459054a7af3a1769`.

The initial ordinary consume/zero/discount rules, explicit closure and inclusive
latest-family deadline, agreed early closure authority, and no post-hoc changes
to reservation amounts/family state/revision remain consistent with the owner's
approved semantics. No-op closure still emits no reservation transition or
revision bump. Stable economic ingress commitment remains separate from result
receipt reference. Unknown commit still forbids inferred rollback or compensating
release. No semantic or byte changes were introduced to resolve R1.

This re-review ran the standalone candidate suite, reviewer probes, independent
numeric comparison and exact Git-object checks. It did not rerun the unrelated
full baseline Rust suite; the successor has no production/SQL/Cargo changes.
No real-store lifecycle, physical crash, cancellation, active-commit or actual
concurrency evidence is claimed.

## Reproduce

Using the existing offline dependency environment, invoke this script with a
local archive of exact `d311d9622b527863c79fd5e5820f5b890ab2c6b1`:

```sh
python3 -B crates/ledgerlab-testkit/oracle/phase3/reviews/d311d96/recheck.py /absolute/path/to/successor-archive
python3 -B crates/ledgerlab-testkit/oracle/phase3/reviews/557a4a6/compare_lifecycle.py /absolute/path/to/successor-archive
```

Both scripts read candidate files only. The predecessor reproducer is preserved
as historical evidence and is intentionally not the successor's passing test.
The independent oracle, all frozen files and candidate files remain untouched.

Owner approval/freeze decisions and runtime gates remain separate: actual
retained authority verification, full v2 reference/receipt bridge, atomic base
registration, aliases, both-store reopen equality, per-write cancellation,
barrier races and unknown-commit resolution. No merge, push, tag, publication,
deployment, registration, paid infrastructure or spend occurred.
