# Ledger Lab — canonical records v1 addendum

Normative design repair · revision 1 · 20 September 2026 · Synthetic first-slice fixture · No product implementation

## 1. Authority, scope and amendments

This addendum closes the five canonical-record blockers in `PHASE-0-BLOCKERS.md`. It controls the encodings left open by detailed design revision 1, §§5–6, 9, 12 and 26. It preserves all ten published E/C/D/R/F1/F2/A1/A2/O/I identities and the exact +100/−20 = 80 USD-atom result. It changes no authority, pricing, cap, reversal, storage, platform or release decision. The six documents and every byte below are synthetic fixture constants, not real assent.

**Explicit amendments**, not claims that the previous prose already determined these bytes:

1. Replace the abbreviated `PolicySnapshot` notation with §3 and Appendix C's complete `ledger-snapshot/1` wire schema. Document type is `snapshot`, version integer 1. Freeze the six input purposes and `decision_snapshot` association.
2. Replace ambiguous explanation `Vec<Digest>` with typed document/action ID strings in `input_refs`; add `basis_name`; freeze the seven tagged input variants, reduced ratio strings and complete bodies below. No bare digest can stand in for an action reference.
3. Add domain `effect-facts` and its precise projection in §5. Freeze a completion claim-facts object as well, since a complete journal cannot leave its payload implicit.
4. Interpret §12's “every accepted record” literally, including immutable join rows, original delivery mapping and chain revision. The manifest also includes each of the six reused input documents. This supersedes §26's abbreviated membership list. Composite primary keys are arrays, never concatenated strings or new generated IDs.
5. Add `schema:"ledger-receipt/1"` to the public receipt; the shape-only HTTP example in §17 omitted it. Distinguish its event `content_hash` from the outer receipt record hash. Freeze exact action, intention, control-transition, revision, mapping and fixture initializer encodings.
6. Normalize the policy's fixed `"1.00"` to `"1"` before document hashing, consistent with §6 decimal normalization. Replace the illustrative all-`a` assent digest with the actual raw SHA-256 of the 19 UTF-8 bytes `synthetic:fixture-1` (no newline). The demonstration evidence text is not a seventh input document.

The closed schemas here fully specify this first-slice record family. The completion claim-facts and chain control-transition forms are the only variants introduced by this repair; acquisition/link/reversal facts and invocation/reservation transition fixtures remain the existing later Phase 0 work. No unlisted field or variant is authorized by an analogy to these records. Snapshot v1 here uses the six input categories needed by this slice; additional document categories for broader fixtures require a reviewed contract amendment before implementation, not an arbitrary purpose string.

## 2. Bytes, identity and hash rules

Keep `ledger-canonical-v1`: strict UTF-8 JSON, no BOM, duplicate keys, lone surrogates, floating/exponent numeric tokens, negative numeric zero or numeric integers outside ±9007199254740991. Economic decimals/atoms and persisted counters are strings. No Unicode normalization. Object keys use UTF-16 code-unit order; output uses JCS UTF-8. Every hash input below has **no trailing LF**.

`H(k,v) = lowercase_hex(SHA256(UTF8("ledgerlab/"+k+"/1") || 0x00 || JCS(v)))`. A digest field is `sha256:` plus that hex. Identity is the published prefix plus `_` plus the appropriate H. Schema version in document/record tuples is the JSON integer `1`; revision in identity tuples is a canonical unsigned string such as `"1"`. Scope is always `[tenant,environment]` in that order.

Document body has no computed DocumentId. Its identity is `doc_` + H(`document`, `[document_type,1,body]`). The document record's outer `content_hash` is `sha256:` plus this same digest; **do not** apply `record-content` again. The event body is the existing normalized DTO, whose `id` remains external `generation-1`; its outer ID is E and content hash is H(`event-content`,body). Normalized ingress is retained as a nested object in the original delivery-key body; `ingress_hash` is H(`ingress`,ingress). This retains its exact canonical bytes and binds the original retry mapping without changing published event bytes.

All other immutable rows have H(`record-content`, `[kind,1,body]`), except the manifest whose outer content hash is H(`decision-content`,manifest_body). Its body contains neither its own hash nor its receipt. Receipt body `content_hash` equals E's event-content digest; receipt outer `content_hash` equals H(`record-content`, `["receipt",1,receipt_body]`). No self-reference or hash cycle exists.

Every envelope has exactly `{kind,scope,id,body,content_hash}`; document envelopes additionally have `document_type`. Envelopes are fixture framing, not another hashed economic record. Body schemas and duplicate indexed columns must agree. `schema_version` is always integer1. Indexed event `decision_id` comes from CR/D, `claim_facts_hash` from C, and `ingress_bytes`/`ingress_hash` from its original delivery-key body; do not add them to the event-content DTO. Event `source/external_id/operation_id/kind/chain_id` project its existing source/id/operation_id/type/chain fields; occurred_us is absent. Action currency/scale/atoms project amount. Every other indexed reference is the identically named body field or its explicitly named `_doc`/`_id` mapping in the detailed design; no independent mutable duplicate is authoritative. Generic bodies include scope, except the public receipt, which inherits scope through E and its envelope. Documents inherit envelope scope (snapshot also explicitly carries scope). The event's scoped internal identity is outside its existing body. Composite-key bodies have no redundant `id` field.

All optional fields are **omitted**, never null. Required empty arrays/objects remain `[]`/`{}`. Reject unknown fields recursively. IDs and sources obey the detailed design's UTF-8 byte limits even where JSON Schema `maxLength` counts characters. Signed atoms: `0` or optional `-` followed by nonzero digit and remaining digits; magnitude ≤10^30−1. Counters: canonical unsigned decimal, ≤9223372036854775807; schema scales 0–18 and explanation ordinals 0–255 are JSON integers. Reduced ratio numerator/positive denominator are canonical integer strings, each ≤512 bits; GCD=1, and zero is exactly `0/1`. Decimal `exact` must equal its normalized decimal `value`; normalized coefficient≤30 digits and fractional scale≤18. Schema regexes are structural bounds; enforce these value constraints too.

Timestamps are valid Gregorian UTC strings with exactly six fractional digits, year 0001–9999, no leap second. Wire `_at` values map to checked signed epoch-microseconds in physical `_us` columns. All fixture timestamps are `2026-09-20T14:00:00.000000Z`, including seed assent/start times, received/observed time and initial held delivery's next-attempt time. Equal timestamps do not imply equal commit order; initialization precedes acceptance. The fixed received/observed/next-attempt times are operational and absent from the manifest. This slice has no time-dependent economic input, so snapshot `decision_context={}`. If an allowed decision uses report-arrival eligibility, its `decision_context.received_at` must be retained and hashed; never read a replay clock.

**Array ordering:** scope and identity/hash tuples preserve positional order. Policy rules/`when`, explanation `inputs`, and manifest `explanation_ids` preserve evaluation order (ordinals 0 then 1); they are not sets. Every other array in the canonical bodies here is a set sorted by unsigned lexicographic comparison of JCS(element) UTF-8 bytes, rejecting duplicate elements and duplicate semantic keys. In particular, snapshot document ordering is by the whole element's bytes, not its purpose; payload `actions` sort by whole entry bytes (their first canonical field is `action_id`). Reject duplicate document `(purpose,document_id)`, authority `(principal_id,source,grant_id)`, prior action ID, input-ref ID and payload action ID. Snapshot prior actions include both `action_id` and that action's generic `content_hash`.

Manifest memberships and fixture journal envelopes have a special order: compare kind UTF-8 bytes, then JCS(id) UTF-8 bytes. A kind/ID may appear only once. This journal listing order does not prescribe physical insert order or claim a global commit cursor. It differs intentionally from the portable export's file/subtype order. Standalone `.json` fixture files are canonical bytes with no final newline. `.jsonl` is each canonical envelope followed by exactly one LF, including the final record. `synthetic-assent-evidence.txt` has no final newline. Pretty-printed contract files are not hash inputs. No locale, OS path, database row order, request ID, receive-time generation, credentials or floating-point arithmetic participates.

## 3. Six input documents and snapshot S

Appendix A freezes every body. P/RO/AS/G/CX/B are document types `policy`, `roles`, `assent`, `source-grant`, `context`, `binding` respectively. RO carries `schema:"ledger-roles/1"`; the six-field `roles` value used in O/actions/payload removes **only** this schema discriminator and contains no null payer delegation.

New fixture constants: grant ID `demo-source-grant-v1`, source `urn:demo:app`, principal `demo-app`, assent acceptor `demo-admin`, service `generation`, maximum quantity `"1"`, correction source `urn:demo:app`, context funding `byok`, allocation_view false, no priority/stage/invocation/offer/exposure/end time/delegation. `byok` supplies the required context field; this explicit two-rule policy emits no additional funding rule or explanation. The registered grant permits only content.generated submission plus read. Its sorted permission array is `["read","submit"]`; relation grants are empty. The binding includes this exact policy/roles/assent/context by their derived IDs. None of these choices changes the two published actions.

S has exactly `schema,scope,dsl_version,semantics_version,documents,context,authority,prior_actions,decision_context`. The two version values are integer1. `documents` lists all six input documents as `(purpose,document_id)` objects. `context` copies CX without its schema; `authority` retains the active grant identity/principal/source/document and revision `"1"`. `prior_actions=[]`. S is newly inserted while all six inputs are retained. Multiple values allowed by the schemas must still obey document type, reference, uniqueness, bounded-input and authority checks; the exact first-slice value is not inferred from a map iteration.

All seven `snapshot-ref` bodies have schema/id/scope/event_id/purpose/document_id. ID is `sr_` + H(`snapshot-ref`, `[E,purpose,document_id]`):

| Literal purpose | Required first-slice document |
|---|---|
| `policy` | P |
| `roles` | RO |
| `assent` | AS |
| `source_grant` | G |
| `binding` | B |
| `chain_context` | CX |
| `decision_snapshot` | S |

The hyphen in document type `source-grant` and underscore in association purpose `source_grant` are deliberate. S's `documents` excludes itself. Its seven associations are ordinary independently hashed immutable rows. Inputs needed for replay are verified locally; a document ID is not permission to fetch a network resource.

## 4. Explanations and tagged inputs

`ledger-explanation/1` has schema/id/scope/event_id/ordinal, optional rule_id, outcome, code, binding_id, input_refs, optional paired basis_name+basis, ordered inputs, optional paired unrounded_atoms+rounded_atoms, and action_ids. Exact structural schemas, allowed outcome/reason enums and conditional required fields are in Appendix C. Applied steps require both numeric results and at least one action; skipped/zero steps have empty action_ids. Zero rounding has numeric result `0`, no action. `basis` and unrounded values are rational **atoms**; percent is represented by the decimal tagged input (`20` and exact20/1), not an untyped string or a guessed name on `basis`.

Each input has exactly `{kind,name,value}`; kind `decimal` additionally requires `exact:{numerator,denominator}`. Allowed kinds are `decimal`, `money`, `boolean`, `source_id`, `binding_field`, `document_ref`, `action_ref`. Their value types are normalized nonnegative decimal string, Money, boolean, SourceId, bounded binding-field string, DocumentId, ActionId respectively. `name` is a bounded stable semantic label; it does not select executable code. No untagged/unknown input shape, arbitrary JSON, or null value is permitted. `input_refs` is a sorted set of document/action IDs; it is independent of ordered tagged display/evaluation inputs.

Ordinal0: generation-base, applied BASE_APPLIED, binding demo-retail-v1; input_refs[P]; inputs[decimal fixed1 exact1/1]; **omit basis_name and basis**; unrounded100/1, rounded`"100"`, actions[A1]. Ordinal1: tier-discount, applied DISCOUNT_APPLIED, same binding; sorted input_refs[A1,CX,P]; inputs in order binding.tier=`enterprise`, percent20 exact20/1, basis action_ref A1; basis_name=`self.generation.base`, basis100/1; unrounded−20/1, rounded`"-20"`, actions[A2]. IDs are xp_+H(`explanation`,[E,0]) and xp_+H(`explanation`,[E,1]); record hashes use kind `explanation`.

## 5. Facts projections and action provenance

Completion claim facts are the exact `ledger-claim-facts/1` body in Appendix A: type, resolved chain, customer, status, normalized quantity/unit, empty links/evidence. Optional binding_id/invocation_id are included **only if explicitly requested**; occurred_at/corrects appear only when supplied. Omit external delivery ID, operation label, source, scope and extensions (source/scope/operation are in claim identity). Resolved link facts, when present, are `[relation,child_event_id,predecessor_event_id]` tuples. Evidence values are retained DocumentIds. Hash with domain `claim-facts`; retain in C. C keeps source/operation/kind/token/event and facts_hash exactly as Appendix A. This defines the completion variant only.

For each original effect use domain `effect-facts` and the full `ledger-effect-facts/1` object. Copy effect `scope,agreement_id,component,claim_id,match_key,namespace`; copy action `kind,book,amount,roles,sources,links,inputs` and optional `reverses,allocation_parent` if present. Add only its schema discriminator. Include the exact signed Money and role **value**, not a role document wrapper. Exclude effect/action IDs, obligation ID, event/decision container IDs, binding version/ID, rule label, snapshot/roles document IDs and all hashes. Source event IDs and input action/link IDs remain included because they express contributing facts. Thus repricing or a changed dependency changes facts, while a provenance-only rule/version rename does not redefine the stable economic slot. IDs still use the published effect tuple and action derivation. Never hash the complete action as effect facts.

Action bodies use the exact snake_case fields in Appendix C: `amount` is a Money object, `roles` a value object plus its `roles_doc`; `rule_id`, `binding_id`, `snapshot_doc` are the retained provenance. `sources`, `links`, `inputs` are present even when empty. The inline roles agree with RO; O is independently recomputed from scope, agreement resolved through B, book, currency, scale and the roles value. No obligation table row exists. Every action-source and action-dependency edge is a separate immutable body as well as a checked projection of the action arrays. No action_links or intention_dependencies row exists in this fixture.

## 6. Intention, control and seed state

