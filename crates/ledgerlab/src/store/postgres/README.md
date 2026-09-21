# PostgreSQL first-slice adapter

This concrete private adapter implements the shared acceptance port with typed,
bound tokio-postgres queries. It uses its own PostgreSQL migration and SQL. The
coordinator resolves authority and economics; the adapter does neither. See
`PHASE-1-STATUS.md` for the integration checkpoint and
`PHASE-1-REVIEW-FIXES.md` for subsequent review fixes. The current combined evidence
and outbox/preview boundaries are in `PHASE-2-INTEGRATION-STATUS.md`.

## Configuration and initialization

`Ledger::open_postgres` takes fully resolved `PostgresConfig` values. Credentials
are not printable through Debug. The production connection path directly reuses
ADR 022's reviewed `proof/connect.rs` and `proof/tls.rs`: required TLS, normal
hostname/chain verification, explicitly selected Public or PemOnly roots,
explicit TCP settings, fixed startup options, and a three-second whole-connect
bound. It does not parse URLs or read ambient PostgreSQL, HOME, certificate,
filesystem or platform trust settings. No insecure profile is implemented.

Opening checks PostgreSQL 17/18, the authoritative writable primary, negotiated
TLS, fsync/full_page_writes/synchronous_commit, permanent tables, the exact
checksums of all three migrations and installation identity. The runtime role cannot own the
schema/tables, create schema objects, or delete/truncate retained tables. Runtime
SQL qualifies `ledgerlab`; startup search_path is `pg_catalog`.

`migrate::create` is a separate private migration-owner entry point for an empty
database. It takes a transaction-scoped advisory migration lock, installs backend
schema 3, records all three migration digests and grants narrow rights to an existing
runtime role. Logical economic schema 1 and migration 0001 are unchanged; 0002
adds outbox state/evidence; 0003 adds permanent quarantine and terminal guards. `open` never creates, migrates, repairs or seeds, and
rejects old backend-schema-1/2 stores pending explicit upgrade support. Provisioning and seed
operations remain private integration seams; no administration CLI/API was added.
Seed/accepted fixture data are used only in tests, never in production migrations.

## Transaction lifetime

At most five dedicated sessions are leased concurrently. Session acquisition is
bounded by one second and the coordinator's deadline. Each admitted transaction
has a registered supervising task before opening its socket. That task owns the
session, permit and driver's borrowed SERIALIZABLE transaction. The private port
returns an owned message handle, avoiding self-referential transaction storage.

The coordinator reads pre-existing admission, authority, binding and chain heads
in that order. PostgreSQL uses shared row locks for the first three and an
exclusive row lock for the chain. There are no lock-on-missing-row claims:
missing chains return Waiting without reserving identity or economics. Future
scope families require their own reviewed extensions.

The handle is poisoned before every awaited operation. A failed/cancelled handle
cannot commit earlier writes. The supervisor separately records statement
failure, bounds statement and lock waits against the remaining deadline, and
rolls back on dropped handles or expired admission. COMMIT is queued without an
intervening await; the registered supervisor drains it even if the caller drops
its response future. An acknowledged commit is success. Explicit 40001/40P01
commit aborts are retryable; other commit errors/timeouts remain OutcomeUnknown.
Retryable statement conflicts become whole-transaction retries only after
rollback, never automatic economic duplicates.

Every session is discarded and its driver joined after its transaction. There
is no connection reuse after ambiguity, or after ordinary completion. Close
stops admission and joins registered work. Overlapping close callers await the
same drain, and cancelling a closer retains task handles for the next closer.
Request cancellation, transport loss,
OS process death and power loss remain different events; local tests do not
certify physical power-loss behavior.

## Local evidence

The explicit opt-in `service::pg_tests` adapter initializes fresh isolated real
databases from the frozen seed or an independently rehashed 100% discount variant.
A separate repeatable-read observer reads
every physical table/column and compares the resulting bytes and projections
with the independent testkit. The transaction-state probe observes its target
from a different connection and has an open-transaction negative control.

The suite covers the 83 acceptance cases, 45 main-path await cancellations,
semantic-alias cancellation, four-request barrier races with actual lock-timeout
responses from distinct backend PIDs, poisoned transactions, all 21 immutable
UPDATE/DELETE/TRUNCATE guards, role rejection, and 12 abandoned real-driver
COMMIT/socket-cut trials per server version. Those trials pass the replacement
through the original chain lock, then require exactly no journal or one complete
journal before same-identity retry and reopen.

Review regressions add two distinct decisions sharing one snapshot, an independent
100% discount with actions but no intention/delivery, retries after binding changes,
three integrity-collision variants, and eight overlapping distinct-operation trials.
The production-supervisor transport test runs twelve opaque TLS relay cuts: six
before COMMIT reaches the server and six after durable commit while its reply is
withheld. It asserts the real `PostgresTx` result, backend disappearance, original
identity retry and reopen; durable cases overlap two close calls and cancel one.

Every database initializer queries and asserts `server_version_num` against an
explicit intended major. Each suite process prints that number, `server_version`
and `version()`; the returned backend evidence retains the queried version text.

Set explicit `LEDGERLAB_PG_TEST_MAJOR` (18 or 17), `LEDGERLAB_PG_TEST_PORT`,
`LEDGERLAB_PG_TEST_PASSWORD` and
`LEDGERLAB_PG_TEST_CA` for an isolated localhost server with the migration owner
`postgres`, database `ledgerlab`, and restricted role `ledgerlab_phase1_runtime`.
Tests create only uniquely named synthetic databases; successful cases remove
their own database. Test secrets, certificates, databases and detailed logs stay
under ignored local operational directories.

```sh
cargo test -p ledgerlab --all-features --locked --offline service::pg_tests:: -- --ignored --nocapture --test-threads=1
sh crates/ledgerlab/src/store/postgres/proof/run.sh
```

The standalone proof tests synthetic TLS/protocol peers and exact trust-root
membership. Real-store tests establish persistence evidence separately. Public
CA server handshakes, OS process-kill/power-loss tests and the full native target
matrix are not claimed.

Current outbox query bounds, quarantine, reconciliation and upgrade gates: [OUTBOX-HARDENING.md](../../../../../OUTBOX-HARDENING.md).

The explicit fenced `maintenance::upgrade_postgres` operation upgrades exact schema 1/2 to 3 with the migration owner and existing restricted runtime role. See [PHASE-2-OUTBOX-MERGE.md](../../../../../PHASE-2-OUTBOX-MERGE.md) for prerequisites, uncertainty resolution and populated-database evidence.
