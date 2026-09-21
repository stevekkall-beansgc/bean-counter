# Product Phase 3 bounded integration candidate

Status: validated bounded local integration candidate; independent review and
external host gates remain. No release or Phase 4 approval.

This candidate combines the reviewed reservation-settlement amendment, one
private validating coordinator, independent numeric/economic checks, and concrete
SQLite/PostgreSQL schema-4 adapters. The tested synthetic path accepts one final
base, an ordinary supplier outcome, correction to zero and explicit closure.
Original receipt pairs and canonical history remain stable after reopening.
The real host authority/provisioning implementation and public submission wiring
remain external gates; synthetic evidence is not real-world authorization.

## Exact inputs and bounded integration changes

| Input | Exact local commit |
| --- | --- |
| Reviewed Phase 2 base | `6194376a053b8a27887a9b09459054a7af3a1769` |
| Approved reservation freeze | `1170ddc13e8f4e799d000a842cb978b6aa2adebb` |
| Coordinator and freeze chain | `131584f130fd6777ffa6b4215fa5b3498d926fec` |
| Independent numeric/conformance oracle | `6ae64d14fac96068caa3e0d91a70f5e6ba23143d` |
| SQLite backend, parent final coordinator | `b63b0416648a9ee4e0fcd6e73023dc9be9f29e25` |
| PostgreSQL backend, parent final coordinator | `ca4681338efbf51c6d48f737f3de8c6ca8d34029` |

Inputs were inspected from exact local Git objects, then applied without
intermediate integration commits in coordinator → oracle → SQLite → PostgreSQL
order. No content conflict occurred. Shared types/plan getters are the final
coordinator version; no dependencies or crates were added. Exactly three
production crates plus unpublished testkit remain.

Integration adds test-only durable observers and an independent Python bridge.
Each store accepts through the real private coordinator, closes/reopens after
every prefix, inventories every physical table, checks all canonical envelopes
against direct SQL, and returns original delivery pairs, anchors and heads.
Python validates economic history using the existing independent verifier,
reconstructs settlement IDs/hashes using the frozen codec, and checks reservation
arithmetic with the pre-encoding numeric oracle. A cross-store comparison requires
exact agreement at all four prefixes. Eight negative probes check that the observer
rejects missing records, a changed old receipt pair and changed reservation/claim revisions and missing anchors. No accepted plan or expected journal is fabricated in an adapter.
See `crates/ledgerlab-testkit/oracle/phase3/DURABLE.md` for scope and commands.

The only additional production-file edit corrects the PostgreSQL maintenance
comment to schema 1/2/3 → 4. The imported SQLite shared changes similarly update
its maintenance comment and the CLI's migration count. PostgreSQL's shared
physical observer verifies exact namespace keys **and profile values**, derives
namespace growth from independently expected delivery deltas, and retains
immutable-row checks/default-zero checks for other tables. SQLite instead uses
reciprocal delivery triggers; it is not required to have PostgreSQL's extra table.

## Invariants and evidence interpretation

The ordinary positive 2,500-atom result changes held capacity from 12,000 to
9,500 and consumed from 3,000 to 5,500. Correction to zero preserves the entire
reservation. Closure releases 9,500, leaving held zero and total capacity 15,000.
Original aliases/retries remain receipt lookups; a renamed correction is a new
request subject to current correction guards. New stale/closed/deadline requests
return domain rejection without writes. Unknown commits remain unknown until
an authoritative original-identity retry resolves them.

All 173 saved baseline pins (including all 159 legacy frozen files and original
migrations) remain exact. The reviewed 13 reservation files and five control
artifacts remain pinned. Frozen ROADMAP bytes and old first-slice semantics are
unchanged. Existing 80-atom/25-row/29-member tests remain in the aggregate suite.

The lane write-lock defect was corrected by coordinator `cf136d6`, retained in
`131584f`; SQLite's strict guard was not weakened. Final coordinator classification
fixes were included before both backend handoffs. Full physical inventories
retain backend-specific namespace, scope-lock and held-intention rows where
present; these are not compared as identical schemas across backends.

## Integration gate results

