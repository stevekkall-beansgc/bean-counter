# M5 architecture and contract proposal

**Status (2026-09-29): Package 3 design draft for independent review; no product code has changed.** The [billing roadmap](billing-roadmap.md) is canonical. The [M5 decision register](m5-decision-register.md) is the owner-approved policy record. This proposal translates those choices into a compatible implementation shape; it does not authorize reopening a frozen economic rule.

## 1. Scope and existing baseline

M5 extends the provider-free, one-business-per-installation, local SQLite billing profile. M4 currently accepts fixed-price and scale-18 unit-price work into an append-only billing history, retains each receipt's accepted time, and derives `ledger-billing-statement/2` or `/3` as a read projection. It does not persist billing-period membership or closed statements. The schema-10 `billing_m3_index` supplies a bounded accepted-time index; `billing_m3_entries` retains the exact accepted bundles. Agreement and permission changes are already effective-dated M2 controls.

The M5 implementation must preserve every M1–M4 accepted bundle, alias, retry identity, agreement version, outcome decision, exact amount, and statement output for the profiles already qualified. M5 adds an append-only billing lifecycle sidecar and versioned statement projections. It must not rewrite M4 records or extend the frozen `ledger-canonical-v1` first-slice family by analogy. Any required amendment to that record family is a separate reviewed contract change.

M5 adds these capabilities:

- Immutable customer billing-term versions and immutable organization fiscal-calendar versions.
- Deterministic period assignment by the accepted timestamp retained by the ledger.
- Explicit, idempotent period close with an immutable statement snapshot.
- A separate cumulative-activity model that aggregates within a period and performs configured conversion only at period close.
- Append-only quantity deltas before a period boundary and linked post-close adjustments after it.
- Stable recurring-occurrence identities with operator-triggered generation, explicit catch-up, opt-in renewal, cancellation, and no implicit proration.
- Internal fiscal reports reproducible from a pinned calendar version and ledger snapshot.

It adds no payment provider, payment execution, payment verification, tax/legal invoice semantics, multi-business tenancy, or automatic expiry.

## 2. Governing invariants

1. **One accepted-time clock.** Period membership uses the ledger's retained acceptance timestamp, represented as signed UTC epoch microseconds. Caller `occurred_at` and scheduled occurrence time remain evidence. They never backdate agreement selection, customer billing period, or fiscal period.
2. **Half-open periods.** Every period is `[start_utc, end_utc)`. An acceptance at `end_utc` belongs only to the next period. Calendar calculation and assignment use integer timestamps; local wall time is only used to define calendar boundaries.
3. **Immutable versions and close records.** Terms, recurrence rules, fiscal-calendar definitions, adjustment decisions, period close records, and issued ad hoc statements are append-only. A close pins its resolved UTC boundaries, term version, accepted ledger cutoff, included record IDs, exact totals, and canonical statement bytes. Re-running the same close identity returns the same statement; conflicting reuse refuses.
4. **No repricing.** A schedule or calendar change cannot alter the accepted agreement version or amount on a work item. A recurring occurrence uses the agreement version selected by its first successful acceptance, not the version current at its scheduled time.
5. **No closed-period rewrite.** Corrections accepted before the applicable period end are signed deltas in that period. A correction accepted at or after the period end is a post-close adjustment assigned from its own accepted time; it never edits a close record. The standard close operation is permitted only once `accepted_now >= period_end`.
6. **Exact economics.** Preserve M4 USD scale-18 integer atoms. Quantity correction arithmetic is exact rate-atoms × signed integer delta. Aggregate totals use exact unbounded integer arithmetic with explicit overflow/size refusal; no cents or four-decimal rounding is introduced.
7. **Statement classification is not collection.** A positive period net is receivable, a negative period net is payable, and zero has no direction. A standard statement has no prior-balance carry-forward. Direction does not assert payment, settlement, or provider verification.
8. **Operator-owned side effects.** No background job creates or charges recurrence occurrences, catches up missed occurrences, issues adjustments, or closes statements. Commands are explicit, bounded, replay-safe operations.

## 3. Calendar term and period calculation

### Customer billing terms

A customer chooses an immutable version from the centrally validated term grammar in the decision register. The proposed wire family is `ledger-billing-term/1`, containing:

```text
schema, customer, version, effective_at, interval, unit,
alignment, anchor, timezone, week_start?, month_end_rule,
boundary_rule_version, timezone_rules_version, proration
```

