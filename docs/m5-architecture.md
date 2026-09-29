# M5 architecture and contract proposal

**Status (2026-09-29): Architectural baseline passed independent Astra review; Package 3 contract drafting is next. The owner-approved defaults are frozen; product code has not changed.** The [billing roadmap](billing-roadmap.md) is canonical. The [M5 decision register](m5-decision-register.md) is the owner-approved policy record. This proposal translates those choices into a compatible implementation shape; it does not authorize reopening a frozen economic rule.

## 1. Scope and existing baseline

M5 extends the provider-free, one-business-per-installation, local SQLite billing profile. M4 currently accepts fixed-price and scale-18 unit-price work into an append-only billing history, retains each receipt's accepted time, and derives `ledger-billing-statement/2` or `/3` as a read projection. It does not persist billing-period membership or closed statements. The schema-10 `billing_m3_index` supplies a bounded accepted-time index; `billing_m3_entries` retains the exact accepted bundles. Agreement and permission changes are already effective-dated M2 controls.

The M5 implementation must preserve every M1–M4 accepted bundle, alias, retry identity, agreement version, outcome decision, exact amount, and statement output for the profiles already qualified. M5 adds an append-only billing lifecycle sidecar and versioned statement projections. It must not rewrite M4 records or extend the frozen `ledger-canonical-v1` first-slice family by analogy. Any required amendment to that record family is a separate reviewed contract change.

M5 adds these capabilities:

- Immutable customer billing-term versions and immutable organization fiscal-calendar versions.
- Deterministic period assignment by the accepted timestamp retained by the ledger.
- Explicit, idempotent period close with an immutable statement snapshot.
- A separate cumulative-activity model that aggregates within a period and performs configured conversion only at period close.
- Append-only quantity deltas before durable period close and linked post-close adjustments after it.
- Stable recurring-occurrence identities with operator-triggered generation, explicit catch-up, opt-in renewal, cancellation, and no implicit proration.
- Internal fiscal reports reproducible from a pinned calendar version and ledger snapshot.

It adds no payment provider, payment execution, payment verification, tax/legal invoice semantics, multi-business tenancy, or automatic expiry.

## 2. Governing invariants

1. **One accepted-time clock, with one explicit correction link.** Ordinary accepted M3 economic postings, recurring charges, new outcome facts, M5 post-close adjustments, and fiscal monetary effects use the retained ledger acceptance timestamp, represented as signed UTC epoch microseconds. Caller `occurred_at` and scheduled occurrence time remain evidence. A correction to an existing customer-period fact may link to that original logical period until durable close; its own acceptance time remains authoritative for fiscal reporting.
2. **Half-open periods.** Calendar periods are `[start_utc, end_utc)`. An ordinary acceptance at `end_utc` belongs only to the next period. A pre-close correction is a specific exception: it keeps its accepted timestamp but links to the original logical customer period while that period remains unclosed. Calendar calculation uses integer timestamps; local wall time defines boundaries.
3. **Immutable versions and close records.** Terms, recurrence rules, fiscal-calendar definitions, adjustment decisions, period close records, and issued ad hoc statements are append-only. A close pins its resolved UTC boundaries, term version, accepted ledger cutoff, included record IDs, exact totals, and canonical statement bytes. Re-running the same close identity returns the same statement; conflicting reuse refuses.
4. **No repricing.** A schedule or calendar change cannot alter the accepted agreement version or amount on a work item. A recurring occurrence uses the agreement version selected by its first successful acceptance, not the version current at its scheduled time.
5. **No closed-period rewrite.** An original period accepts a correction until its immutable close record is durably committed, even after the scheduled end. A correction that wins the serialized write lock before close is linked to the original period; if close wins first, the change becomes a post-close adjustment. New economic facts after the scheduled end still belong to their acceptance-time period. A close and a correction cannot both win for the same period.
6. **Exact economics.** Preserve M4 USD scale-18 integer atoms. Per-work quantity correction arithmetic is exact rate-atoms × signed integer delta. Cumulative-period changes use the difference between the old and new rounded booked bucket amounts, not an independently rounded delta. Aggregate totals use exact unbounded integer arithmetic with explicit overflow/size refusal; no cents or four-decimal rounding is introduced.
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

