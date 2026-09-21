# PostgreSQL outcome persistence lane

This lane implements the private coordinator seam and validating fixture supplied through `131584f130fd6777ffa6b4215fa5b3498d926fec`, extending frozen amendment `1170ddc13e8f4e799d000a842cb978b6aa2adebb`. Canonical construction, settlement arithmetic, pricing, authority and complete semantic validation remain coordinator responsibilities. There is no public raw-action append.

## Storage and upgrade

Backend migration `0004_outcomes.sql` is additive. Migrations 0001–0003, the old freeze inventories and all reviewed contract bytes remain unchanged. Normal open verifies backend schema 4 and never migrates. Explicit fenced owner upgrade supports populated schemas 1, 2 and 3; the existing logical-schema marker and old record profiles remain unchanged.

The concrete adapter stores full canonical envelopes keyed by scoped kind and canonical ID bytes. Hashes are values, not extra identity dimensions. An insert may reuse an exact immutable record but rejects changed bytes or hashes for that identity. Membership rows retain the complete coordinator-supplied target/invocation partition. Anchors retain scoped kind/ID/hash references with deferred foreign keys to immutable records; they are original trust anchors, not recomputed replacement roots. Readback includes all partition members plus the exact required immutable documents and rejects missing or mismatched references. The 8 MiB history bound rejects excess instead of truncating.

Opaque control heads retain the complete coordinator-owned canonical value and numeric revision, indexed by the exact supplied scoped lock key/class. Every append asserts all observed heads, including locked absence and unchanged reservation observations. Writes require an exclusive held lock and expected old revision/value; existing heads advance exactly once. Head meaning is not interpreted in SQL.

Composite delivery rows reference the original economic envelope when present and the mandatory reservation receipt. Aliases reference their original canonical delivery and exact original receipt pair. Closure needs no fabricated economic receipt. Intentions remain immutable envelopes, with a separate held operational row; this lane adds no dispatch behavior.

A shared delivery namespace is populated from legacy keys during upgrade. Owner-defined triggers reserve one scoped source/label for every legacy or outcome/control delivery insert, in both directions. It is not possible for a v1 writer and an outcome writer to occupy the same key concurrently. A confirmed legacy winner returns the coordinator-owned non-retryable `DeliveryConflict`; a bare uniqueness error never establishes an economic duplicate. The namespace is immutable and runtime code cannot insert directly into it.

New records, membership, anchors, delivery mappings and the namespace have immutable guards. Runtime grants are restricted to the required reads, inserts and operational head updates. No migration ownership or delete/truncate rights are granted.

## Locks and supervision

The adapter acquires the installation admission fence and then the coordinator's sorted lock set. The class order is admission, authority, binding, reservation, target, claim, binding aggregate, invocation consumption, base reversal. Keys are canonical scoped UTF-8 bytes. Read modes use shared row locks; write modes use exclusive locks. Unique operational scope rows serialize missing heads without reserving an identity or a claim. Creation and locking are transaction-bound; retained empty rows contain no accepted economics.

A transaction accepts one complete ordered acquisition. Missing or stronger locks return `MoreLocks` through resolution; callers must roll back and restart, never upgrade in place. Complete snapshots include all exact head observations. Appending checks the resolution partition, supplied locks, observed revisions/values and write modes again. `ExpectedCurrent` is eligible for retry only after rollback and complete re-resolution.

The existing registered PostgreSQL task owns the SERIALIZABLE transaction. Outcome requests use the same poison-on-cancel handle, bounded statement/lock deadlines, rollback, commit drain and session discard as the original acceptance path. Cancelled/failed operations cannot commit earlier writes. Only acknowledged COMMIT establishes success; ambiguous commit results remain `OutcomeUnknown`. The coordinator owns bounded authoritative lookup and same-identity retry decisions.

## Test interpretation

The tests use the real driver and restricted runtime role on isolated PostgreSQL 17/18 services. Primitive storage checks are separate from composite acceptance tests built exclusively by the coordinator's complete decoder/evaluator/validator. No fake `ValidatedOutcomePlan` constructor is exposed. Authority proofs are explicitly synthetic fixtures, not evidence of real-world authority.

The validated four-step lifecycle stores original base acceptance, positive ordinary consumption, a correction that preserves the reservation exactly, and explicit closure. Its 386 before/after mutating-statement boundaries run through the registered production supervisor; each failed transaction is compared against every physical table. Exact envelopes, original anchors, opaque heads, both receipts and stable duplicate lookup are checked after reopen. Deferred missing-companion references fail commit and roll back the shared namespace as well.

Independent coordinator transactions race at every lifecycle step; distinct correction identities also compete for one current claim revision. Additional cases cover zero/negative ordinary outcomes, ordinary versus closure, permanent ordinary aliases after correction/closure, renamed stale corrections, stale plans, and already-closed no-op observation/receipt. A no-op close preserves the reservation and all control heads other than the target history index, which must include its new immutable observation/receipt.

Opaque TLS cuts interrupt actual COMMIT requests and replies without synthesizing results. For each of registration, ordinary consumption, correction and closure, both ambiguous outcomes remain `OutcomeUnknown`; the backend is discarded and a new coordinator attempt recovers the original complete acceptance or creates it once after confirmed absence. A real blocked delivery INSERT is also cancelled after companion/namespace writes; the poisoned handle cannot report commit success, the supervisor drains, and exact physical rollback plus backend disappearance is verified. A lost rollback acknowledgement remains conservatively unknown even when the post-drain observer proves rollback.

Populated upgrades preserve all old physical rows and original receipt retries across schemas 1/2/3, with fences, checksums, wrong-role/wrong-store refusal, DDL rollback, lost application acknowledgement, reopen and idempotent owner retry. The physical first-slice harness retains every new table: it checks exact namespace projection values and key coverage, derives its one-row-per-delivery delta, and still rejects any rewrite/deletion or unrelated extra row. No frozen journal oracle changes.

Final test counts, composite-path coverage and any pending integration gates are recorded in the lane's final evidence report; this document alone makes no complete Phase 3 conformance claim.

During simultaneous diagnostic races and unrelated full stress suites, the second closure request returned `Unavailable`. The exact same current-thread race passed in both complete suites and in isolated timed runs. The logs retain both outcomes; this evidence does not establish throughput or availability under arbitrary concurrent load. No timeout was enlarged and no error was reclassified as success. Hosts must retain the original identity for explicit retry after transient unavailability or unknown outcome.
