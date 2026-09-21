# Outcome records v2 — candidate encoding

Status: **candidate, not frozen; independent review pending**.
Base: `b35258425970052ed71481eca1f33ef857c61be1`.
Profile and body suffix: `2-candidate.1`. Contract-only work; no production
acceptance, evaluator, persistence, migrations, outbox or CLI extension.

## Scope and precedence

This is an additive encoding proposal for the assigned Phase 2 v0 outcome
adjustment semantics: immutable normalized base/outcome events, one permanent
claim per target/agreement/book/rule family, pinned terms, authorized admission,
accepted zero amounts and explicit optimistic corrections. It does not amend
any frozen v1 bytes. It proposes a **bounded exception** to v1's prohibition on
compound replacement: an uncapped outcome claim revision can atomically append
its exact inverse and replacement. It does not permit general partial reversals,
base rerating, a stage reopening, arbitrary credit editing or supplier debt
cancellation through a retail correction.

The task's approved requirements establish these semantics; encoding details
below are candidate choices requiring independent review. Existing source
reports/addendum still govern every v1 path. A consumer must select this exact
candidate profile explicitly; an unknown profile fails closed. There is no
production evaluator version 2 or storage-write version 2 implied by these files.

## Canonical bytes

1. Parse strict UTF-8 JSON. Reject BOM, invalid UTF-8, duplicate object keys,
   unpaired surrogates, null at any depth, NaN/infinity, fractional/exponent
   numeric tokens, numeric negative zero and numeric integers outside
   ±9007199254740991. Nesting is at most 32. Closed schemas reject unknown fields
   recursively. Optional fields are omitted; required empty arrays remain `[]`.
2. Records here are **already normalized**. No Unicode normalization, case
   folding, source URI rewriting, optional-value invention or lookup of current
   configuration occurs in serialization. A future ingress normalizer may
   normalize explicitly offset Gregorian times to UTC before this boundary.
   It must retain the normalized original command before mutable resolution.
3. Serialize the restricted JSON values as JCS UTF-8: object keys in UTF-16
   code-unit order, minimal JSON escaping, no whitespace or final LF. Arrays
   retain their specified order. Hashes never include pretty-printed files or
   envelope serialization. The Python and Node implementations cover astral
   versus BMP key order and distinguish composed/decomposed Unicode.
4. Atoms are canonical signed decimal strings (`0` or `-?[1-9][0-9]*`), magnitude
   at most `10^30-1`; no `-0`, plus sign or leading zeros. Scales are JSON integers
   0–18. Currency is exactly three uppercase ASCII letters; no currency table or
   exchange rate is consulted. Every dependent money amount must agree in both
   currency and scale. No floats or rounding of an aggregate.
5. Exact rates and intermediates are reduced `{numerator,denominator}` decimal
   string pairs with positive denominator, GCD 1 and at most 512 bits per
   magnitude. Zero is `0/1`. Pre-reduction temporary products are also bounded
   to 512 bits; overflow rejects. A percentage `-10%` is rate `-1/10`, **not**
   `-10/1`; the rate is a signed multiplier of integer basis atoms. Fixed rules
   contain signed integer `fixed_atoms`. Round the one exact rule result with
   `nearest_ties_away`; e.g. `-21/2` becomes `-11`. An inverse negates the stored
   integer; it never reevaluates a rule or rerounds.
6. Counter strings are `0|[1-9][0-9]*`, at most 9223372036854775807; claim/chain
   revisions and accepted policy versions start at 1. Timestamps are real
   Gregorian UTC dates in years 0001–9999, exactly
   `YYYY-MM-DDTHH:MM:SS.ffffffZ`, without leap seconds. Equal timestamps do not
   imply equal revision order. IDs/scope parts/party IDs are 1–128 UTF-8 bytes;
   source URIs are 1–256 bytes, without control characters. Slugs are bounded
   ASCII. JSON Schema character lengths are supplemented by byte checks.
7. Every canonical body is at most 256 KiB. A decision bundle is at most 4 MiB,
   resolved inputs 8 MiB, explanations 1 MiB, evidence body 256 KiB, and source
   chain 1,000 accepted events. Schema cardinality bounds are additional
   rejection limits, not permission to exceed aggregate byte bounds. Candidate
   replay uses a complete bounded retained prefix; if that exceeds a limit,
   reject rather than truncate. A compact closure encoding needs another
   reviewed profile. The checker certifies the supplied bounded examples, not
   all possible production resource-limit paths.

