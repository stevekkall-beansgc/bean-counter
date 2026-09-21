> Historical engineering-phase report. For the current owner-roadmap Phase 2
> combined candidate and exact gates, see [Product Phase 2 integration](PRODUCT-PHASE-2-INTEGRATION.md).
> Counts and deferred-work statements below describe the earlier integration only.

# Phase 2 integration candidate

21 September 2026. Work is confined to the supplied
`ledger-lab-v0-phase2-integration` worktree on `codex/phase2-integration`, based on
`dce3ec4feda4025ab6e98ef23608b9e6f812aeb1`. This is a compiling, tested integration
candidate. It is not complete Phase 2 persistence or a released product. No merge
to main, push, publication, deployment, cloud service or paid infrastructure.

## Integrated history

The four reviewed commits were cherry-picked in the assigned order:

| Lane | Reviewed source commit | Integration commit |
|---|---|---|
| Pure core | `ab21609a3b2c341b15496cfd1220b6475d00c8fb` | `e88906b265c9b02c0c5af2a2618e429c8a4dae9b` |
| Independent oracle | `3ad2e57e882a33e20528e43f300281c3407aaf6b` | `4f42da5c32c50fcd1f884c9609c097acb5eeba78` |
| Outbox/recovery | `df9ac58e0a07727ae2ba3feb507d0cd27aa0fc61` | `473d2da06f1c02cb6e536fd55a2e2094285a6388` |
| Developer CLI | `1e3e9b09ed42b85340d6fd2d80110a2e58b433f1` | `2e6fa2f748f4d5c91e8642c291a175ef14735bf4` |

Only the CLI cherry-pick conflicted: adjacent facade module declarations in
`crates/ledgerlab/src/lib.rs`. Both `pub mod local` and `pub mod outbox` were
retained. SQLite's `inspect` and `outbox` modules merged automatically. No economic
semantics or expected result was selected through conflict resolution.

The final integration commit adds the comparison adapter, cross-lane regression
tests, explicit CLI boundary text and current documentation. It changes no
production pricing equation, oracle expectation, frozen contract or original
migration. The only additional dependency is a test-only edge to the already
pinned `serde_json = 1.0.151`; no new version or production crate was added.

## Implemented and connected

| Surface | Actual integrated behavior |
|---|---|
| Pure Phase 2 core | Typed `Bundle::compile/evaluate` and `reverse`: linked pricing, supplier obligations, funding/tier predicates, cap/share, exact reversal and bounded authority inputs. Returns `Evaluation`, not a receipt or appendable store plan. |
| Independent proposal | 29 unfrozen synthetic histories and Python reference remain separate from production. Quality semantics remain explicitly proposed. |
| Comparison adapter | Test-only input conversion and semantic projection compare 21 aligned histories: 48 successful evaluations, 18 refusals/waits and three kernel duplicate guards. All 29 histories have an explicit disposition. |
| Outbox | The one facade coordinator uses concrete SQLite/PG transactions to lease/fence, dispatch, retain attempts, reconcile unknowns and hold/resume existing intentions. No economic rerating. |
| CLI | Native `init --demo`, file/stdin `accept`, noncommitting `preview` and stored `explain` on SQLite. Continues to use the Phase 1 facade and frozen generation example. |
| Shared storage | New databases install backend schema 2 using unchanged 0001 plus additive 0002. Logical economic schema remains 1. Normal open verifies both checksums and does not migrate. |

The comparison checks complete common postings (roles, book, binding, amounts,
bases, reversals), exact explanation rationals, obligations, dependencies and
per-step state; it rechecks immutable history after each step. Its alias, time,
ordering and refusal-code mappings, synthetic authority assumptions and all
exclusions are documented in
[the adapter README](crates/ledgerlab-testkit/tests/phase2_core/README.md).
It never supplies expected values to core input construction, imports the Python
oracle into production, or claims to compare canonical bytes.

Two proposal histories check compile rejection separately. Quality/version/
missing-assent histories and two evidence/source-specific steps are excluded with
reasons. Proposed duplicate receipts are not forged by the adapter: the pure core
must refuse an existing claim, while the future coordinator must resolve the
original receipt before evaluating. Original Phase 1 retry/receipt guarantees
remain verified on both stores.

## Unresolved semantic decision — stopped at the boundary

