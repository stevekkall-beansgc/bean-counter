# Billing product roadmap

**Status (2026-09-26): M0 and M1 are complete. M2 policy is approved; M2 implementation has not started.** The accepted scope and M2 rules below are the product decision record for the provider-free OSS 1.0 billing journey. This roadmap is specific to the ordinary local billing profile; the historical product roadmap at the repository root retains its separate scope and status.

## Product goal and boundary

Bean Counter is a self-hosted billing engine for one business per installation, with multiple customers in the 1.0 target. Applications use the language-neutral CLI/JSON interface. Operators provide and administer the host, storage and backups. The product records agreed charges and retained evidence; it does not determine whether work was valuable or establish customer consent on its own.

OSS 1.0 targets a practical billing lifecycle: fixed and quantity-based work, explicit outcomes and corrections, agreement changes, billing periods, recurrence, approved late facts and immutable statements. No built-in Stripe or other payment-provider adapter, payment execution or provider refund flow is included. Multi-business tenancy and Bean-operated infrastructure are deferred until Bean Labs decides whether it wants to own infrastructure. Remote APIs, multi-host writers, payment-card handling, a general tax engine, tax/legal invoice claims, universal currencies and automatic collection are also outside the current target.

## Milestone record

| Milestone | Status | Scope and exit |
| --- | --- | --- |
| **M0 — Completion contract** | **Complete** | The owner-approved boundary, observable 1.0 journeys, M2 rules and later M4/M5 policy gates are recorded in this roadmap. |
| **M1 — Interface compatibility and recovery** | **Complete for the documented local profile** | Current CLI/JSON families, caller-owned retry behavior and compatibility policy are documented. Source-built v0.3.0→v0.4.0 evidence covers ordinary SQLite schema 8 only. See [M1 qualification](m1-current-format-qualification.md). |
| **M2 — Customers and agreements** | **Policy approved; implementation not started** | Isolate multiple customers, require explicit customer scope, apply immutable effective-dated terms, and preserve customer-scoped retry identity and accepted history. |
| **M3 — Continuous history and concurrency** | Not started | Measure a single-host workload beyond the current 1,000-decision ceiling; prove oldest-identity retry, concurrent duplicate/conflict behavior and complete snapshots. Report the tested workload, not a general capacity guarantee. |
| **M4 — Usage and outcomes** | Not started | Add quantity billing and a runnable workflow example linking work identity, usage, agreed charge, supplied outcome and correction. Define units, rounding, authority and unresolved-outcome behavior before implementation. |
| **M5 — Billing lifecycle** | Not started | Deliver bounded journeys for period assignment and close, recurrence, renewal/cancellation, late facts, corrections and immutable statements after their rules are approved. |
| **M6 — External collection** | Deferred beyond OSS 1.0 | No provider adapter, payment execution or provider refund workflow is required for the provider-free 1.0 journey. Reopen only by separate owner decision. |
| **M7 — Operations and recovery** | Partial | Extend health, backup/restore, reconciliation and period operations alongside the lifecycle features they support. |
| **M8 — Distribution and first use** | Partial | Qualify the exact advertised packages and fresh-install journey on each supported platform. Native artifact conformance is not established by M1's source-built evidence. |
| **M9 — Outside adoption and OSS 1.0** | Not started | Have two owner-approved unfamiliar developers complete the documented final journey, one on Mac and one on Linux, with two caller languages represented. |

## Approved M2 rules

- The agreement and price for a work record are selected by the ledger timestamp of its first successful acceptance. Caller-reported work time remains separate evidence and does not backdate pricing. An exact retry returns the original result and price selection.
- Accepting a request for work does not create a charge by itself. A separately submitted billable-work record may create the charge specified by the active agreement.
- Starting, amending or changing the price creates an immutable agreement version with an explicit effective time. Changes do not reprice accepted work. An agreement end blocks new billable work after its effective time while preserving accepted history and exact-identity retry resolution.
- Customer-scoped commands require an explicit customer. Delivery and semantic retry identities are scoped by `(source, customer)`: different customers may reuse an application ID, identical same-scope retries return the original result, and conflicting reuse refuses. Wrong-customer or unauthorized operations refuse without disclosure or mutation.
- Operator-configured customer/source authority remains explicit. M2 adds no automatic expiry or deletion of accepted records or retry identities; changing that policy requires a separate owner decision.

## Storage-format activation gate

The demonstrated M1 path is local SQLite schema 8. Before a future production format accepts writes, its release must name supported source versions, refuse unsupported stores before mutation and qualify an exact transition. The transition may be an in-place migration or a verified fresh-store transfer, but it must preserve accepted records and identities and prove its backup and recovery boundary. Selecting the mechanism is an engineering decision; passing the exact qualification is required before writes.

The unchanged-format M1 result is not evidence for a new schema, an older-binary refusal case, a native package, another host or every historical store. See the [compatibility policy](compatibility.md) and [qualification evidence](m1-current-format-qualification.md).

## Later policy gates

M4 requires explicit billable milestones, usage units and precision, rounding and minimums, outcome authority and evidence, unresolved-outcome behavior, and correction/credit rules. M5 requires billing timezone and period assignment, close/reopen behavior, late-fact treatment, recurrence and missed-run behavior, renewal/amendment/cancellation/proration, statement versus any separate non-tax invoice artifact, manual external payment status, and retention. These unrelated policies do not block M2.
