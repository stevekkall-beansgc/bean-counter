# Outcome canonical records — reconciled candidate

**Review candidate, not frozen. Fresh-context independent review pending.**
Profile `2-candidate.2` supersedes candidate `.1` from commit `c95cae9`; it does
not supersede or alter any v1 contract, fixture, hash or receipt. The changed
candidate hash domain prevents silent reinterpretation of the earlier draft.
Only roadmap Phase 1 is being executed; see `ROADMAP.md`.

Semantic authority is commit `1e0ba3f886788c08f427d3aae1d916b341187e76`, especially
`crates/ledgerlab-core/src/policy/chaining/outcomes/mod.rs` and its README. This
lane transcribes its retained-data contract; it does not integrate that branch,
implement a production decoder, price work, or broaden a store append port.
The schema is `contracts/candidates/v2/schemas/canonical-records.schema.json`.

## Exact semantic reconciliation

- Every percentage uses the **original final target retail net after booking
  discounts**, before any outcome, tax, funding/payment or supplier cost. This
  includes supplier adjustments. A rate `-10/1` means **minus ten percent**:
  `exact_atoms = retail_basis_atoms × numerator / denominator / 100`.
  This differs from candidate `.1`'s fractional-multiplier convention.
- Supplier booked net is its **discount capacity**, not a percentage denominator.
  Each binding has a finite nonnegative premium limit. Supplier premium must
  fit `invocation.held - supplier_booked_net`, and booked net plus premium limit
  must fit binding maximum exposure. The actual nominated supplier completion,
  invocation, source, operation, roles and verified offer/assent are retained.
- Permanent claim key is `[scope,agreement_id,family_id,target]`. Neither book,
  binding/policy version, code, price, delivery label nor source is an eligibility
  dimension. Book remains binding/posting/obligation data. Target membership
  rejects duplicate `(agreement,family)` even when a version or book differs.
- Base acceptance freezes **all** eligible families, their exact policy records,
  bindings, limits and verification, including never-claimed families. A later
  one-family command can select only this set. The first outcome cannot define
  the set, and a later policy cannot attach a family or enlarge limits.
- Ordinary and correction windows are separate four-field objects. For either:
  `starts_at <= occurred_at < occurs_before`, `occurred_at <= received_at <=
  accepted_at`, `received_at <= received_by`, `accepted_at <= accepted_by`.
  Each window satisfies base occurrence `<= starts_at < occurs_before <=
  received_by <= accepted_by`. No candidate-only 90-day/7-day limit is added
  to the approved typed surface.
- Claim acceptance is `>=` base acceptance. Correction acceptance is `>=` the
  current revision's acceptance; equality is allowed. A correction occurrence
  need not be after that prior acceptance and need not be in the ordinary
  occurrence window. The approved `Request.occurred_at` is the correction's
  occurrence time; `.1`'s extra `corrected_at` deadline is removed.
- Corrections retain the original target policy and basis, check the exact
  current revision, and require a separately permitted replacement code or
  `allow_reversal`. Full reversal uses `{kind:"reverse"}`; a zero-valued code is
  distinct and uses `{kind:"code",code:...}`. Neither is represented by null.
- Corrections retain two ordered explanations: `EXACT_REVERSAL`, then
  `CLAIM_REVERSED`, `ZERO_ROUNDED` or `OUTCOME_APPLIED`. Both explanations exist
  even when no money posting is needed. Nonzero inverse and replacement remain
  separate immutable actions. Zero results permanently consume the claim.
- Aggregate discounts and premiums sum the latest live results separately for
  `(scope,target,binding_id)`. Premium does not buy discount headroom. Check
  gross discount against original binding booked net, gross premium against its
  frozen ceiling, current net and atomic correction delta against atom bounds.
- Any cap in the original base bundle rejects target registration with
  `OUTCOME_CAP_COMPOSITION`, regardless of whether it fired or a future result
  would be zero. The rejected-target vector has no base-acceptance receipt.

