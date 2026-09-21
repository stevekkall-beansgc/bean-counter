# Approved v0 outcome semantic histories

`histories.json` is synthetic, unfrozen testkit input, outside all frozen Phase 0/1
inventories. It is not a production wire schema or a canonical record proposal.
The approved contract and integration boundary are documented in
`ledgerlab-core/src/policy/chaining/outcomes/README.md`.

23 histories cover fixed fees, retail-net percentage rebates, exact rational and
signed half rounding, correction/reinstatement, independent families, aggregate
premium/discount rejection, failed atomic replacement, supplier separation,
zero-dollar permanent claims and zero-net targets, missing-target retry, finality,
failed/reversed work, authority/evidence, distinct times/deadlines, source/family/
scope isolation, policy version changes, and cap composition refusal.

`oracle/phase2/outcomes.py` uses only Python's standard library and Fraction. It
reads input stories, not Rust results or fixture expectations. The Rust adapter
only constructs production typed inputs and projects outputs; it has no outcome
pricing equations. The test compares every attempt's status and complete growing
journal, and checks a few independently hand-authored numerical/result anchors.
Every accepted revision is replayed from retained input. An extra small integer
rounding check covers 8,442 signed basis/rate combinations.

`oracle/phase2/regressions.py` separately derives five additional histories from
unchanged legacy proposal inputs, covering receipt before occurrence, failed and
reversed publication, an unrelated supplier path, and current-config reversal
roles/permissions. Its full journals and state compare after every attempt.

Run `cargo test -p ledgerlab-testkit --test phase2_outcomes --test phase2_core
--test phase2 --locked --offline` and `sh scripts/check.sh` using the repository's
verified toolchain. Real authority/persistence/concurrency remains outside this
pure-core evidence. No fixture writer runs during tests.

Validation on 2026-09-21 with the repository's Rust 1.98.1 toolchain:
`sh scripts/check.sh` passed **113 Rust tests, zero failures, 13 explicit ignored
later/opt-in gates**, formatting, warnings-denied Clippy, no-default compilation,
resolved dependency/source boundary checks, and independent Python/Node contract
audits. All 99 frozen entries, 60 vectors, 25 accepted rows and 29 manifest members
remain unchanged. The new suite compared **86 submissions across 23 histories**
(38 accepted, 7 duplicate, 39 refused/waiting and 2 target lifecycle steps), plus
five live legacy discrepancy histories and retained-input replay.

The toolchain activation and Python audit dependencies used were the existing
local paths documented in the testkit README. No dependency/lockfile change,
network package fetch, PostgreSQL server certification, or Phase 2 durable-store
claim is included. Eleven PostgreSQL opt-in tests and two independent destination
restart gates remain explicitly ignored by the offline check.
