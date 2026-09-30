# M5 decision register

**Status (2026-09-30): Owner defaults, the independently reviewed architecture and frozen Package 3 contracts are implemented in the v0.8.0 source-built local SQLite profile. The [M5 qualification](m5-qualification.md) records the release boundary and evidence.** The [billing roadmap](billing-roadmap.md) is canonical. This register remains the approved M5 direction and defaults the implementation preserves.

## 1. Owner-approved constraints

These decisions are settled and must not be reopened as implementation convenience:

- M5 targets the provider-free local SQLite profile, with one business per installation. It does not add a payment provider, execute payments, or claim tax/legal invoice status.
- A customer selects a billing schedule from a centrally governed set of supported terms. The business configures a separate, customizable organization-level fiscal reporting calendar for internal use. Neither calendar controls the other.
- Ledger acceptance time is the normal assignment clock for both calendar projections. A pre-close correction may be linked to its original logical billing period until that period is durably closed; its own acceptance time still determines its fiscal period. Caller-reported occurrence time remains retained evidence and does not backdate either projection.
- Preserve M2's effective-dated immutable agreement versions, first-successful-acceptance pricing rule, customer-scoped identity and retry behavior, and accepted history.
- Preserve M4's exact USD scale-18 usage amounts and existing authority and outcome-window rules. Do not round individual work or quantity corrections to four decimals. Cumulative activity is a distinct period model and is converted at period end.
- A missing outcome does not cancel an initial charge. Include that charge when the billing cycle closes. A first outcome accepted after the original billing period ends is assigned by its acceptance time to a later cycle, subject to the existing authority and admissibility window. A correction to an already accepted outcome is linked to the original billing period only while it remains unclosed; after durable close, its signed delta goes to a later period.
- Before close, a usage quantity correction is an append-only signed delta linked to the accepted usage. After close, represent the change as a linked adjustment in a later period. A manual ad hoc receivable/payable statement may present the adjustment. A closed standard statement is immutable.
- No automatic expiry of accepted records or retry identities is introduced. Existing M2 behavior remains.

## 2. Owner-approved technical defaults

Stephen approved these defaults on 2026-09-29. A later change to these semantics requires a new owner decision where it changes economics, authority, period assignment, retention, or statement treatment.

### Customer billing terms

Use a finite, declarative calendar grammar rather than arbitrary cron expressions:

```text
BillingTerm = {
  interval: positive integer representable by the wire and storage types,
  unit: day | week | month | year,
  alignment: anchored | calendar-aligned,
  anchor: local date and optional local time,
  timezone: IANA timezone identifier,
  month_end_rule: preserve-anchor-and-clamp | explicit-end-of-month,
  boundary_rule_version: immutable version
}
```

Central policy owns the supported units and any product-wide minimum/maximum period duration. The default interval bound is the representable positive integer range; reject any boundary calculation outside supported Gregorian timestamps. A customer can select any valid term inside that grammar. Month and year arithmetic is calendar arithmetic, never a fixed number of seconds. Derive every boundary from the immutable anchor so a short month does not permanently shift later periods. A period is the half-open UTC interval `[start, end)`; an acceptance exactly at `end` belongs to the next period. Store the normalized local rule and resolved UTC boundaries used for close and replay.

Resolve a nonexistent local boundary to the first valid instant after the gap; for an ambiguous local time, choose the earlier UTC instant. A term change is effective at the next open period boundary by default. An explicit immediate effective time may start a short period only if the agreement separately specifies the resulting charge/proration treatment. Never reassign accepted items from a closed period.

### Organization fiscal calendar

Keep a separately versioned organization-level definition. Approved supported forms are Gregorian month/quarter/year calendars with configurable fiscal-year start and week start, plus bounded explicit period patterns for month-style or 4-4-5/4-5-4/5-4-4 reporting. A custom week pattern must declare its week start, period lengths, 52/53-week rule, and where an extra week goes. The same validated date engine can calculate both customer billing periods and fiscal periods, but definitions, versions, and outputs remain separate.

Owner-approved default (2026-09-29): a February 29 week-pattern year-end anchor resolves to February 28 in non-leap years before weekday alignment, following the existing Gregorian clamp rule. This fixes the behavior of an already supported configuration; it does not add a calendar form.

Every report run pins the fiscal-calendar version and a complete accepted-ledger snapshot. Changing the current calendar cannot mutate an issued run or a closed customer statement. Historical re-runs use the explicitly selected retained version and snapshot; no report silently adopts a newer calendar definition.

### Agreement recurrence and stable occurrences

Represent recurrence independently of the customer's billing term and the organization's fiscal calendar. Reuse the bounded unit/interval/anchor/timezone grammar where practical, but retain an immutable recurrence-rule version and explicit effective start/end.