## Ordering and keys

`scope = [tenant,environment]`, supplied by authenticated context and retained
on **every envelope**, never trusted from an unverified request. Body records
inherit that envelope scope; every resolved reference must have the same scope.

Sets sort by unsigned lexicographic comparison of **canonical element UTF-8
bytes**, reject duplicate elements, and additionally reject duplicate semantic
keys. Set fields are `rules` (unique outcome code), `evidence`, `postings`
(unique posting ID), revision action arrays, `current_before` (unique claim),
`action_ids`, replay `inputs` (unique kind/ID), intention `depends_on`, payload
`actions` (unique action ID), and receipt `intention_ids`. Policy rules are a
code lookup set; this candidate has no rule execution order or arbitrary AST.

Scope, hash tuples, composite keys and exact-ratio fields preserve positions.
Histories preserve acceptance order. Manifest `explanation_ids` preserve ordinal
order (one ordinal 0 step in this bounded family). Manifest members and each
seed/new-record listing sort by `(kind UTF-8 bytes, JCS(id) UTF-8 bytes)`.
There is one member per `(kind,id)`; aliases never add canonical records.

Every envelope has exactly `{kind,scope,id,body,content_hash}`. Bodies have the
schema discriminator `ledger-<kind>/2-candidate.1`, no redundant computed ID and
no redundant scope. Generic physical uniqueness is `(tenant,environment,kind,id)`;
for array IDs the structured tuple must also be enforced by typed columns, never
by ambiguous concatenation. The schema and audit both require domain prefixes.

## Hash and identity formulas

Let `H(k,v) = lowercase_hex(SHA256(UTF8("ledgerlab/" + k +
"/2-candidate.1") || 0x00 || JCS(v)))`. `digest(k,v) = "sha256:" + H(k,v)`.
No trailing LF, truncated digest, entropy, implicit clock lookup or database
sequence enters an identity. Content-derived IDs bind explicit retained prices
and times; semantic slot IDs exclude those values. The literal `/2-candidate.1`
separates **every** domain from v1; candidate IDs cannot be mistaken for frozen
IDs. A reviewed successor profile will have separately reviewed hash vectors.

All bodies use `digest("record-content",[kind,2,body])`, including event,
policy, evidence and receipt bodies. **Only** `decision-manifest` uses
`digest("decision-content",body)`. Unlike v1 documents, a content-derived
candidate ID is not the outer row content hash. An event's canonical content
hash is its generic row hash; ingress hash is `digest("ingress",event_body)`.

`ID(kind,v) = prefix + "2_" + H(kind,v)`:

| Kind / domain | Prefix | Exact tuple/value |
|---|---|---|
| evidence | ed | `[scope,body]` |
| policy-snapshot | po | `[scope,body]` |
| event | ev | `[scope,source,external_id]` |
| base-posting | bp | `[scope,event_id,agreement_id,book,ordinal]` |
| target-basis | tb | `[scope,body]` |
| admission | ad | `[event_id]` |
| claim | cl | `[scope,target,agreement_id,book,family_id]` |
| claim-revision | rv | `[claim_id,number]` |
| effect | ef | `[claim_id,revision_id,slot]` |
| action | ac | `[effect_id]` |
| obligation | ob | `[scope,agreement_id,book,currency,scale,roles]` |
| limit-evidence | li | `[event_id,0]` |
| explanation | xp | `[event_id,0]` |
| replay-input | rp | `[scope,body]` |
| intention | in | `[scope,destination,obligation_id,sorted_action_ids]` |
| decision-manifest | dc | `[event_id]` |
| receipt | rc | `[event_id]` |

`number` is a canonical **string**; `ordinal`, scale, and schema version in
record-content are JSON **integers**. `slot` is `replacement` or `inverse`, never
a policy ID, code, price or version. The permanent claim includes neither the
policy version nor the submission source, delivery label, outcome code, rate,
amount, credential version or report time. A changed version cannot create a
second economic slot. Source authorization still applies independently.

Four kinds have array IDs, with **no extra hash-derived ID**:

| Kind | Exact composite ID |
|---|---|
| link | `[scope,"outcome_of",event_id,target]` |
| dependency | `[scope,dependent,input.kind,input.id]` |
| delivery-key | `[scope,source,external_id]` |
| chain-revision | `[scope,chain_id,number]` |

Additional uniqueness: one claim for its tuple permanently (even at zero), one
revision number per claim, one accepted successor to a previous revision, one
original delivery key, one inverse per original action, one action per effect,
one intention per `(scope,destination,idempotency_key)`, and one accepted chain
revision per event. Current claim/chain/aggregate/target guards are mutable
transaction state, **not** alternative canonical history.

`claim.facts_hash = digest("claim-facts", event.data without external_id)`.
Source, chain, payer, occurrence, target, agreement, book, family, code and
retained evidence remain. A semantic retry may change only its delivery label;
a different report/code/evidence/occurrence conflicts and requires an explicit
correction. Matching digests must still compare canonical bytes.

`effect.facts_hash = digest("effect-facts", action.body without schema,
event_id,effect_id,policy_snapshot)`. This includes exact amount, roles,
obligation, basis, claim/revision, book, family, slot and optional `reverses`.
A provenance-only policy version does not redefine the effect identity, while
changed economics change the facts hash. The body/provenance still pins and
hashes the policy; this projection is not permission to use current terms.

## Record inventory and required relationships

The closed schema is the complete field dictionary; the following relationships
are required beyond JSON Schema.

| Record kind | Purpose and checks |
|---|---|
| evidence | Retained exact UTF-8 evidence, purpose and media type; scoped content-derived ID. No URL-only evidence and no credential secret. Synthetic text is not real assent. |
| policy-snapshot | Stable agreement/family/book, explicit version and currency/scale, allowed outcome-code rules, exact signed amount/rate, rounding, roles, assent and source/correction permissions, four time boundaries and aggregate limits. Supplier authorization and payer delegation are required when applicable. |
| event | Normalized `base`, `outcome`, or `correction` tagged data. Outcome/correction explicitly names target, agreement, book and family. Correction names claim and expected current revision, replacement code and corrected_at. Evidence and occurrence are never generated from host time. |
| base-posting | Seed/retained original accepted integer economics, six roles, book and amount, target event and stable ordinal. No base evaluator is defined here. |
| target-basis | Original booked net as the exact sum of named, hash-pinned base postings, currency/scale, payer, finality/evidence and uncapped/capped stage. It never includes an outcome adjustment, current net, hypothetical list price or opposite book. |
| admission | Host-owned authenticated principal, credential revision, authentication/grant evidence, locked grant/target/aggregate guard revisions, final_unreversed state, permission/source, tenant via scope, payer/agreement/book/family, basis/policy refs and received/accepted times. It asserts `allow`; an untrusted caller cannot fabricate one. |
| claim | Permanent tuple, first event, immutable original receipt reference and original semantic-facts hash. Never overwritten when corrected or zeroed. |
| claim-revision | Monotonic number, previous revision when correcting, event/code, pinned policy/basis/admission, new live amount/action, inverse actions and original receipt. A zero revision is accepted history. |
| effect | Stable revision/slot, action and independent economic facts hash. |
| action | Nonzero signed posting, revision/effect/claim, policy/basis, explicit six roles and obligation; inverse additionally names original action. Every inverse preserves original economics/provenance and negates exact stored atoms. |
| obligation | Immutable definition from agreement, retail/supplier book, currency/scale and complete roles. It is not an accumulated balance or a payment receipt. |
| link | Explicit outcome/correction event → target event relation. Same scoped target as claim and basis; no endpoint search. |
| dependency | Explicit action or explanation → hash-pinned prior input; reasons `frozen_basis`, `exact_inverse`, `prior_revision`, `aggregate_limit`. Zero explanations retain economic dependencies too. |
| limit-evidence | Complete current revision set before acceptance for target/agreement/book, replaced revision if any, before/after positive and absolute-negative totals, currency/scale and both pinned maxima. Includes zero claims. |
| explanation | Ordered ordinal, selected code, exact unrounded ratio, rounded atoms and rounding, policy/basis/revision, actions and aggregate evidence. Applied and zero decisions both explain their result. |
| replay-input | Versioned semantics name, complete seed/prior-record prefix and current event/admission refs with content hashes, original received/accepted times. No clock/network/current-policy lookup during replay. |
| intention | One nonzero per-obligation decision delta with complete signed action breakdown, stable key equal to intention ID and predecessor intention set. It is immutable export intent; no dispatch implementation. |
| delivery-key | Original normalized command and ingress hash, source/external label, canonical event and that event's original receipt. Future aliases are operational mappings outside manifests. |
| chain-revision | Immutable chain number/event/decision association; optimistic head advancement belongs to future coordinator/store work. |
| decision-manifest | Complete sorted retained-input/new-row membership and ordered explanation IDs, event/chain/revision and accepted time. No self-hash or own receipt reference. |
| receipt | Original event/decision hashes, chain/revision/time, claim/revision and action/intention IDs. Stored original bytes are returned on retries. |

