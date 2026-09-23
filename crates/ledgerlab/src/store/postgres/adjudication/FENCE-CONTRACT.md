# Private PostgreSQL filesystem fence contract

This module provides a trusted-host filesystem publication record only. No
PostgreSQL session, query, database witness authentication, physical capability,
production admission or restored-primary acceptance is implemented here. Native
SQL gate/witness/read integration remains required. No new dependency is used.

The embedding host supplies a preexisting durable directory outside every
PostgreSQL backup/restore domain, writable only by that trusted host. Commands,
proofs and callers never select it. `acquire` rejects a symlink directory, owns an
actual process-exclusive `owner.lock`, and checks the directory/lock inode
identity. The empty lock inode and directory are synced before the identity is
exposed. A SHA256 domain binds the lock device/inode into a64lowercasehex anchor.
Concurrent mutation by an actor authorized to replace the trusted directory is
outside this host boundary; symlink, hardlink and replacement checks fail closed
at supported entry/write boundaries. The OS lock remains owned until drop.

`DatabaseWitness::from_database(anchor,witness)` parses two64lowercasehex fields.
It is a DTO, not proof that the value came from the current primary. Root-owned
bootstrap must separately exclude writers, verify database lineage and no prior
R3 promises, and bind the SQL singleton. The migration UNBOUND allzero/allzero
row is never accepted by initialize/recover/prepare/finish. No public raw command
can produce an admission capability here.

The closed JSON record has `version:1`, `anchor`, and `publication`, whose exact
shape is either `{"state":"Stable","witness":...}` or
`{"state":"Pending","old":...,"new":...}`. Digests are nonzero64lowercasehex;
pending old and new differ. Extra/duplicate fields, invalid version/digests,
identity mismatch and records over8192bytes reject. A fixed `state.next` is
written/synced, renamed to `state.json`, and the directory synced. Failed
publication poisons that live owner; authoritative reopen is required. No
history-sized file, attempt counter or monotonically increasing epoch exists.

Private method contract:

- `acquire(directory) -> Fence`; `identity() -> &Digest`.
- `initialize(observed)` requires no prior record and exact nonzero anchor.
- `recover(observed) -> UnchangedStable | RecoveredOld | RecoveredNew`: stable
  requires exact equality and writes nothing. Pending permits exactly old/new
  and durably publishes that selected stable witness. Any other observation
  refuses without replacing state.
- `prepare(observed,binding)` requires exact current stable and1..4096 binding
  bytes. It combines `/dev/urandom`32bytes with domain, anchor, old witness and
  binding hash; durably publishes PENDING and returns the proposed new DTO.
  The caller must then conditionally update the SQL witness in its existing
  transaction and COMMIT. Rollback requires recovery from observed old before
  any new attempt. SQL must never commit before PENDING succeeds.
- `finish(observed_new)` only accepts the exact pending new witness. Stable,
  absent pending, old or arbitrary observations reject without overwrite.

Root migration contract: initialized singleton1 with anchor/witness64hex,
allzero paired UNBOUND, runtime SELECT and conditional UPDATE(witness) only.
Only the trusted migration owner may bind anchor+witness. The external STABLE
publication becomes the ACK boundary only after native integration proves
actual SQL outcome and excludes orphan backends. Existing gate PID/start checks
and reusable unresolved work remain necessary. Ordinary legacy mutations and
all reads/exports must participate in that future integration. Physical
PGDATA/WAL/temp/workspace enforcement remains independent and unproved.

Offline tests exercise exact state transitions and rejected no-growth paths,
width/closed-shape/zero/copy mismatch, symlink and replaced-lock rejection,
poison/reopen, actual competing child-process exclusion, and SIGKILL after
pending or stable publication. Pending tests supply synthetic observed old/new
DTOs; they do not claim actual PostgreSQL outcomes. The single ignored helper is
invoked as a child by those process tests, not a skipped conformance scenario.
