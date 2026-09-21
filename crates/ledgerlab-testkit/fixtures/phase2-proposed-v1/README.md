# Phase 2 conformance proposal 1

**Proposed, synthetic, unfrozen.** `ledgerlab-phase2-proposal/1` is a test-history
format, not `ledger-event/1`, a production policy DSL, a receipt encoding, or a
new accepted canonical record family. Review before integration. Do not add this
directory to `contracts/freeze.json` without an explicit contract amendment and
independent review. Existing Phase 0/1 files remain unchanged.

The [schema](history.schema.json), 29 JSON histories and
[test-only reference](../../oracle/phase2/reference.py)
make the core stories inspectable without running a database. The reference uses
Python's existing standard-library `Fraction` and a list of accepted decisions;
it does not import, copy, invoke or parse a production pricing implementation.
Expectations were specified separately as explicit amounts, rational rounding
results, dependencies and outcomes. Tests only read them; there is no golden-update
command. The existing `jsonschema` audit dependency validates the fixtures.

## Authority and scope

The detailed design §§5–11/24/26/27, ADRs 007/008/014/016–018 and the canonical
addendum control existing semantics. The addendum freezes **completion only**;
acquisition, supplier, invocation and reversal encodings need reviewed extensions.
The latest implementation checkpoint is `PHASE-1-REVIEW-FIXES.md`, not the older
scaffold descriptions in some READMEs. Its original 80 atoms, 25 new immutable rows,
29 manifest members and all 99 frozen files remain separately audited.

| Story | Examples and proposed expected economics |
|---|---|
| Generation bills immediately | `generation`: +100−20=80; no future outcome dependency |
| Explicit later conversion/acquisition | `later-acquisition`: generated100 → published50 → acquired500; total650, reverse acquisition →150 |
| Later failure/quality discount | `later-quality`: generated10.5→11, linked quality discount−5.5→−6; total5, full reversal→0 |
| Paid external service | `pay-per-service`: retail call10 and supplier15 in separate obligations; generation100 explicitly consumes that service |
| BYOK and platform funding | `funding-*`: retail10; BYOK no host model cost/payable, platform known cost observation20, unknown cost explicitly unexplained rather than inferred as zero |
| Customer tier | `tier-*`: standard100; accepted enterprise rate80 less10%=72 |
| Multi-stage cap/share | `multi-uncapped`: retail335/supplier80/observation20; `multi-capped`: retail315/supplier75/observation20 |
| Cap and share boundaries | cap not binding; cap below prior135 rejects; cap135 nets closure to zero; share ceiling reduces50→10; multiple shares reject |
| Retries and dependency arrival | Original and renamed acquisition retries return the same receipt alias; reverse arrival waits, explicit retries settle; six DAG permutations tested |
| Reversal and immutable history | Explicit whole decision targets and economic dependency closure; preserve all originals, consumed exposure and closed stage; incomplete/unauthorized/duplicate reversal negatives |
| Invalid authority/input | Missing/ambiguous/mistyped/self links, wrong source/customer, missing endpoint, conflicting outcome authority, missing assent/evidence/invocation, retroactive invocation, exposure overrun, absent payer delegation and unknown config/policy/semantics |
| Exact arithmetic | Per-component zero rounding, signed ties, booked-net discount, magnitude overflow and legacy rational vectors |

**The quality row is a new semantic proposal.** Frozen v0 has no quality outcome
kind or `quality_of` relation, and failed completions MUST remain zero-action
acceptances. `proposal.quality_failed` is therefore disabled unless the synthetic
context explicitly enables it. It proposes one nominated quality source, retained
evidence, one explicit work target, the same half-open occurrence window as other
outcomes, and at most one quality adjustment per target. Its discount uses the
original target's **booked retail net**, rounded once; it cannot erase supplier
fees or rewrite that work. This first proposal only permits unstaged chains; combining it with a cap stage
rejects as `QUALITY_STAGE_UNRESOLVED` (a closed stage already rejects new originals). Whether this extension belongs
in public v0, its final name, claim namespace, window and correction interaction
remain review decisions. It must never silently enter the frozen event/DSL enums.

## Reading the format

Each file contains trusted `config`, named `events`, ordered submission `attempts`,
and fully enumerated `expected.results`, `expected.journal` and `expected.state`.
All money is integer-atom strings; unit prices and percentages are decimal strings.
Currency/scale are pinned in config. Time fields are injected unsigned **relative
microseconds from the synthetic scenario epoch**, not production timestamp syntax.
Accepted decisions retain the attempt's `received` value for eligibility replay.

- IDs are readable **fixture aliases**, scoped to one synthetic installation and
  chain. They are not SHA-256 production IDs. `receipt` names the original decision
  alias, whose full body remains in `journal`; duplicates add no decision/revision.
