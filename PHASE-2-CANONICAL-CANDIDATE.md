# Canonical candidate.4 — Unicode parity correction

**Candidate `2-candidate.4`; not frozen. Fresh-context review pending.**
Branch: `codex/phase2-canonical`.
Reviewed candidate: `abe9781b4a37b0bb23ee86db2e5a7b6786694c80`.
Authoritative semantics: `1e0ba3f886788c08f427d3aae1d916b341187e76`.
Integration base: `b35258425970052ed71481eca1f33ef857c61be1`.

The earlier cycle addressed original identities, resolved evidence and whole-string
scalar spelling. Fresh review of `abe9781` confirmed one remaining blocker:
Node rejected U+FEFF inside a source, although Python and the approved Rust core
accepted it. This follow-up corrects that runtime classification. Only canonical-contract
sources, schemas, synthetic fixtures and review documentation change.
`ROADMAP.md` remains byte-identical. Production core, CLI, outbox, stores,
migrations, Cargo manifests/lockfile and all frozen-v1 files remain unchanged.
The authoritative sibling is read-only at its approved commit; no sibling code
is integrated. No freeze, merge, push, deployment, later-phase work or spending
is authorized here.

## Unicode correction and regression evidence

Node now uses Unicode `White_Space`, matching approved Rust `char::is_whitespace`,
instead of JavaScript `\s`. U+FEFF is neither White_Space nor Unicode Cc, so a
source such as `urn:synthetic:\uFEFFoutcome` remains valid and is preserved exactly.
Python and Rust semantics were not tightened and no source normalization was added.

An exhaustive check runs every **1,112,064 Unicode scalar value** through text
and source validation in Python, Node and the exact approved Rust archive:
**2,224,128 classification checks per runtime**. All agree on 65 rejected text
characters and 84 rejected source characters. This includes U+FEFF, C0/C1
controls, all Unicode whitespace, format characters, supplementary characters
and noncharacters. Surrogates are excluded because they are not Unicode scalar
values; existing malformed-JSON checks reject them separately.

The positive regression rewrites `urn:synthetic:outcome` to the U+FEFF source
throughout fixed-success-fee, recursively inside every canonical source string,
including the original Evaluation, binding, policy/evidence and decision
observations. It rebuilds every affected ID/hash/reference/manifest/receipt.
Python and Node accept the entire **41-record/one-decision** history against its
newly derived original base root. Approved Rust confirms exact complete typed
Evaluation equality, target freeze and the accepted outcome. This fixture is
independent of previously accepted roots, which remain immutable.

The existing 24 goldens (1,450 records/43 decisions), schema and all 23 captured
original Evaluations remain byte-identical to `abe9781`. The additional accepted
regression lives in reproducible audit diagnostics, yielding **24 exact accepted
Evaluation roundtrips and 44 decisions** in the approved comparison. The profile
stays `2-candidate.4`: the fix restores Node to its existing source contract and
does not alter hash domains, source semantics or serialized golden bytes.

## Preserved contract corrections

1. **Actual original Evaluation identities.** The candidate retains the complete
   actual approved-core Evaluation, captured in `original-evaluations.json`,
   including original event/claim/action/effect/obligation/document identities,
   provenance, dependencies and all other fields. No candidate posting ID or
   synthetic claim placeholder replaces an original identity. Every accepted
   base has explicit canonical one-to-one mappings qualified by original and
   candidate target. Exact postings, obligations, bindings and evidence are
   checked against native source; `base-identity` rows cover remaining native
   identities. Missing, extra, duplicate, ambiguous, cross-target and inconsistent
   mappings reject even with the entire hash graph rebuilt. The retained source
   and projection must agree field-for-field after the declared enum/event
   translation. There are 218 mappings across 23 accepted originals.
2. **Resolved-document evidence sets.** Request, verified, explanation and retry
   evidence sets reject repeated original `doc_` IDs, even through distinct
   valid wrappers. Claim facts hash sorted resolved document IDs. A later
   correction validly reuses an earlier document through a different wrapper;
   retrying the original claim with that document returns its original receipt.
   New evidence remains bound to the exact decision and cannot alter frozen
   target membership, pricing, capacity or authority.