## Canonical bytes and bounds

Strict UTF-8 JSON; reject BOM, malformed UTF-8, duplicate keys, lone surrogates,
null anywhere, NaN/infinity, fractional/exponent JSON number tokens, numeric
negative zero, and integers outside ±9007199254740991. Unknown fields reject in
closed record schemas. Omitted optionals stay absent; required empty arrays stay
`[]`. Inputs to this contract are already normalized. No Unicode normalization,
case folding, URI rewriting, implicit current-policy lookup or generated clock.

JCS object keys sort by UTF-16 code units. Output is minimal JSON UTF-8, no
whitespace or final LF. Economic atoms are canonical signed decimal strings
with magnitude at most `10^30-1`; scales are integers 0–18. Currency and scale
must agree across original base, policies, capacities, results and postings.
Exact ratios are reduced signed-numerator/positive-denominator strings, at most
512 bits each; zero is `0/1`. Temporary products are bounded too. Round once,
nearest/ties away from zero. Negate stored integer atoms for inverses.

Counters are canonical unsigned strings at most 9223372036854775807. Claim and
chain revisions start at 1. Policy version is an opaque bounded string, matching
the semantic implementation, not a numeric eligibility axis. Times are Gregorian
UTC, year 0001–9999, exactly six fractional digits, no leap seconds. Text IDs and
scope components are 1–128 UTF-8 bytes, sources 1–256, without controls; slugs
are bounded ASCII. Schema character bounds do not replace byte/value checks.

Nesting <=32; record body/evidence <=256 KiB; decision <=4 MiB; resolved input
<=8 MiB; explanations <=1 MiB. Schema array limits also apply. Target families
<=32, used bindings/limits <=16, codes/replacements <=32, evidence <=16. Complete
retained history remains bounded; no truncation to fit a replay limit. A compact
closure encoding requires another reviewed profile. Tests certify these examples,
not production resource handling or real authority/concurrency.

## Identity and order

Every envelope is `{kind,scope,id,body,content_hash}`; `scope=[tenant,environment]`
comes from authenticated context. Bodies inherit scope. All references resolve
in that same scope and local retained records; no network fetch or fallback to
current configuration. Every internal ID has its kind's prefix.

`H(k,v) = lowercase_hex(SHA256(UTF8("ledgerlab/"+k+"/2-candidate.2") || NUL ||
JCS(v)))`; `digest(k,v)="sha256:"+H(k,v)`.
`ID(kind,v)=prefix+"2_"+H(kind,v)`.
All body hashes are `digest("record-content",[kind,2,body])` except manifests,
which use `digest("decision-content",body)`. Content-derived IDs include scope;
they are not the generic body hash. Retained explicit times/prices affect
content-derived IDs but never permanent eligibility identity.

| Kind/domain | Prefix | Exact input |
|---|---|---|
| evidence, policy-snapshot, target-basis, replay-input | ed, po, tb, rp | `[scope,body]` |
| binding-snapshot, base-evaluation, target-snapshot | bs, be, ts | `[scope,body]` |
| event | ev | `[scope,source,external_id]` |
| base-posting | bp | `[scope,event_id,agreement_id,book,ordinal]` |
| base-acceptance | ba | `[target]` |
| authority-decision, admission, decision-manifest, receipt | au, ad, dc, rc | `[event_id]` |
| claim | cl | `[scope,agreement_id,family_id,target]` |
| claim-revision | rv | `[claim_id,number]` |
| effect | ef | `[claim_id,revision_id,slot]` |
| action | ac | `[effect_id]` |
| obligation | ob | `[scope,agreement_id,book,currency,scale,roles]` |
| limit-evidence | li | `[event_id,0]` |
| explanation | xp | `[event_id,ordinal]` |
| intention | in | `[scope,destination,obligation_id,sorted_action_ids]` |