For an anchored term, derive boundary `n` from the immutable anchor and integer period index `n`; never derive it by repeatedly advancing the previous clamped boundary. For calendar-aligned terms, use the selected natural day/week/month/year boundary and the declared interval. Month/year calculation is calendar arithmetic. Apply the pinned month-end rule (`preserve-anchor-and-clamp` or `explicit-end-of-month`). Resolve a nonexistent local boundary to the first valid instant after the DST gap and an ambiguous local boundary to the earlier UTC instant. Freeze the IANA database release or equivalent timezone-rules data version with the term; do not depend on the host's mutable timezone database. Persist each boundary resolution and any append-only transition that supersedes an unclosed logical period's nominal end. A close references exactly one resolution record.

A regular term change becomes effective at the next boundary of the current term, even if the operator closes that prior period later. Term records are immutable and carry only their own effective instant; disjoint effective intervals are derived from ordered term-change history and are never written back into predecessor records. An explicit immediate change appends a boundary-transition record that clips the predecessor's current open logical period at the change's accepted time and starts a new logical period under the successor term. No M3 event or correction is reassigned to another logical period. A logical period ID is `(customer, term_version, period_index)` and does not contain resolved start/end timestamps; an immutable boundary-resolution record pins the actual UTC interval. A transition may append a successor resolution for an unclosed logical period, superseding its prior nominal end without changing the logical period ID or event assignments. A close targets that logical period ID and pins the selected boundary-resolution record. A close may target any retained historical logical period, even after a newer term becomes active. Resolve an existing close identity by logical period ID and return its saved result before consulting current-term state.

An explicit immediate change may produce a short period only when its billing/proration treatment is included in the accepted term change. The default `proration:none` means the engine does not synthesize a time-proportional amount; accepted recurring occurrences continue to use their agreed amount.

### Organization fiscal calendar

The fiscal calendar is a separately versioned `ledger-fiscal-calendar/1` object scoped to the one installation. It supports Gregorian month, quarter, and year calendars with configurable fiscal-year and week starts, plus bounded 4-4-5, 4-5-4, and 5-4-4 week patterns. A custom week pattern declares its period lengths, week start, 52/53-week rule, and destination period for the extra week. It is internal reporting metadata; it never selects customer periods or changes receivables/payables.

### Deterministic lookup

`period_for(customer, accepted_at_us, snapshot)` resolves exactly one immutable effective term version, derives the natural period index from that version's anchor and pinned timezone rules, applies any retained transition record, and returns a stable logical period ID `(customer, term_version, period_index)` plus the applicable immutable boundary-resolution ID. The resolution refuses if term versions overlap, leave a gap, or do not cover the accepted timestamp. No acceptance can be silently assigned to a default calendar.

**Initial activation for existing M4 history:** the operator must explicitly provide the customer's initial term anchor and effective instant. If the customer has retained billable M4 history, the effective instant must be no later than its oldest retained billable acceptance. M5 refuses activation or close if any retained billable entry lacks exactly one deterministic period assignment. The first term is therefore never inferred from the current date and old work is never silently omitted or moved into an arbitrary current period.

## 4. Storage and compatibility shape

Add an ordinary-schema transition from SQLite schema 10 to schema 11 using an explicit preflight and atomic upgrade path. The regular open/write path never performs a silent schema upgrade. The migration is additive: preserve schema-10 tables and bytes and create M5 lifecycle tables/indexes. A schema-10 writer must refuse a schema-11 installation before any mutation; schema-11 read/write behavior is qualified separately from native package support.

The proposed M5 sidecar contains:

| Record set | Purpose | Mutability |
|---|---|---|
| `billing_m5_records` | Shared append-only sequence of validated term changes, correction deltas, post-close adjustments, occurrence results, close records, ad hoc statements, fiscal-calendar changes, and report snapshots. | Insert only; stable request identity and canonical payload retained. |
| `billing_m5_identity` | Scoped command retry lookup and semantic identity. | Insert only; unique within the identity domain and stable semantic keys. |
| `billing_m5_period_index` | Derived lookup of accepted event IDs, adjustments, logical period IDs, boundary-resolution IDs, and close IDs. | Immutable rows; every field checked against its source record on open/reconciliation. |
| `billing_m5_bounds` | Monotone row/byte guard and storage limit. | Guarded monotone updates only; fail closed at the limit. |

The exact table split and column layout belong in the reviewed migration contract. SQL enforces immutability, unique keys, foreign keys, monotone bounds, and supported-size limits. The service/coordinator validates calendars, authority, money, period membership, correction bounds, and complete statement content; the store does not evaluate economics. Persist canonical JSON bytes plus a domain-separated content hash for each M5 record. Keep M5 records in their own versioned families (for example `ledger-billing-term/1`, `ledger-billing-period-close/1`, and `ledger-billing-adjustment/1`); do not add these variants to `contracts/schemas/v1/canonical-records.schema.json` without a separate amendment. Adjustment variants must cover at least usage quantity, cumulative-period quantity, and outcome decision deltas; one quantity-only structure cannot represent every post-close economic change.

Identity domains are explicit. Application-originated commands use `(customer, source, command_id)` and preserve M2 retry semantics. Customer-specific operator controls use the tagged identity `(customer-admin, customer, command_id)`. Organization-wide fiscal-calendar/report commands use `(installation-admin, command_id)` and do not invent a customer value. These administrator commands are authorized by the existing local installation boundary; M5 does not claim per-human authentication. Persist the identity-domain tag and all present scope fields in each record and uniqueness key so IDs cannot collide across unrelated command classes.

Every write runs in one `BEGIN IMMEDIATE` transaction and appends the M5 decision, scoped identity mapping, and derived index rows atomically. The writer lock serializes close against corrections and close against recurrence occurrence acceptance. Sample the accepted/recorded time after acquiring the writer transaction, then validate strict monotonicity against the maximum retained accepted or control timestamp, including M5 rows. Preserve unknown-commit behavior: lookup the same stable identity and return the original result, or return an explicit unknown outcome without retrying under a new identity.

M5 identity retention follows M2: no accepted command, occurrence key, adjustment, or statement identity expires. Backup/restore captures the whole installation, including M5 sidecars. The schema-10→11 activation gate must validate existing schema-10 history before enabling M5 writes; it must not re-price or re-encode any M4 entry.

## 5. Period close and standard statement

`billing close` accepts an explicit customer and logical period ID. It refuses before that period's resolved end and rejects invalid or ambiguous IDs. It resolves the retained term and latest valid pre-close boundary resolution for that historical logical period, not only the customer's current term. In the same serialized SQLite write transaction it reads accepted M3 entries and M5 period-allocation records, includes linked corrections accepted before the durable close even if their acceptance follows the scheduled period end, includes post-close adjustments allocated to this period, excludes adjustments already atomically presented by an ad hoc statement, computes exact period totals, and appends one immutable close record plus presentation claims for included adjustments. A missing outcome does not remove an initial charge. New outcomes and other first-time economic facts accepted after a period ends belong to the period selected by their later acceptance time. A late fact can never cause the previous period's close bytes to change.

The close stores:

- Stable close ID and command identity.
- Customer, logical period ID, billing-term version, selected boundary-resolution ID, local labels, and exact resolved UTC `[start,end)` boundaries.
- Close acceptance time and M3/M5 high-water snapshot needed for replay.
- Sorted M3 receipt IDs, M5 delta/adjustment IDs, and per-line exact calculation provenance.
- Exact scale-18 USD line amounts and net atoms, with positive/negative direction classification.
- Canonical output bytes and a domain-separated hash.