`unit` is `day | week | month | year`; `interval` is a positive integer. `timezone` is an IANA identifier. `week_start` is required for weekly calendar alignment. `anchor` is a local date plus optional local time. `proration` is `none` unless a separately versioned exact formula is explicitly selected. Reject unknown fields and any term whose boundary arithmetic leaves the supported Gregorian/time range.

For an anchored term, derive boundary `n` from the immutable anchor and integer period index `n`; never derive it by repeatedly advancing the previous clamped boundary. For calendar-aligned terms, use the selected natural day/week/month/year boundary and the declared interval. Month/year calculation is calendar arithmetic. Apply the pinned month-end rule (`preserve-anchor-and-clamp` or `explicit-end-of-month`). Resolve a nonexistent local boundary to the first valid instant after the DST gap and an ambiguous local boundary to the earlier UTC instant. Freeze the IANA database release or equivalent timezone-rules data version with the term; do not depend on the host's mutable timezone database. Persist every resolved boundary used by a close.

A regular term change takes effect at the next unclosed period boundary. An explicit immediate change may produce a short period only when its billing/proration treatment is included in the accepted term change. The default `proration:none` means the engine does not synthesize a time-proportional amount; accepted recurring occurrences continue to use their agreed amount.

### Organization fiscal calendar

The fiscal calendar is a separately versioned `ledger-fiscal-calendar/1` object scoped to the one installation. It supports Gregorian month, quarter, and year calendars with configurable fiscal-year and week starts, plus bounded 4-4-5, 4-5-4, and 5-4-4 week patterns. A custom week pattern declares its period lengths, week start, 52/53-week rule, and destination period for the extra week. It is internal reporting metadata; it never selects customer periods or changes receivables/payables.

### Deterministic lookup

`period_for(customer, accepted_at_us, snapshot)` resolves exactly one immutable effective term version, calculates boundaries from that version's anchor and pinned timezone rules, and returns a stable period key `(customer, term_version, start_utc_us, end_utc_us)`. The resolution refuses if versions overlap, leave a gap, or do not cover the accepted timestamp. No acceptance can be silently assigned to a default calendar.

**Owner gate for Package 3:** the initial M5 term must say how it covers M4 records accepted before that term is configured. The safe proposal is an explicit one-time activation boundary per existing customer, chosen by the operator, with any earlier accepted records exposed as unassigned until explicitly covered; the system must never silently move them into a current statement. This activation/backfill rule affects statement amounts and needs an explicit decision before period-close implementation.

## 4. Storage and compatibility shape

Add an ordinary-schema transition from SQLite schema 10 to schema 11 using an explicit preflight and atomic upgrade path. The regular open/write path never performs a silent schema upgrade. The migration is additive: preserve schema-10 tables and bytes and create M5 lifecycle tables/indexes. A schema-10 writer must refuse a schema-11 installation before any mutation; schema-11 read/write behavior is qualified separately from native package support.

The proposed M5 sidecar contains:

| Record set | Purpose | Mutability |
|---|---|---|
| `billing_m5_records` | Shared append-only sequence of validated term changes, correction deltas, post-close adjustments, occurrence results, close records, ad hoc statements, fiscal-calendar changes, and report snapshots. | Insert only; stable request identity and canonical payload retained. |
| `billing_m5_identity` | Customer/source-scoped command retry lookup and semantic identity. | Insert only; unique `(customer, source, command_id)` and stable semantic keys. |
| `billing_m5_period_index` | Derived lookup of accepted event IDs, adjustments, period keys, and close IDs. | Immutable rows; every field checked against its source record on open/reconciliation. |
| `billing_m5_bounds` | Monotone row/byte guard and storage limit. | Guarded monotone updates only; fail closed at the limit. |

The exact table split and column layout belong in the reviewed migration contract. SQL enforces immutability, unique keys, foreign keys, monotone bounds, and supported-size limits. The service/coordinator validates calendars, authority, money, period membership, and complete statement content; the store does not evaluate economics. Persist canonical JSON bytes plus a domain-separated content hash for each M5 record. Keep M5 records in their own versioned families (for example `ledger-billing-term/1`, `ledger-billing-period-close/1`, and `ledger-billing-quantity-delta/1`); do not add these variants to `contracts/schemas/v1/canonical-records.schema.json` without a separate amendment.

Every write runs in one `BEGIN IMMEDIATE` transaction and appends the M5 decision, scoped identity mapping, and derived index rows atomically. The writer lock serializes close against corrections and close against recurrence occurrence acceptance. Sample the accepted/recorded time after acquiring the writer transaction, then validate strict monotonicity against the maximum retained accepted or control timestamp, including M5 rows. Preserve unknown-commit behavior: lookup the same stable identity and return the original result, or return an explicit unknown outcome without retrying under a new identity.

