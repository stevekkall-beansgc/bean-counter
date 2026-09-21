# Independent reservation lifecycle expectations

The owner's follow-up decision supersedes the earlier semantic stop: original
ordinary authorized outcomes can move held capacity to consumed; explicit closure
or deadline moves remaining held to released. Every later correction, reversal,
chargeback, attribution fix, dispute and reinstatement changes economics only.
Reversal never replenishes the original capacity. Existing correction authority
and ceilings still apply; an excess requires a separate new agreement.

These expectations were authored without reading coordinator candidate fields.
They are numeric test observations, not a new canonical or production schema.
The original stop commit `25a9f451f8bd3dd6433d45117e9716efbc2a5c4d` is preserved.
The semantic blocker is resolved; exact canonical encoding and durable adapter
proof remain pending. The old supplier golden cannot acquire missing transition
bytes merely by lifting the guard in `conformance.py`, so that guard remains.

## Independent assumptions and amounts

Synthetic USD scale 2, one supplier invocation, multiple predeclared ordinary
families sharing that invocation. Total authorized capacity is 15,000 atoms.
Original base has consumed 3,000; held contingent capacity starts at 12,000;
released starts at zero. One binding has gross premium ceiling 10,000 and gross
discount ceiling 3,000. These are declared numeric literals, not defaults.

Consumption for an ordinary result is its nonnegative supplier premium:
`max(amount_atoms, 0)`. Accepted zero and discount consume zero and do not release
or replenish. This interpretation is explicit for coordinator/owner comparison.
Closure releases every remaining held atom exactly once. The invariant is:
`held + consumed + released = 15,000`, with all three nonnegative. Conservation
alone is insufficient: replenishment and early release can conserve the sum.

Post-hoc adjustment effects are an exact inverse of the previous nonzero amount
plus the new nonzero amount, including zero-net replacements. They do not change
held, consumed, released, closed state, or the reservation's logical version.
An initially zero ordinary outcome still owns a permanent claim. A reversed
claim cannot be recreated as a new ordinary outcome.

The numeric model advances an abstract reservation guard on accepted ordinary
operations (including zero) and closure. This is test scaffolding, not a mandate
for an encoded revision bump or a record for a zero delta. Adapter integration
must translate expected-current observations into the coordinator's reviewed
locks/revisions. An internal serialization retry may refresh a guard and proceed;
user correction expected-current must never be silently refreshed.

Chargeback, dispute and attribution-fix are labels for authorized replacement
operations in these numeric examples, not new event types. They require an
already allowed code/replacement and ordinary correction authorization. Nothing
here expands the frozen correction command or admits arbitrary new codes.
A fresh credential alone does not enlarge the frozen ceiling. The new-agreement
workflow is outside this bounded original-invocation history; no fictitious
agreement or retroactive family is created.

## Hand-authored cases

`lifecycle.HISTORIES` contains 10 histories / 40 steps, with literal expected
reservation totals, current amounts, classifications and exact new postings:

| History | Steps | Key expected result |
|---|---:|---|
| ordinary-consume | 1 | +2,500 → held 9,500 / consumed 5,500 |
| accepted-zero | 2 | All 12,000 remains held; claim cannot be used twice |
| ordinary-discount | 1 | −1,000 creates economics with capacity unchanged |
| explicit-closure | 3 | Release 9,500 once; reject a new ordinary outcome |
| deadline-closure-zero | 2 | Release all 12,000 after zero; no early auto-release |
| shared-invocation | 3 | +2,500 and +4,000 consume 6,500; release remaining 5,500 |
| duplicate-and-stale | 5 | No double consume, identity conflict, stale guards |
| post-closure-adjustments | 11 | Correction/reversal/reinstatement/chargeback/fix/dispute and retries never change reservation |
| authority-and-ceilings | 7 | Reject missing correction authority and ±ceiling excess; exact ceiling allowed |
| no-replenishment-before-close | 5 | Reverse +8,000, still only 4,000 held; reject new +5,000 |