The client-facing statement family advances from `/3` to `ledger-billing-statement/4` for lifecycle statements. It contains one period's exact net only, no previous balance, all included source record IDs, accepted-time assignment evidence, active agreement versions, `net_atoms`, `direction: receivable | payable | none`, `complete:true`, and a statement hash. It preserves M4 field meanings where they still apply and identifies the selected period explicitly. Existing `/2` and `/3` outputs remain available for their qualified non-lifecycle profiles; never reinterpret them as closed period statements. CSV export advances independently to `/4` and preserves exact scale-18 atoms as text.

Close retries use `(customer, logical_period_id)` as the stable semantic identity. Resolve this identity before checking the current calendar revision or boundary-resolution head, so a newer active term cannot block a retry for a prior close. The first successful close pins the chosen boundary resolution and is immutable. A changed boundary or close content under the same logical-period key returns a conflict. If storage reports an unknown commit, the CLI retries the identical key and receives the original close or a bounded unknown-result error.

## 6. Usage corrections and post-close adjustments

### Pre-close quantity delta

Proposed request family: `ledger-billing-quantity-correction/1` with `customer`, `source`, stable `id`, original accepted usage target, signed integer `quantity_delta`, `occurred_at` evidence, and retained correction evidence. A write requires current customer/source `correct` authority and the original agreement's correction authorization. The unit, currency, scale, rate, and agreement version are taken from the immutable accepted record, never from the correction request.

Only an integer delta is accepted; replacement quantities are rejected. Until a durable close exists, the correction may be accepted even after the scheduled period end and is linked to the original period. Under the writer lock, compute `current_effective_quantity = original_quantity + all_prior_accepted_deltas`. Reject overflow, a negative result, a result above the original agreement's configured maximum, a correction disallowed by the resolved open-period rule, or an exact amount beyond the existing Money bound. The unit, currency, scale, rate, and agreement version are taken from the immutable accepted record, never from the correction request. For M4 per-work billing, the signed amount is exactly `original_rate_atoms * quantity_delta` at scale 18. Append one immutable correction record; never update the base event or prior deltas. Exact retry returns its original receipt; conflicting identity reuse refuses.

### Post-close adjustment

Once the original period is closed under the owner-approved cutoff rule, append a separate `ledger-billing-adjustment/1` record linked to the original economic decision and retaining correction evidence, source decision ID/revision, prior and revised economic facts, signed exact USD delta, and accepted time. Its tagged variant records the original basis (per-work usage rate/quantity, cumulative-period bucket, or outcome revision) without pretending every post-close change is a quantity delta. The adjustment's own ledger acceptance time determines its later-period key; the old period and any close remain unchanged. It is eligible for the `next-standard-period` by default. An operator may request an ad hoc statement; there is no persistent route-selection state that removes the adjustment from standard close. A uniqueness constraint ensures that each adjustment is presented at most once across standard and ad hoc statements.

The same close cutoff applies to M4 outcome corrections. Retain M4's existing correction authority, evidence, claim revision, and admissibility checks. M4 history remains the authoritative accepted decision sequence: append its existing M3 correction entry and the linked M5 period-allocation record in the same transaction under the same customer/source command identity. The M5 row is a derived, hash-checked projection linked uniquely to the M3 correction receipt; it is never an independent second charge. Compute its signed amount as `revised_target_net - prior_target_net`, including fixed-price outcome replacement; do not assume every adjustment is a usage quantity. For customer statement allocation, a correction accepted while the target's logical period is not durably closed links to that original period, including after scheduled end; if it is already closed, allocate its signed delta to the period containing the correction's acceptance time. Its fiscal monetary effect remains the signed M3 posting at its own accepted time. A late first outcome is a new fact, not a correction; it is assigned to the period containing its acceptance time even when the target's earlier period is not yet closed. A repeated correction replays the M3 receipt and its one linked M5 row; an M3/M5 mismatch fails store integrity. Never revise an earlier close or reopen M4 history.