Composite IDs are structured arrays without an extra hash:
`link=[scope,"outcome_of",event_id,target]`;
`dependency=[scope,dependent,input.kind,input.id]`;
`delivery-key=[scope,source,external_id]`;
`chain-revision=[scope,chain_id,number]`.
Counter `number` is a string; ordinal/scale/record-content version are integers.

Set arrays sort by unsigned lexicographic canonical-element UTF-8 bytes, with
unique elements and unique semantic keys: policy rules by code; family members
by `(agreement,family)`; bindings/limits by binding ID; evidence and references;
postings/action membership; replay inputs; verified evidence/terms; dependency
intention sets; payload breakdowns. Semantic keys are validation constraints,
not alternative sorting orders. Manifest and journal members sort by
`(kind UTF-8,JCS(id) UTF-8)`. One member per `(kind,id)`. Scope/hash tuples,
history order, base execution order and explanation ordinals retain order.

`claim.facts_hash=digest("claim-facts",event.data without external_id)`;
`delivery-key.ingress_hash=digest("ingress",normalized event body)`.
`effect.facts_hash=digest("effect-facts",action body without schema,event_id,
effect_id,policy_snapshot)`. The latter retains exact amount, roles, obligation,
basis, claim/revision, book, binding, family, slot and optional inverse reference.
Every indexed or inline copy must agree with its referenced record.

## Retained base, frozen target and admission

There are 26 closed record kinds. The prior 21 kinds remain: evidence,
policy-snapshot, event, base-posting, target-basis, admission, claim,
claim-revision, effect, action, obligation, link, dependency, limit-evidence,
explanation, replay-input, intention, delivery-key, chain-revision,
decision-manifest and receipt. Five additions close the earlier target gap:

- `binding-snapshot`: original binding identity, agreement/book, six roles,
  verified assent/offer/delegation refs, permitted sources, original booked net,
  exposure and the supplier's nominated invocation/held capacity when applicable.
- `base-evaluation`: exact retained `evaluation_utf8` plus hash-pinned original
  postings/bindings/predecessor evaluations and original receipt time. The JSON
  payload contains event, complete bundle/context, base claim, actions,
  explanations, deltas, consumptions, invocations, source authority and costs.
  Empty lists are retained, not guessed during replay. Original source data is
  never replaced by just the basis sum or a current price.
- `target-snapshot`: the complete policy-family/binding/limit set, original base
  evaluation, retail basis, finality evidence and rated-final observation,
  accepted time, verified assents/offers/delegations. Families with no claim
  remain members. All per-family snapshots encode the one pinned policy version.
- `base-acceptance`: binds the base evaluation and target snapshot to **one base
  acceptance**, the complete sorted seed membership and original receipt bytes.
  Its receipt's `membership_hash=digest("base-membership",members)` prevents a
  circular hash. This root must be verified against the original committed base
  acceptance when loading/importing; a newly rehashed substitute is not trusted.
- `authority-decision`: retained `Verified` observations for the exact scope,
  target/agreement/family/source, principal/grant/revision, active/read/submit/
  correct permissions, verified evidence and both received/accepted times.
  `admission` references this decision, frozen target/policy/binding/basis,
  authentication evidence, credential/authority/target/aggregate revisions and
  final-unreversed observation. These are host-verified observations, not bearer
  capabilities or caller-supplied authorization flags.

`evaluation_utf8` is canonical retained material, not executable input. The
synthetic histories provide explicit base material and original posting
projections; the audit checks their agreement and retains every named component.
General production typed-base encoding/decoding still requires independent
review and integration with the actual evaluator's historical codec. The material preserves original typed rule, action and input fields; its tags
are a candidate codec for retained values, not a new pricing operator or
authority to rate a base. Unsupported historical
codecs must fail replay; never reconstruct omitted data from totals. No claim of
production `Evaluation` roundtrip is made by this contract-only lane.

