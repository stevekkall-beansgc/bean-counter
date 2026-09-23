# PostgreSQL trusted-host publication integration

The concrete publication coordinator and driver join the separate filesystem
record to the primary's schema6 singleton. They do not construct an R3 physical
capability or certify reserved PGDATA/WAL/temp/memory. Runtime R3 admission is
still disabled until those independent prerequisites are met.

`PostgresStore::open_fenced`/`Ledger::open_postgres_fenced` acquire the actual
external owner, exclude prior work, recover exact SQL old/new witnesses to a
durable STABLE record, and only then expose the handle. Plain open and every
unanchored snapshot refuse bound rows. Before resolving any native work, even
an old unanchored handle must prove current UNBOUND under the SQL control gate
and a fresh singleton row-lock barrier.

The private trusted-owner bootstrap is explicit, never normal-open behavior.
It requires the expected logical installation, frozen admission, stopped/held
dispatch, empty R3 retained tables and IDLE staging. It binds UNBOUND to the
actual external owner under the shared gate and installation/singleton locks.
After SQL binding commits it initializes the external record. A crash in that
gap leaves ordinary open disabled; only the same owner may retry while the
installation remains frozen and without R3 rows. It does not unfreeze or resume
anything. Existing unanchored maintenance refuses a bound installation.

Every driver snapshot is pinned before application reads escape. A short shared
visibility guard checks the actual repeatable snapshot against STABLE. Existing
pinned readers can finish their old snapshot concurrently with later writers;
new readers cannot cross an unpublished commit. Comparison readers for bound
stores must inherit the same owner. Bare-config readers check UNBOUND instead.

Every possible retained mutation obtains the SQL gate and singleton FOR UPDATE
in its own transaction. Existing legacy read guards skip INSERT; missing guards
obtain the mutation lease first and report actual insertion. Native guards track
insertions too. Ordinary WriteOp, outbox, outcome append and native append all
use the same driver. Operational unresolved-work arm/clear remains fixed
staging, not an economic publication or protocol counter.

Before a dirty transaction commits, the driver holds exclusive visibility,
durably publishes PENDING and conditionally updates SQL witness with exact old
anchor/witness. It retains owner/gate through session discard and a fresh
singleton lock plus native PID/start exit proof. Only observed exact old/new
can become STABLE. Native saved/nonmembership resolution and COMMIT reply follow
that durable publication. Unresolved results preserve the staging slot and fail
closed. Read-only commits and rollback do not advance the witness.

Schema column privileges restrict identity changes but do not enforce a WHERE
predicate. The trusted driver enforces conditional update and publication order.
Migration-owner SQL is outside runtime authority; adversarial owner mutations in
tests are refusal controls, not supported production write paths.

Initial actual driver tests cover bootstrap, stale unfenced handles, ordinary
write, concurrent reads, read-guard insert/no-op/rollback, reopen and stale
witness refusal on both majors. These tests alone are not process-death,
whole-backup restore, native customer or physical-completion evidence.