3. **Whole-string canonical spelling.** Schema patterns use an absolute end;
   Python additionally uses fullmatch. Python and independent Node validators
   check integer/ratio spelling before numeric conversion. Atoms, ratios, IDs,
   counters, slugs, decimals and timestamps reject trailing newlines/whitespace
   and alternate spellings, including inside retained source bytes. Direct
   vectors and fully rehashed histories agree with the approved Rust parsers.

All earlier corrected economics and guards remain: retail-net percentage basis,
permanent book/version-independent claim identity, frozen complete target
membership, inclusive receipt/acceptance deadlines, inverse/replacement behavior,
zero claim ownership, selected binding provenance and separate supplier capacity.

## Inventory and attack evidence

- **27 record kinds; 24 histories; 1,450 record vectors**: 542 seed/preparation
  records and 908 appended records, including three decision-time proofs.
- **43 accepted decisions**, 25 permanent outcome claims, 43 revisions, 45 actions
  and effects, 61 explanations, 38 intentions and 23 original base roots.
- **23 complete native Evaluation sources**, 218 original-to-projection mappings
  and 50 `base-identity` rows. The capped preparation remains rejected with no
  accepted Evaluation or original base receipt.
- **49 fully rehashed semantic attacks** pass Python and Node schema/hash
  integrity before semantic rejection: 3,570 records and 109 decisions.
- **Five fully rehashed scalar histories** pass Python/Node hash-only integrity
  (205 records/five decisions), then reject in Python, Node and approved Rust.
- **272 negative assertions**: 49 semantic attacks, five scalar histories, one
  book-key invariant, ten malformed JSON inputs and 207 scalar/field checks.
- **217 shared scalar cases**, **38 record-field byte boundaries**, two structural
  source roundtrips and exact-rational cross-cancellation checks.

## Validation and exact original-core comparison

The complete offline repository suite (`sh scripts/check.sh`) passes formatting,
workspace/all-feature tests, warnings-denied Clippy, no-default compilation,
dependency/source boundaries, all frozen-v1 audits and the candidate audit:
**110 Rust tests pass; 13 opt-in/later gates remain ignored**. Candidate goldens
are reconstructed byte-for-byte without regeneration. Independent Node checks
schema/scalars, all candidate keys/hashes, original document/event hashes,
references, membership and receipt bytes.

The reproducible comparison runner first audits current candidate bytes, then
archives exact approved commit `1e0ba3f` under ignored `work/`. Test-only serde
adapters are appended inside that disposable archive. Every actual approved
Evaluation is encoded, decoded into the real typed Evaluation, re-encoded and
compared byte-for-byte with retained original material. All original fields,
IDs and vectors remain exact; no identity normalization hides differences.
Fixed-success-fee retains original action
`ac_c541ddb16b28b7fb383f096c2bc32d5b62998d54279926b04293d6b7392ac8ea`.

The approved Rust comparison passes 24 complete Evaluation roundtrips,
44 decisions and duplicate-document request rejections, the document-reuse
correction/original-receipt retry, 217 scalar cases, exhaustive Unicode
text/source parity, the accepted U+FEFF history, all five rehashed scalar
histories, 11 deadline/ordering cases and percentage cross-cancellation. Its
original semantic suite still checks 86 attempts across 23 histories.

All **99 frozen-v1 file hashes**, 60 original vectors, 25 first-slice records,
29 manifest members, original receipts and the 80-atom result remain unchanged.
No dependencies, remote infrastructure or production behavior were added.
These examples do not certify production historical decoding, authentication,
concurrency or persistence.

## Compatibility and review handoff

Candidate `.3` remains in Git history. The original `.4` cycle changed required fields,
record kinds, scalar acceptance, retry facts and hash domains, so candidate IDs
and receipts change intentionally. There is no automatic migration from earlier
projections: missing original identities/dependencies cannot be recovered by
substituting candidate IDs or guessing source fields. Re-author from retained
original material and verified observations, then review. No v1 compatibility
or support declaration changes.

A fresh-context reviewer must decide whether the U+FEFF blocker is resolved and
whether freeze is justified. Remaining later-integration questions are the
production historical codec/v1 journal bridge without rewriting receipts,
authentic durable base-root retrieval and complete history under locks, and
supplier reservation observations through the combined coordinator/both-store
implementation. The stop condition is a committed candidate and clean worktree;
this report does not authorize freeze or subsequent phases.
