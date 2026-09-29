# M5 decision register

**Status (2026-09-29): proposal for owner review.** The [billing roadmap](billing-roadmap.md) is canonical. This register expands its approved M5 direction into technical defaults and explicit policy gates; proposals below are not owner decisions. No M5 product behavior is implemented or authorized by this document.

## 1. Owner-approved constraints

These decisions are settled and must not be reopened as implementation convenience:

- M5 targets the provider-free local SQLite profile, with one business per installation. It does not add a payment provider, execute payments, or claim tax/legal invoice status.
- A customer selects a billing schedule from a centrally governed set of supported terms. The business configures a separate, customizable organization-level fiscal reporting calendar for internal use. Neither calendar controls the other.
- Ledger acceptance time assigns records to both calendar projections. Caller-reported occurrence time remains retained evidence and does not backdate the projections.
- Preserve M2's effective-dated immutable agreement versions, first-successful-acceptance pricing rule, customer-scoped identity and retry behavior, and accepted history.
- Preserve M4's exact USD scale-18 usage amounts and existing authority and outcome-window rules. Do not round individual work or quantity corrections to four decimals. Cumulative activity is a distinct period model and is converted at period end.
- A missing outcome does not cancel an initial charge. Include that charge when the billing cycle closes. An outcome accepted after close is assigned by its acceptance time to a later cycle, subject to the existing authority and admissibility window.
- Before close, a usage quantity correction is an append-only signed delta linked to the accepted usage. After close, represent the change as a linked adjustment in a later period. A manual ad hoc receivable/payable statement may present the adjustment. A closed standard statement is immutable.
- No automatic expiry of accepted records or retry identities is introduced. Existing M2 behavior remains.

## 2. Technical defaults proposed for owner approval

### Customer billing terms

Use a finite, declarative calendar grammar rather than arbitrary cron expressions:

```text
BillingTerm = {
  interval: positive integer within central limits,
  unit: day | week | month | year,
  alignment: anchored | calendar-aligned,
  anchor: local date and optional local time,
  timezone: IANA timezone identifier,
  month_end_rule: preserve-anchor-and-clamp | explicit-end-of-month,
  boundary_rule_version: immutable version
}
```

Central policy owns the supported units, numeric bounds, timezone database behavior, and any required minimum/maximum period duration. A customer can select any valid term inside that grammar. Month and year arithmetic is calendar arithmetic, never a fixed number of seconds. Derive every boundary from the immutable anchor so a short month does not permanently shift later periods. A period is the half-open UTC interval `[start, end)`; an acceptance exactly at `end` belongs to the next period. Store the normalized local rule and resolved UTC boundaries used for close and replay.

**Proposed DST rule:** resolve a nonexistent local boundary to the first valid instant after the gap; for an ambiguous local time, choose the earlier UTC instant. A term change is effective at the next open period boundary by default. An explicit immediate effective time may start a short period only if the agreement separately specifies the resulting charge/proration treatment. Never reassign accepted items from a closed period.

### Organization fiscal calendar

Keep a separately versioned organization-level definition. Proposed supported forms are Gregorian month/quarter/year calendars with configurable fiscal-year start and week start, plus bounded explicit period patterns for month-style or 4-4-5/4-5-4/5-4-4 reporting. A custom week pattern must declare its week start, period lengths, 52/53-week rule, and where an extra week goes. The same validated date engine can calculate both customer billing periods and fiscal periods, but definitions, versions, and outputs remain separate.

Every report run pins the fiscal-calendar version and a complete accepted-ledger snapshot. Changing the current calendar cannot mutate an issued run or a closed customer statement. Historical re-runs use the explicitly selected retained version and snapshot; no report silently adopts a newer calendar definition.

### Agreement recurrence and stable occurrences

Represent recurrence independently of the customer's billing term and the organization's fiscal calendar. Reuse the bounded unit/interval/anchor/timezone grammar where practical, but retain an immutable recurrence-rule version and explicit effective start/end.

Proposed occurrence identity is `(agreement_id, recurrence_version_id, scheduled_local_occurrence_label)`. Retain the corresponding resolved UTC instant. The identity must not depend on process start time, retry count, or a mutable “next run” counter. Repeating an identical occurrence request returns its original result; changed content under that identity refuses without mutation.

Proposed default: no background process autonomously creates or charges occurrences. An operator-triggered operation enumerates due occurrences deterministically. Missed occurrences are not billed automatically; catch-up requires an explicit operator action and each accepted occurrence is assigned to a customer billing period by its ledger acceptance time. Whether a catch-up is priced under its scheduled-time or acceptance-time agreement version remains a policy gate; M2's acceptance-time rule must not be silently bypassed.

Renewal creates a new immutable agreement/recurrence version. Automatic renewal is opt-in and must have explicit renewal terms. Cancellation is effective at a recorded instant and blocks later occurrences while retaining prior history and retries. No partial-term proration is inferred. The owner must choose the cancellation effective-time rule, missed-occurrence behavior, renewal semantics, and any proration formula before recurrence code is authorized.

### Usage quantity corrections and post-close adjustments

Store each correction as a signed integer quantity delta linked to the original accepted usage and the original agreement/rate basis. Never overwrite or replace the accepted quantity. For a pre-close correction, require the original customer's billing period to remain open; the delta contributes to that period's effective quantity. If that period is closed, use the post-close adjustment path instead.

