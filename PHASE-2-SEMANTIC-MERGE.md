# Phase 2 semantic/core/canonical integration handoff

This bounded lane integrates exact semantic freeze
`1e0ba3f886788c08f427d3aae1d916b341187e76` onto exact canonical freeze
`3e56ff172d4dba326272c66c85512c8a481f7a52`. The owner explicitly authorized
this local Phase 2 lane after the Phase 1 freeze. It does not close the full
Phase 2 integration gate or authorize Phase 3 persistence.

## Integration and scope

- Branch: `codex/p2-semantic-merge` in a separate no-network local clone.
- The merge has canonical freeze as its first parent and semantic freeze as its
  second parent. Their common ancestor is
  `b35258425970052ed71481eca1f33ef857c61be1`.
- No merge conflicts or manual semantic resolutions were needed. The semantic
  commit supplies exactly 20 changed files in core/testkit. This report is the
  only additional tracked change.
- All 78 tracked files under core/testkit match the semantic freeze byte for
  byte. No authority, identity, timing, rounding, correction or immutable-history
  rule was changed during integration.
- All 159 registered frozen files, including all 99 v1 files, remain byte-identical
  to the canonical freeze and match their registered SHA-256 digests. The freeze
  registry itself is also byte-identical. No fixture writer was run.
- No shared manifest, lockfile, dependency, toolchain pin, acceptance port,
  storage, migration, outbox or CLI changes were necessary.
- `ROADMAP.md`, `PHASE-1-FREEZE.md`, candidate labels and review records are pinned
  historical bytes. They remain unchanged; this separate handoff records the
  subsequently authorized integration work without rewriting that evidence.

## Validation

`sh scripts/check.sh` passed completely offline with Rust 1.98.1:

- **113 Rust tests passed, zero failed, 13 ignored.** The ignored tests are the
  11 existing explicit local PostgreSQL opt-in tests and two independent fake
  destination process-restart gates. They are not new passing evidence.
- Formatting, warnings-denied Clippy, all-target/all-feature tests, no-default
  compilation, resolved dependency/source boundaries and negative probes passed.
- Frozen v1: 60 hash vectors, 25 accepted immutable records, 29 manifest members,
  original receipts and the 80-atom result reconstructed independently in Python
  and Node; all 99 original file pins preserved.
- Frozen outcome profile `2-candidate.4`: 27 record kinds, 24 histories,
  1,450 records, 43 decisions, 23 retained original Evaluations and 218 identity
  mappings; reconstruction checks existing bytes rather than regenerating them.
- 49 fully rehashed semantic attacks (3,570 records/109 decisions), five rehashed
  scalar attacks (205 records/five decisions), 272 packaged negative assertions,
  and four freeze-metadata rejection checks passed.
- 217 scalar cases, 38 field byte boundaries, and Python/Node text/source parity
  for 1,112,064 Unicode scalar values (2,224,128 checks per runtime) passed.
  The accepted U+FEFF regression contributes another 41 records/one decision.

The frozen exact-commit comparison runner also passed: one Rust scalar/Unicode
comparison test and six outcome tests, zero failures. It verified 24 complete
typed Evaluation roundtrips with original IDs/fields/vectors, 44 decisions,
duplicate-document rejection, document reuse and original-receipt retry,
11 deadline/ordering cases, cross-cancellation, all five rehashed scalar
rejections, 217 scalar cases and exhaustive Unicode parity against approved Rust.
Its original semantic suite compared 86 attempts across 23 histories and retained
input replay; the integrated suite also exercised the five legacy discrepancy
histories and 8,442 signed arithmetic combinations.

Operational logs and disposable test adapters remain ignored under `work/`;
no local credentials, databases or toolchains are committed.

## Reusable offline environment

No packages were added or downloaded. The audit uses the existing prepared
Python dependency directory, not the system interpreter's default packages:

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
export CARGO_NET_OFFLINE=true
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
sh scripts/check.sh
python3 scripts/contract_checks/v2_candidate/compare_semantic.py --source "$PWD/work/approved-semantic"
```

The observed Python executable is
`/opt/homebrew/opt/python@3.14/bin/python3.14` (3.14.7).
`jsonschema` 4.25.1 and PyYAML 6.0.2 resolve from the `PYTHONPATH` above.
The existing Rust activation supplies `CARGO_HOME`, `RUSTUP_HOME`, the Git wrapper
and `DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Selecting installed
`stable` yields the pinned Rust/Cargo 1.98.1 without a rustup download.
No Xcode license acceptance or global developer-tool change was needed.

`work/approved-semantic` is a detached worktree of this clone at exact
`1e0ba3f886788c08f427d3aae1d916b341187e76`, created locally with
`git worktree add --detach work/approved-semantic 1e0ba3f886788c08f427d3aae1d916b341187e76`.
The frozen comparison runner requires that exact HEAD. It archives the approved
commit into another disposable directory, injects test-only serialization
adapters there, and checks the frozen material. Neither approved production
source nor the frozen comparator is edited. The independent 78-file equality
check ties that archived semantic code to this integrated tree.

## Integration-owner instructions and remaining gates

Fetch this branch from the reported absolute local clone path and integrate its
merge commit into the owner's isolated integration branch. Preserve both freeze
ancestors. If applying it by cherry-pick instead, it is a merge commit: first
parent `-m 1` is the canonical baseline. Do not cherry-pick the semantic changes
again afterward. Re-run the complete combined suite after other lanes merge.

No shared-file exception is required. Keep the frozen registry and every pinned
file byte-identical; an interface issue cannot be resolved by rewriting the
contract or lowering checks. Use the prepared Python environment above.

The owner still owns the outbox/CLI hardening lanes, any schema-upgrade tests and
combined integration review. This lane adds no migrations. Production historical
decoding/v1 bridging, authenticated complete history, target/claim/aggregate/
authority/base-reversal/supplier-capacity locking, and atomic both-store
base → outcome → correction persistence remain later authorized work. Preserve
original receipt lookup, complete target membership, inclusive receipt/acceptance
deadlines, exact signed rounding, inverse plus replacement, zero claims and
reservation history when implementing that path. Typed outcomes still cannot be
appended through the existing Phase 1 acceptance port.

Stop after this scoped local commit. No main merge, push, publication, deployment,
service registration, remote model use or spending occurred.
