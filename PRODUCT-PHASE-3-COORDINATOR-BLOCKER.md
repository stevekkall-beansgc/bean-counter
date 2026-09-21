# Product Phase 3 coordinator: contract stop

Status: the owner resolved the missing semantics on 2026-09-21 and authorized
an additive candidate amendment. Production implementation remains gated on fresh
independent review. This report preserves the original discovery below.

## Owner resolution and candidate

Ordinary supplier outcomes may consume only their authorized positive result from
held capacity. Explicit closure releases remaining held capacity. Every post-hoc
correction/reversal/reinstatement leaves maximum, consumed, held, released and
reservation revision unchanged, while frozen economic ceilings and correction
authority remain mandatory. There is no correction high-water model. Early
closure needs separately verified closure authority and retained evidence;
deadline closure occurs strictly after the latest inclusive ordinary acceptance
deadline across the frozen eligible set. Zero/discount ordinary results do not
auto-release. Base-work replacement remains separate.

The unfrozen candidate is `contracts/candidates/reservation-settlement-v1/`:
three closed records, numeric histories, canonical vectors, adversarial inputs,
and proposed private port. Read-only Python and independent Node/BigInt checks
live in `scripts/reservation_settlement/`; run
`sh scripts/check-reservation-settlement.sh`. No frozen registry or validator was
changed. No production coordinator, port, SQL or migration is implemented yet.
The old stop condition is resolved by explicit owner authorization to author this
new candidate, not by treating old snapshot fields as new transitions.

## Exact starting point

- Local source: `/Users/stephenkall/Documents/Codex/2026-09-21/ledger-lab-p2-integration-owner/work/ledger-lab`.
- Clone: `/Users/stephenkall/Documents/Codex/2026-09-21/ledger-lab-p3-coordinator/work/ledger-lab`.
- Branch: `codex/p3-coordinator`.
- Reviewed base: `6194376a053b8a27887a9b09459054a7af3a1769`.
- Clone used a local filesystem source with `--no-local`; no network access.

The assignment requires supplier-capacity and reservation guards, forbids
inventing an encoding, and explicitly says to stop if a required record is absent.
The frozen profile is `2-candidate.4`; historical candidate labels do not undo
the approval recorded in `PHASE-1-FREEZE.md` and the freeze registry.

## Exact blocker

There is no frozen canonical **outcome invocation/reservation consumption or
release transition** for the supplier-reservation portion of the requested path.

Evidence in the reviewed base:

1. Detailed design §9 (source line 522) requires supplier outcome fees to consume
   remaining held capacity, consumption and actions to commit together, and
   reversals not to replenish authorization. Section 11 (lines 735–747) requires
   ordered reservation/consumption locks and reservation transitions in the
   complete atomic acceptance plan.
2. `docs/design/CANONICAL-RECORDS-V1.md:18` explicitly leaves invocation/reservation
   transition encodings for a reviewed extension and forbids unlisted variants
   by analogy. The frozen v1 schema's `control-transition.control_kind` is the
   constant `chain`; its body has chain event counts, not reservation amounts.
3. The frozen v2 schema's top-level `oneOf` contains exactly 27 envelope kinds.
   None encodes an invocation/reservation transition. The `invocation` definition
   (line 2789) retains the original `held` observation. `base-consumption`
   (line 4568) retains `invocation_id`, `consume` and `release` inside the original
   base Evaluation. Neither encodes a subsequent outcome/correction reservation
   change with its authoritative current head and immutable audit identity.
4. The v2 design's “Revisions, capacities, receipts and replay” section requires
   mutable supplier guards and prohibits replenishment, but does not supply the
   missing transition body/identity/membership rule. `limit-evidence` records
   economic aggregate bounds and current claim revisions; it is not an
   invocation reservation-consumption audit.

A snapshot of original held capacity cannot prove that capacity remains held at
this acceptance or preserve the audit of subsequent consumption/release. A new
SQL row, private Rust enum, or generic JSON append would not resolve the missing
canonical contract. In particular, the coordinator must not infer correction or
reinstatement consumption semantics from signed monetary deltas.

This finding is specifically about the required supplier-reservation behavior.
It does not claim that the frozen retail outcome/correction records are absent.
Silently reducing the assignment to retail-only acceptance, or rejecting every
supplier case while claiming all requested guards, would leave the assigned
scope incomplete. A separately authorized narrower retail-only task could avoid
this blocker; this report does not make that scope change.

## Required resolution

