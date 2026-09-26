# M2 schema-8 to schema-9 qualification

**Result:** The M2 storage activation gate passed for the source-built v0.4.3 ordinary local SQLite profile. It qualifies the exact source-built v0.4.3 schema-8 store to the M2 implementation at `685ea0aa1f5f5a79042802bfc2d9ccfb1aa2d00f`. It does not qualify native packages, another source version, another host, or every historical store.

## Sources and environment

- Source store writer: tag `v0.4.3`, commit `e9415516ae5904a528fee5b1d45a1e83e08e9c13`; binary SHA-256 `82e1d94b32e0cb749d38f1a54e8c8dfa838a046b1566feb4450d7b79702109da`.
- M2 candidate: commit `685ea0aa1f5f5a79042802bfc2d9ccfb1aa2d00f`; source-built `ledger` binary SHA-256 `dd2532bfd25eb92ebda960238c8637b4a8ea251f52dd052704e8017ffd512d26`.
- Host: macOS 26.6.2, arm64. Toolchain: Rust 1.98.1, Cargo 1.98.1, locked dependencies, offline build.
- Fixture: synthetic v0.4.3 history with schema 8, five economic entries, one semantic alias, two permission changes, and one setup row. Before migration its statement was complete at cutoff 5 with net `0` atoms; permission revision was 3.

## Transition and preservation

The explicit `ledger billing --directory DIR upgrade --json` returned `{"from_schema":8,"status":"upgraded","to_schema":9}`. Migration history advanced from 8 to 9. A logical SQLite snapshot compared every column and value, including BLOBs, for all 52 pre-existing tables; all matched exactly. The original billing tables still contained five entries, one alias, two permission changes, and one setup. Schema 9 added six tables; the migrated customer and first agreement were present for `synthetic-customer` / `urn:example:product`.

The M2 statement was complete with five entries and net `0` atoms. Its cutoff, entries, postings, receipts, and retained records matched the v0.4.3 statement's economic projection. Permission revision 3, its three rights, and both change records also matched.

Seven exact submissions were retried after migration: the base work, its semantic alias, two outcomes, a correction, and the original `/1` permission revoke and restore. The five work/outcome/correction calls returned their original receipts; the permission calls returned revisions 2 and 3 with their original rights. A full logical snapshot before and after the retry set was identical, so no rows were added or changed.

## Older-writer refusal and recovery

The v0.4.3 binary attempted `accept` with the already-retained synthetic work request against schema 9. It exited 9 with `INTEGRITY_FAILURE` / `retained data failed verification`. Logical snapshots before and after the attempt were identical. The persistent database and WAL hashes also remained identical: `local.db` `acd5f27a637f2bad9b98f362884ba17ceb169e8d36d6993b36bd0676dd5d0cf0` and `local.db-wal` (empty) `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. The transient `local.db-shm` file was absent both before and after this checkpointed attempt.

A whole-installation pre-upgrade backup was restored to a new directory. The v0.4.3 statement and permission JSON exactly matched the saved pre-upgrade values. M2 then upgraded that restored copy from schema 8 to 9; the same 52 legacy tables were preserved, the complete statement and permission history reconciled, and an identical base-work retry returned the original receipt.

## Checks

- `sh scripts/check-local-billing.sh`: passed on the candidate commit, including strict Clippy, local billing tests, the atomic upgrade-failure test `billing_upgrade_failures_owner_and_partial_states_refuse_atomically`, and `billing_upgrade_preserves_original_bytes_and_reconciles_unknown_commit`.
- `sh scripts/check-local-billing-e2e.sh`: passed on the candidate commit: 6 billing tests, 17 CLI-local tests, and 1 finance CSV test.
- Astra implementation review: no blocker remained after fixes for the legacy 256-byte source limit and indistinguishable wrong-source/missing-target refusals.

This evidence is limited to the named source-built schema-8 transition on macOS. Native artifacts and other platforms remain M8 work; M3's higher-volume and concurrency claims are not covered.
