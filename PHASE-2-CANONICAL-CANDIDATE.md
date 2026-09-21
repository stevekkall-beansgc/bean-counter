# Canonical candidate.3 — independent-review corrections

**Candidate `2-candidate.3`; not frozen. New fresh-context review pending.**
Branch: `codex/phase2-canonical`.
Reviewed prior candidate: `e8139e6df68880ae9c520ee100a05dcfd0ede673`.
Authoritative semantics: `1e0ba3f886788c08f427d3aae1d916b341187e76`.
Integration base: `b35258425970052ed71481eca1f33ef857c61be1`.

The owner authorized correcting the independent review's five findings. Only
canonical-contract work was performed. `ROADMAP.md` is preserved byte-for-byte.
Production core, CLI, outbox, stores, migrations, Cargo manifests/lockfile and
frozen v1 files remain unchanged. No sibling commit was integrated.

## Exact schema and validator corrections

1. **Decision-time evidence.** Evidence records may be appended with a claim or
   correction. Every new proof must be used by that event, verified by its exact
   authority observation, named by the explanations, and included in replay and
   manifest membership. Existing evidence can be reused. New evidence cannot add
   target families, bindings, policies, money or capacity; seed membership and
   the original base root stay immutable.
2. **Original policy verification.** Target snapshots retain full `policy_utf8`,
   `policy_document`, `policy_document_hash`, `verified_policy_document` and
   `policy_evidence`. Original Policy.document and the verification observation
   must agree. Supporting evidence retains original doc identity/hash and bytes.
   Full policy terms must match their document and family/limit projections.
3. **Lossless base material.** Base evaluations retain original event/ingress
   canonical bytes and original v1 identities/hashes. Extensions, links, corrects,
   supplier nomination and omitted optional fields survive. `binding_utf8`
   preserves every approved Binding field, including complete `outcome` terms.
   Closed typed schemas cover the complete retained bundle/rules/operations,
   context, actions/provenance, explanations, deltas, consumptions, invocations,
   source authority, costs and optional claim/closed-stage fields. Projections
   must derive from the source; unknown fields reject rather than disappear.
4. **Authorized binding references.** Actions and effects carry binding_id,
   binding_snapshot and component. Every original outcome, replacement and
   inverse must use the frozen binding for the target/agreement/family, with
   matching roles, book, currency/scale and family component. Inverses preserve
   original binding provenance. Base action bindings/components are also checked
   against complete original bundle material.
5. **Scalar constraints.** Schema-directed custom keywords enforce UTF-8 byte
   bounds on all bounded strings, including embedded source values. Positive
   Binding/work quantities, nonnegative Decimal/exposure/capacity fields,
   Gregorian timestamps, URI/slug/control constraints, money/ratio/counter bounds
   and normalized representations are checked consistently. Percentage arithmetic
   now follows the approved core's cross-cancellation before intermediate bounds.
   `contracts/candidates/v2/SCALARS.md` documents the complete scalar audit.

Approved economics remain: retail-net percentage basis for both books, permanent
book/version-independent claim identity, frozen complete target membership,
separate inclusive ordinary/correction receipt and acceptance deadlines, exact
inverse/replacement semantics, zero claim ownership and independent supplier
capacity. Source Bundle.outcome duration limits remain separate from the approved
outcome-family windows. Unused bindings need no new premium limit; invocation
exposure may be less than binding maximum, as in the approved core.

## Inventory and evidence

- **26 record kinds; 24 histories; 1,375 record vectors**: 492 seed/preparation
  records plus 883 appended outcome/correction records (including two new proofs).
- **42 accepted decisions**, 25 permanent claims, 42 claim revisions, 44 actions
  and effects, 59 explanations, 37 intentions and 23 original base roots.
- The capped preparation remains rejected with no accepted base receipt.
- New valid histories cover decision-time claim/correction evidence, lossless
  event extensions with complete Binding.outcome terms, and cross-binding
  corrections with both inverse and replacement actions.
- **37 fully rehashed attacks** pass Python and Node structural/hash integrity
  before semantic rejection. They include unverified/unbound evidence, attempted
  family/money injection, policy identity/hash/terms mismatch, discarded original
  fields, and unauthorized/cross-family original/replacement/inverse bindings.
  Node checks 2,498 records and 78 decisions across those attack histories.
- **110 negative assertions** total: 37 semantic attacks, the book-key invariant,
  ten malformed JSON inputs and 62 scalar/field/normalization assertions.
- **61 scalar cases**, **38 record-field byte boundaries**, two lossless source
  round trips and exact-rational cross-cancellation are checked separately.

## Validation

The complete offline repository suite (`sh scripts/check.sh`) passes formatting,
workspace/all-feature tests, warnings-denied Clippy, no-default compilation,
dependency/source boundaries and frozen/candidate audits: **110 Rust tests pass;
13 opt-in/later gates remain ignored**. Python reconstructs every golden byte;
Node independently reconstructs identities, hashes, original document/event
hashes, references, manifests and receipt bytes. Audits never regenerate goldens.

A reproducible comparison runner archives exact approved commit `1e0ba3f` under
ignored `work/` and appends temporary dev tests there. The authoritative sibling
remains clean and unchanged. The approved Rust code checks:

- 61 scalar cases against the same accepted/rejected boundary values;
- all 23 candidate accepted base examples and all 42 decisions: original event
  bytes/IDs, complete typed bindings including outcome terms, verified policy
  document, retail basis, monetary results and action/explanation counts;
- 11 exact deadline/ordering cases and fresh claim/correction evidence;
- percentage cross-cancellation and policy-document mismatch rejection;
- the original semantic suite's 86 attempts across 23 histories.

All **99 frozen v1 file hashes**, 60 original vectors, 25 first-slice records,
29 manifest members, original receipts and 80-atom result remain unchanged.
No new dependencies or remote infrastructure were used. These contract checks
do not certify production authentication, concurrency or persistence.

## Compatibility and review handoff

Candidate `.2` remains at the prior commit. `.3` changes required fields, schema
discriminators and hash domains, so candidate IDs/receipts change intentionally.
There is no automatic migration: missing original material/observations cannot
be reconstructed from `.2` projections. Re-author only from original retained
sources, then review. No v1 bytes, formulas, compatibility declarations or support
claims change. The full design and proposed ADR describe the mapping.

A **new fresh-context reviewer** must decide whether all five findings are
resolved. Remaining review/integration questions are the production historical
codec/v1 journal bridge without rewriting receipts, authentic durable base-root
retrieval and complete history under locks, and supplier reservation observations
through the combined coordinator/both-store implementation. The checked typed
examples are not a complete production decoder certification.

Stop condition: committed candidate and clean worktree. No freeze, merge, push,
deployment, later-phase implementation or spending is authorized by this report.
