# Bean Counter v0.6.0 — M3 continuous billing history

This source release completes M3 for the documented local SQLite profile. It adds continuous retained billing history beyond the former 1,000-decision ceiling, durable target and retry indexing, concurrent duplicate/conflict handling, and an explicit schema-9 to schema-10 upgrade path. The local billing facade contract advances additively to v0.3.

## Qualification

- Three fresh installations each retained decisions through ordinal 1,026, including the oldest and crossing identity retries after reopen.
- A separate synthetic workload retained 3,499 decisions; all 3,850 submissions received an answer. Its complete statement and CSV reconciled to 608,350 atoms.
- The source-built M2 schema-9 history upgraded to schema 10 with receipts, aliases, controls, statements, backup restore, and exact retries preserved. Older-writer refusal and explicit-upgrade behavior were checked.
- Local billing unit/contract and CLI end-to-end gates passed. The source-built schema-9 path is the qualification boundary.

## Scope

The workload is one synthetic macOS run. Its timing and size do not establish general throughput, memory use, or completion at the 100,000-entry and byte ceilings. Each write scans retained rows to validate the history meter; full-history validation and reporting also grow with history size. Native artifacts, other source histories, other operating systems, PostgreSQL billing, multi-host writers, hosted operation, payment collection, and general capacity are not qualified by this release.

No Rust crates or native packages are published by this source release. The previously released v0.3.0 native packages remain the latest native distribution and are not M3-qualified.