The two negative-outcome proposals are different and were preserved separately:

| Question | Pure-core typed proposal | Independent reference proposal |
|---|---|---|
| Trigger | Approved acquisition with an explicit publication or optimization matcher | New `proposal.quality_failed` with `quality_of` targeting work |
| Basis | Named predecessor's original rounded base/premium component, accounting for live reductions | Target's original booked retail net |
| Identity/limits | Existing acquisition claim authority and component discount capacity | Nominated quality source and at most one quality adjustment per target |
| Cap composition | LinkedDiscount plus closure cap fails compilation | Quality in a staged chain rejects `QUALITY_STAGE_UNRESOLVED` |

Neither proposal is a new accepted v0 event/DSL family. This integration does not
rename quality to acquisition, reinterpret failed work as a payable adjustment,
choose either basis, or enable a discount/cap composition. Failed completions
retain zero-action semantics. Public trigger/relation, claim namespace, eligibility
window, discount basis/capacity, reversal interaction and cap-stage behavior need
an explicit reviewed decision. The current stop is specific to that extension;
the aligned nonconflicting work is integrated and tested.

## Why Phase 2 cannot yet be saved or previewed by the CLI

The canonical addendum freezes the completion-only first slice. The existing
facade preparation, `ValidatedPlan`/record assembler and store append families do
not accept the new typed `Evaluation`. The following reviewed definitions are
missing, so there is no speculative persistence bridge:

1. Canonical acquisition, supplier, link and reversal facts and identities;
   expanded snapshot documents/purposes for accepted bindings, source grants,
   offers, assent, delegation, invocations and retained evidence. Preserve the
   existing seven snapshot purposes and original completion bytes.
2. Exact encodings for new actions, explanations (including explanation-only
   economic dependencies), source/link edges, allocations, obligation intentions,
   complete manifest membership and receipt references. Proposal aliases are not
   these encodings; a cost observation or allocation must never become a payable.
3. Invocation authorization, reservation consumption/release and expiry records;
   stage definitions/closure transitions and complete expected-input sets; unique
   claims and full reversal closure/once-only guards, with immutable control audit.
4. A verified retained-record decoder into historical core values. `Input.history`
   currently takes `Evaluation` values with private action fields. Reopen must
   recover booked facts, not rerate old history with current policy.
5. The coordinator bridge that resolves complete pinned history/evidence under
   ordered admission, authority, binding, reservation/invocation and chain locks,
   checks actual assent/delegation and current permissions, evaluates once and
   atomically assembles/persists the entire decision on both stores. Receipt and
   semantic-identity lookup must still precede current economic evaluation.

Until these definitions are reviewed, local commands expose the generation demo.
A valid linked event returns `UNSUPPORTED_SLICE` with an explanatory message;
accept and preview leave every database cell unchanged. The help, generated demo
README and quickstart say this directly. No transport-side rater was introduced.

## Outbox and CLI coexistence; operational limits

Acceptance still writes the original held, zero-attempt 80-atom intention. A new
CLI integration test accepts it, releases the dispatch hold through the library,
loses a fake response, explicitly pauses, reconciles it to delivered, then reopens
through the CLI. The original receipt/explanation, one attempt and one independent
fake receipt survive; duplicate/preview leave every database cell unchanged.
Separate PG preview tests preserve all cells before and after fresh, identity,
semantic-alias and unsupported previews, then reopen the same ledger.

`fake::MemoryDestination` retains receipts independently of ledger transactions
only while that in-memory instance survives. It is not process-durable destination
storage. Restore simulations preserve that instance; they do not certify a real
backup product. A normal open cannot detect an externally restored copy; the
explicit `open_restored_*` path persists a dispatch hold and invalidates mappings.

Normal open rejects old backend-schema-1 stores. Explicit migration-owner upgrade
orchestration is still missing. PostgreSQL uses row leases/fencing; the design's
additional long-lived advisory ownership session and connection accounting are
not implemented. CLI dispatch stays disabled/held; there is no exporter command,
scheduler, background worker or payment adapter.

Further deferred operations: isolated durable fake SQLite/PG storage and restart
adapters; complete backup verification/publication; portable export, import,
cutover and rollback rules; paginated inventories beyond the bounded scan;
full process-kill/power-loss/disk-full tests; native target/release matrix and MSRV.
No release, platform, performance or production-readiness certification follows
from the local tests below.

