# M3 qualification: continuous history and concurrency

**Result:** qualified for the named source-built, single-host local SQLite profile on macOS. This records one synthetic workload and focused migration, retry, concurrency, and snapshot evidence. It does not certify native packages or establish capacity, throughput, or completion guarantees at the admission ceilings.

## Candidate and environment

- M3 implementation source commit: `e2ce81a7f6c0027f661be94b9952a31d60366167`.
- Source-built M3 CLI SHA-256 used by the schema-9 migration exercise: `de3bf512dcb7e5b6a37145a05090ca2ce97b733e6ffd53513175cd8d9835d06d`.
- M2 source commit: `685ea0aa1f5f5a79042802bfc2d9ccfb1aa2d00f`; M2 CLI SHA-256: `f9559b82a711df0b5e204e0f8386a17a8e0864d3d6812c44111a2a9f75e31b6c`.
- Host and toolchain: macOS 26.6.2, Apple silicon (`aarch64-apple-darwin`), Rust 1.98.1, locked dependencies, offline build.

The external schema-9 capture records that it was first produced in a dirty worktree based on `ae03a62df72faad6cfc591237eece6bd95cbeeb9`. To reconcile that record with the release source, all 34 tracked or added candidate files were compared with commit `e2ce81a7f6c0027f661be94b9952a31d60366167`; their contents match byte-for-byte. The recorded M3 binary hash above is the binary used by that migration exercise. This identifies the measured M3 source content with the implementation commit while retaining the capture's original worktree metadata; it is not a claim that the original harness ran from a clean checkout.

## Retained history, retries, and concurrency

The separately invoked acceptance gate `m3_acceptance_retains_decisions_1001_and_later_on_three_fresh_installations` submitted decisions through ordinal 1,026 on each of three fresh installations: the first crossing was ordinal 1,001, followed by 25 further decisions. After reopening, the oldest, crossing, and final identity retries resolved to their original receipts on each installation.

Run this ignored acceptance test explicitly with `cargo test -p ledgerlab --test m3_history m3_acceptance_retains_decisions_1001_and_later_on_three_fresh_installations -- --ignored --exact --nocapture`.

The non-ignored `m3_history` integration suite passed 6 tests. It includes a barrier race where identical submissions produce one economic effect and conflicting reuse refuses, an overlapping writer/snapshot stream reconciled to its cutoff, and oldest-identity retry after reopen. The large workload below also verifies the final complete statement and CSV against independent receipt-chain and integer-total models.

## Measured synthetic workload

Command: `cargo run -p ledgerlab --release --example m3_qualification -- --mode large --decisions 2500`.

| Measure | One-run result |
| --- | ---: |
| Base submissions accepted | 2,500 |
| Outcome submissions accepted | 833 |
| Correction submissions accepted | 166 |
| Retained decisions | 3,499 |
| Exact identity retries / semantic aliases | 251 / 100 |
| Total submissions answered | 3,850 of 3,850 |
| Workload / total elapsed time | 47.682 s / 101.165 s |
| Final statement cutoff and net | 3,499 decisions / 608,350 atoms |
| Final CSV | 2,999 postings; 608,350 atoms |
| `.ledger` files after close | 125,632,512 bytes in aggregate |

Complete statements and CSVs at cutoffs 10, 100, 1,000, and 3,499 reconciled with the independently maintained receipt chain and total. The harness removed its temporary installation after the run. RSS was unavailable because the sandbox refused to start `ps`; this result gives no memory measurement. Each candidate write validates the retained-history meter during metadata read and again before append, scanning retained rows both times. Full-history validation and reporting also grow with retained data; the full snapshot's M3 lookup builds a query from retained ordinals. These timings and file sizes describe this one workload only; the 100,000-entry and byte limits are hard refusal ceilings, not tested capacity claims.

## Schema-9 migration and recovery

The source-built M2 CLI at the exact M2 commit above created a schema-9 fixture with three retained economic entries (base, outcome, correction), one semantic alias, two permission changes, and agreement history. The source-built M3 CLI explicitly upgraded it from schema 9 to schema 10.

The complete statement, permissions, retained row counts, and exact base, alias, outcome, correction, agreement, and permission retries matched before and after migration. A schema-9 billing open before explicit upgrade returned `BILLING_UPGRADE_REQUIRED` without logical storage changes. A schema-9 non-billing store reopened through the shared core path without implicit migration; its `user_version` remained 9 and its logical rows were unchanged. The older M2 writer refused the schema-10 installation with `INTEGRITY_FAILURE` and made no persistent installation-file changes.

A whole-installation backup made before migration matched its source tree. Restoring it to a new directory preserved the statement and permissions; the restored schema-9 installation upgraded to schema 10 and resolved the original identity retry.

The M3 migration tests also exercise the direct schema-8-to-schema-10 code path using source-built in-process fixtures. The external old-writer exercise above qualifies the named schema-9-to-schema-10 transition. Together with [M2 qualification](m2-migration-qualification.md), this does not qualify native artifacts, another source history, another operating system, or every historical store.

## Checks run on the candidate

- `cargo test -p ledgerlab --lib billing_upgrade_tests -- --test-threads=1`: 13 passed, including schema-9 index mismatch refusal, explicit upgrade-required behavior, a corrupted schema-10 meter, and a concurrent schema-9 to schema-10 upgrade retry that reopens through the full billing validator.
- `cargo test -p ledgerlab --test m3_history -- --test-threads=1`: 6 passed, 1 ignored (the separate 1,001-plus acceptance gate is run explicitly).
- The explicit three-installation acceptance gate above: passed, 1 test.
- `cargo test -p ledgerlab-cli --bin ledger output::tests -- --test-threads=1`: 4 passed.
- `sh scripts/check-local-billing-e2e.sh`: passed; CLI output/setup 5, billing 6, finance CSV 1, and local CLI 17 tests.
- `sh scripts/check-local-billing.sh`: passed, including full local billing tests, strict Clippy, contract checks, reservation settlement checks, and default/all/no-default dependency boundary checks. The pinned Python contract requirements were installed in an isolated temporary virtual environment for this run.
- `cargo check --workspace --all-targets --no-default-features --locked --offline`: passed.
- `sh scripts/check-boundaries.sh`: passed.
- Source-built schema-9 migration, older-writer refusal, backup restore, and retry exercise: passed with the M3 binary hash above.
- Exact-commit GitHub compliance gate for M3 implementation commit `e2ce81a7f6c0027f661be94b9952a31d60366167`: [passed](https://github.com/stevekkall-beansgc/bean-counter/actions/runs/36290980411).
- Release workload and source-building checks are recorded above.

No part of this record qualifies PostgreSQL billing support, multi-host writers, native package support, hosted operation, payments, resource reservations, or product use at production scale.