The 11 new unittest methods additionally cover six serialized race schedules:
two shared-invocation orders, two outcome-versus-closure orders, and two competing
correction orders. For shared capacity, reversing an 8,000 outcome leaves only
4,000 held; two new 3,000 ordinary families cannot both win, even though their
combined current premium is below the economic ceiling. For outcome versus
closure, possible final totals are exactly (held 0, consumed 5,500, released
9,500) or (held 0, consumed 3,000, released 12,000), according to lock order.
These are allowed serializations, **not evidence of actual concurrent overlap**.

Crash assertion sensitivity checks all 64 combinations of six changed state
groups in one ordinary commit, accepting only full before/full after and
rejecting 62 partial combinations. The groups include claim, identity and
postings as well as reservation values/version. Unknown-commit retry checks both
known durable outcomes: absent→accepted and present→original duplicate, each
consuming only once. Absence while the original transaction is still active
cannot determine either outcome and requires a production primary/lock probe.

## Reusable hooks and candidate comparison

`lifecycle.run_adapter(adapter, history_name)` sends only abstract input tuples
and checks independently authored numeric projections. The adapter owns seeded
terms and original base acceptance, translates guards and commands, and invokes
the production coordinator. It implements:

- `submit(command) -> (test_category, original_receipt_bytes_or_None)`.
- `observe_numeric() -> (reservation_tuple, family_amounts, posting_sequence)`.
- `observe_full()` for lossless full database inventory plus stored byte bodies.
- `reopen()` to close every connection and reopen the same durable database.

Accepted receipts are captured from the first response, then retry bytes must
match. Existing `conformance.py` remains the independent exact frozen-byte
oracle. No expectation is supplied to the system under test. The hook is
unconnected; neither backend is certified. It must be supplemented with
schema-specific indexed-column/physical-row deltas and real overlap traces.

When the coordinator candidate is supplied:

1. Record its exact commit and file digests; read the candidate without changing
   frozen inputs. Map its quantities into these independent observations.
2. Compare ordinary/zero/discount/closure and all post-hoc steps, guards,
   ownership scope, limits and permanent claim behavior. Do not silently adjust
   this oracle to match candidate output; report discrepancies to the owner.
3. Independently reconstruct candidate byte/hash rules only after their explicit
   specification exists. A new candidate is not frozen just because it hashes.
   Verify every old pin remains byte-identical.
4. Connect the coordinator and both real adapters. Require complete journal and
   receipt bytes after reopen, physical no-residue failure inventories, actual
   barrier/lock overlap, cancellation at every write/await, crash recovery and
   unresolved-active-commit handling. Preserve authority/read checks before
   duplicate receipt disclosure and original receipt lookup after corrections.

No candidate has yet been supplied or inspected for this follow-up. Exact-byte
comparison, current closure/deadline time checks, credential/base-reversal races,
new-agreement authorization, all physical boundaries and real SQLite/PostgreSQL
outcome lifecycle evidence remain open. These are explicit limits of this lane,
not a claim that the numeric tests establish the Phase 3 exit gate.

## Validation of this follow-up

Full offline `sh scripts/check.sh` passed: 129 Rust tests passed, 19 intentional
opt-in/later gates ignored; formatting, warnings-denied Clippy, no-default
compilation, boundary checks, frozen audits and Python/Node reconstruction all
passed. The existing Rust Phase 3 entry now runs both Python modules: 28 methods
pass (17 existing plus 11 lifecycle methods). The lifecycle fixtures have 10
histories and 40 steps; six serial race schedules and 62 partial-state rejection
probes are additional, separately counted checks. All 159 pinned files are
unchanged. Production files edited: zero. Real lifecycle adapter executions:
zero. No candidate comparison has been claimed and no external infrastructure
or dependency installation was used.