I carries schema/id/scope/event_id/destination_id/idempotency_key/obligation_id/action_ids/amount/depends_on/payload. Destination is `fake`, idempotency_key=I, amount80, depends_on[]. Its nested `ledger-obligation-delta/1` payload carries type `obligation_delta`, O, agreement demo-retail, retail book, Money80, six roles and both action breakdowns with ID/kind/component/Money. Payload has no receipt, manifest hash or mutable delivery field. Store/transmit JCS(payload), not a doubly JSON-escaped string; internal CanonicalBytes is these bytes. Its request digest is H(`intention-payload`,payload), also frozen below. The entire payload is included in the intention's generic record hash. Breakdown sums and IDs must agree with canonical actions; never recalculate a current price for delivery.

CT has control_kind `chain`, control_id `demo-slice`, from_revision`"0"`, to_revision`"1"`, event_id E, document_id S, from_event_count`"0"`, to_event_count`"1"`. ID is ct_+H(`control-transition`,[scope,"chain","demo-slice","1"]). CR separately binds chain/revision1/E/D. Neither contains an independently read wall clock. The mutable chain changes exactly once and agrees with both records.

Appendix B freezes initializer and post-acceptance operational state. Existing chain `binding_set_doc` and `context_doc` both reference CX, which already contains the complete binding_ids set; no seventh seed document is invented. The initial binding selector ID is `demo-retail-selector`, with selector_doc=B: this fixture has exactly the customer/service/source/type selector already declared by B. Parties reference RO for synthetic role metadata. Four seed immutable rows (two party, one source-grant-record, one binding-record) are frozen in addition to the six documents. These rows are initializer state, not decision manifest members. Initial heads are established directly by fixture construction; no synthetic seed control transitions are appended to the acceptance. There are no credentials in fixtures. Admin/app principal registration is named synthetic setup, not a persisted secret or extra economic document.

Initial held delivery: attempts`"0"`, generation`"0"`, next_attempt_at fixed T; omit lease_owner, lease_until, last_observation and remote receipt. Mapping observed time and received time are retained operationally as fixed T outside the delivery-key body. Their physical epoch-microseconds are derived exactly, not interpreted in the local timezone. All unlisted tables in the slice begin empty and have zero delta; installation admission remains open, dispatch disabled/held. Initializer rows do not imply a production administrative command may bypass its required audit path.

## 7. Membership, composite keys, manifest and receipt

There are exactly **25 newly accepted immutable envelopes**: document S1 + snapshot-ref7 + event1 + delivery-key1 + claim1 + effect2 + action2 + action-source2 + action-dependency1 + explanation2 + intention1 + control-transition1 + chain-revision1 + decision-manifest1 + receipt1. Additionally delivery_state+1 and one mutable chain update commit atomically, but neither is an immutable member. This exactly preserves §26's table deltas.

Manifest D contains **29 members**: all 23 new immutable envelopes except D and R, plus the six reused input documents. Seven document members total; seven snapshot-ref members. Membership is a set of exactly `{kind,id,content_hash}`. No receipt/self entry, standalone obligation/effect-facts/payload entry, seed party/grant/binding row, later alias, delivery state, lease/attempt/observation, mutable head, cache, request log or operational timestamp is included. Documents are included once regardless of number of associations. Every immutable new join/revision/mapping row is its own member; arrays in parent bodies do not replace that member. Original delivery mapping is included; any later alias is permanently retained operationally without rewriting D.

Exact composite IDs (outer envelope and manifest member) are:

| kind | `id` JSON value | Body key fields (besides schema) |
|---|---|---|
| `action-source` | `[scope,action_id,event_id]` | scope, action_id, event_id |
| `action-dependency` | `[scope,action_id,input_action_id]` | scope, action_id, input_action_id |
| `delivery-key` | `[scope,source,external_id]` | scope, source, external_id; plus canonical_event_id, kind, ingress, ingress_hash |
| `chain-revision` | `[scope,chain_id,revision]` | scope, chain_id, revision; plus event_id, decision_id |

Arrays are actual JSON arrays, not JSON encoded inside an ID string. There is no `ck_` ID or extra composite-identity hash. Composite body content_hash is the same generic `[kind,1,body]` formula. For identity comparison sort/hash the array's JCS bytes. Scope may therefore appear both in a composite ID and its body/envelope; all copies must agree. Ordinals are integers; composite revision is a string.

Manifest schema is `ledger-decision-manifest/1`, fields `id:D,scope,event_id:E,chain_id:"demo-slice",revision:"1",explanation_ids:[XP0,XP1],members:[...]`. `id` is D, derived solely from E; decision hash is H(`decision-content`, the complete body). Receipt schema and exact fields are in Appendix A; action IDs and intention IDs are sorted sets, revision a string. The receipt returns identically on identity and semantic duplicates; status/duplicate_kind are outside it.

## 8. Reproduction and limits of evidence

Machine schemas are a direct transcription of Appendix C, installed at `contracts/schemas/v1/canonical-records.schema.json`; eight convenience schemas refer to its definitions by immutable URN in the bundled local registry. Resolve locally only. Fixtures under `fixtures/journals/first-slice/` are mechanical JCS transcriptions of the bodies/envelopes here, the defined hash vectors, and the fixed state objects. `vectors.json` lists every domain, value, canonical UTF-8/hex, fully framed hash input hex, digest and prefixed ID when applicable, sorted by vector name UTF-8 bytes. `file-digests.json` lists raw bytes/ordinary SHA-256 of every other fixture artifact (it excludes itself to avoid recursion).

Both audits independently construct the documents, projections, IDs, all envelopes and complete manifest/receipt from the written constants. Python and Node each serialize/hash with separately written implementations and compare **every fixture byte** and the complete journal. Audits are read-only; the explicit authoring helper is never invoked by tests. Frozen expected files cannot be updated to accommodate a production implementation. A fixture change requires a documented contract amendment and independent review. The supplementary-key and Unicode vectors remain in the original audit.

This is document/fixture evidence only. It is not production JCS/parser conformance, authority validation, database atomicity, concurrency, cancellation, Rust, TLS, release or platform evidence. The five encoding blockers are resolved; the original Phase 0 lead can resume its remaining contract/scaffold/review gates. Completion of all Phase 0 or readiness for Phase 1 is not asserted.

Reproduce from the shared target using the already present document-check dependencies:

```sh
PYTHONPATH=../ledger-lab-v0-detailed-design/work/check-deps python3 work/phase0-audit/check_design.py --design ../ledger-lab-v0-detailed-design/outputs/LEDGER-LAB-V0-DETAILED-DESIGN.md --output work/phase0-audit/check-result.json
node work/phase0-audit/check_ids.mjs --journal .
```


### Source revisions read completely

| Source | Raw SHA-256 |
|---|---|
| LEDGER-LAB-V0-DETAILED-DESIGN.md | `9a24e039e0057897bb689d30d59eeffe95ca22a8165dd626fb70937e36eaf3d9` |
| LEDGER-LAB-ARCHITECTURE.md | `91539414db8fef5476aadae1f452bde797c4cb3cd707e8022509eead4662ab3e` |
| LEDGER-LAB-RUST-FOUNDATION-PLAN.md | `547edf7c315e94915d2319a8b5b90c5bad012923082c0d019f65b2ecaf7f6448` |
| LEDGER-LAB-ADOPTION-COST-REVIEW.md | `a0c6512ca77b328cbe5b7f681c05902e5ffc5fd96e1121bb8bfd98d3b6a8b90f` |
| LEDGER-LAB-PLATFORM-NEUTRALITY-REVIEW.md | `540ef9dbdea47b7e5db45ed965d251f64fd3fe28efc1edfc48613de7efdb4a4f` |

## Appendix A. Exact canonical bodies and hashes

Each fenced one-line JSON below is JCS(body), no LF in its hash input. The envelope rule in §2 and the listed identity/hash completely determine its journal row. Appendix order is for reading; journal order is §2. All digest values and hash input hex are also independently checked in `vectors.json`.

### P — document

Identity: `"doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83"`. Content hash: `sha256:2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83`.

```json
{"currency":"USD","id":"demo-retail-v1","rounding":"nearest_ties_away","rules":[{"amount":{"fixed":"1"},"book":"retail","component":"generation.base","id":"generation-base","on":"content.generated","op":"base"},{"amount":{"basis":"self.generation.base","percent":"20"},"book":"retail","component":"generation.discount","discount_mode":"additive","id":"tier-discount","on":"content.generated","op":"discount","when":[{"eq":"enterprise","field":"binding.tier"}]}],"scale":2,"schema":"ledger-policy/1"}
```

### RO — document

Identity: `"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd"`. Content hash: `sha256:47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd`.

```json
{"bearer":"demo-customer","beneficiary":"demo-customer","cost_originator":"demo-host","payer":"demo-customer","provider":"demo-host","recipient":"demo-host","schema":"ledger-roles/1"}
```

### AS — document

Identity: `"doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294"`. Content hash: `sha256:a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294`.

```json
{"accepted_at":"2026-09-20T14:00:00.000000Z","acceptor":"demo-admin","agreement_id":"demo-retail","bearer":"demo-customer","evidence_digest":"sha256:ac1be9ac99477fa261320b1de76fabf23651df1f79f2056920bbc71a42286421","evidence_ref":"synthetic:fixture-1","mode":"demo","payer":"demo-customer","recipient":"demo-host","schema":"ledger-assent/1","terms_version":"1"}
```

### G — document

Identity: `"doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc"`. Content hash: `sha256:253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc`.

```json
{"event_types":["content.generated"],"id":"demo-source-grant-v1","permissions":["read","submit"],"principal_id":"demo-app","relations":[],"schema":"ledger-source-grant/1","source":"urn:demo:app","starts_at":"2026-09-20T14:00:00.000000Z"}
```

### CX — document

Identity: `"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68"`. Content hash: `sha256:ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68`.

```json
{"binding_ids":["demo-retail-v1"],"currency":"USD","funding":"byok","scale":2,"schema":"ledger-context/1","tier":"enterprise"}
```

### B — document

Identity: `"doc_97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f"`. Content hash: `sha256:97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f`.

```json
{"accepted_at":"2026-09-20T14:00:00.000000Z","acceptor":"demo-admin","agreement_id":"demo-retail","allocation_view":false,"assent":"doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294","context":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","correction_sources":["urn:demo:app"],"customer":"demo-customer","event_types":["content.generated"],"id":"demo-retail-v1","maximum_quantity":"1","policy":"doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","roles":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","schema":"ledger-binding/1","service":"generation","sources":["urn:demo:app"],"starts_at":"2026-09-20T14:00:00.000000Z","unit":"call","version":1}
```

### S — document

Identity: `"doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6"`. Content hash: `sha256:2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6`.

```json
{"authority":[{"active":true,"grant_document":"doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc","grant_id":"demo-source-grant-v1","principal_id":"demo-app","revision":"1","source":"urn:demo:app"}],"context":{"binding_ids":["demo-retail-v1"],"currency":"USD","funding":"byok","scale":2,"tier":"enterprise"},"decision_context":{},"documents":[{"document_id":"doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc","purpose":"source_grant"},{"document_id":"doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","purpose":"policy"},{"document_id":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","purpose":"roles"},{"document_id":"doc_97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f","purpose":"binding"},{"document_id":"doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294","purpose":"assent"},{"document_id":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","purpose":"chain_context"}],"dsl_version":1,"prior_actions":[],"schema":"ledger-snapshot/1","scope":["demo","sandbox"],"semantics_version":1}
```

### E — event

Identity: `"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"`. Content hash: `sha256:2294c2739ebe9c2dc5b46f4901fcd8164b35e5ce8e743aa04af7a514e090a98b`.

```json
{"chain":"demo-slice","customer":"demo-customer","evidence":[],"extensions":{},"id":"generation-1","links":[],"operation_id":"generation-1","quantity":"1","schema":"ledger-event/1","source":"urn:demo:app","status":"succeeded","type":"content.generated","unit":"call"}
```

### C — claim

Identity: `"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e"`. Content hash: `sha256:cda68cc951b2363d73281490eea7c11f0b35852b54f61f774e8c90412acd24e3`.

```json
{"event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","facts_hash":"sha256:bc10c827f68e042c1dd44cbef232065f750510cd30b66b3f2ff3e28ae9824a83","id":"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e","kind":"completion","operation_id":"generation-1","schema":"ledger-claim/1","scope":["demo","sandbox"],"source":"urn:demo:app","token":"completion"}
```

### F1 — effect

Identity: `"ef_36f01e872c7261a3ee38d53c77705cfcc1c8f24be63b7377bad89f89d73d094e"`. Content hash: `sha256:50579b56daef5525c0bf96a04f1704634fc6bf5be34bdf777e95023c550423cf`.

```json
{"action_id":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","agreement_id":"demo-retail","claim_id":"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e","component":"generation.base","facts_hash":"sha256:a98afa4aa04787ae5e2c2a959dcc002bd32e9ca2357e02b247bda79467f08b5b","id":"ef_36f01e872c7261a3ee38d53c77705cfcc1c8f24be63b7377bad89f89d73d094e","match_key":"self","namespace":"original","schema":"ledger-effect/1","scope":["demo","sandbox"]}
```

### F2 — effect

Identity: `"ef_0f34830f90d5a8ff0a0d67894530be2a90b641cebbfe5b99f8dfae0c9e85600d"`. Content hash: `sha256:a672ad181995c55c5dcbeff326858e18faf1150c1d3dd93c4a35722e21d7f46f`.

```json
{"action_id":"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","agreement_id":"demo-retail","claim_id":"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e","component":"generation.discount","facts_hash":"sha256:20606dc8f571d94bec98513f528efd9573525461e9ec21dbca75f528f7ef543f","id":"ef_0f34830f90d5a8ff0a0d67894530be2a90b641cebbfe5b99f8dfae0c9e85600d","match_key":"self","namespace":"original","schema":"ledger-effect/1","scope":["demo","sandbox"]}
```

### A1 — action

Identity: `"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"`. Content hash: `sha256:c90f4f247a533acefb406cab912531c7dcde696b4578595d8bf59cc8e2e04362`.

