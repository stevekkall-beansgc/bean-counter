# Phase 1 integration — bounded acceptance slice complete

21 September 2026. Branch `codex/phase1-integration`, isolated worktree
`ledger-lab-v0-integration`. The **first SQLite vertical slice is complete for
the assigned frozen completion scenario**, and the same coordinator has passing
real PostgreSQL **18.6 and 17.11** parity evidence. This is a local integration
checkpoint, not a release or a claim that every broader Phase 1/public-v0 gate
is closed.

## Reviewed repairs and preserved history

Both debug reports and full diffs were read before cherry-picking, in order:

| Reviewed commit | Integration commit | Result |
|---|---|---|
| `e883c158d371a8fc4b10246521d3d166e572260a` | `2c9d617` | Preserve original uncertain SQLite connection evidence through reopen/retry |
| `d5b5a8bf455c1472dcc8fcd0f1b0fe9b02bffab7` | `5ef14dd` | Explicitly unlock the final SQLite owner before closing its descriptor |

The repaired gates are explained in `PHASE-1-DISCARD-GATE.md` and
`PHASE-1-PARALLEL-GATE.md`. Their earlier repeated-run counts are historical
repair-lane evidence, not new runs counted below. The initial unrestricted
integration baseline passed: 61 Rust test entries, all 83 SQLite acceptance
cases, all 45 cancellation cases, storage regressions and frozen audits.

Original Phase 0 commit `8004a38` and the earlier reviewed lane history remain
intact: core `c128946`, SQLite `75276ed`, testkit `69f76a0`, historical SQLx
blocker `997b47c`, ADR 022/proof `5cdb7a9`, and checkpoint `910edb7`.
Contracts, frozen fixtures, design sources, original ADRs and freeze manifests
remain unchanged. ADR 022 is the only added ADR relative to Phase 0.

## Implemented result

One private-port coordinator drives pure-core evaluation and both concrete
stores. It checks the installation scope/admission, authenticated source and
receipt permission, retained grant, active heads, binding and chain context
under the ordered locks. It verifies retained documents, evaluates once, writes
the complete decision and returns Accepted only after commit acknowledgement.

The frozen successful decision retains 100 and -20 atom actions, one **80-atom**
intention, **25 new immutable records**, **29 manifest members**, original IDs,
canonical bytes/hashes and the original byte-identical receipt. Identity and
semantic duplicate/conflict paths pass. Identity comparison includes exact
normalized ingress bytes; retained key/receipt canonical bytes and hashes are
checked before returning a duplicate. Semantic aliases preserve accepted history.
Waiting makes no identity/economic reservation and remains a distinct type from
OutcomeUnknown. Same-identity retries resolve on the authoritative store.

PostgreSQL now has its own migration and typed, parameterized SQL. Production
uses the exact reviewed custom Rustls connection/trust code from ADR 022, with
explicit configuration and no ambient PG/HOME/platform roots. SQLx remains
SQLite-only in the activated graph. The PG runtime role is separate from the
migration owner and lacks ownership, DDL and destructive table rights. Opening
verifies primary/durability/TLS/schema compatibility and never migrates or seeds.

A registered bounded task owns each real SERIALIZABLE transaction and dedicated
session. Registration precedes connection establishment and is synchronized with
shutdown. Handles are poisoned on cancelled/failed operations. Started commits
are supervised through the bounded drain; ambiguous outcomes stay unknown.
Sessions are discarded after every transaction, with no uncertain-session reuse.

The blanket storage dead-code allowance is removed. Narrow annotations remain
only on explicit private provisioning seams and retained startup diagnostics.
Exactly three production crates plus the unpublished testkit remain. No HTTP,
CLI, UI, dispatcher, payment, Phase 2 record family or remote service was added.

## Executed validation

Local client/toolchain: **Rust 1.98.1, macOS 26.6.2, ARM64**. Rust/Git commands
used the supplied activation script, stable alias and prepared Python audit
path. `RUST_TEST_THREADS` was unset for the complete workspace checks.

