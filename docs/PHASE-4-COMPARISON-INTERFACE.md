# Private Phase 4 comparison interface

This interface implements the approved bounded local synthetic scope at baseline
`be291388492f82485cfc1b51f46c9ad7a2e1b9f9`. It is not public host authentication,
prospective policy activation, base repricing, export or acceptance. No frozen
profile, schema, record ID, migration or dependency changes belong to this slice.

## Reader contract

`store::comparison::{ComparisonReadStore, ComparisonReadTx}` has only
`begin_read(deadline)`, `load_authority(who)`, `load_retained(selection)` and
consuming `finish()`. It has no acceptance supertrait, raw connection getter,
lock, append, write or commit method. `finish` rolls back the read transaction;
Drop and cancellation must rollback or discard even if begin/finish is interrupted.
No fallback may acquire an owner/writer connection. Stores implement mapping only.

`RetainedSelection` is one exact scope/target/invocation, optionally pinned to a
`SnapshotFingerprint`. `AuthenticatedReadContext` is a separate trusted local
host principal/scope/authority-head selection; candidates cannot supply it.
`ReadAuthorityObservation` supplies scoped heads and exact documents from that
same stable snapshot. Mandatory `ComparisonReadAuthority::verify_scope` runs
before target lookup, including before NotFound. `verify_disclosure` checks all
linked/supplier material before verification/report release. Neither seam grants
submission/correction rights. There is no default permissive implementation.
Permission is as of the pinned read snapshot; no stronger revocation guarantee.

`RawRetainedSnapshot` contains a configured logical `store_identity` (never a
DSN/path/credential), independently queried `anchors`, complete indexed `members`,
exact `records`, complete `heads`, and `original_deliveries`. References use existing
`ScopedRecordRef`; heads reuse `ObservedOutcomeHead` solely as immutable values.
Its lock class/key identifies rows; its lock mode is ignored, confers no capability,
and never triggers locking or lock-row creation. Supply precisely these classes:
Target, Reservation, InvocationConsumption, BaseReversal, every frozen Binding,
BindingAggregate, and Claim (including absent unclaimed families). Authority and
Admission belong to the separate authority observation, not this historical set.

Every retained record has exactly one independently read membership reference;
reference hashes must match full decoded envelopes. No inner join may conceal a
missing row. Exactly two independent anchors must match target base/registration.
Every reservation receipt has exactly one original composite delivery (canonical
key equals key), including registration and closure. Do not load aliases as extra
activity. The shared verifier checks commands, original ingress hashes, both
original receipt bytes and every receipt's inclusion. No receipt payload is
returned as candidate output.

One transaction pins all authority, counts/lengths, anchors, members, rows, heads
and receipt mappings. SQLite must use its reader pool/deferred query-only snapshot;
PostgreSQL must use a SELECT-only, repeatable-read read-only transaction. The
concrete lanes own actual enforcement, real-store races, cancellation and cleanup
proof. This interface does not claim those tests have run.

## Verification and immutable workspace

`service::retained` owns the moved original base decoder/economic replay/settlement
projector and the extracted exact history/anchor/head/delivery checks. Acceptance
passes a neutral `HistorySelection` built from its command and keeps its locked
snapshot/write authorization. Comparison never manufactures an acceptance command.
Original replay still reconstructs original records for exact byte checks; no
candidate is passed to these projectors.

`load_workspace` checks resource admission, opens the read snapshot, verifies
scope and disclosure, ends the read transaction, then verifies the detached rows.
No report is returned on uncertain cleanup. Cancellation is checked between decoded
records, historical economic decisions, settlement observations and receipt pairs;
no spawned/blocking work continues after return. Each synchronous unit remains
bounded by existing profile limits. Core replay receives a no-op checkpoint on
acceptance paths so the accepted behavior is preserved.

`ComparisonWorkspace` has private fields and no public constructor. Immutable
accessors expose original `Target`, decisions, economic rows, ordered settlement
rows (including registration checkpoint and interleaved closures), selected scope,
snapshot fingerprint and explicitly historical receipt IDs. It contains no reader,
transaction, acceptance plan, clock, callback or current read proof. The integration
owner projects this into the core's numeric `ComparisonActivity::from_history`.
Never construct candidate `TargetVerification`, `Verified`, `Decision`, receipt,
manifest, action, intention or acceptance conversion from this workspace/report.

## Resources and provenance

One active operation per process, including all reader clones, with fail-fast
Busy and zero queued jobs. Acquire `ComparisonOperation::begin(Cancellation)`
at the outer operation boundary and retain its RAII permit through
`load_workspace(..., &operation)`, candidate evaluation and report assembly.
Call `operation.checkpoint()` between candidate work units. The permit is never
part of the immutable workspace and is not released/reacquired between phases. Five seconds is the concrete read transaction ceiling: it
bounds snapshot pinning/cleanup and matches the existing local coordinator's
bounded database operation precedent. It is not a measured availability promise.
The complete outer operation has a thirty-second cooperative ceiling; a bounded
synchronous unit may finish before observing cancellation. No hard real-time claim.

Readers must use `ReadBudget::preflight_records` against SQL COUNT/SUM/MAX before
fetching bodies and `ReadBudget::charge` before growing their collections. Facade
postchecks defend against a faulty adapter but cannot replace SQL preallocation.
Limits are comparison-only admission rules, never frozen-contract changes:

- One target/invocation; 4096 records; 16 MiB total retained rows plus metadata and
  original receipt/command mappings. Authority and retained loading share one
  aggregate `ReadBudget` within the transaction (not 16 MiB apiece). No truncation. Maximum encoded envelope
  512 KiB; the original stricter 256 KiB body, 8 MiB resolved-input, 4 MiB decision
  and other original decoder limits still apply.
- At most 128 heads, 1024 authority references/documents, 4096 membership refs,
  999 outcome/control steps plus registration; keys 16 KiB, head values 256 KiB.
  Reference IDs 4 KiB and existing scoped string limits. Reader preflight must
  apply these to metadata/receipt queries as well as envelope queries.
- 2–8 alternative candidates, plus automatic original control. At most 256 KiB
  each, 1 MiB aggregate; existing 32 families, 16 bindings, 32 codes remain.
  `admit_candidates` validates measured sizes before cloning/decoding.
- `BoundedReport` rejects before extending beyond 4 MiB. Serialization must not
  allocate an unbounded intermediate Value/String before using this writer.
  Candidate and report assembly remain the core/integration lanes' work.

The snapshot digest is an internal comparison provenance hash, separate from every
ledger identity domain. Its descriptor includes comparison semantics, trusted
logical store identity, scope/target/invocation, complete sorted member hashes,
independent anchors, sorted head keys/revisions/values, and original receipt pairs.
Member hashes bind original profiles/evaluator versions, evidence and fixed times.
Expected fingerprint mismatch is SnapshotChanged, never implicit reselection.
A fingerprint proves integrity of this observation, not current authority or
anonymity. Cross-backend replicas use the same trusted logical store identity.

Reports must cite this fingerprint and one shared activity digest; report provenance
also covers ordered candidate fingerprints, source build semantics and the fixed
assumptions. Acquisition wall time is not an economic input. Reports state that
amounts are substituted without assent, eligibility changes or posting authority.
Keep original policy IDs under historical provenance and draft policy labels
separate. A failed candidate has no full-history comparable total.

Candidate substitution does not change supplier terms: supplier amount rows must
remain equal to their original retained values in this approved first slice.
The core owns this admission check.

The owner can drive core computation without environmental dependencies in core:

```text
operation = ComparisonOperation::begin(cancellation)
workspace = load_workspace(reader, authority, who, selection, &operation).await
activity = core projection of workspace's verified chronology and checkpoint
admit_candidates(measured bounded candidate sizes)
driver = ComparisonDriver::new(activity, candidates)
loop: operation.yield_and_checkpoint().await; advance one bounded core step
matrix = driver.finish() only after complete original control and all alternatives
provenance = ReportProvenance::new(workspace, candidate digests, source build)
serialize allowlisted numeric report through BoundedReport
operation.checkpoint(); return complete report; drop operation
```

The pure driver lives in the core lane; report integration is a separate owner
commit. Synchronous retained verification checks monotonic deadline and externally
set cancellation between bounded units; it never spawns detached work. A cancel
task on the same single-thread runtime cannot execute during a synchronous unit.
The async numeric driver yields between advances so those tasks can execute.

Digest strings are exactly 64 lowercase hex characters from the shared private
core helper, without a ledger ID prefix. Activity fingerprint is computed only
from verified base/economic membership and ordered reservation history. It cannot
be supplied as caller provenance. ReportProvenance binds that activity, snapshot,
ordered candidate digests, source build, assumptions and constant committed=false.
The facade premeasures descriptor serialization with a bounded writer before JCS;
the descriptor has an additional 4 MiB admission ceiling. This can reject a very
large valid history rather than emitting partial provenance. BoundedReport poisons
itself on overflow and its fallible finish refuses partially assembled output.

Validation for this slice: `sh scripts/check.sh` and
`python3 scripts/check-comparison-boundary.py`. The latter compiles the actual read
port with inert observation structs and rejects commit, write, append, locking and
delivery-lookup method probes; it also checks forbidden facade dependencies.
The three facade comparison tests cover valid full chronology, eleven corrupted
member/anchor/head/receipt cases, expected-snapshot mismatch, reordered-row digest
stability, deterministic noncommitted report provenance, exact/over resource
bounds, combined authority+retained budget, denial before loading, disclosure
denial, cleanup failure, dropped future, and retained global admission after load.
These tests are not concrete SQLite/PostgreSQL reader conformance.

## Integrated foundation entry

`service::comparison::foundation::compare_retained` connects these private seams
without adding a profile or public endpoint. It retains the outer admission guard,
projects the original registration checkpoint and ordered supplier observations,
yields between `ComparisonDriver` advances, checks original capacity against every
retained receipt, and streams an allowlisted report under the 4 MiB limit. Candidate
input sizes include serialized delimiters and fields before the driver clones them.

This is **PHASE-4 FOUNDATION ONLY**. Existing accepted outcome activity is supplier-
only and supplier terms cannot vary. Retail alternatives remain unobserved; none
becomes an earned retail adjustment. Backfill/post-closure profile design and full
Phase 4 completion remain separate. No stored record/receipt/acceptance conversion
is available from `FoundationReport`.