```json
{"amount":{"atoms":"100","currency":"USD","scale":2},"binding_id":"demo-retail-v1","book":"retail","component":"generation.base","decision_id":"dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982","effect_id":"ef_36f01e872c7261a3ee38d53c77705cfcc1c8f24be63b7377bad89f89d73d094e","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","inputs":[],"kind":"charge","links":[],"obligation_id":"ob_174b2b8a9bc19c15c0b1405655327e341f77b8867bd4996d36324b494951b82e","roles":{"bearer":"demo-customer","beneficiary":"demo-customer","cost_originator":"demo-host","payer":"demo-customer","provider":"demo-host","recipient":"demo-host"},"roles_doc":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","rule_id":"generation-base","schema":"ledger-action/1","scope":["demo","sandbox"],"snapshot_doc":"doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6","sources":["ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"]}
```

### A2 — action

Identity: `"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee"`. Content hash: `sha256:a7338b42fb23823ed54e5679b11ebd26bf14ac4a69b8e45918e17af191b04101`.

```json
{"amount":{"atoms":"-20","currency":"USD","scale":2},"binding_id":"demo-retail-v1","book":"retail","component":"generation.discount","decision_id":"dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982","effect_id":"ef_0f34830f90d5a8ff0a0d67894530be2a90b641cebbfe5b99f8dfae0c9e85600d","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","inputs":["ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"],"kind":"discount","links":[],"obligation_id":"ob_174b2b8a9bc19c15c0b1405655327e341f77b8867bd4996d36324b494951b82e","roles":{"bearer":"demo-customer","beneficiary":"demo-customer","cost_originator":"demo-host","payer":"demo-customer","provider":"demo-host","recipient":"demo-host"},"roles_doc":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","rule_id":"tier-discount","schema":"ledger-action/1","scope":["demo","sandbox"],"snapshot_doc":"doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6","sources":["ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"]}
```

### XP0 — explanation

Identity: `"xp_d91c1b880e4df5d5ba98ef80433c3caf28d4179f7cedc72fe1a58c85de4882c4"`. Content hash: `sha256:9c23339fe7730dc24344a4d6d5b8aca6229769d77271eb7a0fdcdd95894eff25`.

```json
{"action_ids":["ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"],"binding_id":"demo-retail-v1","code":"BASE_APPLIED","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"xp_d91c1b880e4df5d5ba98ef80433c3caf28d4179f7cedc72fe1a58c85de4882c4","input_refs":["doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83"],"inputs":[{"exact":{"denominator":"1","numerator":"1"},"kind":"decimal","name":"fixed","value":"1"}],"ordinal":0,"outcome":"applied","rounded_atoms":"100","rule_id":"generation-base","schema":"ledger-explanation/1","scope":["demo","sandbox"],"unrounded_atoms":{"denominator":"1","numerator":"100"}}
```

### XP1 — explanation

Identity: `"xp_afb0b9c6cec9b53b9538bc8ae43735a0350652a4cbed93388f408b45e21d5d9a"`. Content hash: `sha256:df960040b355d5d41e203a29558d7d481de167d387b7dfbe1cf89778af949118`.

```json
{"action_ids":["ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee"],"basis":{"denominator":"1","numerator":"100"},"basis_name":"self.generation.base","binding_id":"demo-retail-v1","code":"DISCOUNT_APPLIED","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"xp_afb0b9c6cec9b53b9538bc8ae43735a0350652a4cbed93388f408b45e21d5d9a","input_refs":["ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68"],"inputs":[{"kind":"binding_field","name":"binding.tier","value":"enterprise"},{"exact":{"denominator":"1","numerator":"20"},"kind":"decimal","name":"percent","value":"20"},{"kind":"action_ref","name":"basis","value":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"}],"ordinal":1,"outcome":"applied","rounded_atoms":"-20","rule_id":"tier-discount","schema":"ledger-explanation/1","scope":["demo","sandbox"],"unrounded_atoms":{"denominator":"1","numerator":"-20"}}
```

### I — intention

Identity: `"in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8"`. Content hash: `sha256:600f748707b15d8b8420d5c22b5b4eec85a46628dd098a9acd629205eaed2449`.

```json
{"action_ids":["ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee"],"amount":{"atoms":"80","currency":"USD","scale":2},"depends_on":[],"destination_id":"fake","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8","idempotency_key":"in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8","obligation_id":"ob_174b2b8a9bc19c15c0b1405655327e341f77b8867bd4996d36324b494951b82e","payload":{"actions":[{"action_id":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","amount":{"atoms":"100","currency":"USD","scale":2},"component":"generation.base","kind":"charge"},{"action_id":"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","amount":{"atoms":"-20","currency":"USD","scale":2},"component":"generation.discount","kind":"discount"}],"agreement_id":"demo-retail","amount":{"atoms":"80","currency":"USD","scale":2},"book":"retail","obligation_id":"ob_174b2b8a9bc19c15c0b1405655327e341f77b8867bd4996d36324b494951b82e","roles":{"bearer":"demo-customer","beneficiary":"demo-customer","cost_originator":"demo-host","payer":"demo-customer","provider":"demo-host","recipient":"demo-host"},"schema":"ledger-obligation-delta/1","type":"obligation_delta"},"schema":"ledger-intention/1","scope":["demo","sandbox"]}
```

### CT — control-transition

Identity: `"ct_7ce8b8ce3d507b523475ff3f2e6e241cd9103213d3c61e794b387fa3bf76e27a"`. Content hash: `sha256:1f14020821e00ff126e18145350100df8588aee97c09c675e87ae5d47d4d3a1e`.

```json
{"control_id":"demo-slice","control_kind":"chain","document_id":"doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","from_event_count":"0","from_revision":"0","id":"ct_7ce8b8ce3d507b523475ff3f2e6e241cd9103213d3c61e794b387fa3bf76e27a","schema":"ledger-control-transition/1","scope":["demo","sandbox"],"to_event_count":"1","to_revision":"1"}
```

### CR — chain-revision

Identity: `[["demo","sandbox"],"demo-slice","1"]`. Content hash: `sha256:bd35435977a920907bdc107ea160c2168c089ce5b65884fa90e18e42cf084446`.

```json
{"chain_id":"demo-slice","decision_id":"dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","revision":"1","schema":"ledger-chain-revision/1","scope":["demo","sandbox"]}
```

### DK — delivery-key

Identity: `[["demo","sandbox"],"urn:demo:app","generation-1"]`. Content hash: `sha256:6555b1af35230495910d2896a9c314d650b403a1e616372e98e5b526e93f9bc8`.

```json
{"canonical_event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","external_id":"generation-1","ingress":{"chain":"demo-slice","customer":"demo-customer","evidence":[],"extensions":{},"id":"generation-1","links":[],"operation_id":"generation-1","quantity":"1","schema":"ledger-event/1","source":"urn:demo:app","status":"succeeded","type":"content.generated","unit":"call"},"ingress_hash":"sha256:aee561a6a83b095b5c90119a5a96684f9d0eae7ccd786822b84d570e483b080d","kind":"original","schema":"ledger-delivery-key/1","scope":["demo","sandbox"],"source":"urn:demo:app"}
```

### SOURCE.1 — action-source

Identity: `[["demo","sandbox"],"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"]`. Content hash: `sha256:1bb1ff6dceafe9e45d0190afa8fc7662647430f94991d0f5c588178a404a1e7a`.

```json
{"action_id":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","schema":"ledger-action-source/1","scope":["demo","sandbox"]}
```

### SOURCE.2 — action-source

Identity: `[["demo","sandbox"],"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"]`. Content hash: `sha256:22386b4b458a61da42954258f700c3459e82da3b7cf43c0f522a683c1f620165`.

```json
{"action_id":"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","schema":"ledger-action-source/1","scope":["demo","sandbox"]}
```

### DEPENDENCY — action-dependency

Identity: `[["demo","sandbox"],"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"]`. Content hash: `sha256:98cf62a3448bdfe5711dcd4f631426c3be9e27faea16a36590a44d74e3a6e846`.

```json
{"action_id":"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","input_action_id":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","schema":"ledger-action-dependency/1","scope":["demo","sandbox"]}
```

### SR.policy — snapshot-ref

Identity: `"sr_7402b021ac9fbd966bf3ca43c0ca6c47d987013ae1cbb286c415665e454d3900"`. Content hash: `sha256:cf338c6e1227bcd0daa7f7196f71f7fe53667cbc4750c6e721129781ec217a14`.

