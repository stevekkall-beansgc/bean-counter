# Proposed private coordinator/store seam — review only

No Rust ABI is implemented or agreed by this document. After independent contract
approval, the shared `service::accept` coordinator owns construction of commands,
resolved observations and validated plans. Existing first-slice behavior remains
unchanged. These are proposed exact operation and type names for adapter planning;
no adapter may expose a raw accepted-action append or run pricing/authorization.

## Additive operations on the private acceptance transaction

```rust
// Not public API and not compilation-ready type definitions.
fn lock_scopes(&mut self, scopes: &[OutcomeLock]) -> Future<Result<(), StoreError>>;
fn lookup_outcome_delivery(&mut self, key: &ScopedDelivery)
    -> Future<Result<Option<StoredCompositeDelivery>, StoreError>>;
fn resolve_outcome(&mut self, request: &OutcomeResolve)
    -> Future<Result<OutcomeResolution, StoreError>>;
fn append_outcome(&mut self, plan: &ValidatedOutcomePlan)
    -> Future<Result<(), StoreError>>;
// Reuse existing begin(deadline), rollback(self), commit(self) and CommitError.
```

Actual Rust lifetimes/static dispatch and named Future signatures are settled
with both store owners after review; these operation semantics are the intended
common seam. No new production crate or erased SQL backend is needed.

| Type | Exact required contents / ownership |
|---|---|
| `ScopedDelivery` | Authenticated tenant/environment, source, external label. All economic and reservation-control identities contend in one namespace. |
| `OutcomeLock` | Ordered class enum and full scoped structured key, read/write mode. Classes: admission, authority, binding, reservation, chain/target, claim, binding-aggregate, invocation-consumption, base-reversal. Within class sort canonical scoped UTF-8 keys. Claimed/aggregate subkeys refine the existing chain/stage class. |
| `StoredCompositeDelivery` | Original normalized command bytes and hash, canonical identity or alias target, original economic receipt bytes/ref when present, mandatory settlement receipt bytes/ref for registered targets. The store returns observations; coordinator verifies content and current read rights before disclosure. Closure has no fabricated economic receipt. |
| `OutcomeResolve` | Bounded coordinator-discovered scoped target, invocation, permanent family claim, required immutable refs and all currently held locks. No caller prices or authorization flags. |
| `OutcomeResolution` | `MoreLocks(Vec<OutcomeLock>)`, `Missing(Vec<ScopedRecordRef>)`, or `Complete(OutcomeSnapshot)`. More locks always roll back and restart in total order; missing dependencies reserve no identity or claim. |
| `OutcomeSnapshot` | Original externally anchored base/registration receipts; exact v2 retained record closure and new settlement prefix; complete frozen family/binding/limit membership; original Evaluation/Policy/replay inputs; all claim and aggregate heads; current source/correction/closure authority and immutable documents; nominated invocation, reservation head and base-reversal state; checked index-to-record correspondence. No selected subset or truncation. |
| `ObservedOutcomeHeads` | Exact expected revisions for authority, binding, chain/target, all relevant claim/aggregate heads, reservation, invocation-consumption and base-reversal guard. Observation guards are retained even where no head changes. |
| `ValidatedOutcomePlan` | Private coordinator-only constructor; original economic canonical bundle, additive canonical bundle, exact complete replay/member refs, composite receipts, shared delivery insert or alias, observed guards, proposed head writes. Stores obtain immutable getters for mapping/constraints only. |

A `reservation-transition` proposes a reservation head compare-and-swap and a
complete ordinary-family status change. Post-hoc observations, registration
observations, duplicates and no-op closures do not masquerade as transitions.
Registration atomically creates the initial revision-0 checkpoint with the base
acceptance. Post-hoc writes update only their economic claim/aggregate heads,
while asserting the reservation guard read under lock; they never update its
amounts, ordinary-family state or revision.

The permanent reservation-control lookup is derived from the new receipt's
scoped source/label and request hash. This operational index is not an invented
v2 delivery-key envelope: the old v2 delivery-key requires an economic event.
Economic aliases keep their original economic event/receipt and resolve its
companion settlement receipt. A uniqueness check across both index variants
prevents an economic event and a closure from occupying the same delivery key.

Atomic append includes identity/alias, economic rows when present, observation,
optional transition, additive receipt, all guarded mutable writes and unchanged
budget observation. Expected-current mismatch is retryable only after confirmed
rollback and full re-resolution, never a store-decided duplicate or repricing.
All newly supplied immutable records are checked by the coordinator against
schemas, hashes, original trust anchors and referenced complete history first.

## Commands owned by the shared coordinator

- `AcceptFinalBase`: existing authenticated work input plus host-resolved frozen
  outcome policy/eligible membership. Evaluate once, freeze target, register
  initial reservation observation and commit the complete base/composite receipt.
- `AcceptOrdinaryOutcome`: existing normalized v2 request. Resolve original
  identity/claim before current submission policy; on a new claim, evaluate the
  pure core and consume only the authorized positive supplier result under locks.
- `CorrectOutcome`: existing normalized v2 correction with expected current claim
  revision. Verify the frozen correction authority, evaluate atomic inverse plus
  replacement, retain an unchanged reservation observation, append together.
- `CloseOutcomeWindow`: scoped invocation/source/label, explicit expected current
  reservation revision and reason `authorized` or `deadline`. Coordinator verifies
  actual closure rights/evidence and timing; store cannot infer authority from
  an administrative principal or a nonempty evidence array.

No public submission API, CLI, HTTP handler, SQL, migration, importer, dispatcher
or authority provider is delivered by this contract candidate. The original
frozen pure core stays deterministic and DB-free. The new lifecycle accounting
must be deterministic coordinator validation with independently checked integers;
it is not a second price evaluator.

## Adapter proof obligations

SQLite uses tracked immediate transactions; PostgreSQL uses serializable
transactions with the ordered shared/exclusive scopes and stale-snapshot restart.
Both must prove complete-or-absent commit at every write boundary, exact readback
after reopen, cross-connection races and rollback/cancellation behavior. Scope
locks must exist even when a claim/head does not. Current revisions cannot be
trusted from an unlocked pre-read. Only acknowledged commit returns acceptance;
unknown commit returns `CommitError::OutcomeUnknown` and the original lookup key.

Both adapters must preserve accepted zero ordinary slots, original first receipts
through corrections, lossless original replay material and exact frozen bytes.
A restored economic receipt without its required settlement companion is an
integrity failure for a registered target. Existing v1 targets remain v1; no
fallback or retroactive registration silently claims this new profile.
