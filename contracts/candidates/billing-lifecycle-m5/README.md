# M5 billing lifecycle contract candidate

**Status:** Package 3 contract frozen after independent Sol and Astra review; not implemented and not accepted as conformance evidence.

This candidate contains additive M5 CLI input/output and sidecar-record families. It does not modify `contracts/schemas/v1/canonical-records.schema.json`, `contracts/freeze.json`, the canonical v1 fixture set, or any M1–M4 family. The product and economic source of truth remains [the canonical billing roadmap](../../../docs/billing-roadmap.md). Detailed behavior is in [the frozen M5 contract](../../../docs/m5-contracts.md) and [the reviewed architecture baseline](../../../docs/m5-architecture.md).

Owner-rule-to-contract-to-evidence mapping is in the [M5 contract traceability matrix](../../../docs/m5-contract-traceability.md).

## Candidate contents

- `schemas/requests.schema.json` — structural Draft 2020-12 schema for M5 request families.
- `schemas/results.schema.json` — structural schema for M5 result and error envelopes.
- `schemas/records.schema.json` — M5 command envelope and one-to-many sidecar domain-record families; these are not canonical ledger-v1 records.
- `schemas/finance-export.schema.json` — strict summary result for lifecycle finance export/4.
- `vectors/m5-oracle.json` — deterministic time, assignment, correction, cumulative-money, recurrence, presentation, fiscal-snapshot, recurrence-scope and canonicalization cases.
- `vectors/m5-command-goldens.json` — exact canonical request, identity, response, child-record bytes/hashes, and M3/M5 snapshot cuts for high-risk state-changing commands, including initial term setup, later empty-period resolution followed by close/retry, fiscal-calendar setup, cumulative-basis setup/activity/semantic alias/pre-close correction/fractional close, per-work corrections before and after close, immediate term transition, standard/ad hoc presentation ordering, fiscal report replay, recurrence opt-in/acceptance/cancellation, and exact occurrence retry.
- `vectors/finance-export-v4.json` — exact UTF-8/CRLF finance export bytes, summary identity and mapping-change vectors.
- `migration/schema-11-m5-sidecar.sql` — candidate additive sidecar DDL.
- `migration/schema-10-to-11.md` — explicit-upgrade preflight, atomicity, recovery and old-writer refusal contract.

Schema validation is only structural. Duplicate JSON keys, UTF-8/canonical-byte constraints, timestamp normalization, rational reduction, authority, ownership, current revisions, period lookup, M3/M5 linkage, snapshot-cut validation, bounds, lock ordering, and exact money conservation remain service/store checks. `billing_m5_commands` owns control idempotency and saved response bytes; one command may have many linked records. Cumulative activity additionally has durable delivery and semantic identity tables so exact semantic retries can reserve fresh delivery IDs without duplicating activity or quantity. `billing_m5_snapshot_boundaries` contains only atomic cross-stream cuts, including equal high-water cuts for new activity aliases. The recurrence golden wraps the exact accepted `base-acceptance` envelope captured from the released v0.7.1 source-built M4 CLI; it is synthetic contract material, not evidence that M5 is implemented or conformant. The contract families reconciled with the existing CLI, every owner-approved rule has an exact oracle case, migration DDL/preflight agree with the existing SQLite migration path, and independent Sol and Astra reviews passed. Product implementation, migration qualification, and release readiness remain separate gates.