The owner-approved default route is `next-standard-period`. An operator may instead issue the adjustment through one ad hoc receivable/payable statement. Selecting or preparing an ad hoc route does not consume the adjustment or exclude it from standard close. Only the transaction that issues the immutable ad hoc statement creates the unique presentation claim. Standard close includes each otherwise eligible adjustment without an existing ad hoc presentation claim and creates its presentation claim in the same transaction. Ad hoc issue and standard close serialize on the writer lock; whichever commits first prevents the other path from presenting that adjustment. Both paths preserve the exact signed delta; neither path reports payment status.

## 7. Cumulative activity conversion

Cumulative activity is a distinct agreement billing basis; it is not M4 per-work usage with per-event rounding. Store each accepted activity quantity as an exact nonnegative integer in its declared source unit, linked to the effective agreement version and customer logical period. A conversion bucket is keyed by customer, logical period ID, agreement ID and version, billing mode, source unit, conversion/rate rule version, currency, and output scale; separate buckets never merge implicitly. Define `E(q)` as the exact rational scale-18 atom amount produced by the pinned conversion/rate rule for aggregate quantity `q`. Define `B(q) = nearest_ties_away(E(q))` as the booked integer scale-18 atoms. At close, aggregate the complete bucket first and book `B(q)` once. Retain `q`, the exact numerator/denominator for `E(q)`, the rule version, and `B(q)`. Never round a work record or use floating point.

The M5 contract must keep per-work unit billing and cumulative-period billing as distinct tagged agreement modes. A period cannot combine them under one rate basis. Cumulative activity quantity corrections follow §6 and change the open-period aggregate by signed deltas, without creating a separate monetary posting at correction acceptance. For a post-close quantity correction, compute the adjustment as `B(q_new) - B(q_old)` under the original bucket's pinned rule; do not round that booked difference again. Successive corrections use the immediately prior corrected aggregate and booked total, so their signed adjustments telescope exactly to the difference between the original and final booked totals. The implementation review must prove conservation across each bucket, close record, and linked adjustments before this model is enabled.

## 8. Recurrence lifecycle

An immutable recurrence version belongs to an agreement and has its own finite interval/anchor/timezone grammar and explicit effective start/end. A scheduled occurrence's stable identity is `(agreement_id, recurrence_version_id, scheduled_local_occurrence_label)`; retain the resolved UTC instant, boundary-rules version, and pinned IANA timezone-rules release. Identities never depend on process start, retry count, a mutable “next” counter, or acceptance time.

An operator-triggered enumerate/run command accepts an explicit due-through boundary and bounded result limit. It lists stable due occurrence IDs without mutation; a separate explicit accept operation creates the recurring billable record. No background process creates or charges work. Missing occurrences are not billed; explicit operator catch-up is required. Repeating a successfully accepted occurrence returns its original result; changed payload under the same identity conflicts.

The occurrence's first successful ledger acceptance timestamp selects the applicable immutable M2 agreement version and price. Scheduled time is retained as evidence only. Cancellation is an immutable control accepted in the same serialized write lane; it blocks not-yet-accepted occurrences from its effective point and preserves prior accepted records and retries. Renewal is disabled unless the agreement explicitly opts in and supplies a new immutable term version. Proration defaults to `none`; no partial-period charge is synthesized.

## 9. Fiscal reporting

The organization fiscal calendar has immutable versions independent of customer term and agreement recurrence versions. It pins the same timezone-rules release used to resolve its local boundaries. A report command pins the fiscal-calendar version and a consistent database snapshot comprising both the M3 accepted-entry high-water mark and the M5 sidecar sequence. It returns an immutable report-run record with resolved fiscal boundaries, sorted source record IDs, exact line effects, and totals. Its monetary set is defined as follows:

| Source record | Fiscal monetary effect | Fiscal timestamp |
|---|---|---|
| M3 accepted posting, including an outcome correction's signed posting | Include the exact retained signed posting once. | Its M3 acceptance timestamp. |
| M5 per-work quantity correction | Include its exact signed rate-times-delta amount once. | Its M5 acceptance timestamp. |
| M5 cumulative activity or pre-close cumulative quantity correction | No monetary effect at acceptance. Optional quantities are a separate nonmonetary measure. | Its M5 acceptance timestamp for quantity reporting only. |
| M5 cumulative close | Include the bucket's booked `B(q)` amount once. | The close record's M5 acceptance timestamp. |
| M5 post-close cumulative correction | Include `B(q_new)-B(q_old)` once, with no additional rounding. | Its M5 acceptance timestamp. |
| M5 post-close per-work adjustment | Include the exact signed amount once. | Its M5 acceptance timestamp. |
| M5 sidecar projection of an M3 outcome correction; standard/ad hoc statement presentation | No additional monetary effect. | Reference only; never summed as a second posting. |

Raw cumulative activity quantities may appear in a separate nonmonetary section and are never added again as money. A report may be recomputed only by explicitly selecting a retained calendar version and snapshot; the current calendar cannot silently replace either. Reports are internal projections and cannot change statements, period assignment, or amounts owed.

## 10. CLI, errors, and ownership

Use separate versioned JSON requests for customer terms, fiscal calendars, period close, quantity correction, post-close adjustment, recurrence, cancellation/renewal, and fiscal-report runs. The CLI remains the thin parser and renderer. The service validates ownership, authority, supported grammar, period resolution, correction bounds, and exact amounts. SQLite constraints enforce uniqueness/immutability and derived-index integrity. The CLI cannot compute an amount independently.

Application-originated writes require explicit customer scope and source authority. Customer-specific administrative writes use the customer-administrator identity domain; organization-wide fiscal-calendar and report writes use the installation-administrator domain and carry no customer or application source. Local installation access authorizes these administrator commands. All administrative writes use a stable command ID, optimistic expected revision where the target is versioned, and retained audit evidence. Errors distinguish invalid request, unauthorized, out-of-window, closed-period, bounds, identity conflict, unsupported schema, and outcome-unknown; none imply successful acceptance. Existing M2/M4 JSON families remain unchanged; M5 command and output schemas are additive.

## 11. Package 3 exit and remaining gate

Package 3 can freeze after:

- Independent read-only review of this proposal and all downstream M5 contracts.
- Explicit initial-term activation that covers all retained billable M4 history, with no unassigned record eligible for close.
- The approved durable-close correction cutoff and mutually exclusive next-standard/ad-hoc adjustment presentation in §6.
- Exact cumulative buckets, pinned conversion rules, one `nearest_ties_away` scale-18 rounding at close, and telescoping post-close quantity adjustments in §7.
- Pinned timezone-rule releases across customer terms, recurrence rules, and fiscal calendars.
- Separate customer-administrator and installation-administrator identity domains, without fabricating a customer for organization-level work.
- Frozen request/response schemas and exact status/error families for each write.
- Frozen schema-10→11 migration and old-writer refusal contract.
- A complete synthetic time/money oracle for anchor/calendar-aligned day/week/month/year boundaries, month end, both DST cases, accepted-time exact-end membership, legacy-history coverage, correction-versus-close orderings, exact cumulative rounding and successive corrections, late outcomes, standard/ad hoc exclusivity, term transitions, recurring retries, and fiscal snapshot replay.
- Traceability from every owner-approved M5 decision to a persistence invariant and later implementation acceptance evidence.

No dependent product implementation starts before the exact record contracts are frozen and pass independent review. The owner decisions identified during architecture review are now resolved. Once Package 3 closes, the rest of M5 may be implemented in isolated worktrees by the package owners in the [execution plan](m5-execution-plan.md), with Sol integrating shared contracts and schema work.
