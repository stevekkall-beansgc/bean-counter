# Phase 1 independent testkit

This crate provides a read-only frozen-journal oracle and reusable conformance
runners. It does not implement or simulate a production backend. Real persistence,
concurrency, cancellation, delivery, TLS, and cross-store parity remain **gated**.
Nine ignored tests in `tests/integration_gates.rs` make those gates visible; running
those placeholders with `--ignored` fails deliberately.

## Executed and gated matrix

| Coverage | Executed here | Becomes live after integration |
|---|---|---|
| Fixture integrity | All 99 freeze entries; 15 artifact digest entries covering the other first-slice files; canonical JSON/JSONL framing and exact inventory | Already live on every oracle load |
| IDs and canonical bytes | All 60 framed vectors; independent scoped identities, document/effect/claim projections, 25 new immutable rows, 29 manifest members, receipt hashes | Actual persisted journal + duplicate indexed columns compared against these expectations |
| Arithmetic | 15 Fraction/integer vector groups, +100/−20 = 80 derived from retained policy, rounding symmetry and allocation conservation | Actual actions, explanations, payload, receipt and row deltas |
| Strict input profile | Raw malformed JSON, duplicate/nested keys, UTF-16 ordering, no Unicode folding, decimal bounds/idempotence | Production rejection with no row/identity residue |
| Harness assertions | Missing/extra/rewritten rows, altered indexed columns/bytes, missing tables, wrong heads/state, missing/multiple failpoint hits and fabricated non-overlapping race traces are rejected | Already live; these self-tests do not exercise a store |
| Zero completion oracle | Transient structural sample passes; paid journal, missing receipt and changed event fail | Failed completion creates 16 immutable rows, one FAILED_WORK explanation, revision/receipt and zero effects/actions/intentions |
| Fresh, identity duplicate/conflict, semantic duplicate/conflict, quantity normalization | Runner compiled, not executed against a store | 6 cases per backend |
| Invalid inputs | Runner compiled; independent parser checks execute | 11 cases: unknown, null, request duplicate key, eight frozen raw negatives |
| Authority | Runner compiled only | Unauthorized source/principal/receipt read and missing real accepted terms: 4 cases |
| Evaluator rollback | Runner compiled only | Invalid discount and overflow after base: 2 cases, no partial base |
| Writes | Complete schedule expanded and self-tested | 54 cases: before and after every one of 27 item writes |
| Commit and response | Runner compiled only | Pre-send rollback/explicit unknown, lost reply, unknown durable and absent branches, live transaction with absent primary lookup: 5 cases |
| Zero-action accept | Independent structural oracle only | 1 acceptance case |
| Cancellation | Runner compiled only | Every adapter await, all 27 write items, error/rollback paths, commit phases, pool probe, resolution and reopen |
| Concurrent identical accept | Trace validator self-tested only | File SQLite and two-connection PG barriers; exactly one Accepted, others identity Duplicate; one revision |
| Fake lost response | Runner compiled only | Durable remote receipt 80; local unknown survives reopen; reconciliation delivered with same key/payload/receipt |

`acceptance_cases()` returns **83** cases. Cancellation, race, and delivery runners
are additional suites. PostgreSQL 17 is also an explicit pre-public-v0 gate.
No backend runner was invoked with a mock to substitute for these missing results.

## Independent oracle

`oracle/oracle.py` uses Python's standard library only: strict JSON hooks, a
UTF-16-key-ordered integer-only canonical serializer, `hashlib.sha256`, and exact
`Fraction`/integer arithmetic. It imports neither production Rust calculations nor
the repository's contract-audit modules. Rust uses only its standard library and
invokes the Python helper with `-B`. No network, package installation, new Cargo
dependency, lockfile modification, or paid service is required.

`FixtureOracle::load(root)` verifies the freeze, file inventory, byte digests,
framing, identities, fact projections, arithmetic, and complete manifest before
returning any expected data. A disagreement is a hard failure and stop condition.
Expected successful first-slice bytes are read from the frozen journal, not rebuilt
from production output. The helper never writes or regenerates a frozen fixture.

The helper's compact tab/hex protocol carries original bytes without escaping
ambiguity. `FixtureOracle::workspace()` uses this checkout's fixture root. The
parser/normalizer is deliberately limited to oracle inputs; it is not a replacement
for the production parser, complete schema validator, evaluator, or 512-bit runtime
arithmetic implementation.

Zero-action completion uses the existing frozen completion encoding: independent
E/C/D/R/ref IDs and hashes, identical decision snapshot inputs, no effect/action/
intention rows, one FAILED_WORK explanation, exact manifest closure and receipt.
There is no frozen byte-for-byte FAILED_WORK display-input golden. The verifier
checks its required semantic fields and bounded supported input/reference shapes,
then preserves its original bytes for hashes and reopen/duplicate equality. It does
not invent a new frozen fixture, record variant, or a rule about unspecified display
inputs. Full first-slice acceptance always requires the exact frozen bytes.

