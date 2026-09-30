# M5 source-built local qualification

## Qualified boundary

The M5 implementation targets Bean Counter v0.8.0 for the provider-free, one-business-per-installation local SQLite profile. Qualification attaches only to a source build from the exact released commit after the gates below pass, using the repository's locked dependencies and pinned Rust 1.98.1 toolchain; before publication, the checkout is a release candidate rather than a qualified release. The boundary includes the explicit schema-10 to schema-11 upgrade. It does not qualify native packages, PostgreSQL billing, multiple simultaneous writers, hosted operation, payment execution, tax/legal invoices or general performance at the admission ceilings.

M5 is additive. Existing fixed-price and per-work usage acceptance, outcomes, corrections, statement/2–3 and finance-export/2–3 remain byte-compatible. Schema 11 adds an immutable sidecar for customer terms and period assignments, fiscal calendars/reports, cumulative activity and basis versions, quantity corrections and post-close adjustments, recurrence/occurrences, statement/4 presentation claims and snapshot boundaries. Ordinary schema-10 writers refuse until the explicit upgrade succeeds.

## Economic and lifecycle evidence

The integrated tests exercise:

- finite day/week/month/year and Gregorian/4-4-5-family calendar calculations, DST handling, term transitions, later empty-period materialization and immutable close retry;
- complete retained M3 assignment, exact acceptance-time period selection and refusal of uncovered or closed-period work;
- scale-18 per-work corrections and cumulative aggregate conversion, including rational normalization, one `nearest_ties_away` booking and telescoping `B(q_new)-B(q_old)` adjustments;
- open versus closed quantity-correction routing under the writer lock, original agreement/unit/rate/bounds, immutable closed statements and mutually exclusive standard/ad hoc presentation;
- explicit recurrence set/cancel/query/accept, stable occurrence identity, opt-in successor agreement selection, no query mutation and no automatic catch-up;
- fiscal reports pinned to calendar version plus M3/M5 high-water marks, with each monetary effect owned and counted once and nonmonetary quantities reported separately;
- exact statement/4 finance export/4 mapping to a fixed 27-column UTF-8/CRLF CSV, including line/source/calculation provenance, reconciliation and a completion trailer.

Every accepted M5 command retains its exact identity, request, response, accepted time and domain-separated hashes. Child records are canonical, append-only and linked to the command. Exact retries return the saved result; changed identity reuse refuses. Open verifies commands, children, projections, high-water boundaries, hashes, monotonic clocks and presentation ownership before permitting work.

## Contract and storage gates

The offline M5 contract gate validates four Draft 2020-12 schemas with 444 local references, 22 command requests, 22 results, 22 command records, 33 child records, three finance vectors, the oracle values, embedded M4 receipts and negative rejection probes. It also recomputes canonical bytes, Base64 envelopes, record IDs and domain-separated hashes.

The SQLite qualification covers fresh schema 11 creation, exact schema-10 preflight, preserved setup/history bytes, explicit upgrade and retry reconciliation, malformed-source refusal before DDL, transactional rollback after injected post-DDL failure, schema-10 ordinary-write refusal and snapshot-boundary advancement. Existing process-exit, unknown-commit, immutable-row, retained-identity and quiescent restore suites continue to pass. These tests are software evidence under SQLite/WAL/FULL and the documented host/storage assumptions; they are not a power-loss or storage-hardware certification.

## Limits and operator obligations

M5 retains at most 100,000 combined command, domain-record, semantic-identity and delivery-identity rows, with 256 MiB shared by their exact canonical material. The limits are refusal ceilings, not reserved capacity or throughput guarantees. There is no automatic pruning, payment status, collection or background recurrence runner.

Keep the installation private and single-owner, stop applications before upgrade or backup, preserve the whole installation directory, verify restored history before admitting writes and retain the exact source commit and binary hash. An unknown outcome is not a rollback: reopen and retry the identical original request. Do not bypass an integrity refusal or invent a replacement identity.

## Release evidence

The release candidate must pass the Agency QA manifest's exact-checkout commands: the local billing gate (formatting, core/testkit coverage, the integrated billing slice, dedicated M5 migrations, clippy with warnings denied, no-default compilation and contract/schema checks), the billing CLI/e2e gate and exact-commit GitHub compliance CI before the annotated tag is published. The GitHub release identifies the exact qualifying commit. Unrelated workspace stress suites remain useful repository evidence but are not M5 release gates. Focused test counts are informative only; the manifest-owned release checks and commit are the authoritative evidence.