- Links always point child→predecessor and name relation, source and event alias.
  Required absent links reject; explicit but unavailable endpoints wait. No causal
  or customer/chain-based attribution is inferred.
- Config identifies accepted retail terms, assent, registered source grants,
  supplier offers, all six responsibility roles, pinned invocations and evidence.
  These are small **resolved authority facts**, not signature verification or a
  real assent repository. The authenticated principal→source mapping, head locking,
  grant revisions and document authenticity remain coordinator responsibilities.
- `authority_refs` identifies the facts used. `inputs` names specific booked
  posting bases; `depends_on` names whole prior decisions whose **economics** were
  used (quality basis or stage prior net). A plain lineage edge alone does not
  assert that a fixed price depends on the predecessor's amount.
- Postings/explanations follow base/observed cost → premium → discount → cap → share;
  supplier agreements sort by UTF-8 bytes. Reversals preserve original decision and
  posting order. This projection is not the canonical journal's kind/ID sort.
- Nonzero retail/supplier totals create per-binding obligation deltas. Observations
  create no payable; zero-net obligations create no instruction. `totals` sums
  **recorded** atoms per book, not an assertion that unknown external costs are zero.
- Waiting, rejection and conflict preserve the complete journal and counters.
  Every successful step appends exactly one projected decision. Consumed invocation
  exposure is never replenished by a reversal; no pending/delivery storage is modeled.

The calculator evaluates a closed family of story equations, not arbitrary policies:
`base=round(unit_price×quantity×10^scale)`, tier discount from that booked base,
quality discount from the named prior retail net, `credit=−max(0,P+C−cap)` with
`cap≥P`, and `share=min(round((C+credit)×percent/100),ceiling)`. Signed rounding uses
`sign(x)×floor(abs(x)+1/2)`. Reversal negates every selected stored atom without
re-evaluation. Configurable tariffs use `story-tariff/1`; context uses
`story-context/1`; unknown versions fail closed. Identity and semantic duplicates
resolve before current tariff validation, while current source/receipt rights still
apply. Sources are assumed canonical economic source registrations.

## Running and integrating

```sh
# Use the repository's existing activated Rust/Python audit environment.
python3 -B crates/ledgerlab-testkit/oracle/phase2/test_reference.py
cargo test -p ledgerlab-testkit --test phase2 --locked --offline
sh scripts/check.sh
# Optional read-only reference projection, never overwrites expectations:
python3 -B crates/ledgerlab-testkit/oracle/phase2/reference.py \
  crates/ledgerlab-testkit/fixtures/phase2-proposed-v1/multi-capped.json
```

The normal workspace check runs 13 Python tests through the new Rust test entry.
These verify 29 histories/86 submissions, full expected projections, append-only
prefixes after every step, six delivery permutations, assertion sensitivity,
rounding/bounds, historical retries and legacy compatibility. Existing audits still
verify the original canonical bytes and exact fixture inventory.

For integration, first review the proposal, then build an adapter from resolved
production inputs/results to these aliases and component projections. Compare every
posting (roles/book/binding/amount/bases), explanation rational, obligation and
per-step outcome/history—not just totals. Keep the oracle outside production and
do not change expectations merely to match engine output. Next define the reviewed
canonical record extensions and independently verify their IDs/bytes/manifests;
these aliases are not a substitute for that gate.

Remaining coverage is explicit: no general DSL compiler, priority/exclusive groups,
additive/sequential discount programs, allocation views, late link assertions,
replacement events, or delegated-payer acceptance model here. Those frozen rules
remain applicable; this small proposal does not redefine them. Invalid unequal
bearer/payer without delegation rejects, but valid delegation needs a later resolved
fixture. Supplier authority tests cover pinned pre-work invocation/quantity/exposure,
not the full reserve/hold/release/expiry state machine or revocation races. No real
store, pending promotion, commit ambiguity, canonical serialization, CLI/HTTP/UI,
export/payment or native platform conformance is established by these new tests.

Validation on 21 September 2026: activated Rust 1.98.1 (`RUSTUP_TOOLCHAIN=stable`)
and the existing Python audit environment; `sh scripts/check.sh` passed **68 Rust
test entries**, zero failures, and 13 Phase 2 Python tests. The 11 existing ignored
entries remain: nine opt-in PostgreSQL tests and two deferred outbox/fake gates.
No opt-in PostgreSQL service was started for this testkit-only task. Formatting,
Clippy, no-default build, boundary checks and all 99 frozen file audits passed.
The initial placement under top-level `fixtures/` correctly failed its frozen-file
inventory guard; proposals were moved here, with that guard unchanged.
