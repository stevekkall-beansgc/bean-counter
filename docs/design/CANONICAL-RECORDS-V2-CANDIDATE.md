# Outcome canonical records — reconciled candidate

**Review candidate, not frozen. Fresh-context independent review pending.**
Profile `2-candidate.4` corrects candidate `.3` from commit `09b076a`; it does
not supersede or alter any v1 contract, fixture, hash or receipt. The changed
candidate hash domain prevents silent reinterpretation of the earlier draft.
Only roadmap Phase 1 is being executed; see `ROADMAP.md`. The follow-up to
reviewed `.4` commit `abe9781` fixes Node's source classification without changing
the profile, schemas, original captures or any golden bytes.

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
  Each binding used by an outcome family has a finite nonnegative premium limit. Supplier premium must
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
  to the approved outcome-family window surface. Retained legacy `Binding.outcome`
  terms keep their own approved 90-day window/7-day grace bounds.
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
512 bits each; zero is `0/1`. Multiplication and division cross-cancel operands before bounding their reduced
products, matching the approved `ExactRatio` implementation. Round once,
nearest/ties away from zero. Negate stored integer atoms for inverses.

Counters are canonical unsigned strings at most 9223372036854775807. Claim and
chain revisions start at 1. Policy version is an opaque bounded string, matching
the semantic implementation, not a numeric eligibility axis. Times are Gregorian
UTC, year 0001–9999, exactly six fractional digits, no leap seconds. Text IDs and
scope components are 1–128 UTF-8 bytes, sources 1–256, without controls; slugs
are bounded ASCII. The normative `x-utf8-maxBytes`, `x-scalar`,
`x-canonical-maxBytes` and `x-canonicalSchema` keywords are enforced by the
candidate validator. A plain Draft 2020-12 validator is shape-only and is not
conformant without these checks. Every declared bounded string has a byte bound;
field names do not decide whether a scalar is validated. Source URIs must be
absolute and contain no whitespace. All Unicode control characters reject in
identifiers, including C1 controls. Source whitespace follows Unicode
White_Space, matching approved Rust; Node uses `\p{White_Space}` rather than
JavaScript `\s`, which incorrectly includes U+FEFF. Python's extra whitespace
characters are already prohibited Cc controls. U+FEFF is accepted inside a source
and is preserved exactly. All 1,112,064 Unicode scalar values are compared as
text and sources in Python, Node and approved Rust (2,224,128 checks per runtime;
65 text and 84 source rejections). Extension values are opaque data and do not
inherit identifier rules based on their key names.

`decimal` is the normalized nonnegative Decimal encoding: at most 30 coefficient
digits and 18 fractional digits, no sign, exponent, leading/trailing redundant
zeros or negative zero. Binding `maximum_quantity` and successful work quantity
are positive. Invocation maximum quantity is nonnegative structurally and must
cover its nominated positive work quantity. Base-policy percentages are decimals
in 0–100; approved outcome percentages remain signed exact ratios. Exposure,
held amounts, booked capacities and premium/discount limits are nonnegative.
Signed result atoms and signed ratio numerators remain permitted. Every pattern
matches the entire string, including its absolute end. Atoms, ratio components,
counters, IDs, hashes, slugs and decimals reject trailing newlines, whitespace,
control characters, signs or alternate digit spellings before numeric conversion.
Python uses fullmatch plus absolute-end patterns; independent Node validation
uses the same schema and absolute-end checks. The same 217 vectors run against
the approved Rust scalar parsers. Five completely rehashed noncanonical scalar
histories pass hash-only integrity and reject in all three language validators.

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