Occurrence identity is `(agreement_id, recurrence_version_id, scheduled_local_occurrence_label)`. Retain the corresponding resolved UTC instant. The identity must not depend on process start time, retry count, or a mutable “next run” counter. Repeating an identical occurrence request returns its original result; changed content under that identity refuses without mutation.

No background process autonomously creates or charges occurrences. An operator-triggered operation enumerates due occurrences deterministically. Missed occurrences are not billed automatically; catch-up requires an explicit operator action. Each accepted occurrence is assigned to a customer billing period by its ledger acceptance time, and M2's first-successful-acceptance rule selects its agreement version and price; scheduled time never backdates pricing.

Renewal creates a new immutable agreement/recurrence version. Automatic renewal is opt-in and must have explicit renewal terms. Cancellation takes effect at its first successful ledger acceptance time and blocks later, not-yet-accepted occurrences while retaining prior history and retries. No partial-term proration is inferred; the default proration policy is `none`. A non-`none` proration method requires a separately versioned, exact formula before it can be accepted.

### Usage quantity corrections and post-close adjustments

Store each correction as a signed integer quantity delta linked to the original accepted usage and the original agreement/rate basis. Never overwrite or replace the accepted quantity. The original customer's billing period remains correction-eligible until its immutable close record is durably committed. A correction accepted after scheduled period end but before close is linked to the original period and contributes to its effective quantity. The writer lock serializes close and correction: whichever commits first determines whether the correction enters that statement or becomes a post-close adjustment.

Approved invariant: the cumulative corrected quantity remains within the original agreement's configured bounds, including a nonnegative lower bound; rate, unit, customer, and applicable agreement version remain those of the original accepted usage. Reject a delta that would violate those bounds. Each correction has a stable scoped identity and exact retry behavior. A post-close adjustment retains a link to the original usage, correction evidence, and later period; an ad hoc statement names the adjustment IDs it presents. Neither path alters the closed standard statement.

Require the existing explicit correction permission/source and retain the evidence required by the original agreement. Refuse corrections outside the original agreement's quantity bounds; there is no special over-maximum override in this profile.

### Outcomes, statement precision, and payment boundary

Keep the current M4 outcome authority and admissibility window unless the owner explicitly extends it. An accepted late outcome cannot reopen the original closed statement. Its economic consequence must be represented in a later open period or a linked adjustment, according to the frozen outcome contract.

Preserve USD scale-18 integer atoms through close, statement and export, with no unapproved close-time cent rounding. If a later regulatory or presentation rule requires rounding, make it an explicit versioned statement-level policy that retains the exact subtotal and records any rounding delta. Standard period statements show only that period's exact net; they do not carry forward prior unpaid balances. A negative exact net is represented as payable. Any future balance-forward presentation requires an explicit owner decision and a distinct statement contract.

Keep M5 provider-free. Owner-approved M5 scope omits manual payment-status tracking; no status label may imply provider verification, execution, or settlement. Adding payment-status tracking later would require separate authority, transition, partial-payment-allocation, and audit rules.

## 3. Owner decision gates

The following defaults were approved on 2026-09-29 and are the M5 policy record:

| Gate | Owner-approved default |
| --- | --- | --- |
| M5-G1 | Finite interval grammar in §2; positive representable integer interval; IANA timezone; month/year calendar arithmetic; half-open UTC periods; anchor-preserving month-end clamp; DST gap moves to the first valid instant and overlap chooses the earlier instant. |
| M5-G2 | Schedule changes start at the next open billing boundary by default. An explicit immediate effective time may start a short period only when its charge/proration treatment is stated. Closed periods are never reassigned. |
| M5-G3 | Support versioned Gregorian month/quarter/year calendars and bounded declared 4-4-5/4-5-4/5-4-4 week patterns with explicit 52/53-week rules. Report runs pin the calendar version and accepted-ledger snapshot. |
| M5-G4 | No background charge creation, no automatic missed-run catch-up, explicit operator catch-up, opt-in renewal, cancellation at first successful acceptance time, and no implicit proration (`none` by default). |
| M5-G5 | The occurrence's first successful ledger acceptance time selects its agreement version and price, preserving M2. Scheduled time does not backdate pricing or period assignment. |
| M5-G6 | Corrections use explicit existing correction authority/evidence, signed append-only deltas, the original unit/rate basis, and the original agreement's quantity bounds. Reject a result below zero or above the configured maximum. Correction eligibility lasts until durable close, including after scheduled period end. Post-close adjustments link to original economics; each is presented exactly once, by default on the next standard statement or through an explicitly issued ad hoc statement first. |
| M5-G7 | Preserve M4 outcome authority and admissibility windows. A first outcome accepted after scheduled period end belongs to its acceptance-time period. A correction to an accepted outcome is linked to the original logical period only until durable close; after close, its signed delta is assigned to its acceptance-time period and never reopens the statement. |
| M5-G8 | Preserve exact USD scale-18 atoms through period close and statement/export. Per-work amounts are never rounded. Cumulative-period conversion aggregates first and rounds a non-representable exact result once at close to scale 18 using `nearest_ties_away`; preserve the exact rational and booked atoms. Standard statements show only their own period's exact net, carry no prior balance, and classify a negative net as payable. |
| M5-G9 | Omit manual external-payment status from M5; no provider, payment-execution, verification, or settlement semantics. |
| M5-G10 | Preserve M2's no-automatic-expiry behavior for accepted records and retry identities. |

