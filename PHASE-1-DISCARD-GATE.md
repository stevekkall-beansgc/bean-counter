# Phase 1 ActiveCommitAbsent repair

21 September 2026. Branch `codex/phase1-debug`, based on integration checkpoint
`910edb79d8ca37582e37be8c8605074609228a14`. This report supersedes only the
ActiveCommitAbsent stop in `PHASE-1-STATUS.md`; Phase 1 remains incomplete.

## Finding

The failed discard assertion was a deterministic **test adapter connection-accounting
defect**, not an invalid expectation or evidence that the production coordinator
rejected an ambiguous result. `resolve_and_retry` drained the held transaction and
closed/reopened the store, then called `accept`. That call overwrote `affected`
with the replacement writer's ID. The final pool probe therefore compared the new
connection against itself. Before the repair the diagnostic read:

```text
original=2
affected_connection="4", next_connection="4"
affected_discarded=true, next_has_open_transaction=false
```

The boolean also prematurely asserted disposal as soon as an unknown fault was
selected. These were two related evidence-lifetime errors in the test adapter.
The independent testkit's discard requirement is valid and remains unchanged.

## Repair and regression

The adapter now retains the uncertain connection ID and actual initial service
outcome separately from subsequent request bookkeeping. Disposal starts false.
After the existing bounded fault drain and store close, it checks that the old
writer pool is closed and has zero connections before recording disposal. The
next pool probe consumes this evidence after reading the replacement connection's
real ID and transaction state. Successful retries cannot erase the evidence;
later ordinary probes cannot inherit a stale disposal flag. The active-commit
probe reports the captured service result instead of manufacturing an unknown
result from the command.

The new real SQLite regression requires all of the following:

- The initial response is OutcomeUnknown with the original scoped identity.
- Before release, a primary lookup is absent, an independent writer observes the
  live SQLite write lock, and the committed physical snapshot remains exact seed.
- Disposal is not reported before cleanup.
- Resolution returns the original identity-duplicate receipt. An additional
  same-identity retry still preserves the original uncertain connection ID.
- The affected connection was disposed, its replacement has a different ID and
  no open transaction, and an ordinary follow-up probe clears the fault evidence.
- Exactly the frozen journal is present after resolution and reopen.

A separate targeted test invokes the unchanged independent ActiveCommitAbsent
runner. The existing real deferred-reference COMMIT-error test now also verifies
that production returns unknown with a closed, empty writer pool and that its
next acquisition fails with PoolClosed, before test-controlled close/reopen.

Only test adapter code, a cfg(test) pool-inspection helper, tests, and this report
change. Production acceptance/transaction logic, the independent testkit assertion,
dependencies, migrations, and frozen bytes are unchanged. No PostgreSQL, CLI, UI,
or later-phase work was performed.

## Executed validation

All Rust checks used the supplied activation script, `RUSTUP_TOOLCHAIN=stable`,
and verified `rustc 1.98.1 (48a229cea 2026-09-01)` on this macOS ARM64 host.
Tests used real file SQLite 3.51.3 with the existing verified durability settings.

| Check | Result |
|---|---|
| Original 83-case runner | Reproduced the reported failure after the first 16 cases passed |
| Targeted original gate and new ID-preservation regression, before repair | Both failed in 5/5 repeated runs; the regression caught the overwritten ID |
| Targeted gate and full evidence-lifecycle regression, after repair | Both passed in 50/50 repeated runs: 100 test executions |
| 83 acceptance cases | All passed, including the previous 16, both unknown branches, ActiveCommitAbsent and all 54 before/after write faults |
| Current cancellation catalogue | All 45 instrumented cases passed standalone and in the serial workspace run |
| Original SQLite storage tests | All 21 passed in the serial run, including deferred-COMMIT unknown, cancelled replies, all typed reads/write futures, begin/rollback cancellation and 12 driver-COMMIT hard-close iterations |
| Workspace all-target/all-feature tests, serial | 59 passed, 9 existing gated placeholders ignored; the real SQLite acceptance/cancellation runners above execute in the facade tests |
| Warnings-denied Clippy, all targets/features | Passed |
| No-default-feature workspace/all-target check | Passed |
| Formatting and whitespace | Passed |
| Resolved dependency/source boundary checks | Passed, including negative probes |
| Python and Node contract audits | Passed: 99 frozen files, 60 vectors, 25 accepted immutable records, 29 manifest members and independent arithmetic |
| Independent Python oracle self-tests | 8 passed |
| Default parallel `sh scripts/check.sh` | **Failed** at cancellation reopen with `SQLite boundary: Owned`; remaining checks were run individually and passed |

The 50 repetitions are a fixed, recorded sample, not a claim that timing failures
are impossible. The original gate and regression also passed in the parallel
workspace attempt and final serial workspace run.

## Separate remaining reopen failure

The parallel suite exposed intermittent `Owned` errors when reacquiring the
SQLite owner after close. To separate this from the repair, all three changed
Rust files were temporarily restored from the original checkpoint, with only a
test-only cancellation-point print added. Three parallel checkpoint runs showed:

1. Driver COMMIT hard-close recovery and rollback/reopen failed with Owned.
2. Driver COMMIT hard-close recovery failed with Owned.
3. The cancellation runner failed with the same `SQLite boundary: Owned` after
   the `document:grant` cancellation marker; driver hard-close recovery also failed.

The expected original ActiveCommitAbsent accounting failure remained present in
all three runs. The repaired source was restored afterward. The exact cancellation
failure therefore also occurs without this repair. Its deeper ownership/cleanup
cause was not established in this bounded task. No retry, sleep, assertion removal,
or test serialization was added to hide it. Serial testing above is supplemental
evidence, not a replacement for the failing default parallel gate. No partial
journal was observed in any completed assertion.

## Integration decision

**ActiveCommitAbsent is repaired and can be integrated.** Integration may resume
investigation of the remaining gates, but the default parallel suite is still red
and Phase 1 cannot be declared safe or complete. The newly reproduced owner/reopen
failure needs its own bounded diagnosis before an unrestricted continuation.
The historical PostgreSQL, coordinator concurrency, delivery, cancellation-catalogue
audit, platform and other remaining gates are not cleared by this repair.

## Reproduction

From the assigned worktree:

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
cargo test -p ledgerlab --locked --offline sqlite_active_commit -- --nocapture
cargo test -p ledgerlab --locked --offline sqlite_acceptance_83_cases -- --nocapture
cargo test -p ledgerlab --locked --offline sqlite_cancellation_all_awaits -- --nocapture
sh scripts/check.sh
```

The default check's failure is retained. Supplemental checks actually executed:

```sh
cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1 --nocapture
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo check --workspace --all-targets --no-default-features --locked --offline
cargo fmt --all -- --check
sh scripts/check-boundaries.sh
sh scripts/check-contracts.sh
python3 -B -m unittest discover -s crates/ledgerlab-testkit/oracle -p 'test_*.py'
git diff --check
```

Raw local logs are retained under ignored `work/phase1-debug/` and are not committed.