Proposed invariant: the cumulative corrected quantity remains within the original agreement's configured bounds, including a nonnegative lower bound; rate, unit, customer, and applicable agreement version remain those of the original accepted usage. Reject a delta that would violate those bounds. Each correction has a stable scoped identity and exact retry behavior. A post-close adjustment retains a link to the original usage, correction evidence, and later period; an ad hoc statement names the adjustment IDs it presents. Neither path alters the closed standard statement.

The correction permission/source, evidence requirement, numeric bounds, over-correction refusal, and whether an over-maximum positive correction may ever be specially authorized require owner approval before implementation.

### Outcomes, statement precision, and payment boundary

Keep the current M4 outcome authority and admissibility window unless the owner explicitly extends it. An accepted late outcome cannot reopen the original closed statement. Its economic consequence must be represented in a later open period or a linked adjustment, according to the frozen outcome contract.

Proposed statement baseline: preserve USD scale-18 integer atoms through close, statement and export, with no unapproved close-time cent rounding. If a later regulatory or presentation rule requires rounding, make it an explicit versioned statement-level policy that retains the exact subtotal and records any rounding delta. Currency-scale rules, rounding method, aggregation order, negative/payable presentation, and balance carry-forward need an owner decision before they can affect a payable.

Keep M5 provider-free. The recommended scope is to omit manual payment-status tracking unless the owner explicitly wants it; status labels must not imply provider verification, execution, or settlement. If included, it needs separate authority, transition, partial-payment-allocation, and audit rules.

## 3. Owner decision gates

The recommendations above are proposals. Before dependent implementation, explicitly resolve each applicable gate:

| Gate | Decision required | Proposed default |
| --- | --- | --- |
| M5-G1 | Billing-term grammar bounds; timezone/DST and month-end conventions | Finite interval grammar above; resolve gap forward, earlier instant in overlap; preserve anchor and clamp short months |
| M5-G2 | Mid-period customer schedule change and whether it can create a short/prorated cycle | Next open boundary by default; immediate changes only with explicit effective time and proration terms |
| M5-G3 | Fiscal-calendar forms, 52/53-week handling, and historical report regeneration | Versioned Gregorian and declared 4-4-5 family; runs pin calendar version plus accepted snapshot |
| M5-G4 | Missed occurrences, catch-up, renewal/auto-renew, cancellation effective time, and proration | No autonomous catch-up; explicit opt-in renewal; no implicit proration |
| M5-G5 | Recurrence price basis when acceptance is delayed across an agreement change | Preserve M2's first-acceptance rule unless a separate immutable recurrence-term rule is explicitly approved |
| M5-G6 | Correction authority/evidence, signed-delta bounds, over-correction, and post-close statement linkage | Existing explicit correction authority; preserve original rate basis; reject outside configured quantity bounds; immutable adjustment and statement links |
| M5-G7 | Late-outcome admissibility and economic treatment after close | Existing M4 window; acceptance-time assignment to later period; never reopen the closed period |
| M5-G8 | Close-time statement rounding and presentation | Preserve scale-18 atoms without additional rounding until explicitly approved |
| M5-G9 | Manual external-payment status in M5 | Omit; no provider/payment execution semantics |
| M5-G10 | Retry/record retention and identity expiry | Preserve M2 no-automatic-expiry behavior |

Stop and return to the owner if any implementation decision changes who owes money, the amount owed, acceptance or authority, period assignment, record/identity retention, or the payment boundary. A proposal in this register is not approval to make that change.

## 4. Package acceptance and stop conditions

| Package | Exit evidence | Stop condition |
| --- | --- | --- |
| 0 — Decision freeze | Owner decision register, supported-term proposal, package criteria, and explicit open gates | Any policy proposal is described as owner-approved without a recorded owner decision |
| 1 — Published-source seam report | Bounded advisory against the exact published v0.7.1 source; no private repository or roadmap input | Fresh exact-route check is missing, Agency denies the route, a safety marker appears, or private input is needed |
| 2 — Synthetic calendar oracle | Generic synthetic boundary, DST, month-end, recurrence, quantity-delta and adjustment cases; no Bean Counter private inputs | Model route is not freshly authorized, any case requires real customer/private data, or expected results imply an unapproved money rule |
| 3 — Architecture and contracts | Frozen calendar, recurrence, event-assignment, correction, close and statement contracts; independent review | Any amount, authority, assignment or statement gate remains unresolved, or compatibility/canonical-record impact is unclear |
| 4 — Customer cycles and cumulative conversion | Different customer schedules, deterministic half-open assignment, idempotent close/retry, exact M4 precision, reconciled totals | Duplicate close, changed accepted history, unresolved rounding economics, or any schedule boundary is nondeterministic |
| 5 — Corrections and late facts | Authorized signed deltas, pre/post-close split, linked adjustment/ad hoc statement, accepted late-outcome rules, unchanged closed statements | Correction bounds/rights, late-outcome treatment, or statement linkage is unresolved |
| 6 — Recurrence lifecycle | Stable occurrence identity across retry/restart, immutable versions, renewal/cancel/missed-run/proration rules | Any charge-on-miss, renewal, cancellation, price-basis, or proration rule is unresolved |
| 7 — Fiscal reporting | Independent organization-level projection pinned to calendar version and ledger snapshot; reconciles without changing client statements | Report behavior implies tax/legal claims or affects customer amounts |
| 8 — Integrated qualification/release | Synthetic end-to-end lifecycle, required recovery evidence, independent read-only review, exact candidate and normal release evidence | Any acceptance journey, reconciliation, recovery, review, or release gate fails |

Packages 1 and 2 remain advisory and conditional on the exact Agency-controlled public/synthetic route in the execution plan. Their output cannot approve a policy gate or edit product files.

