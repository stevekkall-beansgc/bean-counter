# Reservation settlement / 1 — review candidate

Status: **unfrozen candidate**, authorized by the owner on 2026-09-21. This is a
narrow additive amendment to `2-candidate.4`, not a replacement for its bytes,
pricing semantics, identities, receipts or validators. No SQL or runtime is
implemented. Approval of this candidate is required before coordinator integration.

## Owner decision and scope

Only an original ordinary supplier outcome and explicit ordinary-window closure
can change an invocation's operational budget. An ordinary outcome consumes its
authorized positive supplier result from held capacity. Zero or negative ordinary
results consume zero. Release of remaining held capacity requires a closure transition. An
ordinary result, including zero or discount and the last ordinary result, never
auto-releases capacity. An authorized explicit closure closes any remaining
ordinary families and releases the remainder once.

Every later correction, reversal, chargeback, attribution fix, dispute resolution
or reinstatement is a post-hoc economic adjustment. It changes **none** of maximum,
consumed, held, released, ordinary-family status or reservation revision. Frozen
correction authority and economic ceilings still apply independently. Exhausted
or released reservation capacity is not an additional barrier to an otherwise
permitted post-hoc adjustment. An adjustment outside that frozen authority fails
or requires a new agreement. This amendment adds no event types for those labels;
the bounded implemented path will use the existing authorized correction command.
Base-work replacement remains separate. No high-water consumption rule exists.

This package covers one invocation, one final successful target, its complete
frozen supplier-family set (1–32), and one currency/scale. It cannot attach policy
later, import an untrusted base, authorize an invocation, replace base work, or
share a reservation across targets. Initial invocation authorization remains a
precondition: one pre-work authorization, no prior work consumption. Base booking
and registration occur in the same transaction. Existing databases cannot acquire
registration retroactively merely by recomputing this package's hashes.

## Three closed records

The schema is `records.schema.json`. Every envelope is exactly
`{kind,scope,id,body,content_hash}`. Scope is authenticated `[tenant,environment]`.
Bodies inherit scope. References are exactly `{kind,id,content_hash}`; all resolve
within the same scope, against retained immutable bytes and an independently
trusted original registration receipt. A reference's kind fixes its ID prefix.

1. **reservation-observation**: the operation, original authorization/base/target
   references, complete frozen eligible family set, current reservation state,
   receipt/acceptance times, current authority observation and economic receipt
   where applicable. The first observation registers the exact checkpoint after
   base consumption. Subsequent observations bind the original registration
   receipt and preceding settlement receipt. A correction retains an observation
   even though it cannot create a transition.
2. **reservation-transition**: reference to that observation, exact before and
   after states, nonnegative consumed/released deltas, and next revision. Only
   ordinary acceptance or explicit closure may create this record. The complete
   state includes each family's permanent key and original ordinary receipt (if
   claimed), so zero ordinary results still consume their one ordinary slot.
3. **reservation-receipt**: original command identity and request hash, ordered
   observation/optional-transition references, resulting state, complete bounded
   replay references and optional original economic receipt. It commits the whole
   settlement decision. No economic receipt or manifest is rewritten to point
   back to it; the composite acceptance commits both atomically. Registration's
   receipt is an externally trusted root, never a root supplied by a replay caller.

A monetary no-op can still close an ordinary slot and must then have a transition
and revision increment. Corrections and registration have no transition. A fresh
explicit close on an already closed reservation creates a no-op observation and
receipt, without a new revision. An identity or ordinary semantic duplicate
creates no new canonical records and returns the original composite receipt.

## Canonical representation and identities

Profile string: `reservation-settlement/1`. Schema discriminator:
`ledger-<kind>/reservation-settlement/1`. Draft 2020-12 schemas are closed; optional
values are absent, never null. Inert evidence cannot set prices, amounts, authority,
membership or deadlines. Do not add unspecified fields to a frozen v2 envelope.