`H(k,v) = lowercase_hex(SHA256(UTF8("ledgerlab/"+k+"/2-candidate.4") || NUL ||
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
| base-identity | bi | `[scope,target,original_kind,original_id]` |
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

`claim.facts_hash=digest("claim-facts",facts)` where `facts` is event.data
without external_id, with every evidence wrapper resolved to its original `doc_`
identity and the resulting document IDs sorted. Duplicate resolved documents
reject before facts comparison; wrapper purpose never changes retry facts;
`delivery-key.ingress_hash=digest("ingress",normalized event body)`.
`effect.facts_hash=digest("effect-facts",action body without schema,event_id,
effect_id,policy_snapshot)`. The latter retains exact amount, roles, obligation,
basis, claim/revision, book, binding, family, slot and optional inverse reference.
Every indexed or inline copy must agree with its referenced record.

## Retained base, frozen target and admission

There are 27 closed record kinds. The prior 21 kinds remain: evidence,
policy-snapshot, event, base-posting, target-basis, admission, claim,
claim-revision, effect, action, obligation, link, dependency, limit-evidence,
explanation, replay-input, intention, delivery-key, chain-revision,
decision-manifest and receipt. Six additions close the target and original-identity gaps:

- `binding-snapshot`: original binding identity, agreement/book, six roles,
  verified assent/offer/delegation refs, permitted sources, original booked net,
  exposure and the supplier's nominated invocation/held capacity when applicable.
  `binding_utf8` retains every approved `Binding` field, including optional
  `outcome`, offer, exposure and `roles.payer_delegation`; indexing fields are
  derived from it and checked, rather than treated as the original source.
- `base-evaluation`: exact retained `original_evaluation_utf8`, explicit
  `identity_mappings`, derived `evaluation_utf8`, and hash-pinned original
  postings/bindings/predecessor evaluations and original receipt time. The JSON
  payload contains event, complete bundle/context, base claim, actions,
  explanations, deltas, consumptions, invocations, source authority and costs.
  Empty lists are retained, not guessed during replay. Original source data is
  never replaced by just the basis sum or a current price.
- `base-identity`: one target-qualified projection for each original identity
  without a standalone candidate economic row, including base claims and effects.
  Its required fields are `target`, `original_target`, `original_kind`, and
  `original_id`; it creates no claim eligibility or monetary effect.
- `target-snapshot`: the complete policy-family/binding/limit set, original base
  evaluation, retail basis, finality evidence and rated-final observation,
  accepted time, verified assents/offers/delegations. `policy_utf8` retains the
  full approved Policy with version, document, ordered families/codes and limits.
  `policy_document`, `policy_document_hash`, `verified_policy_document` and
  `policy_evidence` preserve the original document and verification observation.
  Families with no claim
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

### Lossless original material and explicit projections

`base-evaluation.original_event_utf8` and `original_ingress_utf8` retain exact
canonical `ledger-event/1` bytes. Their original `ev_` identity, event-content
hash and ingress hash are checked with the unchanged v1 formulas. Source omission
and pre-resolution chain omission remain absent. The full normalized work-event
shape includes links, evidence, extensions, corrects and supplier nomination.
Extensions allow only the original 16 scalar entries/4096 canonical bytes and
cannot be accessed as pricing or authority inputs. No unknown field is silently
removed: unknown economic fields reject; approved extension values are retained.

`original_evaluation_utf8` preserves the complete actual Evaluation returned by
the approved core, including its private retained fields, native enum variants,
original event state, bundle/order, context, claim, actions, explanations, deltas,
consumptions, invocations, closed stage, receipt time, source authority and costs.
`original-evaluations.json` contains the 23 captured synthetic evaluations at the
exact approved commit. This is source material, not a reconstructed monetary
summary. Original `ev_`, `cl_`, `ac_`, `ef_`, `ob_`, `doc_` and link identities remain
unchanged everywhere, including action provenance and explanation dependencies.
For example, fixed-success-fee retains action
`ac_c541ddb16b28b7fb383f096c2bc32d5b62998d54279926b04293d6b7392ac8ea`.
No `bp2_` ID or synthetic placeholder is substituted into original material.

The typed contract encoding uses the approved model's serde enum tags; optional
fields stay absent. Explanation rounded atoms are strings. Event retains scope,
authenticated source, canonical ingress/event bytes, both hashes and original ID;
decoding normalizes and resolves it and checks every retained value and byte.
The disposable codec adds serde derives to the actual approved model and a
complete Event/Explanation adapter. It does not introduce production code.

`evaluation_utf8` is a derived projection: enum tags are translated to the
candidate vocabulary and event state is projected to its full original DTO.
Every other field, identity, optional value and vector remains exact. The entire
projection must equal the retained source after that deterministic translation.
Original vector order is preserved even where the original model permits repeat
values. `binding_utf8` must equal the complete original binding in the bundle;
original action bindings must match it.

`identity_mappings` is a canonical sorted set of
`{target,original_target,original_kind,original_id,projection}`. `projection` is a
full `{kind,id,content_hash}` reference. There is exactly one mapping per distinct
original internal identity and per binding/invocation label. Original external
labels, agreements, parties, rule/component/operation IDs remain exact source
values; they are not assigned a replacement namespace. The complete registry is
derived from typed original material, excluding opaque extensions. Each mapping
is qualified by both original and candidate target; the same genuine native ID
may occur in another target's independently checked evaluation.

The current original event maps to the candidate base event; payable base actions
to their exact posting (amount, unit, book, binding, agreement, roles and original
binding-local ordinal); original obligations to the corresponding full obligation;
bindings to their exact snapshots; and documents to matching retained evidence.
Remaining internal identities map one-to-one to `base-identity` rows. Native
sources/inputs still reference native IDs; the mapping table supplies projection
references without rewriting them. Missing, extra or duplicate original keys,
ambiguous projection IDs, cross-target mappings, wrong action/binding/obligation/
document projections and unmapped identity rows reject after complete rehashing.
All rows and mappings are members of the immutable base-acceptance root.

Accepted bases require `source_state=accepted` and full original Evaluation bytes.
The rejected cap preparation uses `source_state=rejected_preparation`, carries no
accepted Evaluation/identity mappings or fabricated base receipt, and never
enters accepted-base roundtrip counts.

The retained Binding maps `id`/`agreement` to projection
`binding_id`/`agreement_id`. Original doc references in assent/offer/delegation
resolve to retained evidence wrappers, with the original `doc_` references still
present in source bytes. `booked_net` and supplier invocation are evaluation
projections, not invented Binding fields. Every optional field in the approved
binding, rule, price, operation, matcher and context model has a declared slot.
Absence stays absent. Base actions must use their original binding's agreement,
book, roles, currency/scale and a component in that binding's applicable rule set.

Each evidence wrapper retains `document_type`, `document_version`, original
`document_id`, `document_hash` and canonical JSON `utf8`. Original document
identity/hash use v1 `H("document",[type,1,parsed_utf8])`, independently of the
candidate wrapper identity. These are synthetic documents in the examples;
no credentials are retained. The outcome policy document body contains the full
version/families/limits, while the retained Policy additionally carries its
`document` reference (avoiding a self-referential hash). At target freeze:
`Policy.document == policy_document == verified_policy_document`; the referenced
retained policy evidence must have that identity/hash and those exact terms.
All family and limit projections must derive from this full Policy. Supporting
policy evidence references are included in original base membership.

The schema-complete codec and round trips are contract artifacts. The disposable
approved-core comparison normalizes the actual original event bytes, compiles
the retained bindings, re-evaluates each of the 23 accepted bases, and compares
complete canonical original Evaluation bytes after a typed decode/encode
roundtrip. This checks all fields, IDs and ordered vectors, alongside all 43
outcome/correction results. A coherently rehashed accepted U+FEFF-source history
adds one complete 41-record/one-decision case, for 24 exact typed roundtrips and
44 decisions in the approved comparison. It does
not install a production historical decoder or v1 journal migration bridge.

### Evidence introduced by a decision

An outcome or correction may append newly verified `evidence` records. All
other target-preparation kinds remain forbidden in decision rows. New evidence
must be referenced by that exact event and its verifying `authority-decision`;
every explanation carries that authority decision and the exact request evidence
set. Replay includes new evidence alongside the current event, admission and
authority observation. The decision manifest binds all of these records and the
receipt binds that manifest. Previously retained evidence may be reused,
including a later correction that retains a different purpose wrapper for the
same original document. Request, verified, explanation and retry evidence sets
are checked for uniqueness after resolving wrappers to `doc_` IDs. Two wrappers
for one document in the same set reject; reuse across different decisions is
valid. Claim facts use these resolved document IDs, so changing only a wrapper
returns the original claim receipt, including after subsequent corrections.

Evidence JSON is inert. It cannot supply a family, policy version, binding, rate,
amount, capacity or authorization flags. Those come only from frozen membership
and the checked decision observations. New evidence never changes seed membership
or the original base-acceptance receipt. An unreferenced or unverified new proof,
a proof used by another decision's observation, or an explanation naming a
different evidence set rejects even after complete rehashing.

### Action and effect binding provenance

Every action carries `binding_id`, content-derived `binding_snapshot` and
`component`. For outcome actions, component is exactly the selected stable family
ID: it is an attribution label, not a new rule or an eligibility axis. The
binding must be the one frozen for that target/agreement/family. Its agreement,
book, roles and currency/scale agree with the policy, obligation and action.
Effects carry the same three fields and bind the exact action facts. An inverse
also preserves its prior action's binding snapshot, binding ID and component,
along with obligation, roles, book and policy provenance. Full rehashing cannot
make an unauthorized or cross-family binding substitution valid.

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
current authority decision and newly introduced evidence. Earlier manifests/receipts are prior inputs; the
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

Candidate `.4` is submitted for a **separate fresh-context review**, not self-certified
freeze. Required review questions: lossless production base codec and v1 receipt
mapping; the original-base trust-anchor/read boundary; completeness of retained
supplier reservation observations; and canonical-to-typed equivalence across
both stores. Review the schema/ID choices and test the combined semantic lane
before allowing a decoder or persistence bridge. The durable roadmap contains
the later integration, both-store persistence, nonposting comparison, CLI/CSV
and final independent-review gates.

## Compatibility from candidate.3

Candidate `.3` at `09b076a3034064a85fd3da8d626ad02a9fb9a38a` failed independent
review on original Evaluation identity preservation, resolved-document evidence
uniqueness and terminal-newline scalar acceptance. Its bytes remain in Git
history. `.4` changes schema discriminators/hash domains, requires the actual
original Evaluation and explicit one-to-one mappings, adds `base-identity`,
normalizes retry facts by original document ID, and rejects noncanonical whole
scalar spellings. Candidate IDs and receipts therefore change intentionally.
No `.3` bytes are silently interpreted as `.4`.

There is no automatic migration: prior projections cannot recover missing
original IDs, dependencies or fields. Re-author only from retained original
source material and verified observations, then independently review it. The
captured sources and exact approved-core roundtrips make this candidate's
examples reviewable; they are not production historical-decoder certification.
Historical v1 bytes, IDs, receipts, schemas, compatibility declarations and
support claims remain untouched. `ROADMAP.md` remains byte-identical to the
committed Phase 1 roadmap. A fresh reviewer decides whether this candidate can
freeze; this document does not authorize it.

The follow-up to reviewed `.4` commit `abe9781b4a37b0bb23ee86db2e5a7b6786694c80`
restores Node acceptance of U+FEFF to the existing approved source rule. There is
no new format migration or hash-domain change. Existing goldens, schemas and
native Evaluation captures remain byte-identical; only the validator, parity
checks, accepted regression and review package change.