## Exact integration interface

Implement these test-only traits from `src/stores.rs` in the owning integration
lane. They wrap the public production facade and concrete backend, not a new
production transaction abstraction. The adapter may own a test runtime and bridge
these synchronous test methods to async facade calls; the facade must not create a
nested runtime. There are no successful default implementations or silent skips.

| Trait / method | Required behavior |
|---|---|
| `BackendFactory::seeded(oracle)` | A unique real file SQLite or isolated real PG database/schema; seed exactly the six documents, four seed records and preseed state. Precreate all needed scope locks. No accepted records seeded. Dispatch held/stopped, diagnostics and projection writes disabled. Preserve initializer rows across tests. |
| `BackendFactory::real_without_terms(oracle)` | Separate real-mode store with an authenticated authorized source but no valid accepted real binding. Demo assent cannot authorize acceptance. |
| `AcceptanceBackend::evidence()` | Actual kind, SQLite path/redacted PG identifier, queried version text/number and durability settings. SQLite must be a real file with linked version >=3.51.3; server numbers must identify PG18/17. No credentialed URLs in reports. |
| `accept(command, injection)` | Submit the exact raw bytes through the shared facade with typed principal and fixed received time. Map public results to `Outcome`; never collapse unknown into rejected/rollback. Return canonical stored receipt bytes, duplicate/conflict classification and actual hook hit. |
| `observe()` | Read committed persisted data, independent of the plan/expected values, into `Snapshot` below. Detect duplicate database keys before constructing maps. |
| `reopen()` | Close all original pools/connections/SQLite owners, await bounded cleanup, then reopen the same durable store. No in-memory reconstruction or reseeding. |
| `resolve_and_retry(command)` | Resolve on authoritative primary under original scoped identity/locks; retry identical bytes. Release/drain any test-held old commit. Return Accepted or original identity Duplicate once known. |
| `probe_active_commit(command)` | While old transaction is test-held and still active, prove primary rows absent and service result remains original-identity OutcomeUnknown. The bounded barrier must not become an unbounded detached commit. |
| `pool_probe()` | Query/probe next connection for an open transaction. Report actual affected/next connection IDs and whether the uncertain connection was discarded. Never hardcode cleanliness. |
| `cancellation_points()` / `cancel()` | Return complete production await-hook catalogue, unique names, `AwaitClass`, commit phase and trigger for error paths. Trigger requested stage, cancel there, and report exact hit/result. Include begin, lock, read, each write name/item, commit, rollback and cleanup. |
| `zero_evidence()` | Read stored event/receipt, reason codes, actions/intentions and chain counters for the failed-work case. No evaluation. |
| `RaceBackend::concurrent_identical()` | Bounded barrier-driven simultaneous calls on real connections; globally ordered trace of request/transaction/lock/commit events. PG must show every participant transaction and a blocked competing connection before winner commit; SQLite must show blocked BEGIN on another connection. Sleeps alone fail. |
| `DeliveryBackend` methods | Enable fake dispatch/release hold, lose reply after remote durable commit, read unknown, reopen local and destination storage, reconcile then reconcile again. Report one remote receipt with exact I, payload digest/bytes and 80 atoms. Runtime fake remains in facade outbox. |

`Command` always carries raw event bytes, `Principal::{DemoApp,NoSubmitPermission,
NoReadPermission}`, and `2026-09-20T14:00:00.000000Z`. Authentication belongs to the
fixture adapter; no credentials are passed to the oracle or stored in its report.
Errors are `HarnessError`, not a test skip. Service faults are classified `Outcome`
values; unexpected infrastructure failures should return an error and fail the case.

### Snapshot contract

Every `Snapshot` has five independent readback views:

1. `journal`: original immutable canonical envelopes, including seed records,
   ordered by kind UTF-8 then JCS(id) UTF-8. It has 10 seed rows and 35 post-accept
   rows; receipt and manifest are included. Later aliases are not journal members.
2. `indexes`: canonical projection objects returned by `indexed_projections` in the
   Python oracle, in the same row order. Envelope identity/scope, content hash,
   schema version and original canonical byte hex come from actual stored columns.
   `columns` covers §12 duplicates, including event ingress bytes/hash, claim facts,
   decision and chain fields; action money/roles-doc/snapshot; effect match bytes;
   manifest decision hash; every join key. The adapter MUST read these columns
   directly, not derive them from parsed canonical body JSON. Optional SQL NULL is
   omitted; counters are canonical strings, versions/scales/ordinals integers.
3. `state`: canonical preseed-state shape with current chain substituted. Initial
   and successful acceptance values are read from frozen state files. Enabling
   delivery changes dispatch_enabled to true and dispatch_hold to false.
