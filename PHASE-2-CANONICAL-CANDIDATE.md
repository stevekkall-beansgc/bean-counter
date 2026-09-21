# Canonical reconciliation — roadmap Phase 1 review candidate

**Candidate `2-candidate.2`; not frozen. Fresh-context independent review pending.**
Branch: `codex/phase2-canonical`.
Prior candidate: `c95cae928fadc98b45def9ac11c32738afc922e1`.
Semantic authority: `1e0ba3f886788c08f427d3aae1d916b341187e76`.
Integration base: `b35258425970052ed71481eca1f33ef857c61be1`.

Only canonical-contract reconciliation and roadmap preservation were executed.
No sibling commits were integrated; production core, CLI, outbox, stores,
migrations, Cargo manifests/lockfile and frozen v1 files were untouched.

## Exact changes

1. All percentages now use frozen original **retail net**, including supplier
   percentages. Ratios are percentage points (`-10/1` = −10%), matching the
   approved typed `Amount::Percent`; supplier net is discount capacity only.
2. Ordinary and correction windows each retain starts_at/occurs_before/
   received_by/accepted_by. Occurrence uses inclusive start/exclusive end;
   receipt and acceptance deadlines are inclusive. Occurrence <= receipt <=
   acceptance, base acceptance <= claim acceptance, and current-revision
   acceptance <= correction acceptance. The extra corrected_at restriction was
   removed. Corrections can occur outside the ordinary window.
3. Claim identity is `[scope,agreement_id,family_id,target]`. Book is resolved
   economic data and cannot split eligibility. Policy version remains provenance.
4. Base acceptance now binds complete family/binding/limit membership, including
   unclaimed families, and the original base evaluation/verification/receipt.
   No later attachment, version change or first-outcome inference can enlarge it.
5. Verified admission observations are retained separately with scoped rights,
   evidence and authority revisions, target guard/finality, original roles,
   policy and bindings, base evaluation and supplier invocation/held/exposure
   inputs. Replay retains original observations rather than reauthenticating now.
6. Full reversal is distinct from a zero-valued code; permitted correction codes
   and allow_reversal are pinned. Numeric and immutable-ID current-revision guards
   agree. Exact inverse/result explanations are retained even at zero, together
   with any inverse-plus-replacement actions. Original receipts remain immutable.
7. Semantic audits reject fully rehashed invalid histories. Original committed
   base acceptance is an explicit required trust anchor; replacing every byte
   and supplying a different purported anchor is not authentication.
8. `ROADMAP.md` preserves remaining phases, checkbox entry/exit gates, parallel
   $0 validation, recommended first workflow/defaults, deferrals and non-goals.
   Only Phase 1 is active; independent review remains its unchecked exit gate.

The changed candidate contract uses a new `2-candidate.2` hash domain. Candidate
`.1` is preserved in Git history; no v1 profile or compatibility support changes.
The detailed field dictionary/IDs are in the candidate schema and
`docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md`; the reconciliation ADR is under
`docs/adr/candidates/`. `semantic-source.json` records exact source digests.

## Inventory and vectors

**26 record kinds:** the original 21 candidate kinds plus binding-snapshot,
base-evaluation, target-snapshot, base-acceptance and authority-decision.

**21 golden histories; 1,134 record vectors** (406 seed/preparation records and
728 outcome/correction records); **35 accepted decisions**, 21 permanent claims,
35 claim revisions, 35 actions/effects, 49 ordered explanations and 31 intentions.
Twenty original base acceptances carry immutable receipt roots. The one capped
target preparation is rejected and produces no base-acceptance receipt.

The original fixed/rebate/correction/reinstatement/zero/rounding/supplier/retry/
limit/cap examples remain, reconciled to approved semantics. Added histories
cover exact inclusive ordinary/correction receipt and acceptance deadlines,
equal observation times, retail net after booking discount, zero-net base,
a predeclared unclaimed family, and explicit full reversal/reinstatement.
Supplier −10% now computes −1000 from retail basis 10000 while checking against
its separate booked capacity 3000, rather than computing −300 from supplier net.

Thirteen fully rehashed attacks cover altered retail basis, rewritten complete
unclaimed-family membership, skipped revision, supplier held-capacity misuse,
supplier discount-capacity misuse, stale correction, all four deadline-plus-one-
microsecond cases, both exclusive occurrence endpoints, and eligibility added
through a policy-version duplicate. Every attack passes separate Python and
Node structural/hash checks before semantic rejection. The book-independent
key assertion and ten malformed JSON cases bring focused negative assertions
to **24**. Review checks never rewrite fixtures.

## Validation and compatibility

The complete offline repository checks (`sh scripts/check.sh`) include formatting,
workspace/all-feature tests, warnings-denied Clippy, no-default compilation,
boundaries and all frozen/candidate contract audits. The inherited production
suite has **110 passing Rust tests, zero failures and 13 ignored opt-in/later
gates**. Focused Python reconstruction, Node reconstruction and fully rehashed
semantic attacks run through `sh scripts/check-candidate-contracts.sh`.

All **99 frozen files**, 60 v1 hash vectors, 25 first-slice records, 29 first-slice
manifest members, original receipts and 80-atom result are unchanged. Original
frozen audit inventory rules remain intact. No PostgreSQL-server gate, new
storage behavior, real authentication, reservation concurrency, dispatcher or
production historical-codec conformance is claimed by these document tests.
Use the existing pinned offline Rust toolchain and Python contract dependencies;
no new dependencies, paid providers or network infrastructure are needed.

## Review questions and integration handoff

- Have a **separate fresh-context reviewer** verify the candidate-to-typed mapping
  against `1e0ba3f`, including all window comparisons and correction permissions.
- Review the proposed retained base-material codec and lossless production
  decoder, preserving original event/action/receipt identities without v1
  rewriting. The candidate retains inputs; a production codec/bridge is not
  implemented or certified in this lane.
- Confirm how the coordinator obtains and verifies the durable original base
  acceptance root and complete target/claim/base-reversal history under locks.
  A self-consistent replacement root must never be treated as trusted.
- Verify supplier reservation observations and capacity accounting against the
  combined binding/invocation path; corrections must not replenish authorization.
- Then follow roadmap Phase 2: integration owner reviews/cherry-picks the revised
  candidate with semantic, outbox-hardening and CLI-hardening lanes, resolves
  interfaces and runs combined checks and schema-upgrade tests. Phase 3 owns the
  atomic SQLite/PostgreSQL persistence bridge. Later phases own isolated policy
  comparison, local CLI/CSV, and final independent review before main merge.

Stop condition: clean committed review candidate, with fresh independent review
still pending. No freeze, merge, push, publication, deployment or spending.
