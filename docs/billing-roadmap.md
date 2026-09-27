# Billing product roadmap

**Status (2026-09-27): M0, M1, M2, M3, and M4 are complete for their documented source-built local profiles; M4 release publication is pending.** The accepted scope and M2 rules below are the product decision record for the provider-free OSS 1.0 billing journey. This roadmap is specific to the ordinary local billing profile; the historical product roadmap at the repository root retains its separate scope and status.

## Product goal and boundary

Bean Counter is a self-hosted billing engine for one business per installation, with multiple customers in the 1.0 target. Applications use the language-neutral CLI/JSON interface. Operators provide and administer the host, storage and backups. The product records agreed charges and retained evidence; it does not determine whether work was valuable or establish customer consent on its own.

OSS 1.0 targets a practical billing lifecycle: fixed and quantity-based work, explicit outcomes and corrections, agreement changes, billing periods, recurrence, approved late facts and immutable statements. No built-in Stripe or other payment-provider adapter, payment execution or provider refund flow is included. Multi-business tenancy and Bean-operated infrastructure are deferred until Bean Labs decides whether it wants to own infrastructure. Remote APIs, multi-host writers, payment-card handling, a general tax engine, tax/legal invoice claims, universal currencies and automatic collection are also outside the current target.

## Milestone record

| Milestone | Status | Scope and exit |
| --- | --- | --- |
| **M0 — Completion contract** | **Complete** | The owner-approved boundary, observable 1.0 journeys, M2 rules and later M4/M5 policy gates are recorded in this roadmap. |
| **M1 — Interface compatibility and recovery** | **Complete for the documented local profile** | Current CLI/JSON families, caller-owned retry behavior and compatibility policy are documented. Source-built v0.3.0→v0.4.0 evidence covers ordinary SQLite schema 8 only. See [M1 qualification](m1-current-format-qualification.md). |
| **M2 — Customers and agreements** | **Complete for the qualified source-built profile** | Isolate customers, require explicit customer scope, apply immutable effective-dated terms, and preserve customer-scoped retry identity and accepted history. The exact source-built v0.4.3 schema-8 to schema-9 transition is qualified. See [M2 migration qualification](m2-migration-qualification.md). |
| **M3 — Continuous history and concurrency** | **Complete for the measured source-built local profile** | Retain and replay history beyond 1,000 decisions; prove oldest-identity retry, concurrent duplicate/conflict behavior and complete snapshots. One synthetic macOS run exercised 3,499 retained decisions. This measurement does not guarantee general throughput or capacity. See [M3 qualification](m3-qualification.md). |
| **M4 — Usage and outcomes** | **Complete for the qualified source-built local profile; release pending** | Add an opt-in profile for successful `content.generated` work, with one configured integer unit and maximum quantity per agreement, exact USD scale-18 rating, versioned statement/export projections, and a runnable work→usage→charge→outcome→outcome-correction workflow. No minimum charge. Existing fixed-price records stay unchanged. See [M4 qualification](m4-qualification.md). |
| **M5 — Billing lifecycle** | Not started | Deliver bounded journeys for period assignment and close, recurrence, renewal/cancellation, late facts, corrections and immutable statements after their rules are approved. |
| **M6 — External collection** | Deferred beyond OSS 1.0 | No provider adapter, payment execution or provider refund workflow is required for the provider-free 1.0 journey. Reopen only by separate owner decision. |
| **M7 — Operations and recovery** | Partial | Extend health, backup/restore, reconciliation and period operations alongside the lifecycle features they support. |
| **M8 — Distribution and first use** | Partial | Qualify the exact advertised packages and fresh-install journey on each supported platform. Native artifact conformance is not established by the M1, M2, or M3 source-built evidence. |
| **M9 — Outside adoption and OSS 1.0** | Not started | Have two owner-approved unfamiliar developers complete the documented final journey, one on Mac and one on Linux, with two caller languages represented. |

## Approved M2 rules

- The agreement and price for a work record are selected by the ledger timestamp of its first successful acceptance. Caller-reported work time remains separate evidence and does not backdate pricing. An exact retry returns the original result and price selection.
- Accepting a request for work does not create a charge by itself. A separately submitted billable-work record may create the charge specified by the active agreement.
- Starting, amending or changing the price creates an immutable agreement version with an explicit effective time. Changes do not reprice accepted work. An agreement end blocks new billable work after its effective time while preserving accepted history and exact-identity retry resolution.
- Customer-scoped commands require an explicit customer. Delivery and semantic retry identities are scoped by `(source, customer)`: different customers may reuse an application ID, identical same-scope retries return the original result, and conflicting reuse refuses. Wrong-customer or unauthorized operations refuse without disclosure or mutation.
- Operator-configured customer/source authority remains explicit. M2 adds no automatic expiry or deletion of accepted records or retry identities; changing that policy requires a separate owner decision.

