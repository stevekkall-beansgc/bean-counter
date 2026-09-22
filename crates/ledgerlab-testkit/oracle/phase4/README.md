# Independent Phase 4 first-slice oracle

This is testkit-only arithmetic and assertion machinery based on the independent
Phase 4 plan and its 66-scenario table. Base commit:
`be291388492f82485cfc1b51f46c9ad7a2e1b9f9`. It is **not a Phase 4 PASS**, a host
authorization implementation, a store adapter, or a new canonical encoding.
All fixture identities and evidence are synthetic test labels. No real assent
is inferred. No Rust evaluator/output generated the expected amounts.

Run offline:

```sh
python3 -B -m unittest discover -s crates/ledgerlab-testkit/oracle/phase4 -v
cargo test -p ledgerlab-testkit --test phase4_oracle --locked --offline
sh scripts/check.sh
```

The Rust entry runs the independent Python tests within the normal workspace
suite. Python needs only its standard library; no network or new package.

## Authorship and bounds

`stories.json` contains literal input stories, explicit six-role/book/binding
provenance, fixed original retail basis, limits, membership, synthetic authority
assumptions and literal expected arrays. JSON atom and ratio values are strings.
`arithmetic.json` independently records ten exact fraction/rounding boundary
vectors. `reference.py` uses Python Fraction/integer arithmetic only. It is a
closed test-story evaluator, not another production pricing language.

The approved scope narrows the earlier design plan: only 2–8 existing fixed or
signed-percentage alternatives on one fixed booked final base. Candidate
membership, roles, evidence, windows, limits, activity, currency/scale/unit and
supplier terms must equal the source. F05 changed base and F07 added family are
unsupported, regardless of the earlier plan's illustrative hypothetical totals.
A01/A04/A06–A11/A16–A21/A28 concern legacy base/cap/cost surfaces and do not
expand this comparison slice. A23/A24 exercise original retail denominator
arithmetic and capacity boundaries; they do not authorize changing supplier
terms. No base is rerated by this oracle.

P1/P2 explicitly inherit P0's permitted fixed correction code +1000 and zero
code; only retail success/quality amounts vary. That is fully specified fixture
data, not guessed candidate consent. Full reversal stays distinct from a
zero-valued replacement code. The original supplier bonus/zero rules stay
identical in every candidate, including never-used membership.

The principal chronology is base → retail F → retail R → supplier bonus →
retail F-to-zero → supplier-to-zero → retail F-to-1000 → retail F reversal →
retail F reinstatement → supplier closure. Every correction refers to its exact
next claim revision; simultaneous timestamps represent ordered retained history.
The original base has no predecessor; the empty predecessor list is explicit.
A source with a predecessor is unsupported in this first slice, not silently
truncated. Source capacity (held/consumed/released), retained independently of
candidate economics, is:

| Prefix | Held | Consumed | Released |
|---|---:|---:|---:|
| Base and retail F/R | 12000 | 3000 | 0 |
| Supplier ordinary +2500 | 9500 | 5500 | 0 |
| All corrections, zero, reversal, reinstatement | 9500 | 5500 | 0 |
| Explicit supplier closure | 0 | 5500 | 9500 |

Supplier correction -2500 returns supplier live economics to 3000 but restores
no capacity. Every row remains nonposting. A core implementation may expose a
separate **projected** numeric reservation mirror; it must agree here because
supplier terms/activity are unchanged, and must never become source transitions.

Original replay selects P0 only. Hypothetical selects candidate terms without
activation. Future-terms-description lists prerequisites without creating a
future target. Authorized-correction-description is original-term explanation
only; actual correction requires the separate existing ordinary acceptance route.
The results never contain new canonical receipts, actions or runnable intentions.
`nonzero_components` names hypothetical arithmetic components, not posting DTOs.

## Coverage and evidence levels

