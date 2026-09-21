# Phase 1 integration checkpoint — stopped, incomplete

20 September 2026. Branch `codex/phase1-integration`. This is a reviewable partial
integration, not a completed Phase 1 slice or a release candidate.

## Required stop

The independent testkit's `ActiveCommitAbsent` case failed:

```text
ActiveCommitAbsent: uncertain connection was not discarded
```

This is the required connection-discard assertion in the unknown-outcome suite.
The task explicitly says to stop if any atomicity test fails. Implementation and
validation stopped when this result was inspected. No attempt was made to turn the
failed gate into a pass, remove the assertion, or weaken its discard requirement.

No partial journal was observed in the tests executed. The preceding live-writer
probe and primary-absence check completed; the failure occurred at the pool probe
after same-identity resolution. Inspection suggests test adapter bookkeeping:
`resolve_and_retry` reopens the store, then `accept` replaces `affected` with the
new connection ID before the final probe. That is a hypothesis, not verified
cleanup evidence. It must be investigated and the complete suite rerun before
claiming safe unknown-outcome handling.

## Integrated history and changes

The original Phase 0 commit `8004a38` remains the base. Reviewed lanes were
cherry-picked in this order:

| Original | Integration commit | Lane |
|---|---|---|
| da2aa3c7aa62b49611fe19bf21f8f006d89079a2 | c128946 | Pure core |
| eb5c449 | 75276ed | SQLite |
| fd624dbd44f07620c5e7ce66eaf8fb52c3e74bd7 | 69f76a0 | Independent testkit |
| 530b3e9b006200a1fcdbfd54110d04c2078a6667 | 997b47c | Historical SQLx PostgreSQL blocker |
| 582fb279dd1469068f1e910a34122f70775d61ed | 5cdb7a9 | ADR 022 and custom Rustls proof |

The sole cherry-pick conflict was Cargo.lock. It was resolved from both pinned
manifests with offline Cargo resolution; neither lane's dependency requirements
were discarded. No frozen contract, fixture, design source, original ADR, or
freeze manifest was edited. ADR 022 and its lane's document-inventory adjustment
remain the authorized PostgreSQL driver amendment.

The partial coordinator has one generic acceptance path over private transaction
ports. It reads installation admission, current source authority, pinned chain and
binding context; verifies retained document bytes and hashes; calls pure core;
projects the complete decision into 27 item writes; and returns Accepted only
after commit acknowledgment. It supports identity and semantic duplicate/conflict
classification, byte-identical original receipts, operational aliases, failed-work
zero acceptance, and separate Waiting and OutcomeUnknown types. Waiting currently
reports a missing existing chain; durable pending promotion is not implemented.
Only the SQLite facade is connected. There is no public raw-action append.

Test-only adapters invoke the independent testkit and read all physical SQLite
columns through a separate read-only observer. Expected journals remain frozen
oracle data. Fault instrumentation is test-only in the facade. Core has an
explicit deterministic `test-failpoints` feature for errors after the base
calculation; it does not read environment, time or runtime state.

Two schema/harness seams were reconciled without changing frozen economic bytes:

- SQLite's unreleased schema now initializes the disabled dispatcher singleton
  required by the testkit's seed inventory. No dispatcher behavior was added.
- The harness has an explicit, restricted absent-table inventory for later-phase
  tables whose required count/delta is zero. Actual tables and all their persisted
  columns remain inventoried. Required tables cannot be omitted; arbitrary absent
  tables are rejected. No future economic record schemas were invented.

These changes still need full review and validation. The SQLite lane's blanket
private-boundary dead-code allowance has not yet been removed.

## Exact executed evidence

All Rust commands used the supplied activation script and `RUSTUP_TOOLCHAIN=stable`.

