# Implementation handoff

Phase 0 freezes input contracts, the complete first-slice canonical record family, economic/authority oracles, and release obligations. It introduces no production parser, evaluator, transaction port or storage behavior. Source snapshots are in `docs/design/sources/`; `source-digests.json` checks their exact bytes. The canonical addendum supersedes the incomplete record encodings. Original stop report `PHASE-0-BLOCKERS.md` is historical.

## Phase 1 ownership

| Lane | Allowed implementation area | Invariant / validation | Stop condition |
|---|---|---|---|
| Core | `crates/ledgerlab-core/src/{wire,domain,canonical,money}` | Strict parse/normalize and exact first-slice bytes, IDs and numeric bounds; independent frozen fixtures | No environmental dependencies; no new DSL/record variant or changed oracle |
| Coordinator + SQLite | facade `service/accept`, `store/ports`, `store/errors`, SQLite modules and SQLite migrations | One acceptance algorithm, tracked immediate transactions, linked patched SQLite, rollback/cancel checks | Agree the private transaction boundary with PG lane before integration; no guessed cleanup behavior |
| PostgreSQL + TLS | facade PostgreSQL modules and PostgreSQL migrations | SERIALIZABLE plus ordered scope locks; primary unknown-commit resolution; public/PEM-only trust tests | No weaker isolation, default Prefer, ambient roots, or adapter economics |
| Independent testkit | testkit fixtures/history/failpoint/race runners | Real file SQLite + PG18 same journal, each write boundary, lost response, cancellation, actual overlap | No core evaluator used as expected-value oracle; no mocks substituted for persistence evidence |

The integration owner controls root Cargo files, dependency/toolchain pins, shared ports, contract files and merging. Begin each session in its own Git worktree on an assigned `codex/phase1-*` branch. Agree concrete handle/enum or GAT plumbing in the small driver spike; the design notation is not a frozen Rust ABI. Coordinate ports before store integration, then converge on the same full 80-atom decision. PG17 joins before public v0. Rust tests that need CLI behavior invoke the built `ledger` binary; testkit need not depend on a nonexistent CLI library target.

## Contract ownership

`canonical-records.schema.json` and the addendum are immutable revision-1 reference artifacts. Input wrappers reuse their definitions. JSON Schema establishes shape only: implement the additional byte/range/timestamp/topology/authority/order validators. In particular, the repair schema's `event-id` definition is structurally broad because compact external event IDs share it; internal EventId fields MUST still match `ev_` plus 64 lowercase hex under §6 and are checked independently in the audit. Never confuse this structural permissiveness with permission to accept arbitrary internal IDs.

Only the completion snapshot/chain-transition record family is frozen as exact full journal bytes for Phase 1. Acquisition/link/reversal claim-facts and invocation/reservation transitions need their own reviewed encoding extension before later phases implement them. Their financial semantics already appear in the detailed design and economic/authority fixtures. This is not permission to invent a variant from analogy.

No empty SQL migration or offline SQLx metadata has been fabricated. Introduce those with the real driver slice and test each dialect. No Rust wire DTO generator is implemented in Phase 0: checked-in schemas are the reviewed contract, and the Phase 1/6 generator must reproduce them rather than redefining them.
