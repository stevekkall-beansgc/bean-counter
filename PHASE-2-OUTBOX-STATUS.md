# Assigned Phase 2 reliability slice: delivery and recovery primitives

Historical implementation report. Current hardening, schema 3 and operational
state rules are documented in [OUTBOX-HARDENING.md](OUTBOX-HARDENING.md).

21 September 2026. Isolated `codex/outbox-recovery`, based on
`dce3ec4feda4025ab6e98ef23608b9e6f812aeb1`. This implements the assigned bounded
slice of detailed design §§13–15 and ADRs 010/011. It does not close the full
Phase 4/5 destination, backup or transfer gates in the source design.

## Result and boundaries

`ledgerlab::outbox` supplies one coordinator over the existing tracked SQLite
writer and PostgreSQL SERIALIZABLE transaction supervisor. Delivery consumes
existing immutable intentions, verifies their retained canonical bytes/hash,
scoped identity and destination, and transmits the original canonical payload.
It never evaluates pricing or adds economic records. No runtime dependency,
manifest/lockfile, pure-core code, frozen contract/fixture, original migration,
design source or ADR changed.

The independently owned `fake::MemoryDestination` implements atomic stable-key
receipt retention and fencing **in memory**, separately from all ledger
transactions. Retain the same instance through ledger reopen/recovery tests.
Its lifetime is deliberately explicit: it is not process-durable storage and
must not be presented as the design's completed durable fake adapter. There is
no network destination, payment provider, CLI, HTTP, UI or automatic scheduler.

The operational scan fails closed beyond 1,000 intentions or 8 MiB of retained
intention bytes per installation. Queries are ordered by intention ID. This is
a bounded first-slice implementation, not a throughput claim; paginated scans
and large-installation reporting remain integration work.

## State machine

- Economic acceptance still creates exactly the original **held**, zero-attempt
  delivery row in its transaction. Dispatch success is a later commit.
- After explicit reconciliation/resume: `held/pending/retry → leased` when due
  and when every named intention dependency is delivered. A newly accepted held
  row is eligible if the installation is already enabled. Missing dependencies
  fail integrity checks; no reversal economics or record family is introduced.
- Claim atomically persists the lease and append-only dispatch attempt **before**
  returning a send capability. The stable destination is `fake`; the downstream
  key remains exactly the IntentionId. Store ID scopes the independent simulator.
- The fake receives outside the ledger transaction. Exact matching receipt gives
  `delivered`; an explicitly confirmed pre-receipt failure gives `retry`; lost
  response gives `unknown`; permanent mismatch gives `rejected` (needs review).
- Confirmed-absent retries wait exactly 1s, 2s, 4s, ... capped at 5 minutes.
  After 20 attempts, confirmed absence becomes `rejected`. Unknown outcomes are
  never treated as confirmed absence and are never blindly resent.
- Dispatcher replacement/expired delivery leases turn potentially sent leased
  work into `unknown`. Paused reconciliation resolves matching receipts to
  `delivered`, authoritative absence to `pending`, mismatch/attempt exhaustion
  to `rejected`, and unavailable inventory to `unknown`.

## Fencing, evidence and recovery invariants

The durable singleton dispatcher head has one owner, increasing fencing token,
15-second lease and operational evidence sequence. Delivery leases last 30
seconds. The host should renew ownership every 5 seconds using its trusted UTC
microsecond clock; there is no background task. Expired ownership requires an
explicit replacement acquisition. A live owner returns `Owned` to competitors.

Each outbox transaction takes the installation lock before the dispatcher head.
SQLite uses the same bounded BEGIN IMMEDIATE writer as acceptance; PostgreSQL
uses exclusive installation/head row locks under the existing SERIALIZABLE
supervisor. Ownership, token, installation generation, hold/admission and lease
expiry are validated while locked. PostgreSQL serialization/lock conflicts
return `Retryable` for a complete caller retry; unknown commits remain distinct.
No second transaction implementation or uncertain-session reuse was added.

The in-memory destination also checks the fencing token and lease atomically
with receipt insertion. Acquisition/pause advances its fence before committing
local control state. A failed/unknown local commit can therefore stop an old
worker early, but cannot authorize it late. Replacement chooses a token above
both the retained database head and the surviving fake's high-water mark; old
restored database counters cannot revive an old capability. Renewal, stale send,
and late completion all check ownership. A request already received before
fencing may have executed; stable-key reconciliation resolves that boundary.
Late results append evidence and return `Fenced` without changing delivery state.
This is no claim of exactly-once delivery to arbitrary external systems.

Attempts, observations and reconciliation reports are separate append-only
operational tables with defensive mutation triggers. Reconciliation retains
per-key outcome, expected/remote request hashes and remote receipt ID, including
when the original response was lost. Its report digest binds store identity,
restore/fencing generation, complete current intention IDs/hashes, delivery
state, attempt schedule, observations and the independent destination inventory.
A changed intention set, generation, delivery state or remote inventory invalidates
resume. Unknown inventory, unresolved/mismatched keys and remote orphan keys
cannot yield a resume-capable report, including an empty local intention set.

`hold(true, now)` is the reusable restore preparation primitive: increase the
installation generation, revoke ownership, clear leases, invalidate all delivery
mappings (including `delivered`), and durably hold dispatch. Acceptance remains
independently available when admission is open. `open_restored_sqlite` and
`open_restored_postgres` perform that hold before returning a usable ledger;
on preparation failure they close the handle. The operator must first stop/fence
the original installation and verify the restored journal. Ordinary `open_*`
means normal reopen and cannot detect that an operator copied an old database.

