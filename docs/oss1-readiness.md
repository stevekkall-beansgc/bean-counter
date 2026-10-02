# OSS 1.0 readiness

Status: owner approved continued work on 2026-10-02. M8 is released; M7 readiness and M9 outside adoption remain open. This document is a preparation and evidence map, not 1.0 acceptance.

Coordination: [Core #350](https://github.com/stevekkall-beansgc/legume-labs-core/issues/350) under BEANA, with Codex Local owning the bounded caller compatibility and operator/readiness checks. The [billing roadmap](billing-roadmap.md) remains the scope boundary. Fixed post-hoc fees, arbitrary event chains and the experimental manager in Core #269 are separate work.

## Existing evidence and M7 gaps

| Operator need | Existing evidence | Remaining readiness work |
| --- | --- | --- |
| Health and integrity | M5 open validates retained requests, canonical hashes, projections, identities, snapshot boundaries and presentation ownership before work. CLI exit codes distinguish config, refusal, authority, busy, unknown and integrity failure. | Review the [operator checklist](billing-operations.md) using supported reads, source/binary identity, private-path and resource checks. A successful read is not disk/RAM headroom, storage-hardware certification or continuous monitoring. No dedicated health service is supplied. |
| Whole-installation recovery | v0.9.0 qualification on both native targets verifies all copied file bytes before reopening, retained statements/receipts and identical retries. Earlier local suites cover software write refusal, SQLite full, process exit and unknown commit. | Review the documented operator sequence with an unfamiliar user. Confirm their customer/source inventory, retained checkpoints, writer exclusion, backup generations and recovery reconciliation. Power loss, rollback detection, online backup and copied simultaneous writers remain unqualified. |
| Caller reconciliation | The CLI returns retained receipts for exact scoped retries and refuses conflicting identity reuse. | Python/Node examples in v0.9.0 only acknowledge explanation `/2`. Package the bounded `/2` plus `/3` correction after real-engine regression and release gates. Preserve exact request bytes and identities after unknown results; never recover with a new ID. |
| Period operations | M5 and installed v0.9.0 evidence cover real boundary close, cumulative correction, immutable statements, exactly-once standard/ad hoc presentation, explicit recurrence and fiscal/export reconciliation. | Review the [operator checklist](billing-operations.md) and existing quickstart route through the commands and expected reconciliation outputs. Confirm usability without adding background charges, payment state or new economic semantics. |
| Diagnosis | The previously untriaged replacement-process test was rerun at unchanged v0.9.0 source. It fails inside the sandbox at `ps` with PermissionDenied and passes with process inspection/signal access. | Retain both focused logs in the issue #8 triage receipt. The interrupted original run lacked a diagnostic, so its exact original cause is not independently proved. No broader-library PASS or multi-host product support follows from the focused pass. |

M7 remains Partial until the operator checklist, concrete acceptance cases and actual results resolve this table. Do not mark it complete because native recovery alone passed.

## M9 unfamiliar-developer protocol

Two owner-approved developers unfamiliar with this repository must complete the documented final journey, one on the named Mac environment and one on the named Ubuntu environment, with Python and Node represented. Their participation and consent are prerequisites; no agent, author, automated suite or synthetic persona substitutes for them.

Prepare one release-specific packet containing the exact source/archive/binary identities, installation inputs, supported profile and deferred capabilities, quickstart, two honest product mappings, expected receipts/statements/export totals and recovery procedure. Reconcile the packet with BEANA before use. Both mappings must fit the existing `content.generated` and fixed/quantity profiles truthfully; unsupported business semantics need a separately reviewed proposal.

Run only fresh synthetic scopes and local private installations. Record environment, caller language, prior familiarity, start/end timestamps, time to find the correct entry point and first receipt, every intervention, every refusal/failure, exact request/receipt identities, expected and actual totals, restart/retry and whole-installation recovery results. Preserve passed, failed and unrun cases separately. Agent orientation within 30 seconds and a first synthetic receipt within five minutes after installation are targets, not observed results.

The journey must cover scoped fixed and fractional usage, stable retries and changed-identity refusal, authority refusal, effective-dated terms, before/after-close corrections, immutable close, exactly-once adjustment presentation, separate fiscal reporting and reconciled JSON/CSV. Participants must understand unknown-commit reconciliation and the difference between a statement and payment or a tax/legal invoice. Unsupported operations must refuse or remain clearly unavailable.

Collect evidence first, fix demonstrated usability defects within the approved scope, requalify changed packages and repeat affected participant checks. Steve accepts the evidence before M9 or OSS 1.0 is marked complete. A 1.0 tag requires exact-source QA, independent review where assigned, qualified native assets and all ordinary release gates.
