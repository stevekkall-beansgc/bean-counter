# Native PostgreSQL physical premises — incomplete admission proof

The executable worksheet in `physical.rs` is an inventory of bounded **SQL
value inputs**, not a physical allocation quote. It is compiled only under the
native adapter test module. It does not implement or enable physical admission,
`AdjudicationStore`, or `CommitCapability` construction.

## Exact input inventory

Let J = 1115 bytes (binary journal/full index key limit), N = 16 bytes (count),
H = 64 bytes (hex digest), and V = 8,388,608 bytes (largest head/result value).
The worksheet adds every stored column's maximum payload, including generated
SHA-256 columns, duplicated full object identities, and nullable fields at their
maximum present size. Scalar payload widths are checked against each actual
server. Each native table's index count is checked against `pg_indexes`.

These are adapter-validated bounds. Some SQL fields rely on typed validation or
referenced parent identities rather than local length CHECK constraints. They do
not bound arbitrary migration-owner SQL. Individual maxima need not be jointly
attainable by one valid command.

| Native table | Row value payload, bytes | Index key payloads, bytes |
| --- | ---: | --- |
| r3_scope_locks | 2232 | 2232 |
| r3_journals | 5355 | 1115 |
| r3_storage_profile | 17535 | 4, 1115 |
| r3_segments | 1267 | 1131, 1179 |
| r3_segment_pages | 5231 | 1135 |
| r3_objects | 15631 | 1275, 1179, 1227 |
| r3_object_pages | 11519 | 1279 |
| r3_heads | 8390858 | 2234 |
| r3_head_versions | 8390874 | 2250 |
| r3_commands | 8664203 | 1147, 1131 |
| r3_namespaces | 1531 | 288 |
| r3_deliveries | 18139 | 640 |
| r3_index_pages | 5275 | 1179 |
| r3_index_roots | 2310 | 2246 |
| r3_held_intentions | 9331 | 1135 |
| r3_unresolved_work | 5316 | 4 |

A segment has at most 2048 native 4096-byte page rows; each object has at most
64. These row counts do not remove the duplicated identity payload on each row.
Current heads and immutable head versions both retain their complete values.
Native append rejects nonempty logical-radix index changes; those two tables are
listed for schema completeness, not claimed as a used physical index scheme.

The actual catalog inventory also includes all original/legacy tables and the
shared delivery namespace, including the additional prior-prefix expression
index. They are not covered by the 16-row numerical table. Their trigger writes,
original-base acceptance writes, all indexes and physical costs must be included
in a future complete per-command quote. Omitting them would understate ENROLL.

## Executable boundary evidence

The actual-store primitive test writes three distinct maximum-length keys with
three deterministic 8 MiB canonical JSON values, retaining each in both current
and version tables. Both TOAST relations must have nonzero actual storage. Two
values resolve exactly at the 16 MiB materialized-value budget; requesting a third
returns Overloaded, leaves the transaction unusable, and preserves the complete
retained row-hash inventory after rollback. This exercises native storage and
supervision; it does not create a validated economic plan or capability.

The test records main heap, index, total, TOAST, FSM and VM relation sizes, actual
index definitions, and cluster WAL LSN difference. Those observations depend on
compression, allocation state and possible concurrent cluster activity. They
are not upper bounds. Existing maximum schema/key/count tests and saved-work
recovery tests are complementary evidence, not substitutions for an allocation
proof.

## Missing allocation derivation

A finite worst-case quote still needs version/configuration-pinned derivations
for heap tuple headers, null maps, alignment and line pointers; varlena and
external TOAST references/chunks plus TOAST index entries; all B-tree pages,
split/root growth and maximum key encodings; FSM/VM forks and allocation units;
WAL record headers/alignment, full-page images, commit/abort records, relation
extension and segment recycling; and indexes/trigger writes for original/legacy
operations. Repeated updates of current heads and the fixed recovery slot also
create tuple/index/WAL churn despite a constant number of live rows.

The cached servers report exact versions/build flags and binary identities, but
targeted local searches found no corresponding server headers/source archives.
No download was attempted. The absent source alone does not prevent native
adapter or validated-plan implementation; it prevents asserting unverified
physical constants in this worksheet.

## Missing enforced backing and workspace premise

The inspected cached services place data, WAL, and default temporary files on
named volumes backed by the same ext4 filesystem as container overlay storage.
No isolated hard quota or preallocated exclusive reservation was established.
Docker reports no storage quota, memory limit, or PID limit. Free-space readings
and volume capacity are not reservations for already promised completion.

The inspected profile has 8 KiB database/WAL blocks, 16 MiB WAL segments,
full_page_writes on, wal_compression off, max_wal_size 1024 MiB, checkpoint_timeout
300 seconds, temp_file_limit -1, and max_slot_wal_keep_size -1. max_wal_size is
not a hard aggregate disk limit. There were no replication slots at inspection,
but their permitted creation is not a permanent bounded-retention guarantee.
PG17 checksums were off; PG18 checksums were on.

Workspace cannot be derived as one work_mem allowance: the inspected services
allow 100 connections, 8 parallel workers, 2 workers per gather, and 3 autovacuum
workers. work_mem is 4 MiB per relevant executor operation and the hash multiplier
is 2; temp_buffers is 8 MiB per session and maintenance_work_mem is 64 MiB, also
inherited by autovacuum. A complete profile must bound simultaneous operations,
TLS/client/server buffers, plans and spill, source/read materialization, control
and work sessions, background maintenance, and cancellation cleanup overlap.
The serialized writer gate does not cap independent readers or background work.

The smallest missing operational premise is an approved host-owned, non-stealable
reserved allocation with a hard aggregate boundary covering PGDATA, WAL, all
temporary/tablespace paths and maintenance headroom, together with enforced
session/workspace and WAL-retention/recycling limits. A hard limit alone is not a
promise that space below it remains available. The reserve must exclude unrelated
writers and cover every admitted mandatory completion plus reusable staging.
No filesystem, quota, server configuration or service layout was changed here.

Checkpoint/vacuum progress and recovery must be explicit assumptions or enforced
steps: elapsed checkpoint time alone cannot bound generated WAL, repeated failed
attempts can generate WAL/dead tuples, long snapshots can delay reuse, and slot or
archive retention can prevent recycling. The stable unresolved-work generation
proves no logical per-attempt counter consumption; it does not prove zero disk or
WAL cost per attempt. Fresh optional admission must remain closed until the full
profile establishes reusable recovery capacity independently of those attempts.

## Stop condition and next implementation step

This wave closes the bounded input/catalog and maximum-value exercise. It leaves
physical admission UNPROVED and production capability unavailable. Native full
ValidatedPlan projection can be exercised next using an explicitly internal test
harness without claiming host-backed production admission. Full runtime, actual
profile enforcement, and the complete physical quote remain required gates.
