# Ledger Lab contributor contract

The public project name is Bean Counter. Read [CURRENT-REQUIREMENTS.md](CURRENT-REQUIREMENTS.md) first for the authorized local SQLite release scope and deferred capabilities. Historical contracts and verdicts remain intact; preserve all existing economic, authority and persistence guards.

Read `docs/implementation.md`, `contracts/README.md`, `docs/phase-gates.md`, detailed design §§1–11/26/27 in `docs/design/sources/`, and the canonical-record addendum before coding. Later normative addenda supersede source sketches only where stated.

- The original Phase 0 scaffold and Phase 1 work-item rules are historical development guidance. For current local-profile work, follow [CURRENT-REQUIREMENTS.md](CURRENT-REQUIREMENTS.md), use an assigned bounded scope, and preserve existing guards. No UI/payment expansion in a driver task.
- Keep exactly three production crates plus unpublished testkit. Pure core has no database, runtime, clock, entropy, filesystem, environment, network, model client or first-party unsafe code. Store modules cannot evaluate pricing or independently decide authority. CLI/SDK/inspector cannot compute authoritative economics.
- Never alter a frozen fixture to make an implementation pass. Do not run historical `work/phase0-audit/freeze_records.py`. Contract changes require an explicit amendment, changed fixtures, independent byte/math verification and a reviewed freeze manifest.
- Keep original IDs, canonical bytes, 80-atom first-slice result, 25 new immutable rows and 29 manifest members. Absence/null, UTF-16 key order, signed rounding, scoped claims and exact reversals are contracts.
- Use one coordinator, concrete SQLite/PostgreSQL adapters, separate SQL/migrations. No SQLx Any or public raw-action append. Mutable authority/reservations are checked under the specified ordered locks. Preserve unknown commit outcomes.
- Do not invent dependency versions or MSRV. Resolve/pin dependencies in the owning integration lane and rerun the boundary checks. Do not add an empty production abstraction merely because §4 lists its future path.
- Validate with `sh scripts/check.sh`; record separately real-store/cancellation/TLS tests when implemented. Document tests are not product conformance.
- Use isolated Git worktrees and `codex/` branches for parallel sessions. Shared manifests/lockfile/contracts belong to the integration owner. Publishing, pushing and deployment require an assigned scope and the Bean release gates; paid infrastructure requires separate authorization.
- Keep credentials, local toolchains, `.ledger`, databases and operational evidence out of commits.

## Test commands

- Current local SQLite unit/contract gate: `sh scripts/check-local-billing.sh`.
- Current end-to-end gate: `sh scripts/check-local-billing-e2e.sh`.
- Broader historical regression gate: `sh scripts/check.sh` (unchanged; does not by itself certify deferred capabilities).
- Use the pinned compiler and locked dependencies. Resource guarantees, PostgreSQL product support and multi-host acceptance are deferred, not passed. Amended local Phase 6 closed with owner acceptance and a waiver of separate independent review; no independent review PASS is claimed. Do not substitute a reviewer.

## OpenCode Union Alpha

When explicitly asked to use Union Alpha, run `opencode run --pure --model opencode/union-alpha --format json` from an explicitly supplied isolated Git worktree. Give a bounded task, validation commands and a stop condition. Union Alpha is a remote free model: never send secrets, credentials, private data or cloud-authenticated work. The `bean-local-session` workflow remains local-only and must not be routed through it.
