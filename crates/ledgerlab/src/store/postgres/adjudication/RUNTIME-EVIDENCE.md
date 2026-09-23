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

The initial enrollment checkpoint covers PREPARE_ENROLL/ENROLL/read retry; the
customer successor below extends its provider and command surface. The provider
remains test-only, not production authentication or an all-29 command adapter.
Neither checkpoint proves a public PG gateway facade, protected physical FINISH,
bounded comparison, durable external fencing, or physical allocation/retention/
workspace enforcement. The preceding physical-premises worksheet remains an
incomplete allocation proof. Those gates remain required.


## Customer95 successor

`runtime_customer.rs` extends the same explicitly assumed-capacity test harness
through the exact accepted 95-command customer chronology. It preserves the
frozen fixture bytes; only live proof references, grant head authentication and
command authority-head bindings are hydrated from actual journals. Both exact
source requests and first-use enrollment requests resolve through the configured
host registry and native primary membership. No caller path selects a database.

The 15 economic assertions are pinned from the independent pre-authoring
CUSTOMER-STORY.md/EXPECTATIONS.json and reuse the already reviewed SQLite oracle
checks with PG point-read plumbing. They do not derive expected totals from
candidate output. Actual durable family, entitlement, case and directional pool
heads are read at each prefix. Signed actions are accumulated and compared with
10000,10000,11200,11200,11200,11200,11700,11700,11700,11700,11700,11800,11650,
11650,11450. Supplier remains3000. DENY precedes the distinct qualified upsell;
all five families close before adjustments and correction. The correction
retains inverse−500/replacement+300, five consumed entitlements, original
premium usage1700 and adjustment gross250. Virtual close leaves pending case
head bytes/revisions unchanged while their effective status changes.

Every command has an actual saved retry and unchanged retained-table inventory;
each of five hosts is reopened and retried again. Overspending adjustment101 and
stale correction revision2 refuse with exact error codes and unchanged retained
inventory before the valid commands proceed. The test records all95 host roots
and ordinals and all15 checkpoints; both majors must be compared in the external
source-pinned handoff.

This sequence covers22 of29 command kinds. It does not cover ABORT,
EXTEND_RESOURCES, PREPARE_ROUND, REPLACE_WRITER, RETIRE_GRANT, RETURN_UNUSED or
SUPPLEMENT. The prior enrollment-specific failure/authority tests remain. The
physical envelope and incarnation remain test assumptions; this successor is
not public PG integration, production admission, capacity enforcement, external
writer fencing, bounded comparison, or a complete29-kind PG conformance claim.


### Cross-backend provenance boundary

The SQLite and PostgreSQL adapters intentionally retain different real trusted
observation identifiers. PostgreSQL hashes the canonical array
`["postgres-primary-journal/1", binary_journal_key, ordinal, segment, root]`;
SQLite uses the corresponding `sqlite-primary-journal/1` domain. The first four
PREPARE_ENROLL commands/results are identical. ENROLL is the first divergence:
its four preparation `trusted_observation_ref` values differ, changing the
payload's command digest and then downstream canonical roots/proof hashes.
Original29 base bodies and their raw-SHA256 object keys are unchanged.

The emitted witness includes each actual stored command (verified by bounded
native command-row readback), its raw SHA256, committed result, host ordinal and
root. The external comparison report substitutes only the four observation
identifiers and derived command digest to demonstrate exact first-ENROLL input
alignment. Later commands are compared with explicit derived-provenance fields
listed, never silently normalized into a cross-backend byte-identity claim.
The normative host-provenance contract permits these distinct trusted inputs;
no production observation domain was changed to manufacture equal roots.

## Remaining native branch controls

`runtime_remaining.rs` exercises ABORT, PREPARE_ROUND, RETIRE_GRANT,
RETURN_UNUSED, SUPPLEMENT and EXTEND_RESOURCES through the same real native
transaction/coordinator path. The extension ceiling is an explicitly assumed
**test input**, not a PG allocation or admission observation. Each committed
command has a saved retry with unchanged retained inventory. Refusals compare
all application tables on all five hosts (except the operational unresolved
slot); successful transitions also check that the other four hosts are unchanged.

The branch controls cover cancellation before/after local begin, seal and ready;
close-versus-abort terminal refusal; abort followed by original FINISH_ONLY
completion; permanent grant retirement and returned-unused disposition;
cumulative supplement bounds and immutable original receipt/submission; and
extension up to an assumed host ceiling with over-ceiling refusal. Reopen retains
exact saved outcomes. These remain semantic/persistence tests under the stated
physical assumption, not proof of prebacked PostgreSQL completion.

Positive REPLACE_WRITER is deliberately absent. The real dedicated native
control session establishes live same-primary exclusion and waits for prior
work to exit before resolving its saved outcome. Its fixed unresolved-slot
generation is not a trusted durable storage incarnation, nor proof that a
restored primary contains all acknowledged history. The harness therefore keeps
`writer_fence=None`; a replacement carrying a caller digest must refuse with
FENCE_PROOF and unchanged state. No digest of a PID/head is substituted for the
missing trusted fence. A production replacement path still requires that
external recovery/ownership premise as well as physical admission.

Remaining-branch authoring status at preservation: **WIP, not a passing gate**.
The first PG17 five-group run passed the six-cut cancellation group and failed
four fixtures during PREPARE/ENROLL with Retryable. A traced extension rerun
passed the six commits and over-ceiling refusal, then failed DROP DATABASE
cleanup with statement timeout57014. The setup causes remain unlocalized; no
production timeout was changed. Raw logs/source fingerprints and the separate
cleanup observation are preserved in the external pg-remaining-diagnosis packet.
Further PG runs paused for coordinator/independent acceptance reconciliation.


## Integrated publication-driver regression preparation

The earlier incomplete branch results above remain historical failures. The
assembled driver changes its gate, stale-unbound check and transaction cleanup,
so the existing customer/remaining-kind selections require a fresh serial run.
Each store-error trace now names its exact stage and each command records elapsed
time. These are test-only diagnostics. Successful fixtures now close application
and owner sessions and retain their databases, like failed fixtures, instead of
issuing DROP DATABASE. No statement deadline, assertion or product behavior was
relaxed. This avoids mixing semantic assertions with destructive test cleanup;
it does not explain or erase the historical DROP timeout or setup Retryable.
No fresh pass is claimed by this preparation. Physical admission is still assumed
only in the test harness, and these fixtures still use unbound native execution.