4. `operational`: `None` before acceptance, otherwise frozen post-acceptance-state
   shape. Zero completion has the same received/observed times and updated chain,
   but no delivery_state member. Regular acceptance must exactly retain held state,
   zero attempts/generation and fixed time. Alias records are separately retained
   with canonical ingress/hash, original receipt, full identity and observed time.
5. `rows`: complete physical table inventory, including empty tables and any
   adapter-specific tables. Map every primary key to a deterministic lossless
   encoding of **all persisted columns**, stable for that backend. Actual SQL
   NULL and byte values must remain distinguishable. Include migration/principal/
   infrastructure rows; preserve them exactly. Never discard unlisted tables or
   columns. No production/raw-action write API is required to observe them.

The harness requires exactly the specified row additions, preservation of all old
rows, exactly one changed chain row, and no unlisted table changes. Journal/index/
state projections check expected values; full physical inventories prove exact
reopen equality and catch unintended writes, including previously unknown tables.
The raw inventory may differ across dialects; canonical journal/index/state must not.

Semantic retry adds one operational delivery-key alias only. It then retries that
alias and requires identity Duplicate with no extra rows. For delivery, only
installation, delivery_state, dispatcher_head, dispatch_attempts,
delivery_observations, and isolated fake_receipts may change; economics and all
other tables remain fixed. Destination storage may be a separate file or isolated
PG namespace and must survive local reopen independently.

### Fault hooks and schedules

`FixtureOracle::write_boundaries` expands `fixtures/failures/first-slice.json` in
frozen order. Item indexes are zero-based. There are 27 writes and 54 positions:

| Write | Items |
|---|---:|
| snapshot_document | 1 |
| snapshot_refs | 7 |
| event | 1 |
| original_delivery_key | 1 |
| claim | 1 |
| effects | 2 |
| actions | 2 |
| action_sources | 2 |
| action_dependency | 1 |
| explanations | 2 |
| intention | 1 |
| delivery_state | 1 |
| control_transition | 1 |
| chain_head | 1 |
| chain_revision | 1 |
| manifest | 1 |
| receipt | 1 |

`Boundary::Write {name,item,edge}` fires around each real write. No batching may
hide an item. A supplied injection must fire exactly once. The before/after write
runner requires confirmed rollback, exact seed on reopen, a clean pool borrower,
then successful same-ID acceptance. Evaluator invalid/overflow hooks fire after the
base calculation and before the invalid discount completes, through the shared
evaluator boundary; no successful base-only record is permitted.

`BeforeCommitSend` permits confirmed rollback or explicit unknown when the driver
cannot prove cleanup; its controlled fault must leave exact seed after drain.
`AfterCommitAcknowledged/LoseReply` requires committed journal and original
identity duplicate after reopen. `UnknownCommit {durable}` suppresses acknowledgement
and primary resolution until first unknown response; the fault controller then
releases the connection into the selected committed/absent branch. Both must settle
to exactly one journal. `UnknownCommitActive` independently guards against treating
absence while the first transaction remains live as rollback.

Cancellation coverage derives from the production catalogue and fails if any write
item/class/commit phase is missing. The adapter owner must audit all actual awaits
against this catalogue; the testkit cannot discover uninstrumented code. Before
commit means exact rollback, after acknowledgement means complete journal, in-flight
means complete-or-none plus correct unknown classification/discard and eventual
same-ID resolution. Never count a named hook without observing its actual hit.

## Validation and local reproduction

Executed on the provided macOS development toolchain, Rust 1.98.1. Its installed
local channel is named `stable-aarch64-apple-darwin`; selecting it explicitly avoids
an attempted download by the root numeric selector. The verified binary still
reports **1.98.1**. No toolchain file/cache was edited.

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
cargo test -p ledgerlab-testkit --locked --offline
sh scripts/check.sh
```

The shared check runs fmt, workspace tests, clippy with `-D warnings`, no-default
feature checks, resolved/source boundaries and Python/Node contract audits. Result:
**7 Rust harness tests passed, 8 Python self-tests passed, 9 integration tests
ignored**. Contract audits independently pass 23 schemas, 19 additional negative
checks, 5 economic journals, 13 authority scenarios, 15 arithmetic groups, 60
vectors, 25 accepted rows, 29 members and all 99 frozen files.

The PG lane separately reported commit
`530b3e9b006200a1fcdbfd54110d04c2078a6667`: its adapter/migrations were not implemented
because SQLx's supported Rustls path did not meet PEM-only trust. A reviewed driver
ADR and real PG18/17 installation remain required. This testkit has not independently
rerun those TLS diagnostics, and reports no PG database/cancellation/race evidence.

Integration calls `run_case` for every entry of `acceptance_cases`, then
`run_cancellation`, `run_race` and `run_delivery` for each required real backend.
Remove an ignored gate only when it actually invokes that runner and retains the
result/provenance. Cargo's default test harness does not support a custom
`--backend` argument; no such nonfunctional command is advertised here.
