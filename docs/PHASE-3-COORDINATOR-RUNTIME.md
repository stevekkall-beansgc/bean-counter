# Phase 3 bounded coordinator runtime

This lane implements the private reservation-settlement `PORT.md` seam from
the frozen-unreleased contract at `1170ddc13e8f4e799d000a842cb978b6aa2adebb`.
It covers one final base, ordinary supplier outcomes, authorized corrections,
and explicit reservation closure. It does not integrate either store lane,
publish a submission API, or claim an integrated release.

## Decoding and decisions

The pure core decodes the exact frozen economic and settlement envelopes and
reconstructs their IDs and hashes. A retained original evaluation is accepted
only after recompiling its original inputs, rerunning the unchanged evaluator,
and comparing the complete serialized evaluation. Evaluation outputs cannot
be constructed by deserialization. The facade bridges original identities and
projects the existing pure outcome decision into the complete frozen record
set; it does not add another pricing evaluator.

Every retained target partition is checked against its immutable membership,
base bridge, economic replay, complete settlement prefix, and locked current
heads before either appending or returning an original receipt. Historical
policy snapshots govern replay. A later binding selector does not rewrite
history or hide a receipt from a currently authorized reader.

Only an original ordinary positive supplier amount consumes held capacity.
Zero and negative ordinary results still permanently claim the family slot.
Corrections, reversals, reinstatements, and zero-net inverse/replacement pairs
leave the complete reservation state unchanged. Authorized closure releases
remaining held capacity; deadline closure requires time strictly after all
inclusive ordinary deadlines. A later no-op closure retains a new observation
and receipt, advances the target history index, and leaves reservation and
economic heads unchanged.

## Private interfaces

`store::outcomes` owns `OutcomeStore`, `OutcomeTx`, `OutcomeResolve`, canonical
scoped references, ordered lock classes, observed heads, head writes, and
`StoredCompositeDelivery`. The coordinator alone constructs
`ValidatedOutcomePlan`. Adapters receive getters for complete economic and
settlement bytes, the original composite delivery, resolution, immutable
anchors, expected heads, and writes. They persist that plan atomically and
perform no economic or authorization decisions.

The lock order is Admission, Authority, Binding, Reservation, Target, Claim,
BindingAggregate, InvocationConsumption, BaseReversal, then canonical scoped
key bytes. Each write requires an exclusive held lock. Additional lock
discovery restarts only after confirmed rollback. Expected-current races may
retry only after confirmed rollback; unknown commit never automatically
resubmits. Retry uses the original delivery identity to resolve the outcome.

Operational partition keys are scope, target, and invocation. The family key
is canonical JSON `[scope, agreement, family, target]`. Delivery identity is
shared with the legacy path: `DeliveryConflict` is an identity conflict, not a
retryable store failure. Receipt pairs are retained verbatim. Same-identity
retry works for every operation; renamed semantic aliases apply only to
ordinary claims and retain the original receipt pair after corrections and
closure. A renamed correction is a new correction and must satisfy its own
expected-current revision.

Head values are canonical JSON:

| Class | Value and revision |
| --- | --- |
| Authority | Current active grant reference; host-provisioned positive revision. |
| Binding | Current active binding reference/selector; host-provisioned revision. |
| Target | Base, registration, and complete retained record references; starts at zero and advances per new acceptance. |
| Reservation | Exact frozen reservation result; revision equals that result's revision. |
| Claim | Latest claim revision and first economic receipt references; revision equals pure claim number. |
| BindingAggregate | Sorted latest claim revision references; starts at zero. |
| InvocationConsumption | Target and registration references, written once at registration. |
| BaseReversal | Initial unreversed guard; observed for later outcomes. |

`OutcomeAuthority` is a mandatory host verifier, with no permissive production
implementation. It must verify authentication, current grant, source,
principal, frozen terms, assents/offers/delegations, finality and early-close
rights under the provided locked observations. The coordinator validates the
proof's scope, references, current grant/revision and operation permissions.
Only tests provide synthetic authority proofs. Base registration also ties
the original evaluation's authority and observation times to this proof.

## Validation scope and remaining gates

Core tests reconstruct all 1,450 frozen economic envelopes and 112 settlement
envelopes, and replay all 23 retained original evaluations. Facade tests bridge
all accepted frozen bases and reproduce all 43 frozen economic decisions.
The genuine four-step fixture evaluates fresh inputs through production core
and constructs plans through the production coordinator: 12,000 held,
ordinary +2,500, correction to zero with 9,500 still held, then explicit
9,500 release. Its final reservation remains consumed 5,500, held zero,
released 9,500 against maximum 15,000.

The focused facade suite includes twelve protocol tests plus the three bridge,
projection and fixture tests. It covers stable identity, ordinary aliases,
zero results, corrections after closure, reversals/reinstatements, inverse
and replacement retention at zero net, missing companions/corrupt heads,
expected-current rollback/retry, lock discovery, unknown commits with either
durable outcome, both ordinary/closure winner orders, strict deadlines and
no-op closure. These protocol tests use an in-memory transaction harness;
real database durability evidence belongs to the separate adapter lanes.

The unchanged full offline gate passed: 145 Rust tests passed, zero failed,
and 19 environment-dependent tests remained ignored. Formatting, strict
Clippy, feature checks, architecture checks, all legacy contracts, reservation
freeze metadata, and both independent reservation validators passed. The
reservation validators cover 12 histories, 44 steps, 112 records, 47 adversarial
cases, nine strict-JSON negatives and 13 non-appending lookup cases. Freeze
checks confirm all 159 legacy files, 13 reviewed reservation files and five
control artifacts, including 20 negative metadata probes.

Remaining gates are the real host authority/provisioning implementation,
public submission wiring, reviewed integration with SQLite and PostgreSQL,
and integrated conformance/fault testing. The base bridge is intentionally
bounded to one final base with no predecessor evaluation chain. Broader base
chains require separate work. Frozen bytes and monetary semantics are
unchanged; this is local implementation evidence, not release approval.
