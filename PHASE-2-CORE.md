# Phase 2 bounded pure-core pricing slice

This slice implements typed pricing and explicit event-chain evaluation in
`ledgerlab-core::policy::chaining`. It does not extend the Phase 1 acceptance
service or claim that Phase 3 persistence/authority races are implemented.
No database, HTTP, UI, cloud, model, clock, environment, or new dependency is
introduced. The existing three production crates and frozen contracts remain.

## API surface

- `Bundle::compile(currency, scale, Vec<Policy>)` validates the complete pinned
  policy/binding set, then freezes deterministic phase/binding/agreement/rule
  order. Input structs are configuration data; the compiled bundle and output
  action fields are private. Public field values are validated at the boundary.
- `Bundle::evaluate(Input)` consumes an already normalized `domain::Event`,
  immutable `Context`, complete chain `Evaluation` history, current
  `SourceAuthority`, historical `Invocation` inputs, optional `CostEvidence`,
  and injected receipt time. It returns economic actions, structured exact
  explanations, per-obligation deltas, consumption/release proposals and a
  stage-closure marker. It returns no receipt or runnable destination intention.
- `reverse(event, history, current_source_authority)` has no current-policy
  argument. It negates complete original decisions exactly, including stored
  allocation entries, only with their original correction sources and complete
  live economic dependency closure. Reservation consumption and closure history
  remain intact. Already reversed or zero-economics targets fail explicitly.
- `Roles::new([provider, cost_originator, bearer, payer, beneficiary, recipient],
  delegation)` plus read-only role getters are additive existing-domain APIs.
  There is no change to role serialization or the Phase 1 parser.

`Policy`, `Binding`, `Rule`, `Price`, `Operation`, `Predicate`, `Matcher`,
`Context`, `Stage`, `ExpectedOperation`, `OutcomeTerms`, `Invocation` and
`CostEvidence` are typed data. Strings identify components and accepted records;
they do not evaluate expressions. The deliberate lack of Serde on the new policy
API prevents it from silently becoming a new `ledger-policy/1` wire variant.
Public application ingestion can continue to use the small frozen event DTO.
The expanded authority inputs belong in the shared coordinator, not customer
request bodies or database-specific pricing code.

## Implemented behavior

Generation can book a fixed or unit price immediately. Subsequent publication or
approved acquisition can append a fixed/unit/named-percentage premium after an
explicit typed link. Trusted tier, priority, funding and source predicates select
prices/discounts. Failed work never prices. A skipped or rounded-zero rule emits
an explanation and no action; a +100/-100 decision keeps both action provenances
but produces no obligation delta.

Discounts use fixed amounts or percentages of named rounded bases/premiums.
Additive discounts retain the original base; sequential discounts use the
remaining net and retain intervening dependencies. Their combined reduction
cannot exceed the component. Unit products and percentages use the existing
bounded exact-rational implementation and signed nearest/ties-away rounding.
No float, aggregate rerounding, hidden floor or money saturation is introduced.

A paid tool completion can create separate retail and supplier obligations.
Supplier terms require offer/assent references, explicit six-party roles,
maximum exposure and a matching historically authorized invocation. Attested
start controls work eligibility; report arrival does not retroactively revoke
an authorized call. Work consumes held capacity, contingent outcome capacity
remains held, and qualifying outcome/failure/end-of-noncontingent-work releases
its remainder. Invocation identity/limits cannot be enlarged using later input;
recorded consumption/release cannot be replenished by reversal.

BYOK suppresses host provider-cost observation and gives BYOK_NO_HOST_COST.
Platform mode uses only an explicitly retained known cost; absence produces
COST_UNKNOWN, not a fabricated zero or payable. Separately accepted tool fees
remain payable even when model funding is BYOK. Retail pricing can branch on
funding. Roles remain accepted binding data; beneficiaries never become payers
by inference. Different bearer/payer requires a delegation reference, whose
actual scope/assent is verified by the coordinator.

The one closure cap uses the pinned stage's complete expected operations and
retail component set. Prior P, current booked component net C, and ceiling obey
`credit = -max(0, P+C-cap)` with `cap >= P` and `abs(credit) <= C`. Missing required
inputs wait only when closure is submitted; generation does not wait for future
outcomes. Inactive cap emits CAP_NOT_BINDING without a zero credit. Its prior
basis still participates in reversal closure even without an emitted action.
Closing freezes new originals; reversal does not reopen the stage.

The one supplier share reads the named retail closure net after its cap, rounds
once, then applies the integer ceiling. It adds no retail charge. Optional
allocation entries partition that net into supplier share and host remainder;
they reference the nonzero governing share and never add another payable. A zero
share emits no action or parented allocation. Allocation
recipient is explicit in this typed proposal; its future wire encoding is not
invented here.

Links resolve only explicit source/event endpoints in the same scope, chain and
customer. They enforce type/cardinality, <=16 graph hops and <=128 incoming
children. Matchers allow the fixed direct paths and acquired -> published ->
optimized path. Multiple possible service endpoints fail as ambiguous; no first
match silently wins. These links are supplied commercial context, never an
inferred causal claim. Semantic completion/acquisition and original effect/action/
obligation IDs use the detailed-design formulas; acquisition operation labels
cannot evade an authoritative claim token. The frozen first-slice economic IDs
remain identical.

