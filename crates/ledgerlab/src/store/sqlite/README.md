# SQLite acceptance and outbox boundary

This adapter persists the frozen completion slice and the additive outcome protocol described below. It does not implement the acceptance coordinator, evaluate policy, check submission/receipt permissions, infer duplicates, or authorize seed/administrative writes. Its types are crate-private. Runtime queries use concrete SQLx SQLite parameters and typed result decoding; there is no SQLx Any or offline query metadata.

## Integration seam

- `SqliteStore::create(existing_directory, Installation)` exclusively creates `local.db`, installs backend schema 4 and the installation row. Logical economic schema 1 and migration 0001 are unchanged; 0002 adds outbox state/evidence; 0003 adds permanent quarantine and terminal guards; additive 0004 stores the reviewed outcome and reservation-settlement projections. It refuses an existing database. `open(directory)` never creates or migrates a database: it verifies the linked build, pragmas, all four migration checksums, STRICT tables, integrity, foreign keys and installation presence. Older backend-schema-1/2/3 stores require the explicit fenced `maintenance::upgrade_sqlite` owner operation; see `PHASE-2-OUTBOX-MERGE.md`. Keep the whole durable local directory, including WAL/SHM. This is ordinary reopen/recovery, not a backup/restore implementation.
- `AcceptanceStore::begin(deadline)` returns an owned `SqliteTx`. The write pool has exactly one connection; a 65-permit gate permits one active writer and 64 queued requests. Acquisition is bounded by 500 ms and the caller's deadline. SQLite busy timeout is 250 ms. The caller supplies its five-second acceptance deadline.
- The pool begins with SQLx's tracked literal `BEGIN IMMEDIATE`. All mutations use this gate. SQLx uses its own connection worker; no application query threads are created. The read pool has at most two read-only/query-only connections. Read operations are bounded by two seconds and the acceptance deadline.
- `records.rs` contains `Scope`, `CanonicalRecord`, the closed first-slice `JournalRow` projections and `WriteOp`. Each journal write is one awaited statement, so the coordinator/testkit can place before/after failpoints at all 27 physical operations. No projection is derived from mutable policy in the store. Schema/body/ID/hash agreement, money calculations, snapshot completeness, 29 manifest members, and document rehashing on read remain the core/coordinator's responsibility. The boundary retains bytes and hashes verbatim.
- `load_identity` and `load_claim` return the original receipt, canonical event ID and relevant hash. `None` means absence in this read, **not proof of rollback**. The coordinator compares canonical ingress/facts, checks current read permission and decides duplicate/conflict. After an ambiguous commit, close/reopen/recover and resolve the same identity; never generate a replacement ID. Read-only identity lookup remains available when writes are disabled.
- Transaction reads expose installation, chain, authority head, binding head, document and held-delivery projections. Immediate writer ownership serializes the SQLite scope set. The coordinator must still read/check all applicable heads and admission under that transaction; this adapter makes no authority decisions.
- `AdvanceChain` is a compare-and-swap of the explicitly supplied revisions/counts and requires the matching retained chain transition. Seed variants are for authorized initializer/control integration, not public acceptance. Later economic record families require reviewed extensions. The outbox adds typed operational methods under the same transaction/ownership boundary; scheduling, payment adapters and administration commands remain deferred.
- Constraints include scoped uniqueness, deferred forward/cyclic references (snapshot/event, original-delivery/event, effect/action, event/manifest, revision/manifest), no cascading deletion, checked integer counters/scales, text atoms, immutable-record triggers on 18 economic and three operational evidence tables, and immutable chain/store identity guards. SQL alone does not prove economic completeness.

Shared wiring changed with calling-task authorization: `Cargo.lock`, `crates/ledgerlab/Cargo.toml`, facade `src/lib.rs`, and `src/store/mod.rs`. Integration owns reconciliation with the core and PostgreSQL lanes. The coordinator now consumes the private port. The module-wide dead-code allowance has been removed; narrow annotations remain only for explicit provisioning and retained startup diagnostics. The SQLite lane owns `ports.rs`, `errors.rs`, `records.rs`, SQLite modules and migrations. The port uses an associated owned transaction type, not a runtime-erased driver.