The base acceptance root is an **external immutable trust anchor** at the audit
boundary. Tests pass its original `(kind,id,hash)` separately. If every byte of
both history and its purported root is replaced, hashes alone cannot establish
which history was originally authorized. The fully rehashed membership attack
therefore must fail against the committed original anchor, not a newly supplied
self-consistent root. This is preservation of original acceptance, not a golden
fixture byte comparison masquerading as semantic validation.

## Revisions, capacities, receipts and replay

An ordinary command carries source/label, target, agreement/family, occurrence,
evidence and code. Book/payer/price/rate/policy version are resolved from the
frozen member, not command eligibility fields. The normalized chain is fixed by
the target. Corrections carry a replacement discriminator and both expected
revision ID and canonical number; the ID must equal the claim/number derivation.
This is a redundant checked encoding of the semantic numeric guard.

All mutable authority/target/base-reversal/claim/aggregate/supplier guards must
be checked together by the future coordinator. Frozen snapshot shape is not
proof of authenticity or current authorization. Replay uses the verified original
observations and times; it does not rerun today's authentication or deadlines.

Revision 1 owns the permanent claim even at zero. Each correction appends N+1,
the exact inverse of its current nonzero action and any nonzero replacement.
The previous pointer, expected ID/number and action provenance all agree. A
reversed claim remains reserved; reinstatement needs a permitted correction.
No historical edit, policy reselection, partial acceptance or reservation
replenishment occurs. Supplier correction requires its own frozen family and
permission. Its percentage still uses the same retail basis.

Limit evidence names the frozen target/binding and complete current revision set
for that binding, including zero claims, plus the replaced head and before/after
gross totals. Discount capacity is exactly original binding booked net. The
original supplier invocation, contingent held capacity and exposure remain in
base inputs; a negative retail result cannot reduce supplier obligations.

Authentication and scoped read rights precede receipt disclosure. An identical
accepted identity returns that event's original receipt before current write
permissions, time windows or policy selection. A different label with the same
ordinary original facts returns the claim's **first** receipt, even after a
correction. Changed facts conflict. Repeated corrections resolve original
identity before their now-stale guard. No duplicate adds canonical records.

For decision D, manifest membership is the sorted unique union of every new row
except its own manifest/receipt and every replay-input member. Replay inputs are
the complete bounded seed/prior accepted prefix, current event/admission and
current authority decision. Earlier manifests/receipts are prior inputs; the
current receipt is never in its own manifest. Receipt binds current event and
manifest hashes and preserves exact stored bytes. Base receipt also remains
byte-identical after outcomes/corrections.

One nonzero intention covers the full signed per-obligation decision delta.
Zero-net corrections retain both actions and explanations with no intention.
Intention prerequisites retain all earlier nonzero exports for the claim, even
through zero-net replacements or full reversals. These are candidate export
records; dispatcher execution/fencing and transaction persistence remain out of
scope. No v1 receipt is converted or rewritten into this profile.

## Evidence and integration gate

Python reconstructs every golden byte from hand-authored inputs and audits
schemas, identity, complete replay inputs and semantics. Node independently
reconstructs IDs, hashes, references, base receipts and decision membership.
Adversarial tests rewrite the entire affected hash graph, prove integrity first
in Python and Node, then require semantic rejection for changed basis, frozen
membership, revision, supplier discount/held capacity, stale correction,
ordinary/correction deadline boundaries and version-created eligibility.

Candidate `.2` is ready for a **separate fresh-context review**, not self-certified
freeze. Required review questions: lossless production base codec and v1 receipt
mapping; the original-base trust-anchor/read boundary; completeness of retained
supplier reservation observations; and canonical-to-typed equivalence across
both stores. Review the schema/ID choices and test the combined semantic lane
before allowing a decoder or persistence bridge. The durable roadmap contains
the later integration, both-store persistence, nonposting comparison, CLI/CSV
and final independent-review gates.
