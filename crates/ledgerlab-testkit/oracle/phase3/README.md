# Phase 3 independent lane

Integration update: [the durable lifecycle bridge](DURABLE.md) now observes the
real four-step coordinator path after reopen on SQLite and PostgreSQL. It uses
the independent numeric and economic validators and compares exact stored bytes
across backends. The older factory runner described below remains unconnected;
its historical blocker is not used to claim the new bridge covers all histories.

Update: the owner has resolved the reservation semantics. See [independent numeric
lifecycle expectations](LIFECYCLE.md), authored before coordinator fields. The
prior stop report below remains historical; exact encoding and real-store proof
are still pending. No frozen file has been amended.

## Historical stop report

Based on exact reviewed Phase 2 commit
`6194376a053b8a27887a9b09459054a7af3a1769`. Test-only analysis and an **unconnected
adapter runner**, not Phase 3 completion or persistence evidence. The owner
halted the lane after the coordinator identified a missing frozen transition.

## Exact blocker

The frozen `2-candidate.4` record family has no outcome invocation/reservation
consumption or release transition. `binding-snapshot.supplier_invocation.held`
and retained `base-evaluation` consumptions describe historical observations;
`limit-evidence` describes before/after claim aggregates. Neither encodes an
outcome's atomic reservation change, transition identity, expected-current
reservation guard, or release. The schema's `base-consumption` is retained base
material, not authorization to invent an outcome record kind. A frozen snapshot
cannot prove current capacity conservation under concurrent writes.

Consequently these proposed persistence histories cannot be represented as
complete expected journals using the frozen contract:

- Supplier base consumption/contingent hold followed by outcome consumption or
  release and correction, including zero and reversal without replenishment.
- Supplier capacity races with authority revocation or base reversal, requiring
  an atomic outcome reservation transition and exact expected-current guard.
- Partial-write/cancellation/unknown-commit histories that include that missing
  transition: there is no frozen complete post-state or full write membership.

No new transition, schema, ID, hash domain, fixture encoding, or production
behavior has been created. `run_history` and `run_boundaries` explicitly refuse
`supplier-separation`. Its existing frozen arithmetic is tested only as contract
analysis. Retail aggregate projections likewise do not certify reservations.

## Valid retained work

`conformance.py` independently reconstructs the existing frozen bytes using the
reviewed Python reconstruction code, compares them byte-for-byte, and verifies
them against the original fixture base root. It does not import a production
evaluator for expectations. It covers five existing synthetic variants of the
same base/outcome/correction path: replacement, correction to zero, initially
zero, full reversal/reinstatement, and supplier separation (analysis only).
Frozen source labels and all contract files remain unchanged.

`test_conformance.py` tests exact inverse/replacement math, zero-net actions
without an intention, accepted zero claims, permanent claims, original receipts,
aggregate separation and assertion sensitivity to every missing row of every
accepted prefix. These are oracle self-tests, not black-box product tests.
The Rust test entry invokes them in ordinary workspace checks.

The unconnected runner provides acceptance/reopen/retry/alias/current-guard
scenarios and per-item error/cancellation injection. It has **not** been run
against either adapter. It does not implement authority/base-reversal races,
active-commit absence probing, or proof of a complete await catalogue. Do not
count its definitions as executed cases or use it to close the roadmap gate.

## Owner integration hooks (after contract blocker resolution)

Import `conformance` with this directory on `sys.path`; call
`run_history(factory, name)` and `run_boundaries(factory, name, decision_index)`.
The Python API is test observation framing only, never a proposed wire/storage
format. The production adapter may bridge to Rust through a local helper.

Factory must create a fresh isolated file SQLite or local PG17/18 database,
register synthetic terms/authority, and accept base work through the coordinator
using retained original inputs. It must **not** seed a golden accepted journal.
`with_prefix` accepts preceding commands normally. Clock/principal/evidence are
injected at the exact frozen observations. Acceptance receives only the event
body; admission observations must come from host verification, never from caller
flags. No output oracle bytes are passed to acceptance.

Adapter methods: `evidence`, `observe`, `reopen`, `accept`, `close`; boundary
runner additionally requires `boundaries`, `inject`, `drain`, `pool_clean`,
`resolve_and_retry`. `observe` returns original `journal_utf8` envelope strings
sorted by frozen row order, original ordered `receipts_utf8`, `base_anchor` read
from durable base acceptance, `claim_heads`, `binding_totals` (premium/discount
atom strings by binding), operational `aliases`, and `rows` (ALL physical tables,
including empty tables, mapped primary keys to lossless all-column values).
Never reconstruct observed bytes or indexes from an acceptance plan. Reopen must
close all owners/connections and reopen the same database. Physical comparisons
are within one backend; compare canonical journal/receipt fields between backends.

Replies are test categories `{status, receipt_utf8}`; absent receipt is `None`.
Translate actual facade errors into `guard_rejected` only for revision guard
failure, not arbitrary rejection. Duplicate categories distinguish identity and
claim. Boundary tuples are `(site,item,edge,phase)` with before/after edges and
precommit/commit phase. Instrument each SQL loop item and every await; verify
completeness independently of this adapter-supplied catalogue. Report exact hit,
drain all work, query pool cleanliness, then resolve/retry the original identity.
A missing row during an active commit is never rollback evidence.

Before any conformance claim, also add independently checked operational row
deltas/alias-only writes, authority and reversal barrier races with actual lock
overlap evidence, authenticated/scoped receipt lookup, active commit probes,
crash-process tests, reservation transitions from an approved amendment, and
both-store durable evidence. Current `rows` comparisons detect retry mutations
and reopen drift but do not yet validate all allowed physical acceptance deltas.

## Offline validation

Use the already prepared toolchain/dependency environment documented in
`PHASE-2-SEMANTIC-MERGE.md`; no installs or network are needed:

```sh
python3 -B -m unittest discover -s crates/ledgerlab-testkit/oracle/phase3 -v
sh scripts/check.sh
```

The lane stops at a clean local commit. No merge, push, tag, release, deployment,
registration, cloud access, or spend.

Validation at the stop point: full `sh scripts/check.sh` passed (129 Rust tests,
19 intentionally ignored gates, formatting, warnings-denied Clippy, no-default
compilation, boundaries, Python/Node reconstruction). New oracle checks: 17
self-tests, five histories, 16 decisions, 21 prefixes and 1,130 missing-row
probes. Freeze audit: all 159 pinned files intact, including 99 original v1
pins. New Phase 3 real-store cases executed: **zero**. No PG service was started.
The initial standalone invocation lacked `jsonschema`; rerunning with the
repository's existing offline dependency directory passed without installation.