## Transaction cleanup and uncertainty

Each read/write sets a poison bit **before** awaiting SQL. Cancellation or failure leaves it poisoned, preventing partial-plan commit; rollback remains available. An acknowledged rollback is known rollback. Dropping a transaction uses SQLx's tracked rollback queue. Before a pooled connection is borrowed again, a worker ping drains earlier commands, then `is_in_transaction()` must report false or the pool discards the connection. This is SQLx's tracked depth check, not a claim to access SQLite's raw handle; the adapter never exposes untracked transaction SQL.

After commit starts, a registered Tokio task owns the transaction, permit and store owner. Caller cancellation drops the reply receiver but the task continues under a five-second drain deadline. No await separates spawning from registration; `close()` joins the registered task before closing pools. Any error/timeout after commit send conservatively reports `OutcomeUnknown`, disables writes and closes the writer pool. This includes deferred-FK commit errors even when SQLite would allow a more precise classification. The coordinator can resolve outcome after reopen. No database error becomes an economic duplicate automatically.

The host provides Tokio; the facade does not create a runtime. Before shutdown, stop admission and rollback/drop outstanding non-commit transactions, then call `close()` to drain pools and release ownership. Holding an idle transaction indefinitely can delay this drain. The OS owner guard is also retained by outstanding transactions/pool callbacks; dropping the facade cannot release it while a transaction survives. Runtime teardown or OS process death is distinct from ordinary request cancellation; the Phase 3 tests below inject process kills, but this lane has not certified power-loss behavior.

`owner.lock` uses stable `std::fs::File::try_lock` on the verified Rust 1.98.1 toolchain. Tests prove conflict both on a second file descriptor and in another process. The final owner guard explicitly unlocks before closing its descriptor, so a descriptor temporarily inherited by an unrelated concurrently spawning subprocess cannot prolong ownership after the store drains. Outstanding transactions and pool callbacks still retain the guard until cleanup. The lock inode is never unlinked/replaced; canonical directory/path and symlink checks reject supported path switches. Deliberately hostile hardlink/filesystem races and bypass by the OS file owner are outside the storage contract.

## Resolved build and evidence

Verified locally on ARM64 macOS 26 with Rust 1.98.1; no MSRV or cross-platform certification is asserted.

| Component | Resolved version/features |
|---|---|
| SQLx | 0.9.0, defaults disabled; sqlite, runtime-tokio, migrate |
| libsqlite3-sys | 0.37.0, SQLx bundled SQLite |
| Tokio | 1.53.1; rt, sync, time, macros |
| Test-only | serde_json 1.0.151; tempfile 3.27.0 |
| Linked SQLite | 3.51.3 |
| SQLite source ID | `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6d618` |

Linked compile options are printed by `linked_sqlite_reopen_and_exact_journal`. Verified `THREADSAFE=1`, `ENABLE_API_ARMOR`, FK/trigger/WAL support and actual STRICT behavior. Every connection verifies WAL, FULL synchronous, foreign keys ON, normal locking, 250 ms busy timeout, read-uncommitted OFF and its reader/writer query-only setting. On macOS, fullfsync must read back ON. Version/source mismatches fail startup. This is the minimum patched line required by the first-slice contract, not a claim that 3.51.3 is the newest SQLite release.

The 21 original test entries include a subprocess lock probe. Two additional owner regressions retain a duplicated OS descriptor through final guard destruction and through a real store close with an outstanding transaction. They require immediate reopen after cleanup and continued exclusion while a live owner remains. Real file-backed evidence covers:

