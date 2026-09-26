# Billing M2 CLI contract

This contract describes the M2 local billing command families. It supplements the historical [v0.3.0 and v0.4.0 observed contract](billing-cli-contract.md). M2 changes command scope, accepted input families, storage, and statements independently; a version marker on one family does not version another.

## Commands

```text
ledger billing --directory DIR upgrade
ledger billing --directory DIR accept --customer CUSTOMER --source SOURCE FILE|-
ledger billing --directory DIR outcome --customer CUSTOMER --source SOURCE FILE|-
ledger billing --directory DIR correct --customer CUSTOMER --source SOURCE FILE|-
ledger billing --directory DIR agreement --customer CUSTOMER --source SOURCE FILE|-
ledger billing --directory DIR permissions --customer CUSTOMER --source SOURCE [FILE|-]
ledger billing --directory DIR explain --customer CUSTOMER TARGET_ID
ledger billing --directory DIR statement --customer CUSTOMER
ledger billing --directory DIR export-csv --customer CUSTOMER --snapshot HASH --mapping FILE --output FILE
```

Every write names both customer and source. The command scope must match the request fields when that input family carries them. Base work remains `ledger-event/1`; its customer must match the command, and the source is supplied by the command. An accepted base event is a separately submitted billable-work record. Accepting a request for work is not a billing operation.

## Versioned families

| Family | M2 format | Contract |
| --- | --- | --- |
| First installation setup | `ledger-local-billing/1` | Creates the first customer/source agreement. Existing fields and retained setup bytes remain unchanged. |
| Customer/source registration | `ledger-billing-registration/2` | Carries customer, source, stable `change_id`, `expected_revision: "0"`, explicit `effective_at`, and a complete setup `/1`. A new customer receives a derived scope; registration cannot choose it. |
| Agreement amendment/end/restart | `ledger-billing-amendment/2`, `ledger-billing-ending/2`, registration `/2` | Each carries customer, source, stable `change_id`, expected revision, and effective time. End also carries the exact current `agreement` ID and reason. Controls are append-only and exact retries return the saved result. |
| Permission change/status | `ledger-billing-permissions/2`, `ledger-billing-permission-status/2` | Changes carry customer, source, stable `change_id`, expected revision, permissions, and reason. Status is an administrator read, including after event-read revocation. Exact pre-M2 `/1` retries are accepted only for an exact retained legacy change on the original customer/source; new `/1` changes refuse. |
| Outcome/correction | `ledger-billing-outcome/2`, `ledger-billing-correction/2` | Both carry customer and source. They require current scope authority and use the target's original agreement and policy. The target must belong to the named customer and source; a missing or other-source target returns the same `BILLING_NOT_FOUND` result. |
| Statement/explanation | `ledger-billing-statement/2` | `statement --customer C` covers all registered sources for C. It refuses rather than returning partial data if any source lacks read authority. `explain --customer C TARGET` contains only that target's authorized source history; a missing target and an unreadable target return the same response. It discloses no other customer's or unreadable source's target existence. Ordinals, cutoff, agreements, and snapshot hash are limited to the returned authorized history. |
| Finance export | `ledger-finance-export/2` | Export requires the complete statement `/2` for one customer. The CSV completion row and export result do not indicate delivery or payment. |
| Stored installation | `ledger-billing-installation/1` | The config marker is unchanged. SQLite writer schema is 9. |

Base acceptance resolves the agreement active at the first successful serialized ledger acceptance timestamp, after exact delivery and semantic retries are resolved. Caller `occurred_at` remains evidence and does not select price. Exact retries keep the original receipt, price, agreement version, and target across later amendments or ending. Same source/delivery and operation IDs can be reused by a different customer; within one `(customer, source)` pair, conflicting reuse refuses.

## Storage and upgrade

New installations are created at SQLite schema 9. Opening schema 8 refuses without migration. Run `billing upgrade` explicitly against a v0.4.3 schema-8 installation. The coordinator validates the complete retained setup, permission history, receipts, entries, and aliases before mutation; the migration then rechecks a digest of the exact schema-8 rows under the exclusive owner lock and commits schema 9 and its initial customer/agreement mapping in one transaction. An unknown result is reconciled by reopening: schema 9 with its exact mapping is already current; schema 8 is eligible for the same explicit retry. Never replace a nonempty installation with a new store.

The intended support range is source-built v0.4.3 ordinary billing schema 8 to schema 9. This transition is not qualified for another source version, native archive, or historical billing store. Release activation requires an exact v0.4.3 source-built fixture, unsupported old-writer refusal, row/receipt/alias/permission reconciliation, rollback and backup-recovery evidence. See the [canonical billing roadmap](billing-roadmap.md) and [M2 implementation design](m2-implementation-design.md).

## Retry and authority

For base, outcome, and correction submissions, persist the exact request in a caller-owned durable outbox before invoking the CLI. Keep the same customer, source, delivery ID, operation ID, and bytes after an unknown result. A duplicate acknowledgment returns the original receipt. Permission and agreement controls use their stable `change_id`; exact retries return the original control result before stale-revision checks. A changed request under the same saved ID conflicts. Permission status remains available to the local filesystem administrator for reconciliation after read revocation, and an exact permission retry is safe after either commit outcome.

Agreement changes have explicit future effective times. After an unknown result, retry the exact bytes while the requested effective time remains future; a committed change returns its saved result, while a rolled-back change can be applied once. If the requested effective time has passed, the same exact request refuses with `BILLING_EFFECTIVE_TIME` when no result was saved. The operator can then submit a newly timed control after checking the expected revision. This avoids backdating a change over already accepted work.

Run the durable caller examples as `LEDGER BILLING_DIR CUSTOMER SOURCE EVENT_JSON OUTBOX_DIR`, with one private outbox per customer/source scope. The outbox binds its durable marker to the installation and that pair. See the [integration example guide](../examples/integration/README.md) and [backup/recovery procedure](billing-recovery.md). These local commands do not provide remote authentication, payment execution, tax invoices, or a guarantee that a request for work occurred.