Stop and return to the owner if an implementation detail requires changing an approved default or otherwise changes who owes money, the amount owed, acceptance or authority, period assignment, record/identity retention, or the payment boundary.

### Closed owner gate: statement balance presentation

Approved 2026-09-29: each immutable standard statement presents only its current period's exact net, with no carry-forward of prior unpaid balances. A negative exact net is represented as payable. This is a statement classification rule; it does not create payment, settlement, or cross-period balance-allocation behavior.

### Additional defaults approved 2026-09-29

The owner approved these remaining architecture defaults:

| Decision | Approved rule |
|---|---|
| Existing M4 history | The operator explicitly sets the initial customer term anchor and effective instant. Where retained billable M4 history exists, that instant must be no later than the oldest retained billable acceptance. Activation and close refuse if any retained billable entry lacks exactly one deterministic period assignment; M5 never silently omits or moves older work. |
| Correction cutoff | A period remains correction-eligible until its immutable close record is durably committed, even after scheduled end. The serialized writer lock makes correction-versus-close ordering decisive. A correction that commits first is linked into the original period; a close that commits first sends the change to the post-close adjustment path. |
| Cumulative rounding | Let `E(q)` be the exact rational scale-18 atom result and `B(q)=nearest_ties_away(E(q))` be the booked atoms. Aggregate a conversion bucket exactly and apply `B` once at close, retaining the exact rational and booked atoms. Post-close cumulative corrections use `B(q_new)-B(q_old)` under the original pinned rule, with no second rounding. |
| Adjustment presentation | Default to the next standard period. An operator may issue one ad hoc receivable/payable statement before standard inclusion. A uniqueness constraint ensures each adjustment is presented once. |

These rules close the owner-decision gates identified by the architecture review. Package 3 is frozen after contract review and explicit acceptance; dependent implementation is underway.

## 4. Package acceptance and stop conditions

| Package | Exit evidence | Stop condition |
| --- | --- | --- |
| 0 — Decision freeze | Owner decision register, supported-term proposal, package criteria, approved decisions, and any remaining gates | Any policy proposal is described as owner-approved without a recorded owner decision |
| 1 — Published-source seam report | Bounded advisory against the exact published v0.7.1 source; no private repository or roadmap input | Fresh exact-route check is missing, Agency denies the route, a safety marker appears, or private input is needed |
| 2 — Synthetic calendar oracle | Generic synthetic boundary, DST, month-end, recurrence, quantity-delta and adjustment cases; no Bean Counter private inputs | Model route is not freshly authorized, any case requires real customer/private data, or expected results imply an unapproved money rule |
| 3 — Architecture and contracts | Frozen calendar, recurrence, event-assignment, correction, close and statement contracts; independent review | Any amount, authority, assignment or statement rule conflicts with an approved decision, or compatibility/canonical-record impact is unclear |
| 4 — Customer cycles and cumulative conversion | Different customer schedules, deterministic half-open assignment, idempotent close/retry, exact M4 precision, one approved close-time scale-18 rounding per exact cumulative bucket, reconciled totals | Duplicate close, changed accepted history, nondeterministic schedule boundary, incorrect telescoping cumulative adjustments, or silent legacy-history omission |
| 5 — Corrections and late facts | Authorized signed deltas, pre/post-close split, linked adjustment/ad hoc statement, accepted late-outcome rules, unchanged closed statements | Correction bounds/rights, late-outcome treatment, or statement linkage is unresolved |
| 6 — Recurrence lifecycle | Stable occurrence identity across retry/restart, immutable versions, renewal/cancel/missed-run/proration rules | Any charge-on-miss, renewal, cancellation, price-basis, or proration rule is unresolved |
| 7 — Fiscal reporting | Independent organization-level projection pinned to calendar version and ledger snapshot; reconciles without changing client statements | Report behavior implies tax/legal claims or affects customer amounts |
| 8 — Integrated qualification/release | Synthetic end-to-end lifecycle, required recovery evidence, independent read-only review, exact candidate and normal release evidence | Any acceptance journey, reconciliation, recovery, review, or release gate fails |

Packages 1 and 2 remain advisory and conditional on the exact Agency-controlled public/synthetic route in the execution plan. Their output cannot approve a policy gate or edit product files. No remote model call is required to implement the owner-approved M5 defaults.
