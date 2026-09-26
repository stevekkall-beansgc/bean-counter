# M1 current-format qualification

**Result:** M1 is complete for the documented local billing profile. Evidence covers source-built v0.3.0→v0.4.0 ordinary local SQLite schema 8, with no production-format migration. v0.4.1 added caller documentation and runnable synthetic Python/Node examples; v0.4.2 updated compatibility and billing-contract documentation and rebranded the README. Neither changed billing implementation, production storage schema or native binaries. This qualification does not establish native-package conformance or a future-format transition.

## Compatibility policy

The owner-approved policy is published in [compatibility.md](compatibility.md). It preserves documented CLI/JSON behavior within declared families and support ranges; documents breaking 0.x changes; preserves patch contracts except for correctness or security fixes; versions families independently; refuses unsupported storage before accepting writes; and requires an exact record-preserving upgrade/recovery qualification before a future format accepts writes. Each release must state the exact source/build and versions it has evidence to support.

## Source versions and environment

- v0.3.0 source: `87892ac011902b286776e26925d8a3fb2c2aaa88`.
- v0.4.0 source: `67bc9ff5aca6537c1e8498ee8a9050bae9649e45`.
- Qualification host: macOS 26.6.2 on Apple Silicon (`aarch64-apple-darwin`). Both binaries were source-built in detached isolated worktrees with Rust 1.98.1, locked dependencies and offline Cargo mode.
- Ordinary billing reported SQLite `PRAGMA user_version = 8` before and after reopen.

## Evidence

| Check | Result |
| --- | --- |
| `sh scripts/check-local-billing-e2e.sh` at v0.3.0 | Passed: 25 tests covering reopen/retry, aliases, permissions, corrections and quiescent restore. |
| Same end-to-end command at v0.4.0 | Passed: 25 tests covering the same areas. |
| v0.3.0 synthetic integration | Passed setup, accepted events, outcomes, correction, complete statement, permissions, identical retry and copy/reopen; corrected total was 0 atoms. |
| v0.4.0 local billing library tests | Relevant tests passed, including unknown-commit reopen; 7 passed and 1 ignored. |
| v0.3.0 installation reopened by v0.4.0 | Complete statement and permission-status JSON values matched. Original delivery and semantic-alias retries resolved to the retained original receipt. |
| Reads after v0.4.0 writes | Python and Node caller examples accepted and retried synthetic events with the same receipt. v0.3.0 then read the added history; the complete statement and permission status matched. |
| Caller examples | Python and Node acknowledged an accepted event and, after restart, a duplicate with the same verified receipt. |

The comparisons included setup, retained record IDs and contents, receipts, aliases, permission status and statement totals. A later read-only SQLite comparison confirmed every pre-existing column value, including BLOBs, by primary key: one `billing_setup` row, five `billing_entries` rows, one `billing_aliases` row and zero `billing_permissions` rows. The reopened database contained two later entries; no original row was missing or changed. The `billing.json` files were byte-identical (147 bytes; SHA-256 `591189141bc4e821d25ab8c6201649c329139505c033613ec7f0ef69f84c94a9`). This compares retained values, not physical SQLite or WAL bytes.

For the v0.4.2 release candidate, the manifest-owned `sh scripts/check-local-billing.sh` and `sh scripts/check-local-billing-e2e.sh` gates passed in the isolated worktree and again in the exact main checkout. Exact-commit compliance workflow [36252305806](https://github.com/stevekkall-beansgc/bean-counter/actions/runs/36252305806) passed for commit `cf931dc63f071ae183521d604b3ec3d9dcc4b5e3`. The six Bean release gates passed for v0.4.2.

## Limits

Both source tags use schema 8, so no migration was required and no new-format older-binary refusal was tested. The evidence does not qualify published native binaries, cross-host operation, every historic store or any future production format. A future format must pass the exact supported-version, refusal, migration/transfer and recovery gate before it accepts writes. Native artifact qualification belongs to M8.