Reconciliation checks **all** local intentions and the destination inventory.
It never fabricates history for a remote key absent from the restored journal.
Only an explicit `resume(report.digest, now)` with a complete current report
releases the hold. Reopening the ledger does not clear a persisted hold.

## Integration notes

The host API is `ledger.outbox(&destination)` with `acquire`, `renew`, `claim`,
`send`, `observe`, `dispatch_one`, `hold`, `reconcile`, `resume`, and read-only
`deliveries`. The separate claim/send/observe methods expose bounded failpoint
boundaries for recovery tests. Host code is trusted to supply time and own the
independent fake lifetime; this is not an untrusted transport API.

Each backend adds `0002_outbox.sql`, preserving 0001 bytes and economic schema
version 1. Initialization installs backend schema 2; normal open verifies both
migration checksums and never migrates. Existing backend-schema-1 stores are
rejected until an explicit owner-driven upgrade is integrated. PostgreSQL uses
its existing restricted runtime role with delivery/control DML and append-only
evidence INSERT rights; it receives no ownership, DDL, DELETE or TRUNCATE rights.
The initializer/migration seams remain private.

This bounded implementation uses the durable row lease rather than the source
design's additional long-lived PostgreSQL advisory-ownership connection. It
provides one owner and tested fencing with explicit replacement, not automatic
leader election. A continuously running dispatcher service should add that
session guard and connection-budget accounting along with scheduling.

## Executed validation

Prepared local Rust 1.98.1 on macOS ARM64, offline locked dependencies. Commands
source `work/toolchain/activate.sh`, select `RUSTUP_TOOLCHAIN=stable`, and use the
already installed Python audit dependencies. `scripts/check.sh` is not executable
in the supplied checkout, so the repository-documented `sh scripts/check.sh` is
used. Operational logs and toolchain wrappers stay ignored under `work/`.

- Full `sh scripts/check.sh`: **70 passed, zero failed**, with 12 declared
  ignored entries (10 real-PostgreSQL entries separately executed, 2 deferred
  process-durable fake gates). Formatting, warnings-denied Clippy, no-default
  build, dependency/source boundaries and frozen audits all passed.
- PostgreSQL 18.6 and 17.11: **10/10 integration entries passed on each major**.
  After the final reconciliation-evidence and fake-retry refinements, the ten
  outbox histories were rerun on each major and passed. Final fake retries also
  prove that an existing receipt cannot become falsely authoritative absence
  when a pre-receipt fault is injected.
- Shared delivery histories run on real file-backed SQLite, PostgreSQL 18.6
  (`180006`) and PostgreSQL 17.11 (`170011`): held acceptance; lost response;
  unknown lookup; late result and stale pre-send worker; exact exponential retry
  and exhaustion; orphan inventory; payload mismatch; new-intention/stale-generation
  report refusal; duplicate destination request/identical receipt; delivered-state
  recovery; barrier-started competing dispatchers; and final-statement failure
  after attempt/state writes proving complete rollback.
- Ten histories per backend preserve the original economic journal/index bytes
  and duplicate receipt. PostgreSQL additionally reopens each history and compares
  all physical rows. The added second acceptance case preserves every original row.
- SQLite close/reopen retains a potentially sent lease, replacement marks it
  unknown, and reconciliation resolves it. A closed database+WAL snapshot restored
  into a new directory finds the surviving remote receipt without a second action.
- A second SQLite restore drill actually accepts/delivers a new intention after
  the snapshot. Restoring the old snapshot detects that real IntentionId as an
  orphan, preserves both remote receipts, preserves the old journal exactly and
  refuses dispatch resume. This is a recovery simulation, not backup publication.
- Immutable guards now cover all 21 retained tables, including the three new
  evidence tables. SQLite populates each before testing UPDATE/DELETE rejection;
  PostgreSQL verifies UPDATE/DELETE/TRUNCATE rejection.
- All available PostgreSQL acceptance, cancellation, race, history/collision,
  direct-driver and supervised transport-cut regressions are executed with the
  existing free, isolated `ledgerlab-phase1` Colima harness and verified local CA.
  No paid infrastructure or cloud authentication is used.
- Frozen audits retain all 99 files, 60 hash vectors, 25 new immutable economic
  rows, 29 manifest members, the original receipt and the 80-atom intention.

The default workspace retains two explicitly ignored process-durable fake gates;
their messages now identify the precise missing storage/restart adapter. The ten
real-PostgreSQL entries are opt-in and separately executed on each major. They
are not silently counted as passing in the default run.

## Deferred work (not implied complete)

1. Separate SQLite fake-destination database and PostgreSQL `ledgerlab_fake`
   namespace with process-durable receipts/fence retention; restart adapters for
   the two remaining testkit delivery gates. The current fake survives a ledger
   rollback/reopen/restore only while its independent in-memory instance survives.
2. Complete backup production: fenced drain, VACUUM INTO/pg_dump orchestration,
   backup descriptors, full canonical/reference/manifest verification, fsync and
   publication, operator retention, and native PostgreSQL restore drills.
3. Portable `ledger-export/1`, byte-preserving SQLite→PostgreSQL import/cutover,
   staged import/restart, old-source retirement and post-target-write rollback
   refusal. No transfer or rerating path was added.
4. Explicit upgrade orchestration for existing backend-schema-1 databases;
   compatibility metadata/public schema support belongs to the integration owner.
5. Service scheduling, PostgreSQL session advisory ownership and connection
   budget, paginated/bounded streaming inventories, real destination capability
   and retention disclosure, timeout adapters, and complete reversal-dependency
   histories after those canonical economic families are assigned.
6. Process-kill/power-loss/disk-full certification, full platform matrix and
   production trials. Existing driver/socket/cancellation tests are not those
   claims. No push, merge, release, payment or external deployment occurs here.
