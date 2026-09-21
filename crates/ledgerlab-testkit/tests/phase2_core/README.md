# Proposed histories compared with the production pure core

`../phase2_core.rs` reads the independent, unfrozen `phase2-proposed-v1` histories
and calls `ledgerlab_core::policy::chaining`. This adapter lives only in an
unpublished test target. Production never imports the Python reference or these
fixtures. Expected results are not supplied to input construction or evaluation;
the adapter contains no pricing, rounding, cap, share or reversal equations.

The reference self-tests remain separate and unchanged: 29 histories, 86
submissions, six dependency-arrival permutations and 13 Python tests. This bridge
compares the subset with common meaning; it does not certify every proposal as
production behavior.

## Compared behavior

Twenty-one histories exercise 48 successful evaluations, 18 refusals/waits and
three pure-core duplicate guards. Two additional refusal steps are explicitly
excluded. The test compares every accepted decision's complete common projection:

- Every posting's component, action kind, book, binding, six roles, amount,
  named input bases and reversal reference. Currency and scale are checked
  against the pinned configuration before projecting atoms.
- Exact explanation code, rational intermediate, rounded atoms and named basis
  whenever represented in the proposal.
- Per-binding obligation amounts, roles, book and posting membership; economic
  dependencies including explanation-only cap dependencies.
- State after every step: revision, stage closure, reversed decisions, consumed
  invocation exposure and recorded totals per book. Prior core results are
  reprojected after each step and must retain their original projection.

The aligned histories are `cap-below-booked`, `cap-not-binding`, `cap-zero`,
`component-rounding`, `exposure-exceeded`, `failed-completion`, `funding-byok`,
`funding-platform-known`, `funding-platform-unknown`, `generation`,
`invalid-links-authority-order`, `later-acquisition`, `missing-invocation`,
`multi-capped`, `multi-uncapped`, `out-of-order`, `pay-per-service`,
`retroactive-invocation`, `share-ceiling`, `tier-enterprise`, and `tier-standard`.

## Representation mapping

- `story-tariff/1` selects its pinned tier's concrete typed core rules. Integer
  fixed-price atoms become decimal digits at the configured scale; no money
  calculation happens in that conversion. Input versions remain test-only.
- Document aliases become deterministic synthetic document IDs tagged
  `comparison-alias`. They are opaque references, not canonical assent/policy
  encodings or proof of authenticity. Events use `comparison/sandbox` scope.
- Relative microseconds map to the synthetic Unix epoch. The omitted standing
  retail quantity limit is explicitly 100. Supplier invocations retain their
  fixture quantities/exposure; their synthetic outcome deadline is 999999 µs,
  later than these histories' binding windows. These defaults are test context,
  not production admission or policy defaults.
- The fixture's pinned invocation supplies the event binding ID. The next call's
  held capacity comes from the core's actual consumption/release proposals.
  Known cost evidence is supplied explicitly. This feedback is test state, not
  a persistent authority/reservation implementation.
- Core IDs project to stable `event/component` aliases. Posting, explanation and
  obligation arrays are sorted for comparison because the proposal's presentation
  order differs from the core's deterministic ID order, especially for reversals.
  No canonical bytes or canonical ordering claim follows from that sorting.
- `COST_OBSERVED` maps to the proposal's `BASE_APPLIED` observation code. An
  inactive cap has extra exact-zero diagnostics in core; these are asserted before
  projecting the proposal's smaller explanation. Refusal-code mappings are
  enumerated in `core_refusal`, not treated as interchangeable successes.

## Explicit boundaries

| History/step | Disposition |
|---|---|
| `later-quality`, `quality-not-v0` | Excluded: quality kind/relation and prior booked-net basis differ from core's typed acquisition/component discount. No translation or semantics decision. |
| `missing-assent` | Excluded: retained document existence/authenticity is coordinator work. |
| `unknown-config`, `unknown-policy`, `unknown-semantics` | Excluded: synthetic story version selectors are not production selectors. Existing production parser tests remain separate. |
| `multiple-shares` | Separately asserts core bundle compilation rejects `POLICY_LIMIT`; the oracle rejects later at acquisition. Earlier history is not compared. |
| `payer-delegation-required` | Separately asserts core bundle compilation rejects `PAYER_DELEGATION_REQUIRED`; real delegation lookup remains a coordinator gate. |
| `invalid-links-authority-order/wrong-source` | Excluded: the oracle knows globally unique aliases and reports `LINK_SOURCE`; production scopes the endpoint by source and can report missing dependency. |
| `invalid-links-authority-order/missing-proof` | Excluded: a supplied opaque document ID cannot prove evidence retention in a pure call. |
| Three proposed duplicate attempts | Core must return `CLAIM_CONFLICT` without new economics. Original-receipt lookup and semantic delivery aliases are not reimplemented here. |

The inventory test requires every one of the 29 history files to have an explicit
disposition. Attempt/result lengths and accepted journal lengths must match.
Expected files and Python oracle were not changed to accommodate the adapter.

The proposal does not expose the full release/reservation protocol, canonical
authority/source/link provenance, wire bundle encoding, allocation views or all
core DSL programs. This test therefore cannot certify those surfaces. Existing
core tests exercise additional typed behavior, and Phase 1 real-store tests
separately prove original receipt and commit semantics. A reviewed Phase 2 decoder,
assembler and coordinator bridge are still required.

```sh
source work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
cargo test -p ledgerlab-testkit --test phase2_core --locked --offline -- --nocapture
```
