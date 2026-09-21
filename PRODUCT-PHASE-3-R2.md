# Phase 3 R2: restart stale snapshots before planning; reuse locked validation

Independent re-review of `8335b0d94432ab4679109cb761703f1ade7a5dbe`
closed R1 authorization but returned CONDITIONAL for reproducible PostgreSQL
same-identity failures. This successor addresses the bounded path diagnosed
below. It does not expand the five-second budget, change error classification,
weaken assertions or certify arbitrary-load availability.

## Correlated reproduction

Test-only tracing records request side/stage, attempts, backend PID, remaining
budget, operation results and synchronous validation durations. A separate OS
thread/runtime samples PostgreSQL activity and blocking PIDs, so synchronous
coordinator work cannot prevent server observation. A diagnostic compares every
application table against a separate sequential control database, then closes
and reopens the raced store and checks its complete snapshot before asserting
that both callers succeeded. A failed result still fails the test.

Normal-priority traced runs on PG17/18 passed. A controlled PG17 background-
priority process reproduced ordinary-step `Unavailable` in 5,148ms. This is a
reproduction condition, not a claim about the independent review's scheduling.
The captured chain is:

1. The losing request's PID 205 receives lock timeout SQLSTATE 55P03.
2. PID 206 acquires the immutable scope guard with an older serializable
   snapshot, replays/plans stale history, and receives 40001 at append.
3. Retry PID 208 finishes resolution with 895ms remaining. It spends about
   1.04 seconds in synchronous validation before its next database operation.
   Server samples show `ClientRead`, with no blocking PIDs during this interval.
4. The server logs PID 208's idle-in-transaction termination at
   `2026-09-21 23:31:35.858 UTC`. The next lookup returns `Deadline`, then rollback
   returns a closed-connection error. Existing classification returns
   `Unavailable`; no COMMIT was attempted by this losing retry.
5. The failed pair's 37 tables exactly equal the complete sequential result,
   including after reopen. The winner's original receipt pair and all heads,
   anchors and records remain complete.

The three captured failed-pair inventories (ordinary, correction and closure)
also exactly match all 37 tables in the corresponding fresh, independently
verified durable prefixes. This checks the actual captured failures, not only
the separate successful regression histories.

All earlier and intermediate failures are retained. A head-lock-only attempt
still failed correction under background scheduling; history-replay reuse alone
with that lock correction still failed closure. Their exact post-failure state
also matched the complete control. These experiments are not passing evidence.
The original review failures lack this correlation and cannot retrospectively be
assigned a proven cause or post-failure state from the new trace alone.

## Bounded correction

PostgreSQL now locks each existing mutable write head immediately after its
ordered immutable scope guard. A head changed after the serializable snapshot
causes 40001 at lock acquisition, before expensive replay/planning. Absent heads
remain protected by the existing scope guard, uniqueness and append comparisons.
A deterministic regression establishes an old snapshot, commits a new head in
another transaction, and requires the stale transaction to fail at lock
acquisition, with exact unchanged tables after rollback/reopen and the original
read-authorized receipt retained. It fails on the predecessor's implementation.
See PostgreSQL's [row-lock rules](https://www.postgresql.org/docs/17/explicit-locking.html)
for the existing SERIALIZABLE behavior used here.

The coordinator keeps one fully replayed history for the current held snapshot
and reuses it for identity handling, semantic lookup and fresh planning. It
also copies already decoded immutable envelopes rather than encoding and
re-decoding the same bytes for proof validation and planning. Raw incoming
records and new evidence still pass canonical decoding; all reference, document,
replay, head and host-proof checks remain. No state or authority proof is cached
across rollback, lock discovery or re-resolution. R1's write callback and its
returned proof remain mandatory before fresh planning.

The remaining changes are test-only tracing, diagnostics and documentation.
No core economics, frozen files, migrations, dependencies, transaction budget,
retry limit, unknown-commit behavior or error classifications change.

## Validation and disposition

Final source results (Rust 1.98.1, locked offline dependencies, linked SQLite
3.51.3, cached PostgreSQL 17.11/18.6):

| Gate | Result |
| --- | --- |
| Complete `sh scripts/check.sh` | PASS: 172 tests, 0 failed, 34 ignored; formatting, strict Clippy, feature/no-default, architecture and independent contract/freeze checks |
| All affected PG17 outcome tests | PASS: 15/15, 138.02s |
| All affected PG18 outcome tests | PASS: 15/15, 138.34s |
| Final background-priority diagnostic | PASS on both majors: all four pairs, one acceptance/one duplicate each, exact complete 37-table state and reopened snapshot |
| Fresh independent durable comparison | PASS: four prefixes, 82 retained records, eight negative probes per store; exact SQLite/PG17/PG18 records, original receipts, anchors and heads |
| Captured failed-pair state | PASS: each failed ordinary/correction/closure inventory equals all 37 tables of its matching independently verified durable prefix |
| Predecessor canonical comparison | PASS: all four SQLite prefixes match R1 exactly for records, receipts, anchors and heads |
| Preservation | PASS: all 173 baseline pins, 159 legacy frozen files, 13 reservation files, five controls, eight migrations, core and Cargo manifests unchanged |

The 34 ignored offline entries comprise 32 opt-in PostgreSQL tests and two
pre-existing deferred fake-destination gates. The affected PostgreSQL filter
executes 15 tests; unchanged legacy service/outbox/upgrade/TLS suites are not
claimed as freshly rerun. Each major includes 386 validated-plan failure
positions, 14 primitive positions, eight actual COMMIT request/reply cuts,
cancellation and races. Cancellation is not claimed at all 386 PG positions.
SQLite's full aggregate includes its 360 failure/cancellation positions and
eight process kills. No failed result was reclassified as success.

The external R2 handoff records exact commands, source identity and artifact
hashes. All 20 original and 26 R1 evidence entries remain unchanged. The
independent review's failed logs, all new failed diagnostics, raw state
inventories and correlated server logs are retained. Temporary containers and
volumes were removed, the initially stopped VM was stopped, and Docker context
was restored to `default`. Infrastructure spending: $0.

Focused independent re-review of the exact successor is required. Historical
availability qualifications and all previously deferred public host, broader
base-chain, platform, release and Phase 4 gates remain. No broader availability
acceptance is implied. No main merge, push, release, deployment, registration or
infrastructure spending is performed.
