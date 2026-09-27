# M4 qualification: exact unit rating and outcomes

**Result:** qualified for the named source-built, single-host local SQLite profile on macOS 26.6.2 (Apple silicon). The M4 profile adds a billing setup family and statement/export projections; it does not change the SQLite schema. This record does not qualify a published native artifact, another host or operating system, PostgreSQL, hosted operation, multi-host writers, or general production capacity.

## Candidate and environment

- M4 implementation source commit: `d6fd10620c47b66b48e9179018ce3210df03606c`.
- Pre-M4 v0.6.0 source commit: `32138eaf7f6009d6de1aea316b6a600f3b49e377`.
- Source-built M4 CLI SHA-256: `718f8f92f1b56db4ae6a8b9150c1d714093610f08f6a7f42cf9f7baa90fb625a`.
- Source-built pre-M4 CLI SHA-256: `713b3b2660aa7e3ea016fa5e2a1516a56fc680372d5a5654a63c8fd819c55dc0`.
- Toolchain and host: Rust 1.98.1, Cargo 1.98.1, `aarch64-apple-darwin`, macOS 26.6.2.
- Dependencies were locked and builds ran offline.
- The M4 candidate shipped as v0.7.0 from exact commit `48d76ae249fbfcb79a5012b265da51a71cb996e3`. At publication, Bean's release proof verified that the annotated tag, `origin/main`, and GitHub Release all resolved to that commit. The later v0.7.1 documentation-only patch synchronizes roadmap and requirements status; it does not change M4 implementation or qualification.

## Qualified M4 behavior

- `ledger-local-billing/2` is additive. Setup/1 retains its fixed USD/call meaning and serialized form. Usage agreements select one integer unit, a positive USD unit rate with up to 18 decimal places, and an explicit maximum quantity; no minimum charge is applied.
- Only successful `content.generated` work is chargeable in this profile. A failed work assertion is refused before charge. Failed-work fees require a later explicit profile.
- Unit rates and accepted quantities are multiplied exactly and booked at USD scale 18. An exact retry returns the original result. Accepted base quantities stay immutable; the existing outcome and outcome-correction path remains available.
- Legacy-only scale-2 histories retain statement/2 and export/2. A usage or mixed-scale history uses statement/3 and export/3 at scale 18. Legacy scale-2 records convert exactly by appending 16 decimal places; no work record or statement total is rounded to four decimal places.
- The runnable synthetic usage example covers setup, accepted work, outcome, outcome correction and CSV mapping/export.

The focused CLI test priced 100 tokens at USD `0.00000025` each, producing USD `0.000025`, or exactly `25000000000000` scale-18 atoms. It refused failed work, a quantity above the configured maximum, and a fractional quantity; after those refusals the statement remained identical. It also applied the configured outcome and correction. A mixed fixed/usage statement converted the USD 2.50 legacy charge to `2500000000000000000` scale-18 atoms, retained the usage charge of `25000000000000` atoms, and reconciled to `2500025000000000000` atoms. The mixed CSV assertions preserve both exact postings.

## Pre-M4 writer refusal and recovery

A synthetic store began with setup/1 and one accepted fixed-price record, then registered a setup/2 usage agreement and accepted one usage record using the M4 CLI. The exact source-built v0.6.0 CLI attempted a new usage submission against that store and exited 3 with `BILLING_SETUP: request rejected`.

After checkpointing SQLite WAL, the durable `local.db` SHA-256 was unchanged across the refusal (`ee3369784d61a54e383285848a179080b16953636e948da33f4c1e1426952896`). The current M4 CLI returned the exact same complete statement JSON before and after the old-writer attempt: cutoff 2 and net `2500025000000000000` scale-18 atoms. This demonstrates refusal without an economic-store change for the exercised mixed fixture and confirms the M4 reader can still recover the accepted statement. It is not a proof for every old build or arbitrary damaged store; SQLite runtime sidecars are not used as the semantic-state oracle.

## Checks run

- `sh scripts/check-local-billing.sh`: passed with pinned Rust 1.98.1. This includes workspace formatting, core and ledger tests, Clippy with warnings denied, no-default-feature checking, boundaries, contract checks, reservation checks, and dependency-profile checks.
- `sh scripts/check-local-billing-e2e.sh`: passed. CLI output/setup: 5 tests; billing: 8; finance CSV: 1; local CLI: 17.
- The M4 CLI tests cover exact unit-rate arithmetic, failed-work refusal, configured maximum and integer-quantity validation, outcomes and outcome corrections, mixed scale-2/scale-18 statements and CSV, and legacy totals exceeding one individual money value's bound.
- The five `examples/billing/usage/*.json` fixtures parse as JSON.
- `git diff --check`: passed before the implementation commit.

## Deliberate limits

M4 does not deliver cumulative-activity billing, billing-cycle assignment or close, minimum charges, multiple billable units or milestones per agreement, explicit failed-work fees, corrections to accepted usage quantities, or post-close adjustments. Cycle-bounded corrections and post-close adjustments remain M5 work, along with cycle membership, final statement/payable rounding and the other open lifecycle policies in the [canonical roadmap](billing-roadmap.md). Payment-provider integration is outside this profile.
