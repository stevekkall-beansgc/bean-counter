# M2 implementation design

This design implements the customer and agreement scope in [the canonical billing roadmap](billing-roadmap.md). It keeps one business per installation and adds customer/source partitions inside that business. Multi-business tenancy and hosted infrastructure remain deferred.

## Identity and isolation

- A customer is an installation-local identifier. Each registered `(customer, source)` pair has its own operator-configured authority and agreement timeline. A customer may have more than one source; a source may serve more than one customer.
- Delivery, semantic, and control-command identities are scoped by `(source, customer)`. SQLite keys and alias checks use both values.
- Existing v0.4.3 records keep their original canonical scope and bytes. New customer scopes use the installation tenant plus a domain-separated SHA-256 of the installation scope and customer ID. The resulting scope is stored once per customer and checked on every open; it gives different customers separate event, chain, receipt, target, and economic-record IDs without changing the frozen core identity algorithm.
- Customer/source is required on each write command and is cross-checked against the request and registered profile. Statements and exports require a customer and cover all of that customer's sources; they refuse rather than return a partial statement if any source lacks read authority. Explain also requires a customer and never reports whether a target exists outside that customer.
- A request-intake operation is not implemented. `accept` records a separately submitted billable-work event; accepting a request, if added later, must not create a posting.

## Agreement records and time selection

- A `(customer, source)` timeline is append-only. `start` adds version 1, `amend` adds the next version of the same agreement, and `end` adds a terminal transition. A new agreement may start after an end. Every control request has a stable `change_id`, exact-retry behavior, and an expected revision.
- Start/amend inputs carry a complete validated terms setup plus an explicit `effective_at`. End inputs carry the customer, source, agreement, expected revision, effective time, stable ID, and reason. Assent time remains separate from effective time.
- Control transitions must form one unambiguous timeline. Effective times increase by transition order; an end and a new start may share a boundary timestamp, with the start later in the same serialized order. At any work-acceptance time there is either one active agreement or none.
- The writer begins its serialized SQLite transaction before sampling the ledger clock. Only after an exact delivery or semantic retry has been resolved does a new billable event select the agreement active at that timestamp. Caller `occurred_at` remains evidence and never selects price. The selected agreement ID/version is retained with the entry and in its immutable economic records.
- Exact delivery/semantic retries are resolved before current effective terms. They return the original result across amendments and ending. A new semantic alias still resolves to the original accepted price. Outcomes and corrections use the target's original terms/policy and current customer/source authority.
- Agreement amendments cannot reset customer/source permission ceilings. Permission updates are append-only, customer/source-scoped control records with their own stable identities.

## Storage transition

- M2 introduces ordinary billing SQLite schema 9. A new store is created directly at schema 9. Existing schema-8 installations require an explicit `billing upgrade` command; ordinary M2 opens and writes never silently migrate a store.
- Migration 0009 preserves the schema-8 setup blob and every pre-existing entry, alias, permission row, record bundle, receipt, and external identity byte-for-byte. It associates those rows with the legacy customer and source, records the original scope, and creates agreement version 1 from the existing setup. The first migrated customer's scope remains unchanged.
- The transition runs in one SQLite transaction and refuses unsupported version/checksum/store-identity states before mutation. A release can enable schema-9 writes only after exact v0.4.3 schema-8 migration, old-writer refusal, row/receipt reconciliation, and pre-migration backup restore are qualified. The existing quiescent whole-installation recovery procedure remains the backup boundary.
- M2 retains the existing global 1,000-entry, 1,000-alias, 1,000-permission-change, 32 MiB economic, and 32 MiB alias-ingress ceilings. Customer/agreement/control storage receives explicit bounds; M3 owns any higher-throughput claim.

## Versioned command families

- Existing setup `/1` remains the first-customer setup and migration format.
- New customer/source registration wraps a `/1` setup with an explicit start time and stable change ID.
- Agreement amend/end, permission updates, outcomes, corrections, statements, and CLI command shapes use new `/2` family boundaries where their customer scope or semantics change. Each changed family is listed independently in the v0.5 compatibility record.
- Statement output identifies the customer and all agreement versions represented by its entries. Its cutoff and digest cover that customer's complete retained history only; they do not reveal another customer's row count or receipts.

## M2 exit evidence

In addition to the storage transition, M2 must demonstrate two customers reusing the same source, delivery ID, and operation ID with separate histories; scoped exact/semantic retries and conflicts; missing/wrong/unauthorized customer refusal without disclosure or mutation; agreement start/amend/end boundaries chosen by ledger acceptance time; old-price retry and target-policy outcome/correction after change/end; customer-complete statements and exports; scoped permission revocation and restart; and unknown-commit recovery for work and control changes. A request intake does not post a charge.
