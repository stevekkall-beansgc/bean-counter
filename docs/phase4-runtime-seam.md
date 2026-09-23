# Phase 4 typed runtime seam

This is an interface review target, not a runtime or backend conformance claim. It starts at `96a3e1296fb5684a15d34c35159b18df3b0d80cb` and consumes the additive freeze of canonical candidate `c8fbddc682e22e10c6122abbcf4c8212a1c407fa`. The integration owner will combine it with the independently reviewed freeze-check successor. No frozen candidate, legacy profile, migration, dependency or concrete adapter is changed.

The governing requirements are full R3 (SHA256 `341043f03c7879c0d1ef02b76e054e5fd7ab287d745f48d434eb59162d148db6`), the accepted candidate schema/protocol/resource worksheet, and the independently pinned customer oracle. Customer chronology stays DENY before qualified upsell, closure of all families before the accepted-result correction, actual11450 and comparison-only11750, supplier3000 unchanged. This seam makes no new commercial or availability choice.

## Concrete boundary and ownership

| File area | This review target | Next implementation owner |
|---|---|---|
| `ledgerlab-core/src/adjudication` | All29 typed command variants and closed companion/authority/proof/response shapes; checked scalar/counter encodings, exact bounded JSON/Base64, full membership identity checks, resource arithmetic, paged read and virtual transfer types | Runtime author: pure transition/replay/economic rules and schema-negative parity |
| `service/accept/adjudication.rs` | Private validated plan, trusted prefix/source/capability types, fresh original-base subplan, authority observation and verified continuation | Runtime author: sole coordinator, trusted-host factories, outcome/retry handling and bounded semantic fold |
| `store/adjudication.rs` | Concrete transaction, point-resolution, append and SELECT-only page interfaces; stable lock tags/ranks | Integration owner: separate SQLite/PostgreSQL mappings and new migrations; gateway adapter shares the same host-local contract |
| Existing `OutcomeLockClass`, `AcceptanceTx`, `CommitError` | Reused unchanged | Integration owner: retain legacy shared exclusion, cleanup and unknown-commit behavior |

This target adds minimal module declarations only. No production dispatch method is exposed and no adapter claims to implement these ports yet. The private unused seam items have item-local lint expectations with an explicit next-gate reason. Executable validators and lock-order checks are exercised by focused tests; there is no blanket dead-code suppression.

## Wire and validation layers

`commands.rs` transcribes the accepted closed schema into explicit Rust structs/enums. `Command` has29 variants, each with its own typed payload, key and authority. `ParsedCommand::parse` requires canonical bytes, the256KiB envelope bound, closed fields, no null, exact tuples, checked byte lengths, canonical decimals and timestamps, bounded arrays, canonical Base64 and set ordering. Original enrollment family and gateway order are preserved using the frozen schema's `x-ordered` rule. The scalar counter supports0 through10^30−1 independently of the legacy i64 revision type.

A parsed command establishes shape and byte validity only. It does not establish economics, current authority, evidence truth, authenticated remote membership, physical storage capability or complete semantic replay. Those checks must precede the sole coordinator's private plan construction in the next wave. Both actual retained authority source bytes and current trusted host verification remain required; typed `AuthoritySourceBody` includes authorization, assent and delegation without reducing them to booleans or hashes.

R3 canonical encoding uses an explicit maximum8MiB; command256KiB and introduced trust2MiB remain distinct. Legacy `CanonicalBytes` retains its4MiB default. The R3 encoder preserves integer-only JSON and UTF-16 key ordering. Object identity checks compare complete source store/scope/registration/host/ordinal, kind, full key, hash and length. Hash verification intentionally cannot manufacture `VerifiedSource` or `TrustedPrefix`.

The accepted logical index bound is K1115, path8922 pages. Core types carry it without asserting that SQL's physical pages or WAL match the logical worksheet. Adapter enforcement and measured/proved physical envelopes remain required.

## One atomic plan, one owning journal

Each plan names exactly one `JournalIdentity` and its observed predecessor. `AdjudicationTx::append_adjudication` reasserts all observed heads, capability, credits and complete plan membership before applying any write. The transaction atomically persists exact segment bytes, retained source bodies, dependencies, permanent identity mappings, index changes, resource/counter conversions, economic state and held intentions. A gateway plan cannot mutate center state, and a center plan cannot mutate a gateway's resource account or journal.

ENROLL additionally carries `FreshBaseAcceptance`: original-profile validated output, concrete original writes, locked absence observations, exact manifest/receipt and full original companions. V1 uses the core `DecisionPlan` and its validated write projection; v2 uses the existing private `ValidatedOutcomePlan` built through the original base decoder/coordinator. Both paths append the original base and R3 enrollment in this SAME transaction. A separately accepted base, a digest-only base, or an existing old target is not an enrollment input. Refactoring the existing v2 plan builder into a noncommitting reusable helper is next-wave coordinator work; calling its existing committing runner first is prohibited.

PREPARE_ENROLL and PREPARE_ROUND commit locally before their independently authenticated proofs are consumed centrally. Grant creation, registration, issue, activation, first receipt, import, terminal reconciliation and installation are separate durable boundaries. No port hides a distributed atomic step. A remote fact uses an immutable historical source prefix and exact source inventory membership; source dependencies form a verified historical DAG, not references to another journal's current head. Original receipt import precedes alias import. Wrong-owner lookup refuses/forwards before disclosing another host's saved outcome.