| Gate | Result |
| --- | --- |
| Exact source ancestry/scope and clean lane inputs | PASS |
| Initial combined offline aggregate | PASS: 168 passed, 0 failed, 30 ignored |
| Final combined offline aggregate after integration-owned tests | PASS: 169 passed, 0 failed, 31 ignored; formatting, strict Clippy, no-default/features and architecture checks pass |
| Approved-core semantic comparison | PASS: 217 scalar cases; approved snapshot `1e0ba3f886788c08f427d3aae1d916b341187e76` |
| Real SQLite reopened independent lifecycle | PASS: four prefixes, 82 retained records, eight negative probes |
| PostgreSQL 17.11 integrated coverage | PASS: 28 existing real-store tests in 661.08s; corrected observer focused rerun 1/1 in 9.18s; initial observer error retained below |
| PostgreSQL 18.6 full integrated suite | PASS: 29 passed, 0 failed, 701.35s |
| Exact SQLite/PG17/PG18 durable comparison | PASS: four prefixes, 82 records per store, eight negative probes per store; exact records/receipt pairs/anchors/heads |
| Standalone driver/TLS proof and source audit | PASS: 16 tests; one child entry exercised by parent; 150-package source/feature/license audit |
| Final pins and scope | PASS: all 173 baseline pins, 159 legacy + 13 reservation files + five controls; original migrations/Cargo exact; 20 reservation metadata negatives |

The SQLite suite executes all 360 before/after positions with injected failure
and actual cancellation; a focused evidence run confirms that count. Eight
subprocess kills cover four steps before commit and after durable commit before
reply. Each PostgreSQL major executes 386 validated-plan positions plus 14
primitive positions, all eight outcome COMMIT request/reply cut trials, real
same-identity/distinct-correction/ordinary-close races, and populated schema
1/2/3 upgrades. Exact old or complete new state is checked after rollback/reopen.

The prior lane results are source-input evidence: SQLite 166/0/19, PostgreSQL
146/0/30 offline and 28/28 real tests on each major. They are not substituted for
the combined runs above. Full SQLite write/cancellation and process-kill checks
run in the aggregate; PostgreSQL real suites explicitly execute ignored tests.
The offline ignored entries comprise opt-in PostgreSQL service tests plus two
pre-existing deferred process-durable fake-destination gates. The standalone TLS
proof has a separate child-only ignored entry executed by its parent. The fake
destination gates remain deferred; they are not claimed as passing.

Validation uses installed Rust 1.98.1, linked SQLite 3.51.3 (source identity and
options captured), locked cached dependencies, existing Python contract
dependencies and cached PostgreSQL 17.11/18.6 images. No downloads, remote
services, cloud credentials or spending are required. Per-run logs and observed
JSON stay outside committed source. The final handoff records their hashes. Both disposable integration containers
and their temporary volumes were removed, the initially stopped Colima profile
was stopped again, and Docker context was restored to `default`. Spending: $0.

Commands: unchanged `sh scripts/check.sh`; PostgreSQL majors separately with
`cargo test -p ledgerlab --lib --all-features --locked --offline -- --ignored
--test-threads=1 --nocapture` and explicit synthetic local TLS environment;
`sh crates/ledgerlab/src/store/postgres/proof/run.sh`; approved-core
`compare_semantic.py --source` against the exact saved semantic worktree; and
`durable.py --compare` over the three observed JSON files. The source input and
command/log inventory in the final external evidence report records exact paths.

### Integration-owned observer correction

The first PG17 full-run binary used SQLite's `canonical_bytes` column name for
PostgreSQL's `outcome_records` table. The new observer alone failed with SQLSTATE
42703. The query was corrected to PostgreSQL's existing `envelope` column;
production adapter code and schema were unchanged. The corrected focused observer passes on both PG18 and PG17; the latter ran
after the full suite. All 28 other PG17 tests passed in that original run. Preserve the first
run log and distinguish this test-harness correction from product failures.

## Qualifications and remaining gates

A PostgreSQL diagnostic run overlapping unrelated heavy stress returned
`Unavailable` for a second concurrent closure. The lane's exact test later passed
both full and isolated runs on PG17/18 without enlarged deadlines or error
reinterpretation. The original failure logs are retained; its cause was not
conclusively traced. This is not arbitrary-load throughput or availability
certification. The private coordinator retains its five-second admission budget.

The implemented bridge supports one final base without a predecessor evaluation
chain. Real host authentication/authority/provisioning, public submission and
broader base chains remain external gates. The independent durable bridge covers
one four-step path; it does not execute every numeric oracle history on every
store, connect the older `conformance.run_history` factory, or certify an exhaustive
real authority-revocation/base-reversal/multiple-family race matrix. Tests and
source lock coverage must not be promoted into unexecuted business scenarios.
Independent review of this combined candidate remains separate from self-validation.
Process kills and transport cuts are not host power-loss certification. Native
platform/MSRV/release/export/payment gates remain unchanged. No main merge,
push, tag, release, publication, deployment, service registration or Phase 4 work
is performed by this integration.