Strict UTF-8 JSON, no BOM, invalid UTF-8/surrogates, duplicate keys, nulls,
fraction/exponent/negative-zero JSON numbers, unsafe JSON integers, non-finite
values or unknown fields. JCS UTF-16 key ordering; minimal UTF-8 with no trailing
LF. Strings preserve spelling without Unicode normalization. Text is 1–128 UTF-8
bytes without Unicode control characters; sources are absolute, whitespace-free
URIs of at most 256 bytes. Times are Gregorian UTC year 0001–9999 with exactly six
fractional digits. All economic counters use nonnegative canonical decimal
strings bounded by `10^30-1`. Revisions are canonical unsigned strings bounded by
`9223372036854775807`; registration starts at `0`. Currency is three uppercase
ASCII letters, scale is a JSON integer 0–18. Each body is at most 256 KiB, nesting
at most 32; each composite decision at most 4 MiB. Replay references at most 1024;
fail rather than truncate. Canonical sort order of set arrays is unsigned UTF-8
order of canonical element bytes, with unique semantic keys.

`H(domain,value) = hex(SHA256(UTF8("ledgerlab/" + domain +
"/reservation-settlement/1") || NUL || JCS(value)))`.
`digest(domain,value) = "sha256:" + H(domain,value)`.

| Kind | Prefix | Identity input |
|---|---|---|
| reservation-observation | `rso1_` | `[scope,source,external_id]` |
| reservation-transition | `rst1_` | `[scope,invocation_id,to_revision]` |
| reservation-receipt | `rsr1_` | `[scope,source,external_id]` |

Identity domains equal kind names. Body content hash is
`digest("record-content",[kind,1,body])`. Request hash is
`digest("request",command)`. Command excludes host-injected received/accepted
times, current state and authority observations; it includes its explicit
expected revision for close. Ordinary, post-hoc and registration commands carry `economic_ingress_hash`,
the unchanged frozen ingress hash of their original economic request. Full original
normalized ingress bytes remain in the underlying delivery record and are compared
on retry as well as the hash. The economic expected-current guard remains in that
original v2 command. The resulting economic receipt is in the observation and
settlement receipt, not in the command hash: received/accepted time changes cannot
turn the same original request into a different settlement command. First resolve
the original delivery namespace and compare retained ingress bytes; only after
new evaluation build the observation that names the resulting economic receipt. `source` and
`external_id` are the original economic identity for linked decisions; closure
uses a distinct authenticated control source/label. A single permanent delivery
namespace across economic and closure commands prevents collisions. Command
identity is resolved before current write permission, deadlines or revision.
Read authorization always precedes disclosure. A different body under the same
identity conflicts. An ordinary semantic duplicate under another delivery label
uses the existing v2 claim's first composite receipt and creates only the existing
permanent operational alias mapping, atomically; it cannot settle capacity twice.

Accepted journal history contains each scoped `(kind,id)` once. Content hashes
are values bound to those identities, never part of an identity key. Every
reference registry is keyed by `(scope,kind,id)` and rejects a second hash for
that key; repeated references to exactly the same bytes/hash remain valid. Each
new accepted step must have a previously unused `(scope,source,external_id)`
across registration, ordinary, post-hoc and closure. An identical retry returns
the already stored original receipt without adding a journal step; a changed
request under an existing delivery key conflicts. Renaming the operation kind
cannot reuse a consumed delivery key. The validators enforce both delivery and
record uniqueness independently of replay set ordering and hash integrity.

## Checkpoint, conservation and complete eligibility

`state = {revision,maximum,consumed,held,released,families}`. A family's key is
`{agreement_id,family_id,target}`; invocation and authenticated scope are not
caller-selectable eligibility alternatives. Status is `open`, `claimed` (with
its original ordinary economic receipt), or `closed` (explicitly foreclosed).
Every state repeats the complete sorted frozen keys and deadlines. No correction,
alias, policy version, binding selection or later command may change that set or
turn a terminal family back to open. Every ordinary member must belong to the
same frozen supplier binding and nominated target/invocation.

At registration, the maximum comes from the verified original invocation
maximum exposure. Base consumed and released values come from the original
Evaluation's exact consumption entries for this invocation, which must match
its actions and original authorization. Held is the verified remaining balance;
`maximum = consumed + held + released`. This bounded initial path requires the
pre-base held amount to equal maximum and no earlier spend/release. All members
are initially open, revision is `0`, and the frozen aggregate premium ceiling
must fit the remaining held balance and binding exposure. Registration is not a
new authorization or permission to replay a legacy base as fresh work.

For every observation, current state must exactly equal the locked durable head
and the last receipt's result. At every step all amounts are nonnegative and
within bounds, units equal the original invocation and base, and maximum never
changes. A transition has `after.revision = before.revision + 1` without overflow,
`after.consumed = before.consumed + consume`,
`after.released = before.released + release`,
`after.held = before.held - consume - release`. Conservation holds before and
after. A hash-valid but invented head is not trustworthy.