Every input reference is `{kind,id,content_hash}` and resolves locally to the
exact original envelope in the same scope. No missing reference can be filled
from a cache of current values. Inline economic copies must agree with their
referenced records. Missing, unknown-kind, wrong-prefix, cross-scope, stale-hash,
wrong-book or inconsistent-currency references reject even after rehashing.

## Admission, times, claims and corrections

Authentication and receipt-read authorization precede disclosing a stored
receipt. Resolve an existing original delivery mapping before any current
policy/eligibility lookup: identical canonical ingress returns the stored
receipt byte-for-byte; changed ingress is `IDENTITY_CONFLICT`. A new delivery
label with an existing claim and identical original facts returns that claim's
**first** receipt, even if its head is now corrected or a newer policy is active.
It cannot resurrect a claim. Different facts produce `CLAIM_CONFLICT`.
A repeated accepted correction returns its own original receipt before testing
its now-stale expected revision. No retry changes a manifest or receipt.

Fresh outcome admission requires the exact designated source, actual current
submission grant, real assent/authorization for the named parties and scope,
allowed code, a final unreversed target, frozen positive-or-zero basis and an
uncapped stage. Supplier authorization is separately retained. Corrected
supplier economics require an explicit supplier command and that agreement's
correction permission; retail corrections affect no supplier posting.

Occurrence must be within `[window_start,window_end)` and no later than the
injected `received_at`. An original report requires `received_at < report_before`.
Acceptance is no earlier than receipt. `window_start/end/report_before` are
absolute pinned timestamps, eliminating replay dependence on a current clock.
In v0 the nominated occurrence window is at most 90 days and report grace at
most 7 days; a coordinator must check these source-design bounds.

Correction admission uses the exact **current** claim revision named by the
command, under the claim and aggregate locks, and the original pinned policy
and basis. It does not select a new policy version. Its corrected_at must be no
earlier than the prior admission's accepted_at and no later than received_at;
both corrected_at and received_at must be strictly before `correct_before`.
The corrected occurrence remains inside the original occurrence window.
Corrections use their dedicated deadline, so an authorized correction after the
original report deadline can still qualify. This is an explicit candidate
boundary choice, not a hidden change to v1. Times are injected and retained,
not inferred from acceptance order or evidence arrival.

Initial revision 1 owns the claim even when its rounded amount is zero. A
correction appends revision N+1, one inverse for the prior live nonzero action
(if any), and one replacement for the new nonzero amount (if any). It never
inverts the already-inverted revision, reuses an old action ID, removes a
claim, deletes history or replenishes supplier authorization. Restoring the same
amount later uses the same permanent claim and a new revision/effect/action.
A zero-to-zero correction still appends an explained revision and receipt.

The inverse and replacement must both be present in the same atomic decision.
A nonzero-to-same-nonzero correction retains both actions and emits no intention
because its per-obligation delta is zero. A nonzero-to-zero correction emits the
inverse; a zero-to-nonzero correction emits the replacement. The candidate
`correction-replacement` history covers all nonzero inverse/replacement details,
including a zero-net correction and a later nonzero correction.

## Aggregate limits and books

The group is `(scope,target,agreement_id,book)`, independent of family and policy
version. Policies on this group must pin identical aggregate maxima. The complete
head set includes zero-valued claims. Compute premium as sum of positive live
revision atoms; discount as sum of absolute negative live revision atoms. They
are **separate gross totals**; opposite signs do not create headroom. Replace
only the corrected claim's live amount before calculating the after totals.
Both after totals must fit their maxima, and the maximum aggregate discount
must not exceed the frozen target basis. Reject over-limit amounts; do not clamp,
partially accept, offset against premiums or change earlier claims. The retained
limit evidence must match every current head under the aggregate guard lock.