| Check | Final result |
|---|---|
| `sh scripts/check.sh` | **Passed: 65 Rust test entries, 0 failed**; formatting, warnings-denied Clippy, all-target/all-feature tests, no-default checks, dependency/source checks and Python/Node audits |
| `cargo build --workspace --all-targets --all-features --locked --offline` | Passed |
| SQLite independent acceptance suite | **83/83 scenarios passed**, including all **54 before/after labels across 27 writes**, invalid inputs, duplicates/conflicts, zero-action, authorization, controlled unknown outcomes and reopen |
| SQLite cancellation | **45/45 main-path await cases**, plus semantic-alias write cancellation, passed |
| SQLite barrier race | **4 overlapping requests**, real SQLITE_BUSY on another physical connection, exactly one Accepted; complete journal and original duplicate receipts |
| SQLite storage/regressions | **23 test entries passed**: original 21 plus the 2 ownership regressions; includes all write cut positions, read/write/drop cancellation, uniqueness/FKs/immutability, queue deadlines, OS ownership, normal one-writer concurrent requests, and 12 driver commit/hard-close trials |
| SQLite additional checks | Waiting preserves the entire seed and reserves nothing; wrong installation scope rejected; retained receipt corruption rejected |
| PostgreSQL 18.6 | **6 explicitly executed Rust integration entries passed**: 83 acceptance scenarios; 45 cancellation cases; 4-request barrier race; storage/probe guards; alias cancellation; 12 real-driver COMMIT/socket-cut trials |
| PostgreSQL 17.11 | **The same 6 entries passed**, with the same 83/45/race/guard/alias/12-cut coverage |
| PG transport-cut observations | On **each** version: **6 no-journal and 6 complete-journal outcomes**, no partial journal; replacement passes the original chain lock, same-identity retry succeeds, exact readback survives reopen |
| PG statement/immutability guards | Actual 23505 after an earlier write cannot commit that write; privileged runtime rejected; all **18 retained tables** reject UPDATE/DELETE/TRUNCATE; exact accepted state survives reopen |
| PG cleanup evidence | Actual backend PID disappearance; independent primary lookup while the original transaction remains active; fresh-session transaction state observed from a separate connection; negative control detects a deliberately open transaction |
| Standalone ADR 022 TLS/driver proof | **16 parent tests passed**, 0 failed; ambient-environment child runs through its parent (one outer ignored entry); formatting, Clippy, no-default check and 150-package source/pin/trust audit passed |
| Frozen independent audits | All **99 frozen entries**, **60 hash vectors**, **25 accepted records**, **29 manifest members**; Python and Node independently agree, plus schema, negative and arithmetic/authority checks |
| Diff/frozen-path checks | Whitespace clean; no frozen contract/fixture/design-source changes; original ADR bytes preserved |

The 11-case SQLite basics test overlaps the 83-case suite; it is not counted as
11 additional acceptance scenarios. Final default workspace output has **8
ignored entries**: six opt-in PostgreSQL tests, all separately executed on both
versions, and two explicit later fake-destination/outbox gates. The stale
acceptance/cancellation/race placeholders were replaced by actual facade runners.
The TLS child is separately accounted for above.

SQLite's independent barrier test deliberately creates a second test-only driver
pool under the same retained OS owner to force actual database contention.
Normal production construction still has one writer pool; its separate original
12-request storage concurrency test also passes. PostgreSQL race evidence uses
real separate backend PIDs and observed lock-timeout responses. The testkit
rejects fabricated/sequential overlap evidence.

No semantic or atomicity test failed during this resumed integration. Compile
plumbing errors were corrected before execution. A new dependency audit initially
mistook Cargo metadata's weak optional SQLx edges for active MySQL/PG drivers;
it was corrected to audit activated normal/build dependencies across all targets,
while still rejecting those drivers and prohibited TLS providers if activated.
Evidence review also replaced a self-observing PG transaction probe with the
independent observer/negative control, and reran affected suites on both servers.
No assertion, fixture or required outcome was weakened to obtain a pass.

## Local runtime and reproducibility

Only the isolated `ledgerlab-phase1` Colima profile and loopback-published local
containers were used. No home mounts, SSH-agent forwarding, ambient registry
credentials, paid service, cloud account, sudo or acceptance of system legal
terms was required. Tests used ephemeral private-CA certificates and synthetic
credentials, kept outside committed files. Successful test cases remove their
own isolated databases. The local containers and VM are stopped after validation.

| Server | Official image digest |
|---|---|
| PostgreSQL 18.6 | `sha256:86c951e05bf56c93d95d397747fb8820ac76cc3bedb78f43abd83eedbe3666ae` |
| PostgreSQL 17.11 | `sha256:f4c66b820c6f974249089d3d16d86a3698eae11e8746eb6644b2271031e91232` |

Detailed logs are intentionally ignored operational evidence under
`work/phase1-resume/`: `final-check.log`, `final-build.log`,
`final-pg18-*.log`, `final-pg17-*.log`, and `final-tls-proof.log`.
The PostgreSQL adapter README gives explicit local test prerequisites. With
those supplied, its six real-server entries can be run using:

```sh
cargo test -p ledgerlab --all-features --locked --offline service::pg_tests:: -- --ignored --nocapture
sh crates/ledgerlab/src/store/postgres/proof/run.sh
sh scripts/check.sh
```

## Remaining gates and limits

- The frozen first completion acceptance slice is complete on SQLite and has
  matching real PG18/17 evidence. The broader Phase 1 exit still needs an
  established/tested MSRV and actual required native pipeline runs; the verified
  development compiler is not an MSRV. macOS ARM64 clients and Linux ARM64 test
  servers do not certify the full supported native matrix.
- No power-loss or OS process-kill durability certification, backup/restore,
  restore drill or production deployment was performed. Request cancellation,
  driver/socket cuts and ordinary reopen are the executed failure modes.
- Public-root exclusion is the reviewed deterministic exact-anchor proof;
  a real public-CA-issued server handshake is not claimed. A fresh release-time
  vulnerability/provenance/license process remains a release gate.
- Durable pending promotion, later authority/claim/reversal/invocation families,
  fake destination/outbox reconciliation, export/restore/cutover, generated
  transport surfaces, performance certification and public-v0 release work remain
  in their assigned later phases. Missing-chain Waiting is not durable pending.
- Initializer/migration owner seams are private; no operator CLI or HTTP surface
  has been added. No publish, push, tag, deployment or remote registration occurred.
