# Candidate schema-10 to schema-11 migration contract

**Status:** draft, not frozen, not implementation evidence. The companion SQL is candidate additive DDL only.

## Supported transition

- The only source format accepted by this candidate upgrade is ordinary SQLite writer schema 10.
- `billing upgrade --json` is explicit. Opening a schema-10 store does not migrate it, and ordinary writes refuse until the operator completes the upgrade.
- New installations are created directly at schema 11 when the M5 implementation ships.
- The schema-10-to-11 claim is source-build-specific until a separate fixture and recovery qualification passes. No native package or other host is implied.

## Preflight, before DDL

Acquire the existing exclusive installation owner and `BEGIN IMMEDIATE`. Recheck the path and schema version after acquiring the SQLite writer lock. Require `PRAGMA user_version=10`, a complete matching SQLx migration ledger through version 10, `PRAGMA integrity_check='ok'`, and the current installation marker/owner invariants. Run the existing schema-10 consistency checks. `billing_m3_index` is the merged durable index; each indexed ordinal must match exactly one retained decision row in its original tier: `billing_entries`, `billing_m2_entries`, or `billing_m3_entries`. Do not require every row to be in `billing_m3_entries`. For legacy `billing_entries`, derive customer/scope through the unchanged schema-10 legacy setup/agreement rules. For each tier, verify the exact ordinal and available source, external ID, semantic key, customer, accepted timestamp, agreement/version, retained bundle/receipt and amount against the index using the unchanged M3 verifier. Missing or duplicate tier rows, failed legacy derivation, unknown retained bytes, or any index/bundle/hash mismatch refuses before DDL.

Before any schema change, reject if any object name starts with `billing_m5_`, if an M5 migration identity is already present with an unrecognized value, or if M3 data fails its existing integrity contract. Preflight does not infer initial billing terms, periods, statement totals, or correction eligibility from caller work timestamps. Existing M4 history receives its initial period coverage only through the explicit customer term activation described in the M5 contract.

## Atomic upgrade

Within the same exclusive owner and immediate transaction:

1. Re-run the complete preflight predicates against the locked schema-10 snapshot.
2. Apply the exact `schema-11-m5-sidecar.sql` DDL after collision checks.
3. Insert the singleton sidecar state row with a deterministic migration identity, zero M5 command/record rows and bytes, zero activity-identity rows/bytes, and next command and record sequences both 1. Insert the initial complete snapshot boundary `(max(billing_m3_index.ordinal), 0)`; this is the pre-M5 snapshot and is the only schema-11 report cut at migration time.
4. Record migration 11 in the SQLx migration ledger with the reviewed SQL checksum and set `PRAGMA user_version=11`.
5. Re-read `user_version`, migration checksums, sidecar state, the initial snapshot boundary, unchanged schema-10 table definitions, and unchanged M3 row counts/hashes.
6. Commit once. A failure before commit rolls back every new object, migration row, and version change.

The upgrade does not rewrite or backfill M1–M4 event bytes, receipts, identities, agreement records, outcomes, aliases, statements, or M3 indexes. It creates empty M5 domain and activity-identity tables plus the initial snapshot boundary at the existing M3 high-water. Activation of a customer's first term later validates complete retained-history coverage and creates the exact M5 assignments needed for close.

## Unknown result and recovery

If the caller loses the commit acknowledgment, it reopens the same installation under the normal owner protocol and reads the whole migration state. Schema 11 plus the expected migration-11 checksum, exact singleton migration identity, and intact M3/M5 invariants means the upgrade committed. Schema 10 with no M5 objects and no migration-11 row permits retry of the identical explicit upgrade. Any mixed schema, partial object set, unexpected checksum, or M3 mismatch fails closed; do not drop tables, lower `user_version`, or repair by editing rows.

## Old-writer refusal and bounds

Schema-10 binaries reject `user_version=11` before any billing read or write that could mutate state. The current schema verifier already rejects versions greater than 10; M5 must preserve and qualify that behavior. There is no downgrade path.

The M5 sidecar permits at most 100,000 combined M5 command, domain-record, and activity-identity rows and 268,435,456 combined exact request, response, payload, command-identity, activity-ingress, and semantic-facts bytes. Count and byte reservations are checked and committed with the corresponding rows under one writer transaction. The singleton `activity_identity_count` and `activity_identity_bytes` counters are monotone and must reconcile exactly to both activity identity tables on open. A semantic activity alias consumes one activity-delivery row and its retained ingress bytes, but no command or domain record; it appends a same-cut snapshot boundary atomically. Assignment and adjustment-link projections are not M5 domain records and are separately bounded by the existing M3 source-row limit; snapshot-boundary metadata is bounded by accepted M3/M5 mutation counts. At either M5 limit, the entire operation refuses with `BILLING_M5_BOUNDS`; it must leave no partial M3 row, M5 record, identity, projection, statement claim, counter, or revision. No pruning or automatic identity expiry is introduced.

## Qualification evidence required before claiming support

The candidate contract is not evidence. Before release, preserve an exact schema-10 fixture and compare all preexisting table definitions, row counts, retained bytes, aliases, identities, receipts, permissions, agreements, outcomes, and statement outputs before/after upgrade. Exercise rollback on an injected pre-commit failure, unknown-commit reconciliation, whole-installation backup and restore, and an actual schema-10 writer refusing the upgraded fixture before mutation. Publish only the exact source-built transition that passed those checks.
