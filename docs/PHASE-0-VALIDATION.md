# Phase 0 validation

Status: local contract freeze and minimal scaffold complete; ready to start the bounded Phase 1 slice. No production engine, store, HTTP service or delivery path is implemented. No package, image, remote repository or release was published.

## Verified environment

- Rust 1.98.1, commit `48a229ceaefd4985c50990b14116b6d856af0985`, LLVM 22.1.8, ARM64 Darwin.
- Cargo 1.98.1, rustfmt 1.9.0, Clippy 0.1.98. Development compiler is pinned; no MSRV claim.
- Git 2.53.0; Python 3.14.7; Node 22.23.2. Exact Python test dependencies are in `scripts/requirements-contracts.txt`.
- Local host macOS 26.6.2 (25G83). This does not certify the macOS 15 or Linux reference environments.
- Cargo lockfile has only the four local workspace packages. No external Rust dependency, driver or database version has been resolved.

## Passing checks

`sh scripts/check.sh` runs offline Rust formatting, workspace tests with all targets/features, Clippy with warnings denied, no-default-feature checks, resolved graph/source boundaries, then independent document/fixture checks. The prepared machine used its installed `stable` alias, whose actual compiler version was checked against the 1.98.1 pin.

| Check | Evidence |
|---|---|
| Workspace | Exactly three production packages plus unpublished testkit; `ledger` binary target. All four compile. Rust test targets have zero behavioral tests because implementation has not started. |
| Dependency/source boundaries | Default, all-feature and no-default resolved graphs; forbidden transitive dependency injection and seven source negative probes reject. |
| Source documents | All five complete source snapshots retain their original SHA-256 hashes; canonical repair addendum is unchanged. |
| Detailed design | 12 JSON blocks, six YAML blocks, six event examples and three policy examples; 15 exact-rational arithmetic groups and ten published semantic IDs. |
| Schema inventory | 23 schemas including release evidence; all Draft 2020-12 metaschema and locally resolved reference checks pass. |
| First-slice record oracle | Python and separately written Node independently reconstruct 60 vectors, six seed documents, four seed immutable records, 25 accepted immutable records, 29 manifest members and all 16 fixture files byte for byte. |
| Negative cases | 26 original canonical-record checks plus 19 supplemental parser/schema/decimal/internal-ID checks. Rehashed invalid journals reject. |
| Wider semantic fixtures | Five economic journals: first-party, third-party, capped, BYOK and reversal; 13 authority truth-table scenarios. These are document oracles, not executed acceptance tests. |
| Failure schedule | 27 per-item writes, 54 before/after positions enumerated. No fault injection against a real store has run. |
| Freeze | 99 schema, fixture, ADR, source and metadata files pinned by SHA-256; checks never rewrite expected values. |

Exact immutable journal SHA-256: `51c768879cdd78a0bbcbe436d3ca91c3f6bdeb0df15f02d47e9f3e1ba67eb1a2`.

First-slice decision hash: `sha256:33dd38b0ae18a1a037b18494115b3689c47e9378e9af466fd57837e70091c650`.

Repair addendum SHA-256: `2a28e800a9037de36c9e82070225632eca449be16c26a74e11336c210464c0e8`.

Machine-readable check results and complete local logs are generated under ignored `work/validation/`. They report document checks separately from Rust builds. The canonical checker's historical statements about broader Phase 0 work describe that checker's limited scope, not a reopened encoding blocker.

## Limits and handoff

See [implementation lanes](implementation.md) and [remaining gates](phase-gates.md). Independent core, coordinator/SQLite, PostgreSQL/TLS and testkit work may start in isolated worktrees, with shared ports and dependency pins owned by integration. Full-v0 acquisition/link/reversal and supplier/transition record variants require reviewed encoding extensions before their later implementation phases. JSON Schema is structural: the explicit internal EventId prefix and other prose constraints remain mandatory.

Transaction cleanup, cancellation, unknown commits, real-store equivalence, TLS trust, driver/dependency pins, SQLx offline-query approach and MSRV are Phase 1 gates. Namespace ownership, actual native runners and release certification remain unresolved. This local result makes no CI, cross-platform or runtime-correctness claim.
