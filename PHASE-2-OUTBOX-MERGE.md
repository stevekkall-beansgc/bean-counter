# Phase 2 outbox/reliability integration

Local clone: `/Users/stephenkall/Documents/Codex/2026-09-21/ledger-lab-p2-outbox-merge/work/ledger-lab`

Branch: `codex/p2-outbox-reliability`

This bounded lane starts at exact canonical freeze
`3e56ff172d4dba326272c66c85512c8a481f7a52` and integrates exact outbox-hardening
commit `7637d583fe8a07ba9fccf1fdec505f3b349b3518` without conflicts. The source
commit was applied with `cherry-pick --no-commit`, then the explicit schema-upgrade
path and evidence below were added. One final scoped commit contains this lane.
`ROADMAP.md` is unchanged; this report does not declare the combined Phase 2 gate
complete or authorize Phase 3 persistence.

## Changes and interfaces

The imported lane supplies bounded outbox paging, complete reconciliation,
permanent rejection, audited quarantine, attempt monotonicity and stale-response
fencing on both concrete stores. Immutable intentions, receipts, economics and
stable destination idempotency keys remain unchanged.

New `ledgerlab::maintenance::{upgrade_sqlite, upgrade_postgres}` owner operations
upgrade exact backend schema 1 or 2 to schema 3. All missing migration SQL and
history entries commit in one transaction. SQLite also commits `user_version` in
that transaction. Existing migration 0001/0002 bytes remain exact. Logical schema
stays 1; no outcome record decoder, historical rewrite or Phase 3 persistence is
introduced. An already-current, checksum-valid, fenced database returns
`AlreadyCurrent`; a successful upgrade returns `Upgraded`.

Both operations require the expected logical store ID, admission `frozen`,
dispatch held and disabled, and a dispatcher head with no owner/lease and disabled
state. They preserve these fences and every existing economic/delivery cell.
SQLite retains the OS directory owner and uses tracked `BEGIN IMMEDIATE`, linked
build/durability verification, exact migration checksums and integrity/FK checks.
PostgreSQL uses the verified primary TLS connection, SERIALIZABLE, a migration
advisory lock, then installation and dispatcher row locks in application order.
The explicit runtime role must be non-owning and lack DDL/destructive privileges;
only the required outbox operational grants are added. Malformed migration-history
column types return a validation error rather than panicking. Runtime-role credentials
cannot upgrade a legacy database.

Normal open still refuses old schemas and never upgrades. Unknown commit errors,
failed supervisor acknowledgement and the operation deadline return
`UpgradeError::OutcomeUnknown`. Repeating the same fenced operation verifies
committed history rather than blindly reapplying DDL. Other precommit failures
return `Refused`. Caller cancellation leaves the supervisor running; maintainers
must retain the Tokio runtime until completion or subsequently resolve the state.
The work deadline is 60 seconds, with PostgreSQL statement/lock deadlines of
10 seconds/500 ms. SQLite drains the driver before releasing ownership, so cleanup
may outlast the work deadline. This is not a power-loss or arbitrary-runtime-kill
certification.

Unavoidable shared interface edits: one additive `pub mod maintenance` in facade
`src/lib.rs`; crate-visible SQLite migration module; the source lane's private
outbox transaction-port addition. No production CLI or pure-core edits, manifest,
lockfile, toolchain, frozen fixture or contract changes.

## Upgrade evidence

The two `schema_upgrade_populated_1_and_2_rollback_reopen_retry` tests build real
legacy databases using original migration bytes and independently frozen seed
and full first-slice accepted rows. They never downgrade a modern database to
simulate an old one. Each backend exercises schema 1 and schema 2 with ordinary
success and an injected lost application acknowledgement after durable commit.

They verify:

- Normal open refuses legacy schemas without changing their rows.
- Wrong store IDs, open admission, unheld/enabled dispatch, retained dispatcher
  leases and altered migration checksums refuse upgrade. SQLite additionally
  refuses a competing directory owner; PostgreSQL refuses runtime-role migration
  and a privileged proposed runtime role.
- A deliberate migration-3 table collision fails after earlier DDL executes.
  The transaction restores the original version/history and every retained table;
  removing the collision permits the complete upgrade without partial-DDL residue.
- Successful/lost-ack upgrades survive connection close and reopen. Repeating the
  owner operation returns `AlreadyCurrent` with unchanged economics.
- Every old table's physical values, canonical blobs, identities, receipt bytes,
  chain head and delivery attempts survive. Only the new dispatcher revision
  column and new empty operational tables/history entries are added. Schema-2
  tests seed a nonzero dispatcher revision (7) and require its exact retention.
- A pre-existing rejected delivery with 20 attempts cannot become pending or have
  its attempt count reset after upgrade. PostgreSQL's runtime can SELECT/INSERT
  quarantine evidence but cannot UPDATE/DELETE it.
- After the test owner explicitly reopens admission, the shared acceptance
  coordinator returns the original frozen receipt on retry, with no changed
  physical data or duplicate economics. Dispatch remains held.

The injected upgrade acknowledgement loss is application-boundary evidence, not a
claim of a physical network cut during DDL COMMIT. The separate existing real-store
suite and standalone driver proof cover actual transport cuts, cancellation,
ambiguous acceptance commit and next-borrower cleanup. The imported outbox suite
covers ordering, cross-page dependencies, retry limits/idempotency, stale fencing,
restore/orphan inventory, permanent quarantine and 1,001 real accepted intentions.

