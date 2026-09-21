# Phase 1 independent review fixes

21 September 2026. Isolated worktree `ledger-lab-v0-review-fixes`, branch
`codex/phase1-review-fixes`, based on integrated commit `6e9deae`.

The two BLOCK MERGE findings are repaired, with fresh SQLite and PostgreSQL
validation recorded below. Frozen contracts, fixtures, original IDs/bytes, dependency
pins and canonical record families remain unchanged.

## Root causes and repairs

- **P1 snapshot reuse:** both stores treated every planned document as a new row.
  Distinct completions deliberately produce the same content-addressed snapshot
  under unchanged documents and authority. Both adapters now use a scoped
  insert-on-conflict followed by exact kind, canonical-byte, content-hash and schema
  verification before reuse. A mismatch is an integrity error and poisons the
  transaction. SQLite's immediate writer and PostgreSQL's SERIALIZABLE conflict /
  whole-transaction retry preserve atomicity. Every decision retains its own seven
  associations and includes the reused snapshot in its manifest. No salt or new ID.
- **P2 net zero:** core tested for nonempty actions rather than a nonzero sum. It now
  uses the checked obligation total to decide whether to create an intention.
  The +100/-100 actions, effect facts, dependencies and explanations persist;
  intention, delivery and receipt/manifest intention membership are absent.
  The implemented model permits exactly one retail binding/obligation per decision
  (`ResolvedInput::validate` rejects broader binding sets); no multi-obligation
  product expansion was made. The +100/-20=80 fixture remains byte-identical.
- **Overlapping shutdown:** taking the task registry into one close future let
  another closer return early, and cancellation could drop the join handles.
  Shared, owner-held drain state now serializes closers and retains handles across
  cancellation. Both closers wait for the registered transaction/session disposal.

## Independent evidence added

The testkit's Python history oracle derives synthetic expectations from frozen
records using its own canonical serialization and SHA-256 functions, without core
or evaluator calls. Its identity transformation first reproduces every original
frozen journal and receipt byte. Both store observers read physical columns.

Each store runs a two-decision history and a separate 100% discount history,
comparing complete canonical journals and indexed columns. Tests assert exact
physical row deltas, revision/event_count 2/2, one snapshot document, fourteen
associations, both receipts, no immutable rewrites, and reopen equality. Identity
retries run before binding changes and both identity/semantic retries run after
binding deactivation and replacement with an unusable selector document; historical
receipts must be returned before current selector validation. Net zero retains
both action provenances and produces no intention or delivery row. Three fresh
stores separately reject kind, byte and hash collisions and prove earlier writes
cannot commit. Eight barrier-controlled distinct-operation races per backend
require actual database contention, two Accepted results and the independent full
two-decision journal after reopen.

A new opaque TLS relay test uses the actual production `PostgresTx` command handle,
registered supervisor, COMMIT classification and drain. Twelve trials per PG major
cut six COMMIT requests and withhold then cut six durable COMMIT responses.
`OutcomeUnknown` is returned by production, never fabricated by a test adapter.
An independent connection establishes durable persistence before response cuts;
a replacement crosses the original chain lock before interpreting absence.
Backend PID disappearance, exact none-or-complete journals, original-identity retry
and reopen are asserted. Every durable trial also overlaps two closers, cancels
the first, and proves the second still waits. Existing direct-driver and controlled
fault tests remain, with their narrower evidence meaning unchanged.

Every PostgreSQL initializer asserts the queried numeric version against the
explicit intended major; suite logs print the numeric version, minor text and full
server build string. Backend evidence retains the queried version text.

## Final validation

| Check | Result |
|---|---|
| Unrestricted `sh scripts/check.sh` | 67 Rust test entries passed, zero failed; formatting, warnings-denied Clippy, no-default build, graph/source boundaries and frozen audits passed |
| SQLite within that workspace run | 33 entries: 23 storage tests (including 12 driver COMMIT/hard-close trials) and 10 facade entries; includes 83 acceptance scenarios, 45 main-path cancellations, alias cancellation, four-request identical race and the new review histories/collisions/eight distinct-operation races |
| ADR 022 standalone proof | 16 parent tests passed, zero failed; ambient-environment child runs through its parent (one outer ignored entry); 150-package pin/source/trust audit passed |
| PostgreSQL 18.6 (`180006`) | 9/9 entries passed; server text `18.6 (Debian 18.6-1.pgdg13+2)` |
| PostgreSQL 17.11 (`170011`) | 9/9 entries passed; server text `17.11 (Debian 17.11-1.pgdg13+2)` |
| Frozen invariants | 99 frozen files, 60 hash vectors, 25 new immutable rows, 29 manifest members, exact original receipt and 80-atom intention passed; no frozen-path or pin changes |

Each PostgreSQL suite's nine entries cover the 83-case catalogue, 45 cancellations,
alias cancellation, four-request identical race, storage/runtime-role guards across
18 immutable tables, 12 original direct-driver cuts, the two new histories and
three collision variants, eight distinct-operation races, and 12 new production
supervisor cuts (six none / six complete, with six overlapping/cancelled-close
regressions). New-history assertions compare complete journal bytes, indexes,
receipts and physical row deltas, rather than accepting a matching total alone.

The workspace has 11 declared ignored entries: nine real-PostgreSQL entries,
explicitly executed on each server, and two deferred outbox/fake gates. The
11-case SQLite basics test overlaps the 83-case catalogue and is not counted as
additional scenarios. Additional preliminary runs are not included in these final
counts.

All Rust commands use the supplied activation script and
`RUSTUP_TOOLCHAIN=stable`: rustc 1.98.1 (48a229cea 2026-09-01).
Workspace tests use default parallel execution with `RUST_TEST_THREADS` unset.
Real PostgreSQL entries run sequentially within each server suite on the documented
isolated, free `ledgerlab-phase1` Colima profile, PostgreSQL 18 first and then 17. Both containers exited with status 0, and the
isolated Colima VM was stopped afterward.

Three test-harness setup issues were corrected before final validation: collision
variants originally reused SQLite after its intentional fail-closed integrity
shutdown; each now gets a fresh store. A PostgreSQL control-fixture update initially
used boolean literals for the existing BIGINT active flag; it now uses 0/1.
The existing unique-violation fixture also had to stop using a duplicate document,
which is now valid reuse. Its first replacement targeted a table the runtime role
cannot insert into; a duplicate writable snapshot association now preserves the
original real-23505/rollback test. These failures occurred in test setup before the
required economic or atomicity assertions. No observed semantic or
atomicity failure was worked around, and no invariant or expected result was weakened.

Full operational logs remain uncommitted under `work/final-check.log`,
`work/final-tls-proof.log`, `work/final-pg18.log`, and `work/final-pg17.log`.

## Limits

These fixes close the two review findings and their directly related evidence
requests. Broader Phase 1/public-v0 gates remain as previously documented: MSRV,
required native pipeline runs, later record/authority families, pending promotion,
outbox/fake reconciliation, export/restore, full platform and release certification.
No publication, push, deployment, cloud service, paid infrastructure or frozen
contract amendment occurred. Connection cuts are not power-loss certification.
