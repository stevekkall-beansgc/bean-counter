# M5 execution plan: billing lifecycle

**Status (2026-09-29): Package 0 complete, including statement balance presentation. Package 3 architecture/contract design is in review; product implementation has not started.** The [billing roadmap](billing-roadmap.md) is the product-scope source of truth. The [M5 decision register](m5-decision-register.md) records owner-approved defaults. The [M5 architecture proposal](m5-architecture.md) translates them into contracts and identifies two remaining pre-implementation choices. This document expands M5 into execution packages, acceptance evidence, and remaining contract work. M5 is scoped to the provider-free, one-business-per-installation, local billing profile.

## M5 objective

Deliver a runnable billing lifecycle where customers choose supported, customizable billing schedules, the business separately configures an internal fiscal reporting calendar, and both views reconcile to the same accepted ledger events. The journey includes recurring and usage charges, period close, late outcomes, pre-close usage quantity deltas, post-close adjustments, and immutable standard statements. A post-close adjustment may be handled manually through an ad hoc receivable or payable statement.

## Owner-approved product decisions

### Customer billing schedules and organization fiscal calendar

- Billing schedules are customer-level settings. A shared term model and validator define the supported terms; customers select terms within that centrally governed set. Support as much useful customization as the architecture can validate safely.
- The business has a separate, customizable fiscal reporting calendar for internal reporting. It does not select a customer's billing schedule or change customer amounts or statements.
- Assign records to both calendars using ledger acceptance time as the initial clock. Retain caller-reported occurrence time as evidence and as a possible input to future financial reporting dimensions; it does not backdate these M5 projections.
- Keep customer billing schedules, agreement recurrence rules, and the organization fiscal calendar as separate concepts. OpenAI's usage controls provide a useful structural analogy: Enterprise/Edu usage limits can have workspace defaults with group and individual overrides; the usage period can be a UTC month or align to the workspace billing cycle, and changing it does not change invoice or renewal dates. Bean Counter can adapt this as business defaults plus customer-level schedule choices, without adding a group layer to the one-business profile. OpenAI's product rules are not Bean Counter requirements. See [OpenAI usage limits and overages](https://help.openai.com/en/articles/20001001). The API platform separately distinguishes monthly organization/project spend controls, alerts, and optional hard limits; these are not substitutes for customer billing calendars. See [OpenAI API project limits](https://help.openai.com/en/articles/9186755-managing-projects-in-the-api-platform).

### Usage, outcomes, corrections, and statements

- Keep M4 per-work usage exact at USD scale 18. Do not round each work record to four decimal places. Cumulative activity is a separate period-based model; convert it at period end.
- A missing outcome does not cancel an initial charge. Include the charge at the cycle close. An outcome accepted after close is assigned by its acceptance time to a later cycle, subject to existing authority and outcome-window rules.
- Before close, correct an accepted usage quantity with an append-only signed delta linked to the original. Do not replace or mutate the accepted quantity. Preserve the applicable accepted agreement and rate basis.
- After close, represent a change as a linked post-hoc adjustment in a later period. A manually processed ad hoc statement may present the adjustment as receivable or payable. Standard closed statements remain immutable; M5 does not reopen or rewrite them.
- Ad hoc statements and standard statements are not tax/legal invoices. Bean Counter does not execute payments or provide a payment-provider adapter under this scope.

### Agreement and recurrence lifecycle

- Preserve M2 immutability: agreement changes are versioned and do not reprice accepted work. Customer schedules, agreement recurrence, and the organization's fiscal calendar have independent settings and effective dates.
- M5 should support the widest practical set of centrally validated recurrence rules rather than hard-code one global cadence. Each recurring occurrence needs a stable identity across retries and restart. The schedule model must represent an anchor, interval/calendar rule, applicable timezone, effective start/end, and versioned changes.
- Recurrence, missed-run handling, renewal, cancellation, and proration must have explicit rules before their code is implemented. Configurable policy can provide flexibility, but the system must not silently invent whether a missed occurrence is billed or whether a partial term is prorated.

## Execution packages and model ownership

Personas describe scoped work roles; they are not persistent processes or model choices. One OpenAI Sol session is the M5 integration manager. It owns the exact candidate and serializes changes to shared contracts, manifests, and schema. All implementation work uses isolated Git worktrees after the contract freeze. No two authors edit the same shared files concurrently.

| Order | Work package and output | Owner persona and model | Manager and independent review |
|---|---|---|---|
| 0 | Freeze the M5 decision register, supported-term proposal, per-package acceptance criteria, and stop conditions. Product/economic policy remains owner-approved. | **PINTO · OpenAI Luna** drafts the bounded package and decision record; Stephen owns product decisions. | **OpenAI Sol** manages dependencies and exact scope. |
| 1 | Read-only inspection of the already published v0.7.1 source for generic extension seams. Produce a public-source report only; no M5 roadmap or private repository input. | **MUNG · OpenCode Zen Big Pickle** (`opencode/big-pickle`), conditional on a fresh approved route. | **OpenAI Sol** checks relevance; this is advisory, not approval. |
| 2 | Produce generic synthetic date/quantity/adjustment cases and a small reference oracle for independent calendar projections. No Bean Counter repository or private roadmap input. | **SPROUT · OpenCode Zen Space Bunny** (`opencode/space-bunny-free`), conditional on a fresh approved route. | **OpenAI Sol** adapts only useful results into local acceptance fixtures. |
| 3 | Freeze calendar, recurrence, event-assignment, correction, close, and ad hoc statement contracts; select a validated rule representation and effective-date behavior. | **SPROUT · OpenAI Sol** authors the architecture and contracts. | **FAVA · OpenAI Astra** provides independent read-only review; **Sol** resolves findings and obtains any remaining owner decisions before freeze. |
| 4 | Implement customer billing-term selection, cycle membership/close, cumulative period conversion, and retry-safe close identities. | **SPROUT · OpenAI Sol** owns economic and ledger behavior. **OpenAI Luna** may own isolated CLI, fixtures, and documentation tasks after contracts freeze. | **Sol** integrates; **FAVA · OpenAI Astra** reviews money, period assignment, and idempotency. |
| 5 | Implement append-only usage quantity deltas, outcome windows, post-close adjustments, and manually issued ad hoc receivable/payable statements. | **SPROUT · OpenAI Sol** owns the ledger and adjustment path. | **Sol** integrates; **FAVA · OpenAI Astra** independently reviews immutability, authority, and reconciliation. |
| 6 | Implement versioned recurring terms, stable occurrences, renewal/cancellation, missed-run policy, and agreed proration behavior. | **SPROUT · OpenAI Sol** owns lifecycle economics; **OpenAI Luna** may own non-economic CLI/docs/fixture leaves. | **Sol** manages; **FAVA · OpenAI Astra** reviews agreement and retry semantics. |
| 7 | Implement the organization-level fiscal reporting calendar and internal report projection, independently of customer statement cycles. | **SPROUT · OpenAI Luna** owns the bounded reporting projection after the shared event contract is frozen. | **OpenAI Sol** integrates; **FAVA · OpenAI Astra** reviews report reproducibility and ledger reconciliation. |
| 8 | Run the end-to-end acceptance journey, recovery qualification where needed, independent candidate review, and release evidence. | **FAVA · OpenAI Astra** owns read-only technical review. **NAVY-BEAN · OpenAI Luna** coordinates the authorized release procedure and evidence packet. | **OpenAI Sol** owns candidate integration; normal release authorization and gates still apply. |

### Zen-lane controls

The Zen assignments are restricted to the public and synthetic inputs described above. Before any call, verify the active Agency allowlist, current catalog, and proof for the exact model route. Use Agency's governed `bin/zen_run.py` gateway, not direct `opencode run`. Send only typed synthetic or published `PUBLIC` envelopes; never send private roadmap/repository content, credentials, secrets, or cloud-authenticated work. A denial or safety marker stops that assignment; there is no alias or paid-model fallback. Zen outputs do not edit the product checkout or approve acceptance.

## M5 acceptance journey

The integrated synthetic journey must demonstrate:

1. One business configures a fiscal calendar and two customers select different supported billing schedules. The schedule terms are validated centrally and changes are effective-dated.
2. Both customer periods and the organization's fiscal reports assign records using ledger acceptance time. The same economic records reconcile across the two projections without duplicated charges; caller-reported occurrence timestamps remain evidence.
3. Fixed and scale-18 usage charges appear on the correct customer statements. Cumulative activity is converted at period end under its separate period model. No work-level four-decimal rounding occurs.
4. An initial charge remains in the closing statement when its outcome is missing. An outcome accepted later is assigned using its later acceptance time and follows the existing authority/window rules.
5. A signed usage quantity delta accepted before close affects the open period while preserving the original quantity. A post-close change creates a linked later-period adjustment and, when manually issued, an immutable ad hoc receivable/payable statement. The already closed standard statement remains byte-for-byte unchanged.
6. Recurring occurrences have stable identities across retry and restart. Renewal, amendment, cancellation, and missed-run behavior follow the effective-dated agreement rules.
7. Repeating a period close or adjustment request after an unknown result returns the original result; conflicting identity reuse refuses without mutation. Backup/restore and any required storage-format transition preserve accepted records and identities.

The journey is qualified for its named source-built local profile. It does not imply native-package conformance (M8), PostgreSQL, multi-host operation, tax/legal invoice status, or payment execution. M7 recovery/operator needs should be completed alongside the lifecycle behavior they support.

## Contract freeze inputs

Package 0 is complete. The owner-approved choices in the [decision register](m5-decision-register.md) are the inputs to Package 3: finite customer calendar terms; separate versioned fiscal calendars; acceptance-time assignment; explicit recurrence and operator catch-up; signed bounded quantity deltas; immutable closed statements; period-only statement totals with negative nets classified payable; exact scale-18 totals; and no provider or payment-status lane. The [architecture proposal](m5-architecture.md) is under independent review. It identifies the legacy-history activation rule and mutually exclusive standard/ad hoc presentation of post-close adjustments as pre-implementation decisions. Package 3 must encode the settled rules without weakening existing authority, retry, persistence, or canonical-record guarantees. If contract drafting discovers a conflict or requires a change to an approved economic rule, stop and return that exact conflict to the owner.

Statement totals remain exact at scale 18. Each standard statement covers only its own billing period, without prior-balance carry-forward; a negative exact net is represented as payable. Ad hoc statements remain linked to explicitly named post-close adjustment IDs. These semantics are frozen for M5 contract and implementation work.

Multiple billable milestones or units per agreement, minimum charges, and explicit failed-work fees remain outside M5 unless the owner separately adds them. Failed work is chargeable only when a billing arrangement expressly permits it; M4 continues to refuse failed work in its qualified profile.

## Readiness and stop rules

M5 is ready to begin with Package 0 and the bounded architecture work. Do not implement a dependent behavior until its necessary term and failure semantics are frozen. Stop and return to the owner when a choice changes who owes money, the amount, acceptance/authority, retention, or the external payment boundary. Keep the M5 roadmap status “implementation not started” until the first product implementation is authorized. Do not publish a report-only release; use the normal exact-source release gates when a runnable M5 candidate is complete.