For **ordinary**: verify original v2 revision 1, frozen family, nominated supplier
binding, original claim uniqueness and current authority/economic checks. Consume
exactly `max(authorized supplier result atoms, 0)`, never a caller-provided amount,
never a correction delta, and never clipping to held. Insufficient held rejects
both the economic and settlement decision atomically. Mark only this open family
claimed and retain its ordinary receipt. Release is always zero. Accepting the last ordinary claim is not itself a
window-closure command; explicit closure releases the remaining held balance. Accepted zero
and discounts still close their ordinary slot. Retail-only outcomes cannot alter
a supplier reservation or close one of its families.

For **post_hoc**: validate the existing correction through the frozen economic
rules and explicit expected-current revision. The original family must have an
accepted ordinary receipt; it cannot be merely unclaimed/closed. Require an exact
state match and **no transition**, even if the replacement is higher than the
ordinary premium or happens after expiry/release. Keep the original ordinary
receipt in the state. Correction acceptance still commits its inverse/replacement
atomically, together with this unchanged-budget observation/receipt.

For **close**: check separately verified closure rights and retained evidence,
and expected current reservation revision. Close every remaining open family;
leave claimed families and their original receipts intact. Release all held.
The `reason` is either `deadline` or `authorized`. Deadline closure requires
`accepted_at > max(all frozen ordinary accepted_by)` because those deadlines are
inclusive; occurrence/receipt deadlines alone cannot eliminate a pending eligible
ordinary acceptance. Early `authorized` closure requires explicit retained agreed
permission to foreclose those still-open ordinary rights, verified by the shared
coordinator; mere administrative write access is insufficient. Both variants
require an authenticated closing principal/grant, actual retained authorization
and evidence. A late ordinary command then rejects; an already accepted original
retry still returns its receipt. Explicit close is a control acceptance with no
monetary actions, export intentions or fabricated economic event/receipt.

## Authority, replay, receipt and atomicity

Authority fields are coordinator-verified observations, never public capability
flags or a store decision. They identify principal, current grant and revision,
read/write/close permissions and retained evidence. For ordinary/post-hoc, check
these against the existing v2 authority decision and receipt, including scoped
source, target, agreement/family, correction permission and times. For base,
check retained original assent/offer/delegation and finality. For close, inspect
the exact retained agreement and grant authorizing closure. Replays use these
original observations and times rather than today's credentials.

The receipt's replay set is the sorted unique transitive retained prefix: all
original anchor references, each prior addon record, current observation and any
current transition, original economic receipt and its complete v2 replay closure,
and every authority/evidence document relied on. Include full references, not
only names. The current receipt cannot be its own member. Its result and optional
economic receipt agree with observation/transition. Typed adapters must reject
missing, extra, mismatched, cross-scope or unverifiable references. Hashes prove
integrity, not authority or the provenance of the registered root.

Profile-enabled acceptance is discoverable from the original atomic registration
anchor. An ordinary/correction decision for that target is incomplete without its
settlement receipt. Missing companion records are integrity failure, not a legacy
fallback. Existing v1/v2 validators keep their historical meaning; this additive
validator establishes the extra coverage. Legacy frozen receipts stay exact. API
result is a private composite pair of original economic receipt bytes (when
present) and additive settlement receipt bytes; closure has only the latter.
Returning an economic receipt alone must not certify the new reservation behavior.

Under one transaction acquire ordered installation, authority, binding,
reservation, chain/target, claim/aggregate, invocation-consumption and base-reversal
locks. Within class sort full scoped UTF-8 keys. This placement refines §11's
chain/stage class with target, then claim and aggregate subkeys, before invocation
consumption. All modifying control commands must use the identical order. Missing
heads require precreated operational scope locks/unique keys. Discovering another
scope causes rollback and ordered restart, bounded by existing attempt/deadline
limits. SQLite immediate transactions serialize writers; PostgreSQL serializable
transactions plus scope locks/guarded revisions must restart stale snapshots.

Original ordinary and close race on reservation plus claim/target locks: whichever
commits first determines the eligible current state; the loser retries against
that state. A post-hoc adjustment validates the complete current state under the
same locks but never writes the reservation head. Its economic claim guard still
changes atomically. Store ports enforce expected revisions and row integrity,
not pricing, authority or closure eligibility.

