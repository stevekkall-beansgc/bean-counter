# Current product Phase 3 status

**Bounded Phase 3 persistence is complete. Independent R2 review returned PASS
for the exact code commit `6572f7f06b9c7ad2093d66318ab762b9801791fe`.**
The owner approved recording this disposition. The product is not
production-ready. Phase 4, isolated nonposting policy comparison, is next;
this documentation update implements no Phase 4 work.

This document and the root [README](README.md) are the current progress
pointers. [ROADMAP.md](ROADMAP.md) is a frozen historical contract overlay;
its “not started” text records the earlier freeze state and is stale as a
progress report. Its bytes, checkboxes and normative scope remain unchanged.
Earlier phase reports remain evidence of their own checkpoints, including
the R2 implementation report's then-pending independent review.

## Reviewed boundary and correction lineage

The completed boundary is one final base with no predecessor chain and its
supplier lifecycle: registration, authorized outcome adjustment, authorized
correction, and reservation closure. One coordinator persists the bounded
path on SQLite and PostgreSQL, retaining original receipt pairs, canonical
records, anchors and heads across reopen, retries and tested failures.

- Initial integration: `930497b` ([integration record](PRODUCT-PHASE-3-INTEGRATION.md)).
- R1: `8335b0d94432ab4679109cb761703f1ade7a5dbe` requires the host write-authority
  callback and validates its returned proof for fresh outcomes while preserving
  read-authorized retries and aliases ([R1 record](PRODUCT-PHASE-3-R1.md)).
- R2: `6572f7f06b9c7ad2093d66318ab762b9801791fe`, whose sole parent is R1,
  detects stale mutable write heads before planning and reuses validated
  history only within the current locked snapshot ([R2 record](PRODUCT-PHASE-3-R2.md)).
  Five-second/five-attempt limits, error classifications and unknown-commit
  handling remain unchanged.

The [independent R2 review](../../../ledger-lab-p3-r2-re-review/outputs/PHASE-3-R2-INDEPENDENT-REVIEW.md),
dated 2026-09-21 America/New_York (2026-09-22 UTC), closes R1 and R2 for this
bounded exit. That external local evidence link assumes the sibling task
directory layout; the report is not a shipped repository artifact. Its PASS
applies to the exact code commit above. This status successor changes only
this document and README, preserving the reviewed implementation tree.

## Independent validation at the reviewed commit

| Gate | Fresh independent result |
| --- | --- |
| Full offline workspace/static/contract checks | 172 passed, 0 failed, 34 ignored; formatting, strict Clippy, feature/no-default, architecture and freeze checks passed |
| PostgreSQL 17.11 affected outcome suite | 15/15 passed |
| PostgreSQL 18.6 affected outcome suite | 15/15 passed |
| Controlled background-priority diagnostics | All four pairs passed on each PG major, with exact complete 37-table state after reopen |
| Independent reopened SQLite/PG17/PG18 comparison | Four prefixes, 82 retained records, eight negative probes per store; exact canonical records, original receipt pairs, anchors and heads |

The 34 ignored entries are 32 opt-in PostgreSQL entries and two deferred
process-durable fake-destination gates; ignored tests are not passes. The
15-test PG suites cover the affected outcome path, not a fresh rerun of the
unchanged legacy service/outbox/upgrade/TLS suites. SQLite covers 360
failure/cancellation positions and eight process kills. Each PG major covers
386 validated-plan failure positions, 14 primitive positions and eight actual
COMMIT request/reply cuts; cancellation is not claimed at every PG position.

All frozen/pinned bytes remain unchanged. The review verified preservation of
the 20 original, 26 R1 and 86 R2 evidence entries. Earlier review failures and
intermediate diagnostic failures remain failures. Three captured failed-pair
inventories match their independently verified complete durable prefixes;
that does not establish the cause or post-failure state of older uncorrelated
failures. The successful finite trials do not certify arbitrary-load or
universal availability.

## Remaining limits and next phase

External host authentication, authority, real assent and provisioning, public
submission, broader base chains, exhaustive authority-revocation/base-reversal/
multiple-family races, and connecting every numeric history to every backend
remain outside this exit. So do deferred fake-destination gates, host power-loss
certification, native targets/MSRV, export/payment/release readiness and a CLI
surface for the outcome lifecycle. The existing CLI retains its original
generation workflow.

Phase 4 is the roadmap's isolated comparison of retained activity under
candidate policies, with no authoritative claims, postings or export intents.
It requires separately assigned scope. This status update authorizes no main
merge, push, release, deployment, registration, purchase or infrastructure
spending.