CLOSE persists a bounded certificate, family/prerequisite/entitlement heads and required supplier transitions. It cannot enumerate pending cases in the certificate. `TransferWitness` and `EffectiveLifecycle` derive the first affecting closure at the named prefix. The eventual adapter must reject a closure plan containing per-pending-case transfer writes; constant topology, not pending cardinality, bounds its writes. Finality keeps original transfer lineage. Actual cardinality tests255/256/257/1025 remain next-wave evidence.

## Lock compatibility and shared namespace

Persisted legacy lock tags remain exactly Admission0, Authority1, Binding2, Reservation3, Target4, Claim5, BindingAggregate6, InvocationConsumption7 and BaseReversal8. No cast changes and no mutation to old CHECK0..8 tables occur here. New R3 guards use their own explicit storage tags9–16; the migration owner decides their additive table storage.

One total acquisition order is encoded: legacy Admission WRITE rank0; R3 enrollment/namespace rank1; the remaining legacy classes in their unchanged relative order at ranks2–9; then R3 capacity/allocation10, gateway/round11, family/prerequisite12, case13, entitlement14, supplier15 and adjustment16. Within a class, complete scoped keys order deterministically. R3 guard ordering additionally binds owning host. Discovering more locks requires rollback and restart, never acquiring a lower rank later.

The legacy Admission WRITE gate remains common to old and new writers. ENROLL checks already occupied full keys and permanently reserves all future routing prefixes while holding it. SQLite's reciprocal identity triggers and PostgreSQL's existing shared namespace arbiter must be extended additively so old writers cannot bypass new namespace ownership. Sharing only a new lock enum would not satisfy this contract.

## Capability and trusted-host construction

The live `AdjudicationTx`, not a digest or epoch field, owns the actual cross-process storage exclusion guard and the physical protected work lease. `CommitCapability` is a non-Clone, non-deserializable binding witness held while those resources remain owned by that transaction. Append must compare its transaction ID, journal identity, storage incarnation, writer epoch, recovered prefix and allocation owner against the live guard and current durable state. Commit/drop/cancellation/rollback retain ownership through the existing supervised cleanup boundary. A capability from another transaction cannot authorize a write.

`PhysicalEnvelope` binds backing/index/WAL/staging/reader-retention/protected-workspace limits and the trusted enforcement observation. Merely constructing this data does not prove those limits: the later concrete adapter must enforce them across other processes, old writers, optional tasks and readers. Provisioning refusal precedes new promises; unexpected I/O is operational failure or unknown outcome, never commercial DENY. Counter q/R and retained used/held accounting stay separate from reusable workspace peak lanes.

The trusted factories remain private to the coordinator and assigned backend integration boundary. Future adapter hooks supply primary snapshot observations and a live storage guard to a narrow coordinator-approved factory; there is no conversion from a raw command, caller-selected ExpectedPrefix, signed key, path/inode, or imported hash to a trusted prefix or commit capability. This interface intentionally does not expose a public constructor while those concrete checks are unimplemented.

Replacement needs actual old receipt-boundary exclusion and complete recovery through that old boundary's final authoritative head, including exported/unactivated grants and terminal outcomes. A stale copied journal or central lease invisible to the offline writer cannot mint a replacement capability. Existing service/facade `OutcomeUnknown` and `CommitError::OutcomeUnknown` remain unchanged. Unknown commit resolves the exact original identity without appending a new retry/recovery row, releasing credits or selecting a replacement round.

## Bounded reads and nonposting comparison

The read trait has no transaction/write supertrait, business lock acquisition or commit method. It pins a primary snapshot and independently creates the complete ExpectedPrefix (logicalstore,scope,target,profile,enrollment,registration,host,ordinal,segment,root), with current-at-read or authenticated historical selection. Missing external authority can yield only a verified supplied prefix, never completeness. All registered gateways need a bounded coverage entry, including explicit UNKNOWN entries.

Segment and dependency reads use indexed immutable addresses, preflight lengths,4096-byte fragments and explicit leases. They cannot materialize all history before metering. `VerifiedReadContinuation` belongs to a coordinator session with one live successor, selected prefix, verified source cursors, read lease and semantic state binding; caller-supplied cursor fields cannot skip work or recreate a semantic state. The next wave must implement actual bounded state/page traversal and continuation ownership, cancellation and release. A digest of state is not itself the state or a replay proof.

Every byte/page/segment and dependency/semantic comparison step is charged; INCOMPLETE never carries a successful comparable total. Persistent backend snapshots/version/WAL retention have a bounded separately charged lease and release on finish/drop/cancel. Comparison policy substitutions retain actual ALLOW/DENY, chronology, original base, authority and adjustment/correction terms; the first infeasible prefix has no partial comparable total. No posting, authoritative alias, capacity allocation or external dispatch arises from the read interface. Actual observed attempted-write and external-effect tests remain required.

## Verification and stop

Focused checks round-trip all29 command kinds from frozen positive traces, reject malformed shapes/scalars/Base64, exercise exact q/R boundary failure without mutation, preserve the legacy encoder limit while testing R3's separate bound, bind full proof/source/key and ExpectedPrefix identity, and verify stable old lock tags/shared admission order. Compile and clippy checks cover both affected production crates; root owns the assembled full repository check.

The implementation waves must supply the actual semantic executor, host factories, durable SQLite/PG17/PG18/gateway adapters, all16 R3 runtime rows and REG01–REG09, interruption/restart/unknown/race and physical resource evidence, then genuine stored comparison and independent final acceptance. This commit stops for independent typed-seam review. Interface/codec tests do not satisfy those remaining gates.