- Complete 25-record acceptance plus seed records, byte-identical original receipt and all retained bodies/hashes after reopen; action atoms independently sum to 80.
- All 28 cut positions spanning 27 writes (the 54 before/after labels share adjacent cut states), compared against a full SQL database dump after explicit rollback and reopen. Separate dropped-transaction and cancelled-write-future passes, and every initializer cut, leave no residue.
- Each typed read cancellation, begin/rollback cancellation, cancelled commit reply with completed drain, missing deferred reference at commit, driver COMMIT-future cancellation followed by `close_hard()` and writer-lock/recovery verification (12 iterations), poisoned handles, and next-borrower cleanup.
- Delivery/claim/effect/action/intention/receipt/revision uniqueness primitives, all 21 immutable UPDATE/DELETE guards, cross-scope FK rejection, atom grammar/magnitude, STRICT operational types, schema mismatch, reader write rejection and symlink rejection.
- Actual immediate-lock contention, deadline/queue limits, OS ownership in another process, owner retention while a transaction exists, and 12 concurrent identity-check/write transactions with exactly one complete winner.

Validation commands, after the supplied toolchain activation:

```sh
export RUSTUP_TOOLCHAIN=stable # installed alias; rustc verifies 1.98.1
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
sh scripts/check.sh
cargo test -p ledgerlab --locked --offline linked_sqlite_reopen -- --nocapture
```

The first lane check reached/passed Rust and boundary checks, then lacked Python `jsonschema` on the default path. The existing prepared contract dependencies above resolve that environment issue; no frozen fixture was changed. Current combined results are in `PHASE-2-INTEGRATION-STATUS.md`. Bounded outbox dispatch/reconciliation and in-memory fake recovery are exercised; process-durable destination storage, power-loss recovery, complete backup/restore, scheduling, all-platform runs and MSRV remain gates. Phase 3 adds bounded process-kill evidence for outcome persistence below.

## Integrated coordinator evidence

The shared testkit runs all 83 acceptance cases and 45 main-path await
cancellations against real file SQLite, plus semantic-alias cancellation,
Waiting/no-reservation and installation-scope checks. Duplicate paths verify
retained canonical bytes/hashes and compare exact normalized ingress bytes.

The four-request barrier race uses a test-only second physical driver pool under
the same retained OS owner to exercise actual database contention. It records a
real SQLITE_BUSY response before releasing the leader's chain lock. Production
construction retains one writer pool. A separate original storage test also runs
12 concurrent identity/write requests through the normal single-writer pool.
Both require one complete winner and unchanged original receipts on retries.

Current outbox query bounds, quarantine, reconciliation and upgrade gates: [OUTBOX-HARDENING.md](../../../../../OUTBOX-HARDENING.md).

## Product Phase 3 outcome adapter

The additive implementation in `outcomes.rs` implements the shared private
`store::outcomes::{OutcomeStore, OutcomeTx}` seam. Its only append input is the
coordinator-owned `ValidatedOutcomePlan`, whose constructor is inaccessible to
this adapter. It never evaluates a price, interprets an authority flag, derives
supplier consumption, decides closure eligibility or makes a duplicate decision.
The exact shared interface commits are recorded in the lane handoff.

Backend schema 4 adds scoped immutable envelope storage for the 27 frozen outcome
kinds plus the three `reservation-settlement/1` kinds, immutable partition
membership and original anchors, composite delivery indexes, and opaque guarded
heads. All nine head classes use the coordinator's canonical scoped key bytes and
complete value bytes. No v1 authority/binding projection is invented. The existing
installation admission fence remains authoritative and is read by the coordinator
in the same transaction. Original migrations 0001–0003 are unchanged; maintenance
upgrades populated schema 1/2/3 explicitly and ordinary open never migrates.

The plan supplies its exact `(scope,target,invocation_id)` partition and original
anchors. The adapter retains envelopes verbatim and mechanically extracts scoped
kind/ID/hash indexes. Immutable identity is `(scope,kind,id)`; a new hash cannot
create another identity. Reused records must have identical bytes and hash.
Existing anchors cannot be replaced or enlarged. Reads return all partition
members plus explicitly required current records, never a selected economic
summary. SQL count/byte preflight rejects an oversized history before fetching
it; record count is capped at 4096, total history at 8 MiB, and required references
at 1024. The coordinator retains responsibility for tighter frozen per-record,
replay and decision bounds and complete index/body/hash/reference validation.