## Validation in this lane

`sh scripts/check.sh` passed: **118 tests passed, zero failed, 19 ignored**;
formatting, warnings-denied Clippy, no-default-feature build, dependency/source
boundaries and all independent Python/Node contract audits passed. The 19 ignored
entries are 17 opt-in PostgreSQL tests and two process-durable fake gates. All
159 frozen files (including the 99-file v1 baseline) remain exact. The first-slice
oracle retains 60 hash vectors, 25 accepted records, 29 manifest members and
80 atoms. The frozen outcome-contract audit retains 24 histories, 1,450 records,
43 decisions and 272 negative checks.

Scoped outbox tests passed 10 with seven PostgreSQL entries gated for the separate
real-store run. Final SQLite and PostgreSQL upgrade tests both passed; each covers
four populated histories (schema 1/2 × acknowledged/lost-ack commit), plus the
failure and refusal checks described above. The full offline run was followed by
focused upgrade assertions for retained dispatcher revisions and malformed
metadata; a second full offline check covers the final defensive metadata decoder.

The standalone driver/TLS proof passed **16 tests**, formatting, Clippy,
no-default-feature build and the 150-package pin/feature/source audit. Its one
framework-ignored ambient-environment child is invoked by a passing parent.
The opt-in real PostgreSQL 17.11 suite passed **17 tests, zero failed, zero
ignored** (838.99 seconds), including the 83-case acceptance harness, cancellation,
competing connections, 1,001-intention controls, eleven outbox recovery histories,
quarantine, paging/dependency ordering, later-page rollback, transport cuts and
production-supervisor ambiguous commits. The final focused upgrade rerun additionally
passed malformed-metadata and nonzero dispatcher-revision checks on the final code.
The PostgreSQL suite ran concurrently with the offline checks; its source-lane
outbox code was unchanged throughout.
All dependencies are locked/offline on the existing Rust 1.98.1 toolchain. SQLite
is the linked 3.51.3 build. Real PostgreSQL uses a disposable localhost-only cached
17.11 aarch64 Linux image with newly generated synthetic password and CA. No image
pull or remote database is used. PostgreSQL 18 was not available in the local image
cache; the source lane's historical 18.6 evidence is not represented as a rerun.

Commands:

```sh
cargo test -p ledgerlab --lib --all-features --locked --offline outbox
cargo test -p ledgerlab --lib --all-features --locked --offline schema_upgrade
sh scripts/check.sh
# With explicit local synthetic LEDGERLAB_PG_TEST_* settings:
cargo test -p ledgerlab --lib --all-features --locked --offline -- --ignored --test-threads=1 --nocapture
sh crates/ledgerlab/src/store/postgres/proof/run.sh
```

The local compiler is installed under the `stable-aarch64-apple-darwin` alias;
`RUSTUP_TOOLCHAIN` selects that existing installation and the check script verifies
actual version 1.98.1. Contract checks use the pre-existing local `check-deps`
Python directory. Detailed logs, synthetic credentials, databases, certificates
and build artifacts remain outside this commit.

## Integration-owner instructions

1. Fetch this local clone and cherry-pick its one scoped commit onto the combined
   Phase 2 integration branch. Do not separately cherry-pick `7637d58` afterward;
   this commit already includes it. Do not merge main or alter frozen inventory.
2. The only CLI test change is in
   `migrated_cli_ledger_coexists_with_fake_outbox_and_preserves_receipts`:
   expected `_sqlx_migrations` row count changes from **2 to 3**. When reconciling
   the CLI lane, retain its other assertions and this count of three. The facade's
   new `pub mod maintenance` declaration is additive; retain other lanes' exports.
3. Run the complete combined offline checks, both stores' upgrade tests and the
   opt-in reliability suites against the final combined tree. Recheck the exact
   159-file inventory plus unchanged 0001/0002 migrations. A separate integration
   review decides the product-roadmap Phase 2 exit; this lane does not.
4. For an actual old database: stop all application/dispatcher processes, retain
   a verified restorable backup and review any historical rejection downgrade.
   Under owner maintenance, fence admission as `frozen`, set dispatch hold=1 and
   enabled=0, and clear the stopped dispatcher owner/lease with head enabled=0.
   Verify the intended logical store ID. Call `maintenance::upgrade_sqlite(path,
   expected_store_id)` or `maintenance::upgrade_postgres(owner_config,
   expected_store_id, existing_runtime_role)`. Backup verification and the initial
   durable fence are explicit operator prerequisites, not performed by this API.
5. On `OutcomeUnknown`, keep all fences and repeat against the same authoritative
   database. Do not delete migration history, alter checksums, assign a new store
   identity, initialize over existing data, or downgrade. On `Refused`, resolve
   the mismatch while still fenced; no automatic repair is attempted.
6. After successful schema verification, inspect retained history and explicitly
   reopen admission through the owner. Keep dispatch held until a new complete
   reconciliation and normal explicit resume. Upgrade never asserts settlement,
   revives a rejected key or resolves unknown remote delivery by itself.

Remaining outside scope: process-durable fake destination, general backup/restore
orchestration, PostgreSQL 18 rerun, power-loss/native-platform certification,
Phase 3 outcome persistence, CLI upgrade command, deployment and publication.
