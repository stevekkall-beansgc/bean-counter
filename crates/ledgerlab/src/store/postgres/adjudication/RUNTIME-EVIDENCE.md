# Test-only native runtime enrollment

`runtime_tests.rs` implements the existing private store port only under
`cfg(test)`. It forwards the production acceptance coordinator to real native
PostgreSQL supervised transactions. This is runtime, persistence, and atomicity
evidence under an **assumed physical envelope**, not production admission or a
proof of protected completion. The production PG store still does not implement
`AdjudicationStore` and cannot issue this test capability.

The test capability is explicitly labeled UNPROVED. Its transaction identity is
bound to the actual live work/control backends; the independently observed native
primary journal head is acquired under ordered locks. Append rechecks that
transaction, owner and head, and the production supervisor additionally requires
the exact durable work command identity before dispatching its native Append arm.
Logical genesis budgets and epoch0(center)/epoch1(gateway) are synthetic setup
inputs. Their large finite values are not representations of allocated disk.
The physical observation/incarnation marker is a test assumption, not an external
fence witness. Replacement or physical-capacity tests cannot use it as evidence.

Five independent PostgreSQL databases represent center and four gateway journals.
Only genesis resources and current authority plus original evidence/Authority and
Binding heads are provisioned by the test owner. No accepted base, receipt,
target, posting, membership, invocation consumption or other economic state is
inserted as setup.

The actual path is:

1. Four gateway PREPARE_ENROLL commands pass `service::run`, current-source checks,
   pure transition planning, and private validated-plan native append.
2. Each exact saved retry returns Duplicate with unchanged retained table hashes.
3. Center obtains each preparation through actual committed native primary source
   membership; the canonical proof set is sorted after retrieval.
4. ENROLL uses original locks, actual `PostgresTx::resolve_outcome` and delivery
   lookup, and `prepare_original_base` with the configured synthetic verifier.
   The verifier checks exact evidence and current heads from that same locked
   snapshot. The full original pure evaluator and membership checks remain in
   force. Original object envelopes are rebuilt from `plan.records()` and actual
   next ordinal rather than accepted from fixture envelopes.
5. A wrong principal refuses with unchanged retained inventory. An injected
   failure after the first original membership insert reaches the exact native
   original-write boundary and rolls back all accepted state. The same valid
   command subsequently commits all original records and R3 enrollment together.
6. Saved retry, close/reopen retry, exact 29 original-record readback, 29-member
   closure, one base anchor, one central segment, and primary enrollment-source
   export are verified. Actual retained postings sum to retail10000/supplier3000.

Before/after inventory hashes cover every application table except the mutable
unresolved-work staging row. That row legitimately retains last work identity;
its final state is asserted IDLE with unchanged zero generation. Tests do not
claim that it incurs zero heap/WAL cost. Successful fixtures drop only their own
fresh databases; failed-run evidence and any abandoned failure databases are
preserved.

The fixture provider is test-only and scoped to PREPARE_ENROLL/ENROLL/read retry.
It is not a production authentication provider or all-29 command adapter. This
wave does not claim PG full customer chronology, offline public gateway facade,
protected FINISH, bounded comparison, durable external fencing, or physical
allocation/retention/workspace enforcement. The preceding physical-premises
worksheet remains an incomplete allocation proof. Those gates remain required.