M5 identity retention follows M2: no accepted command, occurrence key, adjustment, or statement identity expires. Backup/restore captures the whole installation, including M5 sidecars. The schema-10→11 activation gate must validate existing schema-10 history before enabling M5 writes; it must not re-price or re-encode any M4 entry.

## 5. Period close and standard statement

`billing close` accepts an explicit customer and period identity. It refuses before the period end, rejects invalid or ambiguous period IDs, and requires the active term version and resolved boundary to match retained history. In the same serialized SQLite write transaction it reads all accepted M3 entries and eligible M5 adjustments whose ledger acceptance time maps to `[start,end)`, applies pre-close quantity deltas to their linked original usage, computes period totals, and appends one immutable close record. A missing outcome does not remove an initial charge. Outcomes and other accepted facts received after a period ends belong to the period selected by their later acceptance time. A late fact can never cause the previous period's close bytes to change.

The close stores:

- Stable close ID and command identity.
- Customer, billing-term version, period key, local labels, and exact resolved UTC `[start,end)` boundaries.
- Close acceptance time and M3/M5 high-water snapshot needed for replay.
- Sorted M3 receipt IDs, M5 delta/adjustment IDs, and per-line exact calculation provenance.
- Exact scale-18 USD line amounts and net atoms, with positive/negative direction classification.
- Canonical output bytes and a domain-separated hash.

The client-facing statement family advances from `/3` to `ledger-billing-statement/4` for lifecycle statements. It contains one period's exact net only, no previous balance, all included source record IDs, accepted-time assignment evidence, active agreement versions, `net_atoms`, `direction: receivable | payable | none`, `complete:true`, and a statement hash. It preserves M4 field meanings where they still apply and identifies the selected period explicitly. Existing `/2` and `/3` outputs remain available for their qualified non-lifecycle profiles; never reinterpret them as closed period statements. CSV export advances independently to `/4` and preserves exact scale-18 atoms as text.

Close retries use `(customer, term_version, period_start_utc)` as the stable semantic identity. The first successful close result is immutable. A changed term/boundary or changed close content under the same key returns a conflict. If storage reports an unknown commit, the CLI retries the identical key and receives the original close or a bounded unknown-result error.

## 6. Usage corrections and post-close adjustments

### Pre-period-end quantity delta

Proposed request family: `ledger-billing-quantity-correction/1` with `customer`, `source`, stable `id`, original accepted usage target, signed integer `quantity_delta`, `occurred_at` evidence, and retained correction evidence. A write requires current customer/source `correct` authority and the original agreement's correction authorization. The unit, currency, scale, rate, and agreement version are taken from the immutable accepted record, never from the correction request.

Only an integer delta is accepted; replacement quantities are rejected. Under the writer lock, compute `current_effective_quantity = original_quantity + all_prior_accepted_deltas`. Reject overflow, a negative result, a result above the original agreement's configured maximum, a closed target period, or a delta whose exact amount exceeds the existing Money bound. The signed amount is `original_rate_atoms * quantity_delta` at scale 18. Append one immutable correction record; never update the base event or prior deltas. Exact retry returns its original receipt; conflicting identity reuse refuses.

### Post-period-end adjustment

For an adjustment accepted at or after the original period end, append a separate `ledger-billing-post-close-adjustment/1` record that links to the original usage and retains correction evidence, signed quantity delta, old/new effective quantities, original agreement/rate basis, source, and accepted time. The adjustment's ledger acceptance time determines its next-period key; the old period and any close remain unchanged. It has an explicit presentation route: `next-standard-period` by default, or `ad-hoc` when an operator explicitly selects it before either statement is issued. A uniqueness constraint ensures that each adjustment is presented at most once across standard and ad hoc statements.

The same cycle boundary applies to M4 outcome corrections. Before the target period end, retain the existing M4 outcome authority/window contract and append its existing correction record. After the target period end, preserve the same authority/window checks but calculate the exact economic delta against the already accepted outcome and record it as a post-close adjustment at the correction's acceptance time. Never revise an earlier statement or re-open M4 history. A late first outcome is a new accepted fact assigned to its acceptance-time period as already approved.

**Review point:** confirm that `next-standard-period` is the sensible default route for post-close adjustments and that explicit ad hoc presentation consumes the adjustment exactly once. Both paths preserve the exact signed delta; neither path reports payment status.

## 7. Cumulative activity conversion

