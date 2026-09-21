# Phase 1 SQLite parallel owner/reopen repair

21 September 2026. Branch `codex/phase1-parallel-debug`, based on
`e883c158d371a8fc4b10246521d3d166e572260a`. This report addresses the separate
parallel reopen gate in `PHASE-1-DISCARD-GATE.md`. Phase 1 remains incomplete.

## Root cause and evidence

The owner guard relied on closing its `File` to release the OS advisory lock.
During parallel tests, unrelated subprocess creation temporarily inherits that
file's open description before close-on-exec takes effect. Closing the last Rust
owner in the parent is therefore not necessarily the last OS descriptor close.
An immediate reopen can correctly receive `Owned` even though every Rust owner
and SQLite connection from that store has finished cleanup.

The service adapter launches an independent Python observer; the oracle and OS
ownership test also launch subprocesses. Their descriptors can transiently retain
locks belonging to **other tests' directories**. Unique temporary paths do not
prevent descriptor inheritance within the shared test process.

Evidence on this macOS ARM64 host:

- The untouched base failed the default workspace run at
  `rollback_at_every_write_boundary_and_reopen` with `Owned`.
- Temporary lifetime diagnostics reproduced failures in the cancellation runner
  and driver COMMIT hard-close recovery. The latter recorded final owner
  destruction and `owner_refs=0` immediately before the failed reacquisition;
  both pools had completed `close()`. The adapter case likewise dropped its final
  owner before reacquisition. Diagnostics were removed after diagnosis.
- An independent Rust/std-only probe used one fresh lock path, 30,000 sequential
  open/try-lock/close attempts, and four concurrent threads launching 300 short
  subprocesses each. It used no SQLx, coordinator, commit task, or owner registry.
  Ten runs of close-only produced **5,040 conflicts / 300,000 attempts**. The same
  ten runs with explicit unlock before close produced **0 / 300,000**. Each mode
  overlapped **12,000 subprocess spawns**; the initial exploratory sample is not
  included in these totals.
- Both committed regressions held an intentional duplicate descriptor open,
  making the lifetime condition deterministic without timing or subprocess
  scheduling assumptions. On the old implementation, **both failed in all five
  runs** at the expected immediate reacquisition with `Owned`.

This matches the [Rust file-lock contract](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock):
closing releases the lock only after all duplicated/inherited descriptors close;
explicit `unlock` releases it independently of their remaining lifetime.

The process-local registry hypothesis was excluded: there is no such registry.
Both fixture constructors already use independently allocated `tempfile::TempDir`
values retained throughout each store lifetime. There is no reused fixed database
path. The store joins its registered commit and closes both pools; the testkit
adapter drains its injected task, closes the store, checks uncertain-pool disposal,
and drops its retained store handle before reopen. Those sequences were correct
for the observed failures and remain unchanged.

## Repair and regression

The final `Owner` destructor explicitly calls `File::unlock()` before closing
its descriptor. The guard is still retained by the store, outstanding transactions
and pool callbacks. Unlock does not occur merely because a caller starts close,
because a clone is dropped, or because a commit result is uncertain. If the OS
unlock call fails, descriptor close remains the fallback; no acquisition failure
is converted to success. The lock inode is never unlinked or replaced.

Two regressions prove the required lifetime ordering:

1. A retained owner reference continues to exclude a second owner. After the
   final reference drops, immediate acquisition succeeds even while a duplicated
   descriptor remains open. A replacement owner excludes competitors both before
   and after that old descriptor closes.
2. A real file SQLite store starts close with a checked-out transaction. Close
   must poll pending and ownership must remain exclusive. After rollback, close
   must finish within five seconds and the same database must immediately reopen
   while the duplicate descriptor still exists. The replacement remains exclusive.

All test directories remain independently allocated. No sleeps, ownership retries,
new ignored tests, global test locks, or test-thread restrictions were introduced.
No changes were needed in coordinator/testkit logic, connection pools, commit tasks,
PostgreSQL, economics, migrations, manifests, lockfile, CLI, or frozen contracts.

## Executed validation

All Rust validation uses the supplied activation script, `RUSTUP_TOOLCHAIN=stable`,
and verified `rustc 1.98.1 (48a229cea 2026-09-01)`. SQLite remains the pinned real
file-backed 3.51.3 build with existing durability verification.

| Check | Result |
|---|---|
| Both new regressions on the old implementation | Failed in 5/5 runs, 10 expected failures at immediate owner reacquisition |
| Both final regressions after repair | Passed in 100/100 runs, 200 test executions |
| Unrestricted workspace, all targets and all features | **20/20 consecutive runs passed**; each had 61 passing Rust test entries and 9 unchanged gated placeholders |
| Standalone acceptance catalogue | **83/83** passed; also executed in every workspace stress run |
| Standalone cancellation catalogue | **45/45** passed; also executed in every workspace stress run |
| Standalone SQLite storage suite | **23/23** passed: all 21 original entries plus 2 new regressions |
| Driver COMMIT hard-close recovery | Existing 12 iterations passed in each workspace run and the standalone storage suite |
| Complete default `sh scripts/check.sh` | Passed, including unrestricted parallel tests |
| Formatting and whitespace | Passed |
| All-target/all-feature Clippy with `-D warnings` | Passed |
| No-default-feature workspace/all-target check | Passed |
| Dependency/source boundary checks | Passed, including negative probes |
| Python and Node frozen audits | Passed: 99 frozen files, 60 vectors, 25 accepted immutable records, 29 manifest members and independent arithmetic |
| Independent Python oracle self-tests | 8/8 passed |
| Out-of-scope/frozen path comparison with the assigned base | No changes |

The 20-run workspace sample contains 1,220 passing Rust test entries and embeds
1,660 acceptance-case executions and 900 cancellation-case executions. These
embedded cases are not additional Rust test entries. An initial repaired workspace
run, standalone catalogue runs, and the default check script passed separately;
they are not included in the 20-run count. The nine pre-existing gated placeholders
remain unmodified; real SQLite acceptance/cancellation execute in the facade tests.
No suite was serialized.

## Integration decision

The owner/reopen correction preserves the existing single-owner contract.
**This parallel gate is clear; the integration owner may resume after integrating
this commit.** This does not clear the historical PostgreSQL, full
coordinator concurrency, delivery, cancellation-catalogue review, native-platform,
or other remaining Phase 1 gates.

## Reproduce

From the assigned worktree:

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
unset RUST_TEST_THREADS
cargo test -p ledgerlab --locked --offline store::sqlite::owner::tests
cargo test -p ledgerlab --locked --offline sqlite_acceptance_83_cases -- --nocapture
cargo test -p ledgerlab --locked --offline sqlite_cancellation_all_awaits -- --nocapture
cargo test -p ledgerlab --locked --offline store::sqlite -- --nocapture
for run in $(seq 1 20); do
  cargo test --workspace --all-targets --all-features --locked --offline || exit 1
done
sh scripts/check.sh
python3 -B -m unittest discover -s crates/ledgerlab-testkit/oracle -p 'test_*.py'
git diff --check
```

Raw logs, diagnostic snapshots and the standalone spawn probe are retained under
ignored `work/parallel-debug/`, outside committed files. The stress sample is
bounded evidence, not a claim of exhaustive scheduling or cross-platform coverage.