## Explicit typed-only proposal: linked outcome discount

The requested later negative outcome adjustment needs a prior booked basis;
the frozen DSL currently only names current-decision discount bases. This slice
therefore introduces **`Operation::LinkedDiscount` only in the typed API**.
It requires an acquisition, approved source/window/evidence, an explicit supported
matcher, accepted retail terms, and a named base/premium on that predecessor.
It appends a new negative effect against the original rounded amount, accounting
for all unreversed prior reductions. It never changes that original posting.
A reversed reduction restores exactly its own discount capacity.

The supported acquisition path reaches a publication (or optimization via the
fixed two-hop path); generation remains connected by explicit lineage. This is
not a new acquisition -> generation relation. Combining linked discounts with a
closure cap fails compilation: prior-stage negative adjustment semantics need a
reviewed contract before that composition can be enabled. This proposal does not
add a generic credit editor, arbitrary predecessor search or automatic rerating.

Review inputs live in `crates/ledgerlab-core/tests/proposed/`. They are outside
`fixtures/` because that directory's complete inventory is frozen. No freeze
manifest, schema, existing fixture, original ADR or source-design byte changed.

## Integration requirements and limits

1. Preserve the existing Phase 1 `CompiledPolicy`, `ResolvedInput`, evaluator,
   canonical record assembly and store append boundary. The new `Evaluation`
   cannot be appended through that port. No existing acceptance behavior was
   broadened. Identity/semantic retries must still return the original receipt
   before resolving current prices or invoking this evaluator.
2. Resolve the complete pinned binding/context set, canonical endpoint aliases,
   retained evidence, full bounded chain history, all relevant authorities and
   reservation heads under the established ordered locks. The kernel checks
   supplied data; it cannot prove that a caller omitted nothing or that an
   arbitrary document ID represents real assent. Authenticate the principal,
   check current grant validity/revision, verify real assent/offer/delegation
   scopes and limits, and retain original invocation evidence. A provider
   receipt or link is never permission to construct these inputs.
3. Freeze/review canonical acquisition/reversal facts, expanded snapshots,
   explanations, allocations, stage and invocation/reservation transitions,
   and complete manifest/receipt encoding before integrating persistence.
   Preserve the seven Phase 1 snapshot purposes and all original bytes. Treat
   the linked-discount operation as a proposed semantics extension requiring
   explicit review; do not smuggle it into `ledger-policy/1`.
4. Persist the entire decision atomically. Convert per-obligation nonzero deltas
   into immutable intentions only in the shared assembler. Observations and
   allocations never create obligations. Include explanation-only economic
   dependencies (notably CAP_NOT_BINDING) in retained snapshot/reversal closure.
   Guard claims, closures, invocation consumption and unique reversals in the
   transaction. Do not interpret a pure result or Waiting error as accepted.
5. `Input.history` currently accepts pure `Evaluation` values, with private
   action fields; no unverified stored-action import constructor exists. Keep
   results as authoritative history only after their containing decisions
   commit. A retained-record decoder/replay bridge belongs with the reviewed
   encoding extension. Do not rerate booked history using current terms.
6. Late link assertions, exclusive-rule groups, YAML/preset parsing, pending
   persistence, invocation expiry control commands, new wire generators and
   actual acceptance/revocation/reversal races remain later slices. The stage
   API deliberately supports only one pinned retail closure cap, one share and
   no reopening. Exact serialized bundle/explanation byte limits must be checked
   by the eventual assembler; the kernel enforces typed counts, identifier/math
   limits and a conservative retained-history size guard.

No real-store Phase 2, PostgreSQL-server, release, platform, performance or MSRV
certification is implied. No push, merge, deployment or publication is part of
this slice.

## Validation

Focused tests cover the frozen 335/315-atom examples and six roles, the original
80-atom first-slice effect/action/obligation IDs, exact unit rounding, additive
versus sequential discounts, net zero, funding predicates, unknown cost,
separate pay-per-tool obligations, half-open outcome windows/report grace,
claim aliases, source/link authority, supplier exposure and immutable invocation
limits, stage closure/waiting/cap bounds, share ceiling/allocation, full reversal
closure, already reversed targets, closed-stage replacement refusal, bounded
ambiguous paths, and rule/price-independent effect identity. An exhaustive
integer oracle checks 1,111 base/percentage combinations and exact cancellation.
The proposed examples use hand-authored expected values.

Final validation: **25 focused tests passed**; `sh scripts/check.sh` passed
**92 Rust test entries, zero failures, 11 explicitly ignored later/opt-in gates**,
formatting, warnings-denied Clippy, no-default compilation, dependency/source
boundaries and independent frozen-contract audits. The 99 frozen entries,
60 hash vectors, 25 first-slice rows and 29 manifest members remain unchanged.
The opt-in PostgreSQL server and later destination gates were not run for this
pure-core change.

Commands (the activation script is local ignored setup, copied from the supplied
verified toolchain; no Xcode license acceptance or toolchain install is needed):

```sh
source work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
cargo test -p ledgerlab-core policy::chaining --locked --offline
sh scripts/check.sh
```

The existing check script is not executable, so it is invoked with `sh`.
The Python path activates the repository's pre-existing independent audit
packages. Initial execution without that path passed Rust checks but stopped
on missing `jsonschema`; it was rerun with the documented environment.