One atomic append commits economic rows (if any), settlement observation,
transition if needed, settlement receipt, identity/alias, guarded reservation and
claim/aggregate heads. Cancellation before commit rolls all back. After commit
starts use the existing bounded drain policy. Unknown commit outcome remains
unknown: no retry can assume absence or perform compensating release. Resolve on
the primary by original scoped source/label and exact request bytes/hash; return
both original receipts or retry only after proven absence/rollback. Concurrent
same-identity winners and semantic aliases never consume again. Duplicate lookup
is read-authorized even if submission/correction/closure authority later expires.

## Evidence and review gate

`histories.json` contains hand-authored integer expectations and synthetic trusted
inputs for this reservation accounting surface. They are not fabricated accepted
v2 journals or proof of genuine external assent. `vectors.json` pins complete
canonical records and identities derived from those inputs. Python validates
schema, reference closure, numeric histories and attacks; Node independently
reconstructs canonical bytes/hashes and reservation arithmetic using BigInt.
The production pure evaluator is not used as either expected-value oracle.
Full composite v2 replay/real authority/store/crash tests remain coordinator and
adapter gates; the package cannot certify those from synthetic authority flags.

Run `sh scripts/check-reservation-settlement.sh` and the unchanged
`sh scripts/check.sh`. Neither validator writes expected files or freezes this
candidate. `review-manifest.json` pins the candidate file inventory for review;
it is not added to `contracts/freeze.json` and cannot constitute freeze approval.

## Candidate validation record (2026-09-21)

- New read-only check: PASS, 12 independent integer histories / 44 literal steps /
  112 canonical records; Python schema, conservation, authority-boundary and
  reference checks; independent Node BigInt arithmetic, complete canonical bytes,
  IDs and content hashes.
- 47 rejected adversarial cases. For 46 cases Node first verifies all rebuilt
  row identities/content hashes and then requires rejection. Null is the one
  pre-canonical rejection. Nine additional strict JSON/parser negatives and 13 non-appending lookup cases
  pass in the relevant Python/Node checks.
- Unchanged full `scripts/check.sh`: PASS, 128 Rust tests, zero failures,
  19 default ignored entries; formatting, warnings-denied Clippy, no-default
  compilation, dependency/source boundaries and old Python/Node contracts pass.
- Exact Git-object comparison against Phase 2 commit
  `6194376a053b8a27887a9b09459054a7af3a1769`: all 159 frozen files and the freeze
  registry are byte-identical. Old validators, Cargo files, migrations and pure
  economic semantics are unchanged.

These are candidate contract and baseline checks. Synthetic trusted economic and
closure observations are explicit fixture boundaries. They do not prove real
external assent, production lossless v2 decoding, real-store atomicity, physical
crash/cancellation, concurrency or unknown-commit recovery. Independent conformance
review and both-store implementation evidence remain required. No new Rust test
or production behavior is claimed by the 128-test baseline count.

## Independent review follow-up

Independent conformance review of candidate `557a4a6de2a518ad1670935849fef680cac460f7`
confirmed lifecycle numeric agreement and found that both validators admitted
fully rehashed histories that reused a delivery label for a later post-hoc or
closure decision. The original candidate remains unapproved. The follow-up adds
scoped delivery uniqueness, new-row identity uniqueness, and reference registries
keyed by identity without hash. Nine regressions cover cross-operation delivery
reuse, registration reuse, identical closure retry appended as a new decision,
conflicting hashes for one reference, reused transition revision identity, and closure-to-closure delivery reuse.
Thirteen non-appending lookup scenarios additionally verify original ordinary,
post-hoc and closure receipts after later decisions; changed commands/ingress;
read denial; proven durable/absent commit outcomes; and inconclusive absence.
They compare exact stored settlement receipt bytes and the original synthetic
economic receipt reference. Full economic receipt byte preservation remains the
existing v2/production bridge's separate verification boundary. No lookup can
add accepted rows or mutate the delivery registry.
All prior positive canonical vectors, numeric histories and schemas are unchanged.
The independent review and reproductions are committed as
`8a6837d97e656de102a172d0f64c2e1a9cbc31eb`. The new candidate requires fresh exact-SHA review; this fix does not grant freeze
or production implementation approval.