## Executed validation

Prepared macOS ARM64, actual Rust 1.98.1, locked/offline Rust dependencies and the
existing Python audit environment. Detailed logs, toolchain wrappers, credentials,
certificates and databases are excluded from Git.

| Check | Result |
|---|---|
| `scripts/check.sh` | 110 Rust tests passed, zero failed; 13 explicitly ignored in the default run |
| Opt-in real PostgreSQL 18.6 (`180006`) | 11/11 passed |
| Opt-in real PostgreSQL 17.11 (`170011`) | 11/11 passed |
| Standalone PostgreSQL TLS proof | 16 parent tests passed; its ignored child entry is invoked by the parent with a synthetic environment; fmt/Clippy/no-default/source audit passed |
| Core/proposal bridge | 2 tests passed; 21 histories and counts above; no expected values changed |
| Native CLI integration | 10/10 passed, including held outbox/reopen and unsupported linked-event no-write regressions |
| Frozen and source audits | 99 files, 60 hash vectors, 25 new immutable first-slice records, 29 manifest members and original 80 atoms preserved |

Of the 13 default ignored entries, 11 are the opt-in PG tests executed separately
on both majors; two remain genuine gates for process-durable fake destinations.
The reference's 13 Python tests validate all 29 proposed histories/86 submissions
and six arrival permutations independently. These are additional semantic tests,
not persisted Phase 2 decisions.

Real-store coverage includes 83 acceptance cases and 45 main-path cancellation
points per backend, alias cancellation, actual barrier contention, distinct
decisions, snapshot reuse, zero-net intentions, 21 immutable-table guards,
12 driver commit-cut trials and 12 production-supervisor transport-cut trials
per PG major. Ten outbox histories per backend preserve economic bytes and original
receipts through delivery faults, fencing, retries, mismatch/orphan reports and
reconciliation. SQLite also exercises closed-snapshot restore and a real
post-snapshot orphan intention with the same independent in-memory fake.

All formatting, warnings-denied Clippy, no-default-feature compilation and resolved
dependency/source boundaries pass. The frozen contracts, fixtures, design sources,
ADRs and both original 0001 migrations have no diff from the base. `git diff
--check` passes. Development failures were confined to the new test harness: a
self-dependency during history reprojection and an outbox test timestamp preceding
the CLI receipt's due time. Both were fixed without changing production economics
or weakening expectations.

Reproduce using the supplied local activation and audit environment:

```sh
source work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
scripts/check.sh
cargo test -p ledgerlab-testkit --test phase2_core --locked --offline -- --nocapture
# Set explicit synthetic localhost PG test port, major, password and CA.
cargo test -p ledgerlab --all-features --locked --offline service::pg_tests:: -- --ignored --nocapture --test-threads=1
sh crates/ledgerlab/src/store/postgres/proof/run.sh
```

PG runs use the existing free `ledgerlab-phase1` Colima fixture and query/assert
the intended major at every initializer. Both dedicated test containers and the
VM were returned to their original stopped state. Nothing connects to cloud
infrastructure.

## Exact next work

1. Review the discount decision table above. Either keep that family deferred or
   explicitly select/version its semantics, including cap and reversal behavior;
   independently amend proposal expectations only after that decision.
2. Author and independently verify the canonical/persistence extension for the
   aligned Phase 2 families, preserving every Phase 0/1 byte. Assign evaluator and
   record versions before exposing new wire input or stored output.
3. Implement the verified history decoder, shared coordinator/assembler bridge
   and separate SQLite/PG migrations. Add real-store full-record parity,
   before/after every-write, unknown-commit, cancellation, closure/reversal and
   authority/reservation race tests before enabling those families.
4. Then connect CLI accept/preview/explain to those accepted facade types and add
   the linked onboarding examples with independent expected receipts. Keep
   `LocalLedger` a thin host/formatting boundary.
5. Separately finish process-durable fake storage, explicit schema-1 upgrade,
   scheduling/PG advisory ownership and full backup/transfer workflows. Run the
   two currently ignored delivery gates only with actual restart evidence.

Original lane reports remain provenance, not a claim that their older base-specific
test counts or absence statements describe this integration candidate.