Cumulative activity is a distinct agreement billing basis; it is not M4 per-work usage with per-event rounding. Store each accepted activity quantity as an exact nonnegative integer in its declared source unit, linked to the effective agreement version and customer period. The applicable agreement selects a versioned period conversion/rate rule. At close, aggregate the complete accepted activity quantities first, apply the conversion once using exact rational/integer arithmetic, and book one exact scale-18 USD amount for that period. Retain both the pre-conversion aggregate and converted calculation provenance. Do not round any work record or use floating point. Any rational result finer than USD scale 18 is rounded once using the existing explicit `nearest_ties_away` money rule; the unrounded rational and booked atoms remain visible.

The M5 contract must keep per-work unit billing and cumulative-period billing as distinct tagged agreement modes. A period cannot combine them under one rate basis. Cumulative activity quantity corrections follow §6 and change the open-period aggregate by signed deltas. The implementation review must prove total conservation across the aggregate and the close record before this model is enabled.

## 8. Recurrence lifecycle

An immutable recurrence version belongs to an agreement and has its own finite interval/anchor/timezone grammar and explicit effective start/end. A scheduled occurrence's stable identity is `(agreement_id, recurrence_version_id, scheduled_local_occurrence_label)`; retain the resolved UTC instant and boundary-rules version. Identities never depend on process start, retry count, a mutable “next” counter, or acceptance time.

An operator-triggered enumerate/run command accepts an explicit due-through boundary and bounded result limit. It lists stable due occurrence IDs without mutation; a separate explicit accept operation creates the recurring billable record. No background process creates or charges work. Missing occurrences are not billed; explicit operator catch-up is required. Repeating a successfully accepted occurrence returns its original result; changed payload under the same identity conflicts.

The occurrence's first successful ledger acceptance timestamp selects the applicable immutable M2 agreement version and price. Scheduled time is retained as evidence only. Cancellation is an immutable control accepted in the same serialized write lane; it blocks not-yet-accepted occurrences from its effective point and preserves prior accepted records and retries. Renewal is disabled unless the agreement explicitly opts in and supplies a new immutable term version. Proration defaults to `none`; no partial-period charge is synthesized.

## 9. Fiscal reporting

The organization fiscal calendar has immutable versions independent of customer term and agreement recurrence versions. A report command pins the fiscal-calendar version and a consistent database snapshot comprising both the M3 accepted-entry high-water mark and the M5 sidecar sequence. It returns an immutable report-run record with resolved fiscal boundaries, sorted source record IDs, and exact totals. A report may be recomputed only by explicitly selecting a retained calendar version and snapshot; the current calendar cannot silently replace either. Reports are internal projections and cannot change statements, period assignment, or amounts owed.

## 10. CLI, errors, and ownership

Use separate versioned JSON requests for customer terms, fiscal calendars, period close, quantity correction, post-close adjustment, recurrence, cancellation/renewal, and fiscal-report runs. The CLI remains the thin parser and renderer. The service validates ownership, authority, supported grammar, period resolution, correction bounds, and exact amounts. SQLite constraints enforce uniqueness/immutability and derived-index integrity. The CLI cannot compute an amount independently.

All write commands require explicit customer scope and, where the request originates from an application, explicit source authority. Administrative term/fiscal-calendar/close operations use operator authority, a stable command ID, optimistic expected revision, and retained audit evidence. Errors distinguish invalid request, unauthorized, out-of-window, closed-period, bounds, identity conflict, unsupported schema, and outcome-unknown; none imply successful acceptance. Existing M2/M4 JSON families remain unchanged; M5 command and output schemas are additive.

## 11. Package 3 exit and remaining gate

Package 3 can freeze after:

- Independent read-only review of this proposal and all downstream M5 contracts.
- Explicit resolution of the pre-M5 accepted-history/initial-term assignment gate in §3.
- Confirmation of the post-close adjustment presentation rule in §6.
- Frozen request/response schemas and exact status/error families for each write.
- Frozen schema-10→11 migration and old-writer refusal contract.
- A complete synthetic time/money oracle for anchor/calendar-aligned day/week/month/year boundaries, month end, both DST cases, accepted-time exact-end membership, one legacy-history example, signed deltas, late outcomes, standard/ad hoc exclusivity, recurring retries, and fiscal snapshot replay.
- Traceability from every owner-approved M5 decision to a persistence invariant and later implementation acceptance evidence.

No product implementation starts before the pre-M5 history rule and exact record contracts are frozen. The rest of M5 may then be implemented in isolated worktrees by the package owners in the [execution plan](m5-execution-plan.md), with Sol integrating shared contracts and schema work.
