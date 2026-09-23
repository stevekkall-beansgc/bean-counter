# Remaining gates

**Current local-profile status:** the reduced Phase 4, published finance CSV Phase 5 and amended Phase 6 are complete. Phase 6 closed with owner acceptance and an explicit waiver of its separate independent-review requirement; independent technical review was not completed or passed. See [STATUS.md](../STATUS.md) and [CURRENT-REQUIREMENTS.md](../CURRENT-REQUIREMENTS.md).

**Historical full-platform gate inventory below:** [Bean Counter local SQLite requirements](../CURRENT-REQUIREMENTS.md) supersede conflicting publication prerequisites for the supported billing profile. The rows below retain the original plan and are not the current local release checklist. Deferred resource guarantees and platforms have not passed.

## Before Phase 1 integration exits

1. Resolve SQLx, bundled SQLite (minimum 3.51.3, actual version/source/options), Rustls and JCS versions/features on this machine, then pin the lockfile. No external Rust dependency is currently selected.
2. Prove tracked SQLite BEGIN IMMEDIATE and PG SERIALIZABLE transaction lifetimes, rollback/drop/cancellation/discard and unknown commits on real stores. Prove every first-slice write boundary and read back after reopen.
3. Prove pure-core dependency/source boundaries for every selected feature; exact strict JSON/JCS and arithmetic fixtures in Rust. Keep Python/Node oracles independent.
4. Prove PEM-only versus public roots, wrong-host/expired/unknown CA rejection, no plaintext fallback or ambient HOME/Keychain trust.
5. Prove separate SQLx offline metadata sets or deliberately select typed parameterized runtime queries plus real-store tests.
6. Establish and test an MSRV only after dependency resolution. Rust 1.98.1 is the verified development toolchain, not an MSRV.
7. Run actual native pipeline smoke jobs. Documented runner labels are an inventory, not execution evidence. Local development on macOS 26 does not certify macOS 15 or Linux.

## Later gates, without blocking the bounded Phase 1 slice

- Reviewed canonical encoding extensions for acquisition/link/reversal facts, supplier snapshots and invocation/reservation transitions before their Phase 2/3 work; preserve the first-slice repair unchanged.
- Phase 3 complete authority/pending/claim/closure/reversal races and all record variants.
- Phase 4 stored explain/replay, isolated fake destination, fencing/unknown delivery, no payment execution.
- Phase 5 export/restore/import/cutover preserving bytes/IDs with dispatch held; restore drills before production trials.
- Phase 6 generated DTO/schema/OpenAPI/SDK agreement, auth/origin/limits, CLI/HTTP/inspector and onboarding.
- Phase 7 full native target matrix, ABI, PG17/18 current patched minors, exact archives/images, SBOM/provenance/license inventory, offline cached build, clean download and performance evidence.
- Historical publication namespace and release-infrastructure gates applied before the public repository existed. The current public repository and v0.2.1 release are documented in [STATUS.md](../STATUS.md); future local-profile releases use the [current release requirements](../CURRENT-REQUIREMENTS.md).

Closed gates: canonical-record blocker addendum accepted and independently checked; usable zero-cost local Rust/Git toolchain verified. `PHASE-0-BLOCKERS.md` remains historical evidence, not an active stop instruction.