```json
{"document_id":"doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_7402b021ac9fbd966bf3ca43c0ca6c47d987013ae1cbb286c415665e454d3900","purpose":"policy","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### SR.roles — snapshot-ref

Identity: `"sr_886429e0aee0529fe7e148f7d7363928e8fd0c5412f31c820c9c13afe26afcfc"`. Content hash: `sha256:185a9c4e64a8b89d9f559852726c8fb34d34e70790b3da5c12e3276d76b3e8e9`.

```json
{"document_id":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_886429e0aee0529fe7e148f7d7363928e8fd0c5412f31c820c9c13afe26afcfc","purpose":"roles","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### SR.assent — snapshot-ref

Identity: `"sr_f0d4ceeed19b3d37bec5e3a179f3205d5fa75dc45b6962ec1aff9fd1586533b4"`. Content hash: `sha256:2eb88d1df294fdb60ba390a9dde331c01a01c0d87451ac178038778e72c1d1a3`.

```json
{"document_id":"doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_f0d4ceeed19b3d37bec5e3a179f3205d5fa75dc45b6962ec1aff9fd1586533b4","purpose":"assent","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### SR.source_grant — snapshot-ref

Identity: `"sr_8d5379ec75c52c74037b7b12ee56f2c88193b76e661b8b1292699f07da643e56"`. Content hash: `sha256:7da8f1d651a8eb1a057ffc8ce711c9db66dc64cd8a2866f1f1f0de285e885cb9`.

```json
{"document_id":"doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_8d5379ec75c52c74037b7b12ee56f2c88193b76e661b8b1292699f07da643e56","purpose":"source_grant","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### SR.binding — snapshot-ref

Identity: `"sr_272609f69cba6564dc1b6a41983443498754efd58d15b0d520d3c89e2d89459c"`. Content hash: `sha256:68035a0042140799605166248aded65e8a1371fa07f37a4e6d12ddfedbd90b84`.

```json
{"document_id":"doc_97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_272609f69cba6564dc1b6a41983443498754efd58d15b0d520d3c89e2d89459c","purpose":"binding","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### SR.chain_context — snapshot-ref

Identity: `"sr_155c9f001ddd5010ede1effb4e3e56d4da66970f9dee6b3e3e02083f26fe46b7"`. Content hash: `sha256:f523f0c41ec7369d487af8c33b747cdc30478c0bc40e30b062983a0d16306888`.

```json
{"document_id":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_155c9f001ddd5010ede1effb4e3e56d4da66970f9dee6b3e3e02083f26fe46b7","purpose":"chain_context","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### SR.decision_snapshot — snapshot-ref

Identity: `"sr_9bc6010495480a7ddfa3a754c44558cbfef2d975759e0f8033de65438aeba5f5"`. Content hash: `sha256:cd2e3a82c7b3c8d7850b2e84f066c5a0193703c1b5ca76fe41f87531508b81fa`.

```json
{"document_id":"doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"sr_9bc6010495480a7ddfa3a754c44558cbfef2d975759e0f8033de65438aeba5f5","purpose":"decision_snapshot","schema":"ledger-snapshot-ref/1","scope":["demo","sandbox"]}
```

### D — decision-manifest

Identity: `"dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982"`. Content hash: `sha256:33dd38b0ae18a1a037b18494115b3689c47e9378e9af466fd57837e70091c650`.

```json
{"chain_id":"demo-slice","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","explanation_ids":["xp_d91c1b880e4df5d5ba98ef80433c3caf28d4179f7cedc72fe1a58c85de4882c4","xp_afb0b9c6cec9b53b9538bc8ae43735a0350652a4cbed93388f408b45e21d5d9a"],"id":"dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982","members":[{"content_hash":"sha256:c90f4f247a533acefb406cab912531c7dcde696b4578595d8bf59cc8e2e04362","id":"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","kind":"action"},{"content_hash":"sha256:a7338b42fb23823ed54e5679b11ebd26bf14ac4a69b8e45918e17af191b04101","id":"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","kind":"action"},{"content_hash":"sha256:98cf62a3448bdfe5711dcd4f631426c3be9e27faea16a36590a44d74e3a6e846","id":[["demo","sandbox"],"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"],"kind":"action-dependency"},{"content_hash":"sha256:1bb1ff6dceafe9e45d0190afa8fc7662647430f94991d0f5c588178a404a1e7a","id":[["demo","sandbox"],"ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"],"kind":"action-source"},{"content_hash":"sha256:22386b4b458a61da42954258f700c3459e82da3b7cf43c0f522a683c1f620165","id":[["demo","sandbox"],"ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee","ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"],"kind":"action-source"},{"content_hash":"sha256:bd35435977a920907bdc107ea160c2168c089ce5b65884fa90e18e42cf084446","id":[["demo","sandbox"],"demo-slice","1"],"kind":"chain-revision"},{"content_hash":"sha256:cda68cc951b2363d73281490eea7c11f0b35852b54f61f774e8c90412acd24e3","id":"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e","kind":"claim"},{"content_hash":"sha256:1f14020821e00ff126e18145350100df8588aee97c09c675e87ae5d47d4d3a1e","id":"ct_7ce8b8ce3d507b523475ff3f2e6e241cd9103213d3c61e794b387fa3bf76e27a","kind":"control-transition"},{"content_hash":"sha256:6555b1af35230495910d2896a9c314d650b403a1e616372e98e5b526e93f9bc8","id":[["demo","sandbox"],"urn:demo:app","generation-1"],"kind":"delivery-key"},{"content_hash":"sha256:253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc","id":"doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc","kind":"document"},{"content_hash":"sha256:2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","id":"doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","kind":"document"},{"content_hash":"sha256:2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6","id":"doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6","kind":"document"},{"content_hash":"sha256:47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","id":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","kind":"document"},{"content_hash":"sha256:97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f","id":"doc_97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f","kind":"document"},{"content_hash":"sha256:a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294","id":"doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294","kind":"document"},{"content_hash":"sha256:ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","id":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","kind":"document"},{"content_hash":"sha256:a672ad181995c55c5dcbeff326858e18faf1150c1d3dd93c4a35722e21d7f46f","id":"ef_0f34830f90d5a8ff0a0d67894530be2a90b641cebbfe5b99f8dfae0c9e85600d","kind":"effect"},{"content_hash":"sha256:50579b56daef5525c0bf96a04f1704634fc6bf5be34bdf777e95023c550423cf","id":"ef_36f01e872c7261a3ee38d53c77705cfcc1c8f24be63b7377bad89f89d73d094e","kind":"effect"},{"content_hash":"sha256:2294c2739ebe9c2dc5b46f4901fcd8164b35e5ce8e743aa04af7a514e090a98b","id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","kind":"event"},{"content_hash":"sha256:df960040b355d5d41e203a29558d7d481de167d387b7dfbe1cf89778af949118","id":"xp_afb0b9c6cec9b53b9538bc8ae43735a0350652a4cbed93388f408b45e21d5d9a","kind":"explanation"},{"content_hash":"sha256:9c23339fe7730dc24344a4d6d5b8aca6229769d77271eb7a0fdcdd95894eff25","id":"xp_d91c1b880e4df5d5ba98ef80433c3caf28d4179f7cedc72fe1a58c85de4882c4","kind":"explanation"},{"content_hash":"sha256:600f748707b15d8b8420d5c22b5b4eec85a46628dd098a9acd629205eaed2449","id":"in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8","kind":"intention"},{"content_hash":"sha256:f523f0c41ec7369d487af8c33b747cdc30478c0bc40e30b062983a0d16306888","id":"sr_155c9f001ddd5010ede1effb4e3e56d4da66970f9dee6b3e3e02083f26fe46b7","kind":"snapshot-ref"},{"content_hash":"sha256:68035a0042140799605166248aded65e8a1371fa07f37a4e6d12ddfedbd90b84","id":"sr_272609f69cba6564dc1b6a41983443498754efd58d15b0d520d3c89e2d89459c","kind":"snapshot-ref"},{"content_hash":"sha256:cf338c6e1227bcd0daa7f7196f71f7fe53667cbc4750c6e721129781ec217a14","id":"sr_7402b021ac9fbd966bf3ca43c0ca6c47d987013ae1cbb286c415665e454d3900","kind":"snapshot-ref"},{"content_hash":"sha256:185a9c4e64a8b89d9f559852726c8fb34d34e70790b3da5c12e3276d76b3e8e9","id":"sr_886429e0aee0529fe7e148f7d7363928e8fd0c5412f31c820c9c13afe26afcfc","kind":"snapshot-ref"},{"content_hash":"sha256:7da8f1d651a8eb1a057ffc8ce711c9db66dc64cd8a2866f1f1f0de285e885cb9","id":"sr_8d5379ec75c52c74037b7b12ee56f2c88193b76e661b8b1292699f07da643e56","kind":"snapshot-ref"},{"content_hash":"sha256:cd2e3a82c7b3c8d7850b2e84f066c5a0193703c1b5ca76fe41f87531508b81fa","id":"sr_9bc6010495480a7ddfa3a754c44558cbfef2d975759e0f8033de65438aeba5f5","kind":"snapshot-ref"},{"content_hash":"sha256:2eb88d1df294fdb60ba390a9dde331c01a01c0d87451ac178038778e72c1d1a3","id":"sr_f0d4ceeed19b3d37bec5e3a179f3205d5fa75dc45b6962ec1aff9fd1586533b4","kind":"snapshot-ref"}],"revision":"1","schema":"ledger-decision-manifest/1","scope":["demo","sandbox"]}
```

### R — receipt

Identity: `"rc_01b0da31e91538160d2fbe857c6b69358457d6ec3a0dc85c7689385e031954ae"`. Content hash: `sha256:e14fb72aa8086999c6b8f67794f35399b031cb331c0a06fb7a2a45aa5a0af1bd`.

```json
{"action_ids":["ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1","ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee"],"chain_id":"demo-slice","content_hash":"sha256:2294c2739ebe9c2dc5b46f4901fcd8164b35e5ce8e743aa04af7a514e090a98b","decision_hash":"sha256:33dd38b0ae18a1a037b18494115b3689c47e9378e9af466fd57837e70091c650","decision_id":"dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982","event_id":"ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b","id":"rc_01b0da31e91538160d2fbe857c6b69358457d6ec3a0dc85c7689385e031954ae","intention_ids":["in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8"],"revision":"1","schema":"ledger-receipt/1"}
```

### Completion claim facts

```json
{"chain":"demo-slice","customer":"demo-customer","evidence":[],"links":[],"quantity":"1","schema":"ledger-claim-facts/1","status":"succeeded","type":"content.generated","unit":"call"}
```

### Effect facts 1

```json
{"agreement_id":"demo-retail","amount":{"atoms":"100","currency":"USD","scale":2},"book":"retail","claim_id":"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e","component":"generation.base","inputs":[],"kind":"charge","links":[],"match_key":"self","namespace":"original","roles":{"bearer":"demo-customer","beneficiary":"demo-customer","cost_originator":"demo-host","payer":"demo-customer","provider":"demo-host","recipient":"demo-host"},"schema":"ledger-effect-facts/1","scope":["demo","sandbox"],"sources":["ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"]}
```

### Effect facts 2

```json
{"agreement_id":"demo-retail","amount":{"atoms":"-20","currency":"USD","scale":2},"book":"retail","claim_id":"cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e","component":"generation.discount","inputs":["ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1"],"kind":"discount","links":[],"match_key":"self","namespace":"original","roles":{"bearer":"demo-customer","beneficiary":"demo-customer","cost_originator":"demo-host","payer":"demo-customer","provider":"demo-host","recipient":"demo-host"},"schema":"ledger-effect-facts/1","scope":["demo","sandbox"],"sources":["ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b"]}
```

### Complete hash vector register

Digests here are hex without the `sha256:` display wrapper. Domain version is 1. The optional ID is prefix+digest; the formula and exact body/tuple are fixed above.

| Vector | Domain | Digest | ID if derived |
|---|---|---|---|
| A1 | `action` | `70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1` | `ac_70ece6b17fa43aed5cb85d652f8b2c2e18ef8ecea0f7664fd4e59763a2eb71d1` |
| A1.content | `record-content` | `c90f4f247a533acefb406cab912531c7dcde696b4578595d8bf59cc8e2e04362` | `—` |
| A2 | `action` | `86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee` | `ac_86b5a003686d863e721cde7d27d79c27cbdb4b1e8c5afb30b040e67f1fa374ee` |
| A2.content | `record-content` | `a7338b42fb23823ed54e5679b11ebd26bf14ac4a69b8e45918e17af191b04101` | `—` |
| AS | `document` | `a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294` | `doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294` |
| B | `document` | `97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f` | `doc_97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f` |
| C | `claim` | `e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e` | `cl_e94c2786c33d6bd2586843bdd896215c0e2afcff37196b9e7e1f42cdd356ff2e` |
| C.content | `record-content` | `cda68cc951b2363d73281490eea7c11f0b35852b54f61f774e8c90412acd24e3` | `—` |
| CR.content | `record-content` | `bd35435977a920907bdc107ea160c2168c089ce5b65884fa90e18e42cf084446` | `—` |
| CT | `control-transition` | `7ce8b8ce3d507b523475ff3f2e6e241cd9103213d3c61e794b387fa3bf76e27a` | `ct_7ce8b8ce3d507b523475ff3f2e6e241cd9103213d3c61e794b387fa3bf76e27a` |
| CT.content | `record-content` | `1f14020821e00ff126e18145350100df8588aee97c09c675e87ae5d47d4d3a1e` | `—` |
| CX | `document` | `ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68` | `doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68` |
| D | `decision` | `2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982` | `dc_2b34a8ec22d757c7a07a02e9bac1a6f5810a2c5e9716ea0783158472be600982` |
| DEPENDENCY.content | `record-content` | `98cf62a3448bdfe5711dcd4f631426c3be9e27faea16a36590a44d74e3a6e846` | `—` |
| DK.content | `record-content` | `6555b1af35230495910d2896a9c314d650b403a1e616372e98e5b526e93f9bc8` | `—` |
| E | `event` | `f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b` | `ev_f4810ef7aa53d2de62eabc0067824efcd635f5f830901d9f1ae09b204ef1cb8b` |
| F1 | `effect` | `36f01e872c7261a3ee38d53c77705cfcc1c8f24be63b7377bad89f89d73d094e` | `ef_36f01e872c7261a3ee38d53c77705cfcc1c8f24be63b7377bad89f89d73d094e` |
| F1.content | `record-content` | `50579b56daef5525c0bf96a04f1704634fc6bf5be34bdf777e95023c550423cf` | `—` |
| F2 | `effect` | `0f34830f90d5a8ff0a0d67894530be2a90b641cebbfe5b99f8dfae0c9e85600d` | `ef_0f34830f90d5a8ff0a0d67894530be2a90b641cebbfe5b99f8dfae0c9e85600d` |
| F2.content | `record-content` | `a672ad181995c55c5dcbeff326858e18faf1150c1d3dd93c4a35722e21d7f46f` | `—` |
| G | `document` | `253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc` | `doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc` |
| I | `intention` | `848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8` | `in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8` |
| I.content | `record-content` | `600f748707b15d8b8420d5c22b5b4eec85a46628dd098a9acd629205eaed2449` | `—` |
| O | `obligation` | `174b2b8a9bc19c15c0b1405655327e341f77b8867bd4996d36324b494951b82e` | `ob_174b2b8a9bc19c15c0b1405655327e341f77b8867bd4996d36324b494951b82e` |
| P | `document` | `2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83` | `doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83` |
| R | `receipt` | `01b0da31e91538160d2fbe857c6b69358457d6ec3a0dc85c7689385e031954ae` | `rc_01b0da31e91538160d2fbe857c6b69358457d6ec3a0dc85c7689385e031954ae` |
| R.content | `record-content` | `e14fb72aa8086999c6b8f67794f35399b031cb331c0a06fb7a2a45aa5a0af1bd` | `—` |
| RO | `document` | `47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd` | `doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd` |
| S | `document` | `2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6` | `doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6` |
| SOURCE.1.content | `record-content` | `1bb1ff6dceafe9e45d0190afa8fc7662647430f94991d0f5c588178a404a1e7a` | `—` |
| SOURCE.2.content | `record-content` | `22386b4b458a61da42954258f700c3459e82da3b7cf43c0f522a683c1f620165` | `—` |
| SR.assent | `snapshot-ref` | `f0d4ceeed19b3d37bec5e3a179f3205d5fa75dc45b6962ec1aff9fd1586533b4` | `sr_f0d4ceeed19b3d37bec5e3a179f3205d5fa75dc45b6962ec1aff9fd1586533b4` |
| SR.assent.content | `record-content` | `2eb88d1df294fdb60ba390a9dde331c01a01c0d87451ac178038778e72c1d1a3` | `—` |
| SR.binding | `snapshot-ref` | `272609f69cba6564dc1b6a41983443498754efd58d15b0d520d3c89e2d89459c` | `sr_272609f69cba6564dc1b6a41983443498754efd58d15b0d520d3c89e2d89459c` |
| SR.binding.content | `record-content` | `68035a0042140799605166248aded65e8a1371fa07f37a4e6d12ddfedbd90b84` | `—` |
| SR.chain_context | `snapshot-ref` | `155c9f001ddd5010ede1effb4e3e56d4da66970f9dee6b3e3e02083f26fe46b7` | `sr_155c9f001ddd5010ede1effb4e3e56d4da66970f9dee6b3e3e02083f26fe46b7` |
| SR.chain_context.content | `record-content` | `f523f0c41ec7369d487af8c33b747cdc30478c0bc40e30b062983a0d16306888` | `—` |
| SR.decision_snapshot | `snapshot-ref` | `9bc6010495480a7ddfa3a754c44558cbfef2d975759e0f8033de65438aeba5f5` | `sr_9bc6010495480a7ddfa3a754c44558cbfef2d975759e0f8033de65438aeba5f5` |
| SR.decision_snapshot.content | `record-content` | `cd2e3a82c7b3c8d7850b2e84f066c5a0193703c1b5ca76fe41f87531508b81fa` | `—` |
| SR.policy | `snapshot-ref` | `7402b021ac9fbd966bf3ca43c0ca6c47d987013ae1cbb286c415665e454d3900` | `sr_7402b021ac9fbd966bf3ca43c0ca6c47d987013ae1cbb286c415665e454d3900` |
| SR.policy.content | `record-content` | `cf338c6e1227bcd0daa7f7196f71f7fe53667cbc4750c6e721129781ec217a14` | `—` |
| SR.roles | `snapshot-ref` | `886429e0aee0529fe7e148f7d7363928e8fd0c5412f31c820c9c13afe26afcfc` | `sr_886429e0aee0529fe7e148f7d7363928e8fd0c5412f31c820c9c13afe26afcfc` |
| SR.roles.content | `record-content` | `185a9c4e64a8b89d9f559852726c8fb34d34e70790b3da5c12e3276d76b3e8e9` | `—` |
| SR.source_grant | `snapshot-ref` | `8d5379ec75c52c74037b7b12ee56f2c88193b76e661b8b1292699f07da643e56` | `sr_8d5379ec75c52c74037b7b12ee56f2c88193b76e661b8b1292699f07da643e56` |
| SR.source_grant.content | `record-content` | `7da8f1d651a8eb1a057ffc8ce711c9db66dc64cd8a2866f1f1f0de285e885cb9` | `—` |
| XP0 | `explanation` | `d91c1b880e4df5d5ba98ef80433c3caf28d4179f7cedc72fe1a58c85de4882c4` | `xp_d91c1b880e4df5d5ba98ef80433c3caf28d4179f7cedc72fe1a58c85de4882c4` |
| XP0.content | `record-content` | `9c23339fe7730dc24344a4d6d5b8aca6229769d77271eb7a0fdcdd95894eff25` | `—` |
| XP1 | `explanation` | `afb0b9c6cec9b53b9538bc8ae43735a0350652a4cbed93388f408b45e21d5d9a` | `xp_afb0b9c6cec9b53b9538bc8ae43735a0350652a4cbed93388f408b45e21d5d9a` |
| XP1.content | `record-content` | `df960040b355d5d41e203a29558d7d481de167d387b7dfbe1cf89778af949118` | `—` |
| binding-record | `record-content` | `9d9ff1bc28aeb3357948cb7003519e2c8cf889fa3cdb58269bc93ac41fdb6755` | `—` |
| claim-facts | `claim-facts` | `bc10c827f68e042c1dd44cbef232065f750510cd30b66b3f2ff3e28ae9824a83` | `—` |
| decision-content | `decision-content` | `33dd38b0ae18a1a037b18494115b3689c47e9378e9af466fd57837e70091c650` | `—` |
| effect-facts.1 | `effect-facts` | `a98afa4aa04787ae5e2c2a959dcc002bd32e9ca2357e02b247bda79467f08b5b` | `—` |
| effect-facts.2 | `effect-facts` | `20606dc8f571d94bec98513f528efd9573525461e9ec21dbca75f528f7ef543f` | `—` |
| event-content | `event-content` | `2294c2739ebe9c2dc5b46f4901fcd8164b35e5ce8e743aa04af7a514e090a98b` | `—` |
| ingress | `ingress` | `aee561a6a83b095b5c90119a5a96684f9d0eae7ccd786822b84d570e483b080d` | `—` |
| intention-payload | `intention-payload` | `150e18f1f982cde7317f9d771405994d3c3b537349c9211b96526df230e588f1` | `—` |
| party.demo-customer | `record-content` | `7d0680fc3160804f9aef78783ebccd2c1077b6baedf7ef9600e82719df8c35ca` | `—` |
| party.demo-host | `record-content` | `fbd854805d7fe69b13b6e4e489cecb9f8afeba153892f18f65a83f8ebf46aee3` | `—` |
| source-grant-record | `record-content` | `6a2a985577d9397257aee520d80c739ccc455545bf5ddfa9431f6434507c3566` | `—` |

## Appendix B. Seed rows, operational state and file integrity

### Seed binding-record demo-retail-v1

```json
{"body":{"agreement_id":"demo-retail","assent_doc":"doc_a0715c2ae612c2833accfc10d96e045027c55dd376b41d90098d695d33dbd294","context_doc":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","currency":"USD","id":"demo-retail-v1","policy_doc":"doc_2a6ca5dab1098f7ee0484b8b8931454bfbfbbed9a47a363472522206a3127e83","roles_doc":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","scale":2,"schema":"ledger-binding-record/1","scope":["demo","sandbox"],"version":1},"content_hash":"sha256:9d9ff1bc28aeb3357948cb7003519e2c8cf889fa3cdb58269bc93ac41fdb6755","id":"demo-retail-v1","kind":"binding-record","scope":["demo","sandbox"]}
```

### Seed party demo-customer

```json
{"body":{"id":"demo-customer","role_metadata_doc":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","schema":"ledger-party/1","scope":["demo","sandbox"]},"content_hash":"sha256:7d0680fc3160804f9aef78783ebccd2c1077b6baedf7ef9600e82719df8c35ca","id":"demo-customer","kind":"party","scope":["demo","sandbox"]}
```

### Seed party demo-host

```json
{"body":{"id":"demo-host","role_metadata_doc":"doc_47f86db532ff17a5b0375945d188c00944c5244a04bd718796fecff11f5781bd","schema":"ledger-party/1","scope":["demo","sandbox"]},"content_hash":"sha256:fbd854805d7fe69b13b6e4e489cecb9f8afeba153892f18f65a83f8ebf46aee3","id":"demo-host","kind":"party","scope":["demo","sandbox"]}
```

### Seed source-grant-record demo-source-grant-v1

```json
{"body":{"grant_doc":"doc_253a2447099103894a9a001df4da76549bd471768f9bc0790d98848fec204ecc","id":"demo-source-grant-v1","principal_id":"demo-app","schema":"ledger-source-grant-record/1","scope":["demo","sandbox"],"source":"urn:demo:app"},"content_hash":"sha256:6a2a985577d9397257aee520d80c739ccc455545bf5ddfa9431f6434507c3566","id":"demo-source-grant-v1","kind":"source-grant-record","scope":["demo","sandbox"]}
```

### Preseed state

```json
{"admission":"open","authority_head":{"active":true,"grant_id":"demo-source-grant-v1","id":"demo-source-grant-v1","revision":"1"},"binding_head":{"active":true,"binding_id":"demo-retail-v1","id":"demo-retail-selector","revision":"1","selector_doc":"doc_97237b9ba83b3e225c8229b2a20e298ae698bbd51672d8891205022d4029088f"},"chain":{"binding_set_doc":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","context_doc":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","currency":"USD","customer":"demo-customer","event_count":"0","id":"demo-slice","revision":"0","scale":2},"credentials":"excluded","dispatch_enabled":false,"dispatch_hold":true,"logical_store_id":"store-demo-slice","mode":"sandbox","principals":["demo-admin","demo-app"],"schema":"ledger-first-slice-state/1","scope":["demo","sandbox"]}
```

### After acceptance operational state

```json
{"chain":{"binding_set_doc":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","context_doc":"doc_ffa3f9d4569e2e7586ce281a2538575a736ddbbfa0868d3a8b6edbd16f494d68","currency":"USD","customer":"demo-customer","event_count":"1","id":"demo-slice","revision":"1","scale":2},"delivery_key_observed_at":"2026-09-20T14:00:00.000000Z","delivery_state":{"attempts":"0","generation":"0","intention_id":"in_848f5bcafa2f73068fd08eaf67eef3578afc4d4996dd5899e329687279fbedf8","next_attempt_at":"2026-09-20T14:00:00.000000Z","state":"held"},"received_at":"2026-09-20T14:00:00.000000Z","schema":"ledger-first-slice-operational/1","scope":["demo","sandbox"]}
```

### Frozen fixture files

| File | Bytes | Raw SHA-256 |
|---|---:|---|
| accepted-records.jsonl | 25014 | `51c768879cdd78a0bbcbe436d3ca91c3f6bdeb0df15f02d47e9f3e1ba67eb1a2` |
| claim-facts.json | 183 | `83027d46d95529e441fdc10bb91f96a91f76ec1b9a13ab24df2df6d11697ee1a` |
| effect-facts-1.json | 598 | `5f5db1b270a746ea3a6c2a8b50c0a09cd7e87efb6854bbfbb414e5b596441b0d` |
| effect-facts-2.json | 673 | `f85ba6c59d41ed378cd7ce61e17b36a9d3c39c6382d8f7771c6fbe2d656c1526` |
| explanation-0.json | 678 | `8986294fb04b1717a90556e851387c1d389d4c26e37fc661bb0c623c5d3f00dc` |
| explanation-1.json | 1090 | `95b4244ea65b186bf92dd28288e44b13d6ce8d0434293165d40d8e5b69a4d4ae` |
| file-digests.json | 1724 | `07261dec0ef26ff61719066a20407876a417f3f53d4373f1fbbd613dcef23484` |
| manifest.json | 6058 | `5f6ca8eb5664e65f4f40d237d54ce21bfb91428b0bbb10068acbfbf2e21d2087` |
| post-acceptance-state.json | 661 | `aa59dcdb02bc3247e268474319fc45d6a18e0feb81890a76a934914390eb1e5b` |
| preseed-state.json | 839 | `8f5481fe90b1f4e26aa0b206701aff365959559503a4c32e16ee4c4e1c248487` |
| receipt.json | 730 | `45dd0fa4eea8a4c96b5a68c2f19954421ecf9a2125f687470b9be55b2e77dbbc` |
| seed-documents.jsonl | 3617 | `47fd9e074fdd119f89f02b54d67ad71ad554335dafd3be5d7c45e0ba3ea1987f` |
| seed-records.jsonl | 1721 | `a95e45680ba5a9b8a21d65b59ec19cd14b2948c9861ea94ab8ec4bcdc838b29b` |
| snapshot.json | 1125 | `04acd3fb79344620ccc7563b2c68f8745023d53eaa5c65cecffb531a0994a311` |
| synthetic-assent-evidence.txt | 19 | `ac1be9ac99477fa261320b1de76fabf23651df1f79f2056920bbc71a42286421` |
| vectors.json | 186463 | `b89afdc86cb5569d730666e020e4e2a8615667fefd438d988c8c93d43abee456` |

## Appendix C. Complete structural schema (normative)

This entire JSON Schema is the machine contract, reproduced without changes in `contracts/schemas/v1/canonical-records.schema.json`. Local `$defs` include every referenced value type. Additional value/reference/economic/order checks in §§2–7 remain mandatory; JSON Schema alone cannot prove them. The published event/policy structural schemas are retained here with renamed internal `$defs` references only. No schema default silently changes the frozen bodies.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:ledgerlab:canonical-records:1",
  "$ref": "#/$defs/record",
  "$defs": {
    "text": {
      "type": "string",
      "minLength": 1,
      "maxLength": 128,
      "pattern": "^[^\\x00-\\x1f\\x7f]+$"
    },
    "source": {
      "type": "string",
      "minLength": 1,
      "maxLength": 256,
      "pattern": "^[^\\x00-\\x1f\\x7f]+$"
    },
    "slug": {
      "type": "string",
      "pattern": "^[a-z][a-z0-9_.-]{0,63}$"
    },
    "uint": {
      "type": "string",
      "pattern": "^(0|[1-9][0-9]{0,18})$"
    },
    "atoms": {
      "type": "string",
      "pattern": "^(0|-?[1-9][0-9]{0,29})$"
    },
    "integer-string": {
      "type": "string",
      "pattern": "^(0|-?[1-9][0-9]{0,154})$"
    },
    "positive-integer-string": {
      "type": "string",
      "pattern": "^[1-9][0-9]{0,154}$"
    },
    "decimal": {
      "type": "string",
      "maxLength": 49,
      "pattern": "^(0|[1-9][0-9]*)(\\.[0-9]{0,17}[1-9])?$"
    },
    "timestamp": {
      "type": "string",
      "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{6}Z$"
    },
    "digest": {
      "type": "string",
      "pattern": "^sha256:[0-9a-f]{64}$"
    },
    "event-id": {
      "type": "string",
      "minLength": 1,
      "maxLength": 128
    },
    "claim-id": {
      "type": "string",
      "pattern": "^cl_[0-9a-f]{64}$"
    },
    "decision-id": {
      "type": "string",
      "pattern": "^dc_[0-9a-f]{64}$"
    },
    "receipt-id": {
      "type": "string",
      "pattern": "^rc_[0-9a-f]{64}$"
    },
    "effect-id": {
      "type": "string",
      "pattern": "^ef_[0-9a-f]{64}$"
    },
    "action-id": {
      "type": "string",
      "pattern": "^ac_[0-9a-f]{64}$"
    },
    "obligation-id": {
      "type": "string",
      "pattern": "^ob_[0-9a-f]{64}$"
    },
    "intention-id": {
      "type": "string",
      "pattern": "^in_[0-9a-f]{64}$"
    },
    "document-id": {
      "type": "string",
      "pattern": "^doc_[0-9a-f]{64}$"
    },
    "snapshot-ref-id": {
      "type": "string",
      "pattern": "^sr_[0-9a-f]{64}$"
    },
    "explanation-id": {
      "type": "string",
      "pattern": "^xp_[0-9a-f]{64}$"
    },
    "control-transition-id": {
      "type": "string",
      "pattern": "^ct_[0-9a-f]{64}$"
    },
    "scope": {
      "type": "array",
      "prefixItems": [
        {
          "$ref": "#/$defs/text"
        },
        {
          "$ref": "#/$defs/text"
        }
      ],
      "items": false,
      "minItems": 2,
      "maxItems": 2
    },
    "money": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "currency": {
          "type": "string",
          "pattern": "^[A-Z]{3}$"
        },
        "scale": {
          "type": "integer",
          "minimum": 0,
          "maximum": 18
        },
        "atoms": {
          "$ref": "#/$defs/atoms"
        }
      },
      "required": [
        "currency",
        "scale",
        "atoms"
      ]
    },
    "ratio": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "numerator": {
          "$ref": "#/$defs/integer-string"
        },
        "denominator": {
          "$ref": "#/$defs/positive-integer-string"
        }
      },
      "required": [
        "numerator",
        "denominator"
      ]
    },
    "roles-value": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "provider": {
          "$ref": "#/$defs/text"
        },
        "cost_originator": {
          "$ref": "#/$defs/text"
        },
        "bearer": {
          "$ref": "#/$defs/text"
        },
        "payer": {
          "$ref": "#/$defs/text"
        },
        "beneficiary": {
          "$ref": "#/$defs/text"
        },
        "recipient": {
          "$ref": "#/$defs/text"
        },
        "payer_delegation": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "provider",
        "cost_originator",
        "bearer",
        "payer",
        "beneficiary",
        "recipient"
      ]
    },
    "roles": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-roles/1"
        },
        "provider": {
          "$ref": "#/$defs/text"
        },
        "cost_originator": {
          "$ref": "#/$defs/text"
        },
        "bearer": {
          "$ref": "#/$defs/text"
        },
        "payer": {
          "$ref": "#/$defs/text"
        },
        "beneficiary": {
          "$ref": "#/$defs/text"
        },
        "recipient": {
          "$ref": "#/$defs/text"
        },
        "payer_delegation": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "schema",
        "provider",
        "cost_originator",
        "bearer",
        "payer",
        "beneficiary",
        "recipient"
      ]
    },
    "event-doc": {
      "type": "string",
      "pattern": "^doc_[0-9a-f]{64}$"
    },
    "event-eventId": {
      "type": "string",
      "pattern": "^ev_[0-9a-f]{64}$"
    },
    "event-ref": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "source",
        "id"
      ],
      "properties": {
        "source": {
          "type": "string",
          "minLength": 1,
          "maxLength": 256
        },
        "id": {
          "$ref": "#/$defs/event-id"
        }
      }
    },
    "event-link": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "relation",
        "from"
      ],
      "properties": {
        "schema": {
          "const": "ledger-link/1"
        },
        "relation": {
          "enum": [
            "generated_from",
            "optimized_from",
            "published_as",
            "attributed_to",
            "consumes_service"
          ]
        },
        "from": {
          "$ref": "#/$defs/event-ref"
        }
      }
    },
    "event": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "schema",
        "id",
        "type",
        "customer"
      ],
      "properties": {
        "schema": {
          "const": "ledger-event/1"
        },
        "id": {
          "$ref": "#/$defs/event-id"
        },
        "source": {
          "type": "string",
          "minLength": 1,
          "maxLength": 256
        },
        "operation_id": {
          "$ref": "#/$defs/event-id"
        },
        "type": {
          "enum": [
            "content.generated",
            "tool.optimized",
            "content.published",
            "outcome.acquired",
            "tool.completed",
            "link.asserted",
            "economic.reversal"
          ]
        },
        "customer": {
          "$ref": "#/$defs/event-id"
        },
        "chain": {
          "$ref": "#/$defs/event-id"
        },
        "occurred_at": {
          "type": "string",
          "format": "date-time"
        },
        "status": {
          "enum": [
            "succeeded",
            "failed"
          ]
        },
        "quantity": {
          "type": "string",
          "pattern": "^[0-9]+(\\.[0-9]{1,18})?$",
          "maxLength": 64
        },
        "unit": {
          "type": "string",
          "pattern": "^[a-z][a-z0-9_.-]{0,63}$"
        },
        "binding_id": {
          "$ref": "#/$defs/event-id"
        },
        "invocation_id": {
          "$ref": "#/$defs/event-id"
        },
        "claim_id": {
          "$ref": "#/$defs/event-id"
        },
        "links": {
          "type": "array",
          "maxItems": 32,
          "uniqueItems": true,
          "items": {
            "$ref": "#/$defs/event-link"
          }
        },
        "evidence": {
          "type": "array",
          "maxItems": 16,
          "uniqueItems": true,
          "items": {
            "$ref": "#/$defs/event-doc"
          }
        },
        "child": {
          "$ref": "#/$defs/event-ref"
        },
        "targets": {
          "type": "array",
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true,
          "items": {
            "$ref": "#/$defs/event-eventId"
          }
        },
        "reason": {
          "type": "string",
          "minLength": 1,
          "maxLength": 256
        },
        "corrects": {
          "$ref": "#/$defs/event-eventId"
        },
        "extensions": {
          "type": "object",
          "maxProperties": 16,
          "additionalProperties": {
            "type": [
              "string",
              "boolean",
              "integer"
            ]
          }
        }
      },
      "allOf": [
        {
          "if": {
            "properties": {
              "type": {
                "enum": [
                  "content.generated",
                  "tool.optimized",
                  "content.published",
                  "tool.completed"
                ]
              }
            }
          },
          "then": {
            "not": {
              "anyOf": [
                {
                  "required": [
                    "claim_id"
                  ]
                },
                {
                  "required": [
                    "child"
                  ]
                },
                {
                  "required": [
                    "targets"
                  ]
                },
                {
                  "required": [
                    "reason"
                  ]
                }
              ]
            }
          }
        },
        {
          "if": {
            "properties": {
              "type": {
                "const": "outcome.acquired"
              }
            }
          },
          "then": {
            "required": [
              "claim_id",
              "occurred_at",
              "links",
              "evidence"
            ],
            "properties": {
              "links": {
                "minItems": 1
              },
              "evidence": {
                "minItems": 1
              }
            },
            "not": {
              "anyOf": [
                {
                  "required": [
                    "status"
                  ]
                },
                {
                  "required": [
                    "quantity"
                  ]
                },
                {
                  "required": [
                    "unit"
                  ]
                },
                {
                  "required": [
                    "child"
                  ]
                },
                {
                  "required": [
                    "targets"
                  ]
                },
                {
                  "required": [
                    "reason"
                  ]
                },
                {
                  "required": [
                    "corrects"
                  ]
                }
              ]
            }
          }
        },
        {
          "if": {
            "properties": {
              "type": {
                "const": "link.asserted"
              }
            }
          },
          "then": {
            "required": [
              "child",
              "links"
            ],
            "properties": {
              "links": {
                "minItems": 1,
                "maxItems": 1
              }
            },
            "not": {
              "anyOf": [
                {
                  "required": [
                    "status"
                  ]
                },
                {
                  "required": [
                    "quantity"
                  ]
                },
                {
                  "required": [
                    "unit"
                  ]
                },
                {
                  "required": [
                    "claim_id"
                  ]
                },
                {
                  "required": [
                    "targets"
                  ]
                },
                {
                  "required": [
                    "reason"
                  ]
                },
                {
                  "required": [
                    "corrects"
                  ]
                }
              ]
            }
          }
        },
        {
          "if": {
            "properties": {
              "type": {
                "const": "economic.reversal"
              }
            }
          },
          "then": {
            "required": [
              "targets",
              "reason",
              "evidence"
            ],
            "properties": {
              "evidence": {
                "minItems": 1,
                "maxItems": 1
              }
            },
            "not": {
              "anyOf": [
                {
                  "required": [
                    "status"
                  ]
                },
                {
                  "required": [
                    "quantity"
                  ]
                },
                {
                  "required": [
                    "unit"
                  ]
                },
                {
                  "required": [
                    "claim_id"
                  ]
                },
                {
                  "required": [
                    "child"
                  ]
                },
                {
                  "required": [
                    "links"
                  ]
                },
                {
                  "required": [
                    "corrects"
                  ]
                },
                {
                  "required": [
                    "binding_id"
                  ]
                },
                {
                  "required": [
                    "invocation_id"
                  ]
                }
              ]
            }
          }
        }
      ]
    },
    "policy-slug": {
      "type": "string",
      "pattern": "^[a-z][a-z0-9_.-]{0,63}$"
    },
    "policy-decimal": {
      "type": "string",
      "pattern": "^[0-9]+(\\.[0-9]{1,18})?$",
      "maxLength": 64
    },
    "policy-predicate": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "field",
        "eq"
      ],
      "properties": {
        "field": {
          "enum": [
            "status",
            "binding.tier",
            "binding.funding",
            "binding.priority",
            "source"
          ]
        },
        "eq": {
          "type": "string",
          "maxLength": 128
        }
      }
    },
    "policy-amount": {
      "oneOf": [
        {
          "type": "object",
          "additionalProperties": false,
          "required": [
            "fixed"
          ],
          "properties": {
            "fixed": {
              "$ref": "#/$defs/policy-decimal"
            }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": [
            "unit_price",
            "unit"
          ],
          "properties": {
            "unit_price": {
              "$ref": "#/$defs/policy-decimal"
            },
            "unit": {
              "$ref": "#/$defs/policy-slug"
            }
          }
        },
        {
          "type": "object",
          "additionalProperties": false,
          "required": [
            "percent",
            "basis"
          ],
          "properties": {
            "percent": {
              "$ref": "#/$defs/policy-decimal"
            },
            "basis": {
              "type": "string",
              "minLength": 1,
              "maxLength": 128
            }
          }
        }
      ]
    },
    "policy-match": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "relation",
        "target_type",
        "hops"
      ],
      "properties": {
        "relation": {
          "enum": [
            "optimized_from",
            "published_as",
            "attributed_to",
            "consumes_service"
          ]
        },
        "target_type": {
          "enum": [
            "content.generated",
            "tool.optimized",
            "content.published",
            "tool.completed"
          ]
        },
        "hops": {
          "type": "integer",
          "minimum": 1,
          "maximum": 2
        }
      }
    },
    "policy-rule": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "id",
        "on",
        "op",
        "component",
        "book"
      ],
      "properties": {
        "id": {
          "$ref": "#/$defs/policy-slug"
        },
        "on": {
          "enum": [
            "content.generated",
            "tool.optimized",
            "content.published",
            "outcome.acquired",
            "tool.completed",
            "link.asserted"
          ]
        },
        "op": {
          "enum": [
            "base",
            "premium",
            "discount",
            "cap",
            "share",
            "observe_cost"
          ]
        },
        "component": {
          "$ref": "#/$defs/policy-slug"
        },
        "book": {
          "enum": [
            "retail",
            "supplier",
            "cost_observation"
          ]
        },
        "when": {
          "type": "array",
          "maxItems": 8,
          "items": {
            "$ref": "#/$defs/policy-predicate"
          }
        },
        "match": {
          "$ref": "#/$defs/policy-match"
        },
        "amount": {
          "$ref": "#/$defs/policy-amount"
        },
        "ceiling": {
          "$ref": "#/$defs/policy-decimal"
        },
        "stage": {
          "$ref": "#/$defs/policy-slug"
        },
        "basis": {
          "type": "string",
          "minLength": 1,
          "maxLength": 128
        },
        "discount_mode": {
          "enum": [
            "additive",
            "sequential"
          ]
        },
        "exclusive_group": {
          "$ref": "#/$defs/policy-slug"
        },
        "priority": {
          "type": "integer",
          "minimum": 0,
          "maximum": 255
        }
      },
      "allOf": [
        {
          "if": {
            "properties": {
              "op": {
                "const": "cap"
              }
            }
          },
          "then": {
            "required": [
              "ceiling",
              "stage",
              "basis"
            ],
            "not": {
              "required": [
                "amount"
              ]
            }
          },
          "else": {
            "required": [
              "amount"
            ]
          }
        },
        {
          "if": {
            "properties": {
              "op": {
                "const": "share"
              }
            }
          },
          "then": {
            "required": [
              "ceiling"
            ]
          }
        },
        {
          "if": {
            "properties": {
              "op": {
                "const": "discount"
              }
            }
          },
          "then": {
            "required": [
              "discount_mode"
            ]
          }
        }
      ]
    },
    "policy": {
      "type": "object",
      "additionalProperties": false,
      "required": [
        "schema",
        "id",
        "currency",
        "scale",
        "rules"
      ],
      "properties": {
        "schema": {
          "const": "ledger-policy/1"
        },
        "id": {
          "$ref": "#/$defs/policy-slug"
        },
        "currency": {
          "type": "string",
          "pattern": "^[A-Z]{3}$"
        },
        "scale": {
          "type": "integer",
          "minimum": 0,
          "maximum": 18
        },
        "rounding": {
          "const": "nearest_ties_away"
        },
        "rules": {
          "type": "array",
          "minItems": 1,
          "maxItems": 64,
          "items": {
            "$ref": "#/$defs/policy-rule"
          }
        }
      }
    },
    "assent": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-assent/1"
        },
        "mode": {
          "enum": [
            "demo",
            "real"
          ]
        },
        "agreement_id": {
          "$ref": "#/$defs/text"
        },
        "terms_version": {
          "$ref": "#/$defs/uint"
        },
        "acceptor": {
          "$ref": "#/$defs/text"
        },
        "bearer": {
          "$ref": "#/$defs/text"
        },
        "payer": {
          "$ref": "#/$defs/text"
        },
        "recipient": {
          "$ref": "#/$defs/text"
        },
        "accepted_at": {
          "$ref": "#/$defs/timestamp"
        },
        "evidence_ref": {
          "type": "string",
          "minLength": 1,
          "maxLength": 256
        },
        "evidence_digest": {
          "$ref": "#/$defs/digest"
        }
      },
      "required": [
        "schema",
        "mode",
        "agreement_id",
        "terms_version",
        "acceptor",
        "bearer",
        "payer",
        "recipient",
        "accepted_at",
        "evidence_ref",
        "evidence_digest"
      ]
    },
    "source-grant": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-source-grant/1"
        },
        "id": {
          "$ref": "#/$defs/text"
        },
        "principal_id": {
          "$ref": "#/$defs/text"
        },
        "source": {
          "$ref": "#/$defs/source"
        },
        "event_types": {
          "type": "array",
          "items": {
            "enum": [
              "content.generated",
              "tool.optimized",
              "content.published",
              "outcome.acquired",
              "tool.completed",
              "link.asserted",
              "economic.reversal"
            ]
          },
          "minItems": 1,
          "maxItems": 7,
          "uniqueItems": true
        },
        "relations": {
          "type": "array",
          "items": {
            "enum": [
              "generated_from",
              "optimized_from",
              "published_as",
              "attributed_to",
              "consumes_service"
            ]
          },
          "minItems": 0,
          "maxItems": 5,
          "uniqueItems": true
        },
        "permissions": {
          "type": "array",
          "items": {
            "enum": [
              "submit",
              "read",
              "preview",
              "authorize_invocation",
              "terms_admin",
              "source_admin",
              "operate"
            ]
          },
          "minItems": 1,
          "maxItems": 7,
          "uniqueItems": true
        },
        "starts_at": {
          "$ref": "#/$defs/timestamp"
        },
        "ends_at": {
          "$ref": "#/$defs/timestamp"
        }
      },
      "required": [
        "schema",
        "id",
        "principal_id",
        "source",
        "event_types",
        "relations",
        "permissions",
        "starts_at"
      ]
    },
    "context": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-context/1"
        },
        "tier": {
          "$ref": "#/$defs/slug"
        },
        "priority": {
          "type": "boolean"
        },
        "funding": {
          "enum": [
            "byok",
            "platform"
          ]
        },
        "currency": {
          "type": "string",
          "pattern": "^[A-Z]{3}$"
        },
        "scale": {
          "type": "integer",
          "minimum": 0,
          "maximum": 18
        },
        "binding_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/text"
          },
          "minItems": 1,
          "maxItems": 16,
          "uniqueItems": true
        },
        "cost_observation": {
          "$ref": "#/$defs/document-id"
        },
        "stage": {
          "$ref": "#/$defs/stage"
        }
      },
      "required": [
        "schema",
        "funding",
        "currency",
        "scale",
        "binding_ids"
      ]
    },
    "stage": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "id": {
          "$ref": "#/$defs/slug"
        },
        "closure_type": {
          "const": "outcome.acquired"
        },
        "expected": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "source": {
                "$ref": "#/$defs/source"
              },
              "operation_id": {
                "$ref": "#/$defs/text"
              },
              "type": {
                "enum": [
                  "content.generated",
                  "tool.optimized",
                  "content.published",
                  "outcome.acquired",
                  "tool.completed",
                  "link.asserted",
                  "economic.reversal"
                ]
              },
              "retail_components": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/slug"
                },
                "minItems": 1,
                "maxItems": 128,
                "uniqueItems": true
              }
            },
            "required": [
              "source",
              "operation_id",
              "type",
              "retail_components"
            ]
          },
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true
        },
        "closure_claim_namespace": {
          "$ref": "#/$defs/slug"
        }
      },
      "required": [
        "id",
        "closure_type",
        "expected",
        "closure_claim_namespace"
      ]
    },
    "outcome-terms": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "source": {
          "$ref": "#/$defs/source"
        },
        "window_us": {
          "$ref": "#/$defs/uint"
        },
        "report_grace_us": {
          "$ref": "#/$defs/uint"
        },
        "claim_namespace": {
          "$ref": "#/$defs/slug"
        },
        "predecessor_type": {
          "const": "content.published"
        }
      },
      "required": [
        "source",
        "window_us",
        "report_grace_us",
        "claim_namespace",
        "predecessor_type"
      ]
    },
    "binding": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-binding/1"
        },
        "id": {
          "$ref": "#/$defs/text"
        },
        "agreement_id": {
          "$ref": "#/$defs/text"
        },
        "version": {
          "type": "integer",
          "minimum": 1,
          "maximum": 4294967295
        },
        "policy": {
          "$ref": "#/$defs/document-id"
        },
        "roles": {
          "$ref": "#/$defs/document-id"
        },
        "assent": {
          "$ref": "#/$defs/document-id"
        },
        "context": {
          "$ref": "#/$defs/document-id"
        },
        "offer": {
          "$ref": "#/$defs/document-id"
        },
        "acceptor": {
          "$ref": "#/$defs/text"
        },
        "accepted_at": {
          "$ref": "#/$defs/timestamp"
        },
        "starts_at": {
          "$ref": "#/$defs/timestamp"
        },
        "ends_at": {
          "$ref": "#/$defs/timestamp"
        },
        "service": {
          "$ref": "#/$defs/slug"
        },
        "customer": {
          "$ref": "#/$defs/text"
        },
        "sources": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/source"
          },
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true
        },
        "event_types": {
          "type": "array",
          "items": {
            "enum": [
              "content.generated",
              "tool.optimized",
              "content.published",
              "outcome.acquired",
              "tool.completed",
              "link.asserted",
              "economic.reversal"
            ]
          },
          "minItems": 1,
          "maxItems": 7,
          "uniqueItems": true
        },
        "unit": {
          "$ref": "#/$defs/slug"
        },
        "maximum_quantity": {
          "$ref": "#/$defs/decimal"
        },
        "maximum_exposure": {
          "$ref": "#/$defs/money"
        },
        "correction_sources": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/source"
          },
          "minItems": 0,
          "maxItems": 32,
          "uniqueItems": true
        },
        "outcome": {
          "$ref": "#/$defs/outcome-terms"
        },
        "allocation_view": {
          "type": "boolean"
        }
      },
      "required": [
        "schema",
        "id",
        "agreement_id",
        "version",
        "policy",
        "roles",
        "assent",
        "context",
        "acceptor",
        "accepted_at",
        "starts_at",
        "service",
        "customer",
        "sources",
        "event_types",
        "unit",
        "maximum_quantity",
        "correction_sources",
        "allocation_view"
      ]
    },
    "input-purpose": {
      "enum": [
        "policy",
        "roles",
        "assent",
        "source_grant",
        "binding",
        "chain_context"
      ]
    },
    "association-purpose": {
      "enum": [
        "policy",
        "roles",
        "assent",
        "source_grant",
        "binding",
        "chain_context",
        "decision_snapshot"
      ]
    },
    "snapshot-document": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "purpose": {
          "$ref": "#/$defs/input-purpose"
        },
        "document_id": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "purpose",
        "document_id"
      ]
    },
    "snapshot-context": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "tier": {
          "$ref": "#/$defs/slug"
        },
        "priority": {
          "type": "boolean"
        },
        "funding": {
          "enum": [
            "byok",
            "platform"
          ]
        },
        "currency": {
          "type": "string",
          "pattern": "^[A-Z]{3}$"
        },
        "scale": {
          "type": "integer",
          "minimum": 0,
          "maximum": 18
        },
        "binding_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/text"
          },
          "minItems": 1,
          "maxItems": 16,
          "uniqueItems": true
        },
        "cost_observation": {
          "$ref": "#/$defs/document-id"
        },
        "stage": {
          "$ref": "#/$defs/stage"
        }
      },
      "required": [
        "funding",
        "currency",
        "scale",
        "binding_ids"
      ]
    },
    "authority-version": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "principal_id": {
          "$ref": "#/$defs/text"
        },
        "source": {
          "$ref": "#/$defs/source"
        },
        "grant_id": {
          "$ref": "#/$defs/text"
        },
        "grant_document": {
          "$ref": "#/$defs/document-id"
        },
        "revision": {
          "$ref": "#/$defs/uint"
        },
        "active": {
          "type": "boolean"
        }
      },
      "required": [
        "principal_id",
        "source",
        "grant_id",
        "grant_document",
        "revision",
        "active"
      ]
    },
    "prior-action": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "action_id": {
          "$ref": "#/$defs/action-id"
        },
        "content_hash": {
          "$ref": "#/$defs/digest"
        }
      },
      "required": [
        "action_id",
        "content_hash"
      ]
    },
    "snapshot": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-snapshot/1"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "dsl_version": {
          "const": 1
        },
        "semantics_version": {
          "const": 1
        },
        "documents": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/snapshot-document"
          },
          "minItems": 1,
          "maxItems": 128,
          "uniqueItems": true
        },
        "context": {
          "$ref": "#/$defs/snapshot-context"
        },
        "authority": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/authority-version"
          },
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true
        },
        "prior_actions": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/prior-action"
          },
          "minItems": 0,
          "maxItems": 128,
          "uniqueItems": true
        },
        "decision_context": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "received_at": {
              "$ref": "#/$defs/timestamp"
            }
          },
          "required": []
        }
      },
      "required": [
        "schema",
        "scope",
        "dsl_version",
        "semantics_version",
        "documents",
        "context",
        "authority",
        "prior_actions",
        "decision_context"
      ]
    },
    "snapshot-ref": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-snapshot-ref/1"
        },
        "id": {
          "$ref": "#/$defs/snapshot-ref-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "purpose": {
          "$ref": "#/$defs/association-purpose"
        },
        "document_id": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "event_id",
        "purpose",
        "document_id"
      ]
    },
    "delivery-key": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-delivery-key/1"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "source": {
          "$ref": "#/$defs/source"
        },
        "external_id": {
          "$ref": "#/$defs/text"
        },
        "canonical_event_id": {
          "$ref": "#/$defs/event-id"
        },
        "kind": {
          "enum": [
            "original",
            "alias"
          ]
        },
        "ingress": {
          "$ref": "#/$defs/event"
        },
        "ingress_hash": {
          "$ref": "#/$defs/digest"
        }
      },
      "required": [
        "schema",
        "scope",
        "source",
        "external_id",
        "canonical_event_id",
        "kind",
        "ingress",
        "ingress_hash"
      ]
    },
    "link-fact": {
      "type": "array",
      "prefixItems": [
        {
          "enum": [
            "generated_from",
            "optimized_from",
            "published_as",
            "attributed_to",
            "consumes_service"
          ]
        },
        {
          "$ref": "#/$defs/event-id"
        },
        {
          "$ref": "#/$defs/event-id"
        }
      ],
      "items": false,
      "minItems": 3,
      "maxItems": 3
    },
    "claim-facts": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-claim-facts/1"
        },
        "type": {
          "enum": [
            "content.generated",
            "tool.optimized",
            "content.published",
            "outcome.acquired",
            "tool.completed",
            "link.asserted",
            "economic.reversal"
          ]
        },
        "chain": {
          "$ref": "#/$defs/text"
        },
        "customer": {
          "$ref": "#/$defs/text"
        },
        "status": {
          "enum": [
            "succeeded",
            "failed"
          ]
        },
        "quantity": {
          "$ref": "#/$defs/decimal"
        },
        "unit": {
          "$ref": "#/$defs/slug"
        },
        "links": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/link-fact"
          },
          "minItems": 0,
          "maxItems": 32,
          "uniqueItems": true
        },
        "evidence": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/document-id"
          },
          "minItems": 0,
          "maxItems": 16,
          "uniqueItems": true
        },
        "binding_id": {
          "$ref": "#/$defs/text"
        },
        "invocation_id": {
          "$ref": "#/$defs/text"
        },
        "occurred_at": {
          "$ref": "#/$defs/timestamp"
        },
        "corrects": {
          "$ref": "#/$defs/event-id"
        }
      },
      "required": [
        "schema",
        "type",
        "chain",
        "customer",
        "status",
        "quantity",
        "unit",
        "links",
        "evidence"
      ]
    },
    "claim": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-claim/1"
        },
        "id": {
          "$ref": "#/$defs/claim-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "source": {
          "$ref": "#/$defs/source"
        },
        "operation_id": {
          "$ref": "#/$defs/text"
        },
        "kind": {
          "enum": [
            "completion",
            "acquisition",
            "link"
          ]
        },
        "token": {
          "$ref": "#/$defs/text"
        },
        "facts_hash": {
          "$ref": "#/$defs/digest"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "source",
        "operation_id",
        "kind",
        "token",
        "facts_hash",
        "event_id"
      ]
    },
    "match-key": {
      "oneOf": [
        {
          "const": "self"
        },
        {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^ln_[0-9a-f]{64}$"
          },
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true
        }
      ]
    },
    "action-kind": {
      "enum": [
        "charge",
        "cost",
        "premium",
        "discount",
        "credit",
        "share",
        "allocation",
        "reversal"
      ]
    },
    "book": {
      "enum": [
        "retail",
        "supplier",
        "cost_observation",
        "allocation"
      ]
    },
    "effect-facts": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-effect-facts/1"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "agreement_id": {
          "$ref": "#/$defs/text"
        },
        "component": {
          "$ref": "#/$defs/slug"
        },
        "claim_id": {
          "$ref": "#/$defs/claim-id"
        },
        "match_key": {
          "$ref": "#/$defs/match-key"
        },
        "namespace": {
          "const": "original"
        },
        "kind": {
          "$ref": "#/$defs/action-kind"
        },
        "book": {
          "$ref": "#/$defs/book"
        },
        "amount": {
          "$ref": "#/$defs/money"
        },
        "roles": {
          "$ref": "#/$defs/roles-value"
        },
        "sources": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/event-id"
          },
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true
        },
        "links": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^ln_[0-9a-f]{64}$"
          },
          "minItems": 0,
          "maxItems": 32,
          "uniqueItems": true
        },
        "inputs": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/action-id"
          },
          "minItems": 0,
          "maxItems": 128,
          "uniqueItems": true
        },
        "reverses": {
          "$ref": "#/$defs/action-id"
        },
        "allocation_parent": {
          "$ref": "#/$defs/action-id"
        }
      },
      "required": [
        "schema",
        "scope",
        "agreement_id",
        "component",
        "claim_id",
        "match_key",
        "namespace",
        "kind",
        "book",
        "amount",
        "roles",
        "sources",
        "links",
        "inputs"
      ]
    },
    "effect": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-effect/1"
        },
        "id": {
          "$ref": "#/$defs/effect-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "agreement_id": {
          "$ref": "#/$defs/text"
        },
        "component": {
          "$ref": "#/$defs/slug"
        },
        "claim_id": {
          "$ref": "#/$defs/claim-id"
        },
        "match_key": {
          "$ref": "#/$defs/match-key"
        },
        "namespace": {
          "const": "original"
        },
        "facts_hash": {
          "$ref": "#/$defs/digest"
        },
        "action_id": {
          "$ref": "#/$defs/action-id"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "agreement_id",
        "component",
        "claim_id",
        "match_key",
        "namespace",
        "facts_hash",
        "action_id"
      ]
    },
    "action": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-action/1"
        },
        "id": {
          "$ref": "#/$defs/action-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "decision_id": {
          "$ref": "#/$defs/decision-id"
        },
        "effect_id": {
          "$ref": "#/$defs/effect-id"
        },
        "obligation_id": {
          "$ref": "#/$defs/obligation-id"
        },
        "component": {
          "$ref": "#/$defs/slug"
        },
        "kind": {
          "$ref": "#/$defs/action-kind"
        },
        "book": {
          "$ref": "#/$defs/book"
        },
        "amount": {
          "$ref": "#/$defs/money"
        },
        "roles": {
          "$ref": "#/$defs/roles-value"
        },
        "sources": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/event-id"
          },
          "minItems": 1,
          "maxItems": 32,
          "uniqueItems": true
        },
        "links": {
          "type": "array",
          "items": {
            "type": "string",
            "pattern": "^ln_[0-9a-f]{64}$"
          },
          "minItems": 0,
          "maxItems": 32,
          "uniqueItems": true
        },
        "inputs": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/action-id"
          },
          "minItems": 0,
          "maxItems": 128,
          "uniqueItems": true
        },
        "reverses": {
          "$ref": "#/$defs/action-id"
        },
        "allocation_parent": {
          "$ref": "#/$defs/action-id"
        },
        "roles_doc": {
          "$ref": "#/$defs/document-id"
        },
        "binding_id": {
          "$ref": "#/$defs/text"
        },
        "rule_id": {
          "$ref": "#/$defs/slug"
        },
        "snapshot_doc": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "event_id",
        "decision_id",
        "effect_id",
        "obligation_id",
        "component",
        "kind",
        "book",
        "amount",
        "roles",
        "sources",
        "links",
        "inputs",
        "roles_doc",
        "binding_id",
        "rule_id",
        "snapshot_doc"
      ]
    },
    "action-source": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-action-source/1"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "action_id": {
          "$ref": "#/$defs/action-id"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        }
      },
      "required": [
        "schema",
        "scope",
        "action_id",
        "event_id"
      ]
    },
    "action-dependency": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-action-dependency/1"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "action_id": {
          "$ref": "#/$defs/action-id"
        },
        "input_action_id": {
          "$ref": "#/$defs/action-id"
        }
      },
      "required": [
        "schema",
        "scope",
        "action_id",
        "input_action_id"
      ]
    },
    "explanation-input": {
      "oneOf": [
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "decimal"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "$ref": "#/$defs/decimal"
            },
            "exact": {
              "$ref": "#/$defs/ratio"
            }
          },
          "required": [
            "kind",
            "name",
            "value",
            "exact"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "money"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "$ref": "#/$defs/money"
            }
          },
          "required": [
            "kind",
            "name",
            "value"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "boolean"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "type": "boolean"
            }
          },
          "required": [
            "kind",
            "name",
            "value"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "source_id"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "$ref": "#/$defs/source"
            }
          },
          "required": [
            "kind",
            "name",
            "value"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "binding_field"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "$ref": "#/$defs/text"
            }
          },
          "required": [
            "kind",
            "name",
            "value"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document_ref"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "$ref": "#/$defs/document-id"
            }
          },
          "required": [
            "kind",
            "name",
            "value"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action_ref"
            },
            "name": {
              "$ref": "#/$defs/text"
            },
            "value": {
              "$ref": "#/$defs/action-id"
            }
          },
          "required": [
            "kind",
            "name",
            "value"
          ]
        }
      ]
    },
    "explanation-reference": {
      "oneOf": [
        {
          "$ref": "#/$defs/document-id"
        },
        {
          "$ref": "#/$defs/action-id"
        }
      ]
    },
    "explanation": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-explanation/1"
        },
        "id": {
          "$ref": "#/$defs/explanation-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "ordinal": {
          "type": "integer",
          "minimum": 0,
          "maximum": 255
        },
        "rule_id": {
          "$ref": "#/$defs/slug"
        },
        "outcome": {
          "enum": [
            "applied",
            "skipped",
            "zero"
          ]
        },
        "code": {
          "enum": [
            "BASE_APPLIED",
            "PREMIUM_APPLIED",
            "DISCOUNT_APPLIED",
            "CAP_APPLIED",
            "CAP_NOT_BINDING",
            "SHARE_APPLIED",
            "SHARE_CEILING",
            "PREDICATE_FALSE",
            "FAILED_WORK",
            "NO_MATCH",
            "ZERO_ROUNDED",
            "COST_UNKNOWN",
            "BYOK_NO_HOST_COST",
            "EXACT_REVERSAL"
          ]
        },
        "binding_id": {
          "$ref": "#/$defs/text"
        },
        "input_refs": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/explanation-reference"
          },
          "minItems": 0,
          "maxItems": 128,
          "uniqueItems": true
        },
        "basis_name": {
          "$ref": "#/$defs/text"
        },
        "basis": {
          "$ref": "#/$defs/ratio"
        },
        "inputs": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/explanation-input"
          },
          "minItems": 0,
          "maxItems": 128
        },
        "unrounded_atoms": {
          "$ref": "#/$defs/ratio"
        },
        "rounded_atoms": {
          "$ref": "#/$defs/atoms"
        },
        "action_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/action-id"
          },
          "minItems": 0,
          "maxItems": 128,
          "uniqueItems": true
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "event_id",
        "ordinal",
        "outcome",
        "code",
        "binding_id",
        "input_refs",
        "inputs",
        "action_ids"
      ],
      "dependentRequired": {
        "basis": [
          "basis_name"
        ],
        "basis_name": [
          "basis"
        ],
        "unrounded_atoms": [
          "rounded_atoms"
        ],
        "rounded_atoms": [
          "unrounded_atoms"
        ]
      },
      "allOf": [
        {
          "if": {
            "properties": {
              "outcome": {
                "const": "applied"
              }
            }
          },
          "then": {
            "required": [
              "unrounded_atoms",
              "rounded_atoms"
            ],
            "properties": {
              "action_ids": {
                "minItems": 1
              }
            }
          }
        },
        {
          "if": {
            "properties": {
              "outcome": {
                "enum": [
                  "skipped",
                  "zero"
                ]
              }
            }
          },
          "then": {
            "properties": {
              "action_ids": {
                "maxItems": 0
              }
            }
          }
        }
      ]
    },
    "obligation-delta": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-obligation-delta/1"
        },
        "type": {
          "const": "obligation_delta"
        },
        "obligation_id": {
          "$ref": "#/$defs/obligation-id"
        },
        "agreement_id": {
          "$ref": "#/$defs/text"
        },
        "book": {
          "$ref": "#/$defs/book"
        },
        "amount": {
          "$ref": "#/$defs/money"
        },
        "roles": {
          "$ref": "#/$defs/roles-value"
        },
        "actions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "action_id": {
                "$ref": "#/$defs/action-id"
              },
              "kind": {
                "$ref": "#/$defs/action-kind"
              },
              "component": {
                "$ref": "#/$defs/slug"
              },
              "amount": {
                "$ref": "#/$defs/money"
              }
            },
            "required": [
              "action_id",
              "kind",
              "component",
              "amount"
            ]
          },
          "minItems": 1,
          "maxItems": 128,
          "uniqueItems": true
        }
      },
      "required": [
        "schema",
        "type",
        "obligation_id",
        "agreement_id",
        "book",
        "amount",
        "roles",
        "actions"
      ]
    },
    "intention": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-intention/1"
        },
        "id": {
          "$ref": "#/$defs/intention-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "destination_id": {
          "const": "fake"
        },
        "idempotency_key": {
          "$ref": "#/$defs/intention-id"
        },
        "obligation_id": {
          "$ref": "#/$defs/obligation-id"
        },
        "action_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/action-id"
          },
          "minItems": 1,
          "maxItems": 128,
          "uniqueItems": true
        },
        "amount": {
          "$ref": "#/$defs/money"
        },
        "depends_on": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/intention-id"
          },
          "minItems": 0,
          "maxItems": 32,
          "uniqueItems": true
        },
        "payload": {
          "$ref": "#/$defs/obligation-delta"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "event_id",
        "destination_id",
        "idempotency_key",
        "obligation_id",
        "action_ids",
        "amount",
        "depends_on",
        "payload"
      ]
    },
    "control-transition": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-control-transition/1"
        },
        "id": {
          "$ref": "#/$defs/control-transition-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "control_kind": {
          "const": "chain"
        },
        "control_id": {
          "$ref": "#/$defs/text"
        },
        "from_revision": {
          "$ref": "#/$defs/uint"
        },
        "to_revision": {
          "$ref": "#/$defs/uint"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "document_id": {
          "$ref": "#/$defs/document-id"
        },
        "from_event_count": {
          "$ref": "#/$defs/uint"
        },
        "to_event_count": {
          "$ref": "#/$defs/uint"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "control_kind",
        "control_id",
        "from_revision",
        "to_revision",
        "event_id",
        "document_id",
        "from_event_count",
        "to_event_count"
      ]
    },
    "chain-revision": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-chain-revision/1"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "chain_id": {
          "$ref": "#/$defs/text"
        },
        "revision": {
          "$ref": "#/$defs/uint"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "decision_id": {
          "$ref": "#/$defs/decision-id"
        }
      },
      "required": [
        "schema",
        "scope",
        "chain_id",
        "revision",
        "event_id",
        "decision_id"
      ]
    },
    "manifest-member": {
      "oneOf": [
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "snapshot-ref"
            },
            "id": {
              "$ref": "#/$defs/snapshot-ref-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "event"
            },
            "id": {
              "$ref": "#/$defs/event-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "claim"
            },
            "id": {
              "$ref": "#/$defs/claim-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "effect"
            },
            "id": {
              "$ref": "#/$defs/effect-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action"
            },
            "id": {
              "$ref": "#/$defs/action-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "explanation"
            },
            "id": {
              "$ref": "#/$defs/explanation-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "intention"
            },
            "id": {
              "$ref": "#/$defs/intention-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "control-transition"
            },
            "id": {
              "$ref": "#/$defs/control-transition-id"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action-source"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/action-id"
                },
                {
                  "$ref": "#/$defs/event-id"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action-dependency"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/action-id"
                },
                {
                  "$ref": "#/$defs/action-id"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "delivery-key"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/source"
                },
                {
                  "$ref": "#/$defs/text"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "chain-revision"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/text"
                },
                {
                  "$ref": "#/$defs/uint"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "id",
            "content_hash"
          ]
        }
      ]
    },
    "decision-manifest": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-decision-manifest/1"
        },
        "id": {
          "$ref": "#/$defs/decision-id"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "chain_id": {
          "$ref": "#/$defs/text"
        },
        "revision": {
          "$ref": "#/$defs/uint"
        },
        "explanation_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/explanation-id"
          },
          "minItems": 1,
          "maxItems": 256,
          "uniqueItems": true
        },
        "members": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/manifest-member"
          },
          "minItems": 1,
          "maxItems": 4096,
          "uniqueItems": true
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "event_id",
        "chain_id",
        "revision",
        "explanation_ids",
        "members"
      ]
    },
    "receipt": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-receipt/1"
        },
        "id": {
          "$ref": "#/$defs/receipt-id"
        },
        "event_id": {
          "$ref": "#/$defs/event-id"
        },
        "decision_id": {
          "$ref": "#/$defs/decision-id"
        },
        "chain_id": {
          "$ref": "#/$defs/text"
        },
        "revision": {
          "$ref": "#/$defs/uint"
        },
        "content_hash": {
          "$ref": "#/$defs/digest"
        },
        "decision_hash": {
          "$ref": "#/$defs/digest"
        },
        "action_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/action-id"
          },
          "minItems": 0,
          "maxItems": 128,
          "uniqueItems": true
        },
        "intention_ids": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/intention-id"
          },
          "minItems": 0,
          "maxItems": 32,
          "uniqueItems": true
        }
      },
      "required": [
        "schema",
        "id",
        "event_id",
        "decision_id",
        "chain_id",
        "revision",
        "content_hash",
        "decision_hash",
        "action_ids",
        "intention_ids"
      ]
    },
    "party": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-party/1"
        },
        "id": {
          "$ref": "#/$defs/text"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "role_metadata_doc": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "role_metadata_doc"
      ]
    },
    "source-grant-record": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-source-grant-record/1"
        },
        "id": {
          "$ref": "#/$defs/text"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "principal_id": {
          "$ref": "#/$defs/text"
        },
        "source": {
          "$ref": "#/$defs/source"
        },
        "grant_doc": {
          "$ref": "#/$defs/document-id"
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "principal_id",
        "source",
        "grant_doc"
      ]
    },
    "binding-record": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "schema": {
          "const": "ledger-binding-record/1"
        },
        "id": {
          "$ref": "#/$defs/text"
        },
        "scope": {
          "$ref": "#/$defs/scope"
        },
        "agreement_id": {
          "$ref": "#/$defs/text"
        },
        "version": {
          "type": "integer",
          "minimum": 1,
          "maximum": 4294967295
        },
        "policy_doc": {
          "$ref": "#/$defs/document-id"
        },
        "roles_doc": {
          "$ref": "#/$defs/document-id"
        },
        "assent_doc": {
          "$ref": "#/$defs/document-id"
        },
        "context_doc": {
          "$ref": "#/$defs/document-id"
        },
        "currency": {
          "type": "string",
          "pattern": "^[A-Z]{3}$"
        },
        "scale": {
          "type": "integer",
          "minimum": 0,
          "maximum": 18
        }
      },
      "required": [
        "schema",
        "id",
        "scope",
        "agreement_id",
        "version",
        "policy_doc",
        "roles_doc",
        "assent_doc",
        "context_doc",
        "currency",
        "scale"
      ]
    },
    "record": {
      "oneOf": [
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "policy"
            },
            "body": {
              "$ref": "#/$defs/policy"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "roles"
            },
            "body": {
              "$ref": "#/$defs/roles"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "assent"
            },
            "body": {
              "$ref": "#/$defs/assent"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "source-grant"
            },
            "body": {
              "$ref": "#/$defs/source-grant"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "context"
            },
            "body": {
              "$ref": "#/$defs/context"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "binding"
            },
            "body": {
              "$ref": "#/$defs/binding"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "document"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/document-id"
            },
            "document_type": {
              "const": "snapshot"
            },
            "body": {
              "$ref": "#/$defs/snapshot"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "document_type",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "snapshot-ref"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/snapshot-ref-id"
            },
            "body": {
              "$ref": "#/$defs/snapshot-ref"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "event"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/event-id"
            },
            "body": {
              "$ref": "#/$defs/event"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "claim"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/claim-id"
            },
            "body": {
              "$ref": "#/$defs/claim"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "effect"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/effect-id"
            },
            "body": {
              "$ref": "#/$defs/effect"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/action-id"
            },
            "body": {
              "$ref": "#/$defs/action"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "explanation"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/explanation-id"
            },
            "body": {
              "$ref": "#/$defs/explanation"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "intention"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/intention-id"
            },
            "body": {
              "$ref": "#/$defs/intention"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "control-transition"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/control-transition-id"
            },
            "body": {
              "$ref": "#/$defs/control-transition"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action-source"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/action-id"
                },
                {
                  "$ref": "#/$defs/event-id"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "body": {
              "$ref": "#/$defs/action-source"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "action-dependency"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/action-id"
                },
                {
                  "$ref": "#/$defs/action-id"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "body": {
              "$ref": "#/$defs/action-dependency"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "delivery-key"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/source"
                },
                {
                  "$ref": "#/$defs/text"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "body": {
              "$ref": "#/$defs/delivery-key"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "chain-revision"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "type": "array",
              "prefixItems": [
                {
                  "$ref": "#/$defs/scope"
                },
                {
                  "$ref": "#/$defs/text"
                },
                {
                  "$ref": "#/$defs/uint"
                }
              ],
              "items": false,
              "minItems": 3,
              "maxItems": 3
            },
            "body": {
              "$ref": "#/$defs/chain-revision"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "decision-manifest"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/decision-id"
            },
            "body": {
              "$ref": "#/$defs/decision-manifest"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "receipt"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/receipt-id"
            },
            "body": {
              "$ref": "#/$defs/receipt"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "party"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/text"
            },
            "body": {
              "$ref": "#/$defs/party"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "source-grant-record"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/text"
            },
            "body": {
              "$ref": "#/$defs/source-grant-record"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "kind": {
              "const": "binding-record"
            },
            "scope": {
              "$ref": "#/$defs/scope"
            },
            "id": {
              "$ref": "#/$defs/text"
            },
            "body": {
              "$ref": "#/$defs/binding-record"
            },
            "content_hash": {
              "$ref": "#/$defs/digest"
            }
          },
          "required": [
            "kind",
            "scope",
            "id",
            "body",
            "content_hash"
          ]
        }
      ]
    }
  }
}
```
