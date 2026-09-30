# Bean Counter v0.8.0 — M5 billing lifecycle

This source release completes M5 for the documented local SQLite profile. It adds customizable customer billing terms, a separate organization fiscal calendar, immutable period close, cumulative usage conversion, signed quantity corrections, linked post-close adjustments, ad hoc receivable/payable statements, explicit recurrence and occurrence acceptance, pinned fiscal reports and deterministic statement/4 finance export/4.

## What shipped

- Explicit schema-10 to schema-11 upgrade and append-only M5 sidecar with cross-stream snapshot boundaries.
- Acceptance-time billing-period assignment, effective-dated term changes and immutable close retry.
- Exact USD scale-18 cumulative aggregation with rational provenance and at most one `nearest_ties_away` booking at close.
- Open-period and post-close quantity-correction routing with original agreement bounds and exact adjustment presentation ownership.
- Explicit recurrence lifecycle with no background charge creation or automatic missed-run catch-up.
- Internal fiscal reports pinned to calendar version and both retained high-water marks.
- Exact 27-column CSV export/4 selected by immutable statement/4 hash.
- Offline schema/golden validation and new CLI coverage for the M5 operator surface.

## Qualification and scope

The release gates cover the locked source-built local SQLite profile, explicit schema upgrade, exact retries/reopen, immutable projections, contract goldens and the integrated billing/CLI suites. See [M5 qualification](../docs/m5-qualification.md) for the precise evidence and limits.

No native packages are published by this release. The v0.3.0 archives remain the latest native distribution and are not M5-qualified. PostgreSQL billing, multi-host writers, hosted operation, remote authentication, payment execution/status, tax/legal invoices, automatic recurrence scheduling and general capacity at the admission ceilings remain outside scope.
