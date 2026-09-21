# Phase 2 outcome canonical contract candidate

Status: **candidate, not frozen; independent review pending**.
Branch: `codex/phase2-canonical`.
Base integration commit: `b35258425970052ed71481eca1f33ef857c61be1`.

This lane adds closed schemas, canonical byte/ID validation, synthetic complete
record histories, independent byte/hash checks and design/ADR documentation.
There are no production Rust, pricing engine, testkit oracle, coordinator,
persistence, migration, outbox, CLI, manifest dependency or lockfile changes.
No merge, push, deployment, paid service or remote model was used.

## Inventory

The candidate lives under `contracts/candidates/v2/`, outside the frozen schema
and fixture inventories. Its normative draft is
`docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md`; its proposed ADR is
`docs/adr/candidates/outcome-records-v2.md`.

The schema defines 21 record kinds: evidence, policy-snapshot, event,
base-posting, target-basis, admission, claim, claim-revision, effect, action,
obligation, link, dependency, limit-evidence, explanation, replay-input,
intention, delivery-key, chain-revision, decision-manifest and receipt.

Fourteen histories contain **174 seed records + 440 newly appended records =
614 record/hash/identity vectors**, including **24 accepted decisions/receipts**,
25 actions/effects, 14 permanent claims, 24 revisions and 21 intentions.
Zero and zero-net decisions retain claims/explanations/receipts without zero
money actions. Retail/supplier evidence uses the same base event and different
books, bases, payers, obligations and explicit correction commands.

| History | Key assertion |
|---|---|
| fixed-success-fee | 10000-atom base +2500 fixed outcome |
| percentage-rebate | 10000 base −1000 from exact −1/10 rate |
| correction-reinstatement | +2500 → zero → +2500; one permanent claim |
| correction-replacement | +2500 → +1000 → +1000 → +2500; exact inverse/replacement, zero-net export omission and retained export ancestors |
| two-rule-families | +2500 and −1000 independent claims; remove only the premium; gross aggregate evidence |
| zero-adjustment | Initial zero owns claim; correction can establish +2500; retries retain original zero receipt |
| rounded-zero | −1/10 atom rounds to zero, explained and accepted |
| signed-rounding | −21/2 atoms rounds to −11 |
| supplier-separation | Retail correction preserves supplier; supplier requires its own correction |
| duplicate-version-retry | Identity and semantic retries return original receipt despite changed version/rate and later revisions; stale correction and changed facts conflict |
| timing-rejection | Both occurrence endpoints and exact report deadline tested |
| correction-timing-rejection | Exact correction deadline rejects without append |
| cap-incompatibility | Capped target/outcome combination rejects |
| aggregate-limit-rejection | Second family would exceed gross premium maximum; first claim remains intact |

## Identity and manifest summary

`H(k,v) = SHA256(UTF8("ledgerlab/"+k+"/2-candidate.1") || NUL || JCS(v))`.
IDs use domain-specific `*2_` prefixes and the full lowercase digest.

- Claim: `[scope,target,agreement_id,book,family_id]` — permanent, independent
  of delivery label, outcome code, amount, source and policy version.
- Revision: `[claim_id,number]`; effect: `[claim_id,revision_id,slot]`;
  action: `[effect_id]`. A correction explicitly names expected current revision.
- Obligation: `[scope,agreement_id,book,currency,scale,roles]`.
- Intention: `[scope,destination,obligation_id,sorted_action_ids]`.
- Decision/receipt: `[event_id]`, each in its own domain.
- Evidence/policy/basis/replay IDs bind `[scope,body]`. Event IDs bind
  `[scope,source,external_id]`. The design lists every remaining formula and
  four unhashed structured composite keys.

All body content hashes use `[kind,2,body]` in `record-content`, except manifests
in `decision-content`. A manifest contains the exact sorted union of all new
immutable rows except its own manifest/receipt and all retained replay inputs.
Original receipts remain immutable, including the original receipt of a claim
that is later corrected. Candidate byte rules retain v1's strict UTF-8/JCS,
UTF-16 key order, omission/null distinction, exact atoms and signed rounding;
the version/domain separation prevents silent v1 reinterpretation.

## Validation

`sh scripts/check.sh` passed on the supplied pinned Rust 1.98.1 development
toolchain: **110 Rust tests passed, zero failures, 13 explicitly ignored gates**;
formatting, warnings-denied Clippy, no-default compilation, crate/source
boundaries, all frozen-contract audits and the candidate checks passed.
Focused contract checks were rerun after final contract-audit/document changes.

Candidate audits pass Python reconstruction of every golden byte, independent
Node identity/hash/manifest reconstruction, closed schemas, reference/pin/
lineage/rounding/inverse/limit/receipt validation and **23 negative checks**,
including rehashed incomplete aggregate evidence, an inexact rehashed inverse,
and a stale correction. `review-manifest.json` hashes the complete candidate
review package and remains explicitly pending independent review.

The 99 frozen files, 60 original hash vectors, 25 first-slice accepted rows,
29 first-slice manifest members, original receipts and 80-atom v1 result remain
unchanged. No frozen checker/inventory rule was weakened. Only an additive call
to the new candidate audit was added to `scripts/check-contracts.sh`.

The full script's existing ignored PostgreSQL-server/other opt-in gates were
not converted into passes. These contract tests establish no new store,
authentication, concurrency, dispatcher or product support claim. Synthetic
seed economics and authority evidence are explicitly structural fixtures.

Reproduction uses the repository's pre-existing local toolchain activation,
`RUSTUP_TOOLCHAIN=stable`, and the same Python contract-audit packages specified
in `scripts/requirements-contracts.txt`. No new dependencies were installed.
Run `sh scripts/check-candidate-contracts.sh`, `sh scripts/check-contracts.sh`
and `sh scripts/check.sh`. Checks never regenerate goldens.

## Unresolved review choices and integration instructions

1. Independently review the proposed claim tuple, one-family command shape,
   scope-bound content IDs, distinct candidate hash profile, exact manifest
   membership and permanent original-receipt behavior. Reconcile the separate
   semantic/core/oracle lanes before assigning a production profile/version.
2. Review the dedicated correction deadline and pinned-original-policy rule.
   Correction is a bounded atomic inverse-plus-replacement exception to v1's
   generic reversal restrictions, only for uncapped outcome claims.
3. Review same-group gross premium/discount limits, common maxima across
   families, frozen original-net basis and all-ancestor intention dependencies.
4. Production v1-to-target decoding must verify original receipts/postings
   without rehashing or rewriting them. The seed adapter, historical evaluator
   decoder, actual authority evidence, supplier invocation/reservation state,
   ordered guards and real two-store races remain later owned work.
5. This complete-prefix replay encoding is deliberately bounded. Any compact
   closure representation or multi-family atomic command needs a separate
   reviewed encoding; do not silently truncate a snapshot.
6. Integrate by reviewing this branch's isolated commit/diff, then let the
   integration owner cherry-pick it. Run the full checks on the combined tree.
   Keep candidate paths, frozen inventories, original first-slice assembler and
   production write-version declarations unchanged. Do not wire these records
   into acceptance/persistence until independent review and the corresponding
   implementation/transaction gates pass.

The candidate is ready for independent review. Automated audits and local
self-review are not an approval to freeze, merge or enable production writes.