| Check | Executed result |
|---|---|
| Combined workspace `cargo check --workspace --offline` | Passed before test instrumentation was added; one unused import was subsequently removed |
| `sqlite_facade_basics` | **1 Rust test passed**, invoking **11 real SQLite testkit scenarios** |
| `sqlite_acceptance_83_cases` | **1 Rust test failed**; **16 of 83 scenarios completed successfully**, case 17 (`ActiveCommitAbsent`) failed, **66 cases not completed** |
| Independent fixture oracle | Loaded successfully for these runs; validates all 99 frozen entries, 60 vectors, 25 accepted immutable records, 29 manifest members and independent arithmetic |
| Fresh accepted readback | Exact frozen journal, indexed columns, 100 / -20 actions, 80-atom intention, original receipt, operational state and physical row delta passed; reopen equality passed |
| Basic duplicate/conflict/authority/zero scenarios | Passed through the shared coordinator and real file SQLite, including reopen |
| Evaluation after-base invalid/overflow injections | Both passed with unchanged seed state and subsequent successful acceptance |
| Pre-commit rollback and lost reply | Passed, including reopen and original receipt retry |
| Controlled unknown durable/absent branches | Both passed their readback/reopen/discard checks before the active-transaction case failed |
| Formatting | `cargo fmt --all` completed at checkpoint |
| Whitespace | `git diff --check` passed |
| Frozen paths | No diff from Phase 0 in contracts, fixtures or design sources; original ADRs remain untouched |

The 11 basic scenarios overlap the 16 completed acceptance scenarios; these are
not 27 distinct scenarios. Earlier lane reports are retained as historical
evidence and are not counted as tests rerun against this integrated checkpoint.
A cancellation-hit bookkeeping adjustment was already in progress when the failed
run was inspected, and final formatting occurred afterward. No tests were rerun
on those final changes.

## PostgreSQL local feasibility

A new isolated `ledgerlab-phase1` Colima profile started without sudo, home mounts,
SSH-agent forwarding, accepting legal terms, or changing the active Docker context.
An empty Docker client configuration avoided ambient registry credentials.
The official `postgres:18` image resolved to:

`sha256:86c951e05bf56c93d95d397747fb8820ac76cc3bedb78f43abd83eedbe3666ae`

The real server reported **PostgreSQL 18.6**, ARM64 Debian, with `fsync=on`,
`full_page_writes=on`, and `ssl=on`. It used ephemeral locally generated private-CA
certificates, synthetic local credentials, and a loopback-only published port.
This proves local server availability, not production-driver authentication,
TLS conformance, migrations, isolation, atomicity or parity. The container and
Colima profile were stopped after the test stop condition. Their retained local
test data/images and temporary keys are outside committed files.

References consulted for local setup: [Colima configuration](https://colima.run/docs/configuration/)
and [official PostgreSQL image](https://hub.docker.com/_/postgres).

## Remaining gates

- Investigate the failed active-commit connection-discard evidence, then rerun all
  83 acceptance cases. The 54 before/after write-failpoint cases were **not reached**.
- Execute the newly wired cancellation suite and its real await catalogue. It has
  not run; its existence is not cancellation evidence.
- Wire and execute independent barrier-based concurrent coordinator acceptance.
- Run all original real SQLite storage tests against the integrated schema.
- Re-run pure-core conformance, Python **and Node** frozen audits, all-feature /
  all-target builds and warnings-denied Clippy, no-default checks, and dependency /
  source boundary audits. These have not completed for this checkpoint.
- Review duplicate ingress-byte comparison, receipt integrity checks, the complete
  authority/context seam, and all supported cancellation/unknown branches before
  treating the coordinator as complete.
- Wire ADR 022's custom Rustls implementation into production dependencies and
  re-audit the unified graph. SQLx PostgreSQL was not adopted.
- Implement the production PostgreSQL adapter, migrations and shared coordinator
  port. None has been implemented in this checkpoint.
- Re-run the standalone TLS proof; run real PostgreSQL 18 tests, then obtain/test
  PostgreSQL 17. PG17 was not attempted after the required stop.
- Fake destination/outbox, full Phase 1 dual-store parity, native platform tests,
  process-kill/power-loss limits, MSRV and later release gates remain open.

**The first SQLite vertical slice is not genuinely complete.** The successful
happy-path and duplicate evidence does not override the failed unknown-outcome
gate or the unexecuted write-failure, cancellation and concurrency suites.