Every declared head is compared against its observed revision and complete value,
including unchanged heads and locked absence, before any append. Proposed head
writes must correspond to declared write locks and use compare-and-swap. In
particular, the coordinator supplies an unchanged reservation observation for a
post-hoc correction; this adapter does not derive a reservation update from its
economic delta. The frozen coordinator validates which head writes are permitted.

Composite delivery rows reference the original economic receipt (when present)
and mandatory settlement receipt. An alias must point directly to its original
delivery and preserve both original receipt envelopes. It cannot append economic
or settlement rows or change heads. Additive reciprocal triggers prevent existing
v1 delivery keys and outcome/control deliveries from claiming the same scoped
source/label. No missing settlement companion is treated as legacy fallback or
absence. Lookup returns original observations; the coordinator checks read rights,
compares exact command/ingress bytes, and classifies retry/conflict.

All operations reuse tracked `BEGIN IMMEDIATE`, the exclusive directory owner,
poison-before-await transaction handling and registered bounded commit drain.
There is no second production writer or background settlement task. `MoreLocks`
requires coordinator rollback and ordered restart; SQLite's existing database
writer lock already excludes concurrent writers. Expected-current mismatch is
retryable only after confirmed rollback and full re-resolution. Unknown commit
is never repaired by a compensating capacity release or a new delivery identity.

The test-only append boundary instrument surrounds each actual immutable insert,
partition/anchor insert, mutable head statement and delivery-index insert. It is
compiled out of production. Each loop item is exposed; statement batching does
not hide failure positions. Tests that write physical fixtures directly are
explicitly labeled storage-primitive evidence and do not certify an accepted
composite transaction or independent authorization. Complete-plan conformance
must use the coordinator's genuine validated plan builder and independent oracle.


### Phase 3 real-store evidence

Ten storage-primitive tests preserve all 112 frozen settlement envelopes and six
existing economic histories across reopen. They cover scoped immutable identity,
all nine complete head observations including locked absence and changed bytes at
the same revision, reciprocal delivery conflicts, missing companion failure,
Missing/MoreLocks resolution, oversized history rejection, cancelled reads and
unknown deferred-reference commit results.

Eleven further test entries use the coordinator's genuine `fixture::lifecycle()`
plans and mandatory test authority verifier. The fresh synthetic base is rated
through the unchanged core; no accepted plan is fabricated in the adapter. This
proves the supplied synthetic authorizations only, not real-world assent.

- Base, positive ordinary result, correction and explicit closure retain complete
  immutable envelopes, original composite receipts, anchors and heads on reopen.
- All 360 before/after physical mutation positions across these four plans are
  tested both for injected failure with poisoned commit and actual task
  cancellation. Reopen must equal the complete pre-transaction database.
- Eight subprocess kills cover each plan before commit after its head writes and
  after durable commit before its reply. Reopen yields the exact prior database or
  the original complete committed pair. This is not power-loss certification.
- A second physical test pool under the same retained owner exercises real writer
  contention; a stale validated plan must fail expected-current checks and leave
  the winner unchanged. Production still has one writer pool.
- The live shared coordinator executes the four-step lifecycle, same-ID retries
  after closure with write permission denied, identity conflict and semantic alias
  after correction/closure. Alias retries preserve the original receipt pair.
- Ordinary zero claims its permanent family slot without actions or intentions;
  later authorized correction changes economics without changing reservation.
  Zero-net correction retains inverse and replacement actions without an
  intention. Failure between the two actions leaves the complete database
  unchanged. Reversal after explicit release never replenishes capacity.
- A cancelled commit reply is drained before close; reopening resolves original
  receipts. The separate original unknown-commit tests remain in the full suite.

The populated schema 1/2/3 upgrade test has six histories (ordinary and lost
acknowledgement for each version), retaining original rows and receipts. A schema
3 collision deliberately occurs at the third additive table, proving rollback of
partial DDL. Migration checksums, explicit fencing, reopen and retry remain
required. All original SQLite and PostgreSQL migrations are byte-identical.