Each rule family has an independent permanent claim. Adding a version to one
family creates no new claim, while a genuinely distinct agreed family can create
its own claim. Family registration/authorization is host-controlled: renaming a
family must not be an untrusted mechanism to bypass deduplication. This candidate
encodes one family per command/decision; it is not a generic multi-binding pricing
bundle or permission for a caller to omit supplier obligations from a base event.
A future atomic multi-family command needs an explicit reviewed encoding.

Retail and supplier groups, bases, roles and obligations remain separate. This
candidate contains no cost-observation/allocation payable, dynamic share,
invocation reservation transition or running cap. A capped target or any capped
stage/outcome composition fails with `CAP_OUTCOME_INCOMPATIBLE`, even if the cap
would not bind, the proposed amount is zero, or the command is a correction.

## Manifest, exports and replay

For accepted decision D, define N as **every newly appended candidate row except
D's manifest and D's receipt**. This includes original delivery mapping,
admission, claim/revision, dependencies, zero explanations, limit evidence,
replay input, chain transition association and any effects/actions/intentions.
Define P as **every `replay-input.inputs` member**: complete seed records, every
previous accepted row in this bounded history (including earlier manifests and
receipts), plus this event and admission. Manifest members are exactly the
unique `(kind,id,content_hash)` union `N ∪ P`, in the order above. No arbitrary
extra unrelated record, omitted dependency or omitted zero claim is allowed.
Earlier manifests/receipts are acyclic prior inputs; the current manifest and
receipt are absent from its own membership. The receipt binds the manifest hash
and event row hash, so there is no self-hash cycle.

New receipt original bytes are retained separately as a golden assertion, not
another hashed record. Correction revisions point back to the first receipt;
the correction's delivery key points to its own receipt. Old receipt hashes and
bytes never change. v1 receipts remain v1 receipts; there is no conversion of
existing receipt IDs or decoding/rewriting of frozen fixture files.

Each intention's signed amount equals its full action breakdown for one
obligation. Zero net creates no intention. Its `depends_on` is the sorted set
of **all earlier nonzero intentions for that permanent claim**, including prior
retractions. A zero-net correction emits no intention but does not erase these
ancestors. A reinstatement therefore cannot export before a prior retraction;
a later correction after a zero-net replacement cannot lose the original
export dependency. Independent families do not imply each other's export order.
Actual ordered/fenced delivery remains out of scope.

Replay validates schema, bytes, IDs, hashes, scope, references, manifest closure,
authority evidence, recorded guard revisions, exact arithmetic and stored prior
postings under the named candidate semantics, with original decision times.
It reconstructs economic output in a side-effect-free context; no new claim,
receipt, intention or destination execution may be produced. This proposal does
not provide the production decoder into the existing private `Evaluation` type.

## Golden histories and review requirements

The candidate package includes fixed success +2500, 10% rebate −1000, correction
and reinstatement, nonzero replacement, same-amount/zero-net correction, two
independent families, accepted zero followed by a correction, rounded zero,
signed half rounding, separate retail/supplier adjustments and explicit supplier
correction, identity/semantic retries after version changes, stale correction,
occurrence/report/correction boundaries and capped-stage rejection.

Every accepted history includes complete seed and appended records, receipt
bytes and all hash vectors. Retry/rejection probes contain no canonical appended
rows; their inputs and expected codes are **contract expectations**, not evidence
that the current product accepts/rejects the candidate command. The seed base
postings are synthetic pre-existing accepted facts, not an implementation or
conformance test of base acceptance. Timing/cap probes are narrow boundary
histories. Real authority evidence, lock races, partial writes and unknown commit
outcomes still require independent coordinator/store conformance work.

Before freezing or integration, an independent reviewer must derive matching
bytes and identities, review the candidate choices above, reconcile them with
the separate semantic/core/oracle lanes, and review all negative cases. The
Python/Node audits are independent implementations of byte/hash checks, **not**
an independent human/agent approval. A freeze manifest must be an explicit later
reviewed artifact; the candidate review inventory cannot substitute for it.