## Owner-approved M4 and M5 policy directions (2026-09-27)

- Failed work is chargeable only when the applicable billing arrangement explicitly says so. There is no implicit failed-work charge. The M4 profile does not implement an explicit failed-work fee and refuses failed work before charge; explicit failed-work pricing is deferred beyond M4.
- M4 quantity is per work record, not a cumulative activity reading. Cumulative activity belongs to a separate billing-period model under M5, with any unit conversion at period end.
- The owner withdrew four-decimal rounding after identifying AI token-scale usage. The M4 quantity profile must preserve token-granularity charges: use exact rate-by-quantity arithmetic and book at USD scale 18, within the existing core limit, so a typical fractional-cent amount is not rounded to zero per work record. Preserve full booked precision in statements and exports; do not round each work record or statement total to four decimal places. The existing fixed-price profile and accepted USD scale-2 records remain unchanged. Any billing-cycle payable rounding and regulatory presentation customization remain M5 decisions.
- Use the existing operator-configured customer/source authority for usage assertions and retain the source assertion and receipt evidence. Keep the existing outcome authority and evidence terms. The service must check authority; a submitted assertion is not independent proof of work or consent.

These directions preserve every approved M2 rule above, including agreement selection at first successful ledger acceptance, the original terms and accepted history, and delivery and semantic retry scope under `(source, customer)`.

An absent outcome does not by itself cancel an existing initial charge. The M4 usage profile rates successful `content.generated` work by one explicitly configured unit and integer quantity per agreement, with a maximum quantity and a positive USD rate of at most 18 fractional digits. It rejects failed work before charge. The profile has no minimum charge, does not amend an accepted base usage quantity, and uses the existing authorized outcome and outcome-correction contract. Accepted base records remain immutable. Additional billable milestones, multiple units within one agreement, minimum charges, explicit failed-work pricing, usage-quantity corrections, and cycle-aware post-close adjustments are deferred beyond this M4 profile; cycle-bounded adjustment behavior belongs to M5.

### M5 billing-cycle direction from the same owner decision

- A missing outcome is allowed. Include the initial charge when its billing-cycle statement closes even if no outcome has been supplied. An outcome occurring after that cycle is included in the next cycle. The treatment of an outcome reported after close but occurring before close remains open; this direction does not override applicable outcome windows or admit otherwise inadmissible late reports.
- Corrections are intended to be bounded by billing cycles. A change after close is expected to become a linked post-hoc adjustment in a later cycle; it must not revise the closed statement or overwrite accepted economic history. Exact eligibility, authority, evidence, linkage, and timing rules remain open.
- Exact cycle boundaries, billing timezone, and the clock that determines cycle membership remain open. In particular, no decision has been made between ledger acceptance time and reported occurrence time for assigning a fact to a cycle.

## Storage-format activation gate

M2 qualifies the exact source-built v0.4.3 ordinary local SQLite schema-8 to schema-9 path, including unsupported older-writer refusal, retained-row and retry reconciliation, rollback behavior, and whole-installation backup recovery. This qualification is the activation gate for that named source-built path only; it does not cover native packages, another source version, another host, or every historical store. See [M2 migration qualification](m2-migration-qualification.md).

M3 qualifies the named source-built schema-9 to schema-10 upgrade, shared-store schema-9 reopen behavior, unsupported older-writer refusal, retained-history recovery, and whole-installation backup restore. It also records the workload and concurrency/snapshot evidence. It does not qualify a native package, another host or operating system, or general maximum-volume capacity. See [M3 qualification](m3-qualification.md).

The unchanged-format M1 result alone is not evidence for a new schema; the separate M2 evidence covers only its named source-built transition. Neither qualifies native packages, another host, or every historical store. See the [compatibility policy](compatibility.md), [M1 qualification](m1-current-format-qualification.md), and [M2 qualification](m2-migration-qualification.md).

## Later policy gates

M4's versioned scale-18 usage setup, statement and export contracts are qualified for the named source-built profile; release publication remains pending. The explicitly scoped M4 profile does not include multiple billable milestones or units per agreement, minimum charges, explicit failed-work fees, or corrections to accepted usage quantities. M5 retains cycle-bounded correction and post-close adjustment behavior, billing timezone, exact cycle boundaries and membership clock, aggregation and final statement/payable rounding, close/reopen details, treatment of late-reported facts, recurrence and missed-run behavior, renewal/amendment/cancellation/proration, statement versus any separate non-tax invoice artifact, manual external payment status, and retention. The owner-approved M5 direction above does not decide those remaining policies. These unrelated policies do not block the completed M0–M4 source-built profiles.