An owner-reviewed contract amendment must define the missing transition's exact
body, identity/hash domain, original invocation and event references, revision
comparison, before/after reservation accounting, and inclusion in atomic
membership/replay/receipt validation. It must define the permitted consumption
behavior across corrections and reinstatements without reopening spent capacity,
and supply independently checked byte and history fixtures. This report proposes
no encoding or accounting rule and changes no frozen asset.

## Adapter expectations after resolution

These are existing design requirements, **not a newly agreed Rust ABI**:

- One transaction boundary with SQLite tracked immediate transactions or
  PostgreSQL serializable transactions, bounded cancellation/commit handling,
  confirmed rollback versus unknown commit outcome, and original-identity lookup.
- Ordered scoped locks: admission; authority; binding selection/revocation;
  reservations; chain/stage; invocation consumption; reversal guards. Target,
  claim and binding-aggregate guards must participate in a single agreed order;
  absent heads cannot be protected by locking nothing. More-lock discovery
  restarts within the existing bounded attempt/expansion budget.
- Complete retained canonical records and the original base acceptance anchor,
  all frozen eligible members, original receipt bytes, identity/alias and first
  claim receipt lookup, complete current claim heads and aggregate membership,
  authority revisions and supplier reservation state. Stores return observations;
  the shared coordinator validates authority and the pure core prices.
- A coordinator-constructed validated plan appended atomically with immutable
  records, receipt, original delivery mapping or operational alias, expected
  current revisions and all required mutable head changes. Inverse and
  replacement are inseparable, including accepted zero results. Supplier
  reservation transitions must use the reviewed encoding still missing above.

At the initial stop, no implementation or tests were added. The owner-authorized
follow-up adds only the candidate contract and its offline document/oracle checks.
No production port, command, decoder, public append method, migration or dependency
has been added. The proposed interface remains review-only.

## Additive candidate validation

Fresh full offline suite: 128 Rust tests passed, zero failures, 19 default ignored;
formatting, Clippy, no-default, source/dependency boundaries and old frozen
contracts passed. The new standalone contract check passes 12 histories / 44
steps / 112 records, 47 adversarial cases (46 independently hash-verified before
rejection), and nine strict JSON negatives. Python and independently authored
Node/BigInt agree on every positive record's bytes/hash and accounting result.
All 159 original frozen files plus the registry match the reviewed base exactly.
No existing frozen validators or production sources changed. Independent review
of candidate `557a4a6` found a delivery/record identity uniqueness gap; both new
validators now reject it, with nine added fully rehashed regressions and 13
non-appending original-receipt/conflict lookup cases in both runtimes. Fresh
exact-SHA review remains required. These results do not
certify real-store behavior or genuine authority, and do not freeze the amendment.

## Initial stop validation

Fresh `sh scripts/check.sh`: PASS (exit 0), using the pinned local Rust 1.98.1
compiler and cached locked dependencies in offline mode.

- 128 Rust tests passed, zero failed; 19 default ignored entries (17 explicit
  PostgreSQL tests and two deferred independent fake-destination gates).
- Formatting, warnings-denied Clippy, no-default build and dependency/source
  boundary checks passed.
- All 159 frozen files, including 99 original v1 files, passed verification;
  four freeze-metadata rejection checks passed.
- Python/Node reconstructed 60 v1 hash vectors, 25 accepted rows and 29 manifest
  members. Outcome checks passed for 24 histories, 1,450 records, 43 decisions,
  27 kinds, 272 negative assertions, 217 scalar cases and 38 field boundaries.
- Unicode parity checked 1,112,064 scalar values and 2,224,128 text/source checks
  per runtime used by the suite; the accepted rehashed history passed with
  41 records and one decision.

The detailed log is local ignored `work/validation/p3-baseline-offline.log`.
A read-only schema inventory probe is retained at
`work/validation/p3-contract-coverage.json`. No real PostgreSQL or separate driver
proof run was performed in this coordinator task. No added implementation means
there are no new focused implementation tests; these baseline results do not
certify the absent Phase 3 behavior.

## Conflicts and integration

No Git conflicts occurred. The original report-only commit is
`376246608024409276dbad2f322ad5adb9a59e9e`; the follow-up candidate is additive.
Do not integrate either as a completed coordinator or adapt stores to unreviewed
types. Review the exact candidate, then resume the coordinator and agree exact
types with both store owners before SQL integration.
The frozen registry, its 159 registered files, manifests/lockfile, migrations,
reviewed semantic core, and all original receipts remain unchanged.

No main merge, push, tag, publication, deployment, service registration, remote
model, network retrieval or paid infrastructure was used. Spend: $0.