`coverage.json` preserves every F01–F12/A01–A29/D01–D25 story and original plan
expectation verbatim, alongside first-slice scope and evidence classification.
This is **66 mapped scenarios**, not 66 executed product scenarios. A local-oracle
entry means some bounded arithmetic/assertion aspect is exercised; it does not
mean the entire compound scenario passed against a product. The 30 Python tests
cover 27 principal candidate-step results, ten extra rational vectors, all six
P0/P1/P2 permutations, A/B/A, B/A, duplicate candidates, a fresh process, failed
candidate isolation, exact output leaf assertion sensitivity, and protected
inventory sensitivity. Fixture input times are retained explicit data; the oracle
has no clock/current-price parameter.

`no-change-matrix.json` carries N01–N16 with **integration-required** evidence.
`observer.no_change` requires exact B0/B1/B2 equality of all 33 SQLite tables or
37 PostgreSQL tables, column inventory, typed cells (NULL distinct from text or
absence), schema/index metadata, migration/control values and destination state.
Its table inventory is independently checked against all current migrations.
Synthetic tests mutate every table in five ways, mutate every metadata group,
and inject every forbidden attempt. These prove checker sensitivity, not actual
SQLite/PG nonmutation. Zero-sum write/restore requires attempt monitoring even
when B0/B1/B2 compare equal. Real held/leased/retrying/quarantined outbox states,
concurrency, cancellation, process death, and restricted-reader evidence remain
integration gates.

## Adapter seam and remaining gates

`observer.run_adapter` requires `inventory()`, `compare(candidates)`, `reopen()`,
`backend` and `attempts()`. It never passes numeric expectations into compare.
Actual results are normalized to the test-only observation fields and matched
against literal arrays by candidate digest and exact source provenance. An
adapter can call `assert_result` directly if its lifecycle harness differs.
Comparison-local hashes use `phase4-test-story/1` plus deterministic JSON; these
are explicitly **not** canonical v1/v2 hashes, records, IDs, or receipt anchors.
Display labels are excluded from candidate identity; economic rules are included.

The synthetic source digest is held externally during complete-rehash attacks.
Real integration must independently verify canonical retained source bytes and
original anchored receipt membership using the existing canonical audit, before
normalization. A newly computed attacker-supplied digest is never an authority
anchor. The normalized projection cannot replace full retained base evaluation,
ingress, evidence, original versions, frozen membership, scoped identities,
receipt pairs or book-specific postings. Bind all those inputs to one coherent
snapshot in the actual adapter and retain the observation separately. Missing
material/semantics must refuse, not fetch, repair, use latest policy or partially
sum history. This testkit does not read a real database or simulate host read
permissions; read-denial/revocation and public forged proof tests remain owned by
coordinator integration.

Window boundary/evidence/authority verification, original retry receipt access,
semantic alias deduplication, bounded cancellation, snapshot races, malformed
public DTOs, promotion into acceptance/dispatch, secret-canary isolation and
real-store attempted-write tracing need integration adapters. No schema adapter
may rewrite a literal expected amount to fit Rust. Public error-code mappings
must be explicit: uppercase oracle categories are test categories, not newly
frozen API codes. Any underdetermined financial meaning stops fixture extension.

No frozen file, production economics, adapter, shared manifest or lockfile is
modified by this lane. No Phase 5/6, public host claim, release or deployment.

Plan provenance (SHA-256 of external design inputs):

- `PHASE-4-ORACLE-PLAN.md`: `e4d4530fef75f7a1963eaa384f1b97d3a4528780f7273ce595125e5e36997f6c`
- `PHASE-4-FIXTURE-TABLE.md`: `1a9dcdb7978924f4c818d0fb23ba11297b86b05a529aac67c35da2ae2274d90f`

Coverage classifications: 32 local-oracle aspects, 17 scope-refusal specifications,
17 integration-required scenarios, plus all 16 N-matrix schedules still requiring
integration execution. Sensitivity rejects 270 altered result leaves and ten
omitted top-level fields. Inventory sensitivity rejects 535 per-table mutations,
nine reopened metadata mutations and twelve forbidden-attempt traces.
