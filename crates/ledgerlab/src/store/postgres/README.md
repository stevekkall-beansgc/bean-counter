# PostgreSQL first-slice adapter

This concrete private adapter implements the shared acceptance port with typed,
bound tokio-postgres queries. It uses its own PostgreSQL migration and SQL. The
coordinator resolves authority and economics; the adapter does neither. See
`PHASE-1-STATUS.md` for executed evidence and remaining gates.

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
migration checksum and installation identity. The runtime role cannot own the
schema/tables, create schema objects, or delete/truncate retained tables. Runtime
SQL qualifies `ledgerlab`; startup search_path is `pg_catalog`.

`migrate::create` is a separate private migration-owner entry point for an empty
database. It takes a transaction-scoped advisory migration lock, installs schema
1, records the migration digest and grants narrow rights to an existing runtime
role. `open` never creates, migrates, repairs or seeds. Provisioning and seed
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
stops admission and joins registered work. Request cancellation, transport loss,
OS process death and power loss remain different events; local tests do not
certify physical power-loss behavior.

## Local evidence

The explicit opt-in `service::pg_tests` adapter initializes fresh isolated real
databases from the frozen seed only. A separate repeatable-read observer reads
every physical table/column and compares the resulting bytes and projections
with the independent testkit. The transaction-state probe observes its target
from a different connection and has an open-transaction negative control.

The suite covers the 83 acceptance cases, 45 main-path await cancellations,
semantic-alias cancellation, four-request barrier races with actual lock-timeout
responses from distinct backend PIDs, poisoned transactions, all 18 immutable
UPDATE/DELETE/TRUNCATE guards, role rejection, and 12 abandoned real-driver
COMMIT/socket-cut trials per server version. Those trials pass the replacement
through the original chain lock, then require exactly no journal or one complete
journal before same-identity retry and reopen.

Set explicit `LEDGERLAB_PG_TEST_PORT`, `LEDGERLAB_PG_TEST_PASSWORD` and
`LEDGERLAB_PG_TEST_CA` for an isolated localhost server with the migration owner
`postgres`, database `ledgerlab`, and restricted role `ledgerlab_phase1_runtime`.
Tests create only uniquely named synthetic databases; successful cases remove
their own database. Test secrets, certificates, databases and detailed logs stay
under ignored local operational directories.

```sh
cargo test -p ledgerlab --all-features --locked --offline service::pg_tests:: -- --ignored --nocapture
sh crates/ledgerlab/src/store/postgres/proof/run.sh
```

The standalone proof tests synthetic TLS/protocol peers and exact trust-root
membership. Real-store tests establish persistence evidence separately. Public
CA server handshakes, OS process-kill/power-loss tests and the full native target
matrix are not claimed.
