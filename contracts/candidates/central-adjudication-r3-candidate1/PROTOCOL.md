# Central adjudication R3 candidate 1

This is a fresh additive canonical candidate, derived from R3 SHA256
341043f03c7879c0d1ef02b76e054e5fd7ab287d745f48d434eb59162d148db6 and the independent oracle
manifest c634758eec89247aecd52a4fa1b152b71480914af6403fea81e5156150607f5f.
It changes no inherited profile. It has no freeze or independent PASS yet.
No candidate4 code, schema or fixture is reused.

## Encoding and validation interface

`protocol/schema.json` is the closed Draft 2020-12 structural schema. Every object
rejects unknown fields, including prototype-like names. Required fields remain
present. Optional fields are omitted, never null. Schema tuples preserve order;
sets with `uniqueItems` must additionally be in ascending canonical-byte order.
The ordered gateway list is the one exception: enrollment preserves supplied
order, rejects duplicates and routing uses that order. Family order also remains
supplied, with prerequisite indices referring to that immutable list.

Parse UTF-8 strictly, reject BOM, duplicate keys, lone surrogates, nonfinite,
fractional or exponential numeric tokens, negative numeric zero and integers
outside the safe integral JSON interval. Integers of economic or persistent
significance use normalized decimal strings. New counters and absolute atoms
are at most M=999999999999999999999999999999; inherited records retain their
original bound, notably 9223372036854775807 for frozen v2 counters. No widening.
IDs have no C0/DEL controls. ID strings retain exact bytes; no normalization.
Sources allow 256 UTF-8 bytes; other identity components 128. Dates are Gregorian
UTC, year 0001..9999, exactly six fractional digits and no leap second. Evidence
body is canonical padded standard Base64, whose decoded byte length is <=4096;
its sha256 is over decoded bytes. There are at most16 distinct evidence bodies
per case, cumulatively after supplements. No evidence URL retrieval.

Canonical JSON uses UTF-16 code-unit object-key ordering, minimal string escapes
(standard short escapes for backspace/tab/LF/formfeed/CR; other C0 as lowercase
\\u00xx), unescaped non-ASCII UTF-8, no whitespace or terminal LF. Arrays retain
order. Each canonical operation command is <=262144 bytes, each introduced trust
set <=2097152 bytes and each complete local segment <=8388608 bytes. These are
independent checked limits. Old history/dependencies are referenced and fetched
with explicit reader budget, never recopied into an unbounded catalog.

`H(domain,value) = lowercase SHA256(UTF8("ledgerlab/central-r3/" + domain + "/1")
|| 00 || canonical(value))`. Closed domains: command, submission, grant, claim,
receipt, action, closure, segment, replay, route, namespace, authority, evidence,
result, enrollment. Command digest covers `[kind,key,payload]`, excluding the
observation to avoid recursion. Authority.command must equal it. Submission digest
covers the full submission; aliases use the same submission, never the delivery.
Grant authentication hashes the grant with authentication omitted, but the hash
alone authenticates nothing: the exact grant must also be a member of the
externally trusted authoritative grant/journal observations supplied by the host.
Claim hash covers `[grant_id,token_id,gateway,allocation,category]`. Result root is
H(replay,[previous_root,command_digest,effects]); segment identity is H(segment,
complete segment). Initial previous/root digest is 64 zeroes. Refused, duplicate,
unknown and read results never append a segment or a fresh authority observation.
No request record or diagnostic may borrow a mandatory allocation.

Python CLI: `python3 validate.py FILE` emits one JSON result to stdout, exits0 for
an internally valid input and exits1 with `{accepted:false,error:CODE}` otherwise.
Node CLI: `node validate.mjs FILE`, identical contract. No subprocess cross-calls.
`--self-test` is an additive local test mode. Public independent functions:
`canonical(value)`, `digest(domain,value)`, `validate_shape(type_name,value)`,
`replay(trace)`; CLI fixture file envelope is `{"format":"r3-trace/1","initial":
{...},"commands":[...]}`. `initial` has externally trusted `authority_documents`
(array of raw digest strings), `grant_authentications` (array of raw digests),
`original_base_receipt`, `original_base_manifest`, `initial_resources` (one full `resource` vector per host), `optional_rounds` (count), `initial_counters`
(map host→map counter→{q,R}, omitted names mean0,0), and `writer_capabilities` (map
logical gateway→{epoch,journal_head,fence}, supplied by trusted actual adapter).
These are synthetic observations in contract fixtures, never a claim that caller
JSON can grant authority in production. Commands are structural command objects.
The semantic test runner may inject `BEFORE_COMMIT`/`AFTER_COMMIT` cuts *outside*
canonical commands and must resolve the same command identity; no cut flag is
commercial input. Fixtures' expected summaries are checked by the runner only,
never used by replay to construct results.

## Identity, namespace, authority

Full scope is `[tenant,environment]`; family `[scope,agreement,family,target]`;
case `[family,event_source,business_id]`; delivery/control `[scope,source,id]`.
The oracle's single synthetic scope label expands to `[label,"synthetic"]`;
this refinement does not collapse any semantic key. Hashes index full tuples;
uniqueness always compares complete bytes. One enrolled final target and one
original accepted base anchor commit atomically. No enrollment of an existing
accepted target through a separate later command. No added family at rollover.

Namespace `(scope,tag,gateway)` owns all sources for external IDs with prefix
`gw1.`+32 lowercase hexadecimal tag+`.` (37 bytes), followed by1..91 UTF-8 bytes.
Legacy occupancy across the *entire* range must be absent under the shared scope
lock before enrollment. Legacy and new writers enforce the permanent exclusion.
Gateway owner index is unsigned H(route,full_case_tuple) modulo ordered gateway
count1..4. Epoch replacement does not change the list, tag, case route or IDs.
At delivery-key authority: check occupied full key before case route or semantic
alias. Changed bytes conflict; exact bytes return saved immutable receipt under
current read authority. For a free key, route+namespace must agree. Exact case
submission alias requires a distinct unused token but returns the original
receipt/time/position. Same case changed submission conflicts. No central or
legacy side channel creates a competing designated receipt for an enrolled case.

Authority observation binds exact command digest, current head, principal,
permission, document/revision and genuine observation time. Exact stored retry is
looked up before business preconditions but still requires current read authority.
A fresh command needs current appropriate authority: enroll/capacity/submit/
decide/adjust/correct/close/replace. A sender backfill bit supplies none. Trusted
host checks current authority document and revision under ordered locks, including
historical terms/assent and independent directional adjustment consent. Synthetic
fixtures pin document membership explicitly. Hash equality does not prove assent.

## Durable transition contract

All changes listed for one command are atomic at its own authoritative journal.
Local and central commits are separate. Unknown outcome holds backing, IDs and
staging until same-identity authoritative resolution. Exact retry is read-only.

| Kind | Guard and exact durable result | One-time allocation owner |
|---|---|---|
| ENROLL | Original base receipt/manifest externally pinned; one target; bounded acyclic family graph; <=4 distinct gateways, <=8 supplier pools, <=3 adjustment pools; supplier maximum=consumed+held+released; establish immutable namespace, topology, economic anchor and32 successful-close bundles | enrollment optional costs plus protected close/work |
| LOCAL_GRANT | Actual exclusive writer, correct center/registration/owner/template/namespace and sufficient full local resource/counter envelope; retain unique LOCAL_HELD grant before export | new local grant |
| REGISTER_GRANT | Exact authenticated LOCAL_HELD proof, current unique map, all binding fields; retain REGISTERED_UNCLAIMED and complete administration/retirement funding | central grant |
| ISSUE | Grant registered, not claimed/retired; one exact token claim and contiguous allocation together; ordinary issuance blocked during active round; ADJUSTMENT issuance may continue before local seal, with new independent backing | central token; existing local grant |
| ACTIVATE | Verify exact current claim; current epoch and OPEN gateway; convert held local backing without acquiring capacity; terminal grant/token cannot revive | original local grant |
| RECEIVE | Current exclusive epoch/OPEN/clock floor; delivery occupancy precedes routing+case; token owned/active/unused; new case commits exact submission+evidence+one receipt; alias commits only mapping to existing original receipt | local token |
| RETURN_UNUSED | Authenticated unique claim, held or active token unused; direct disposal requires no activation; permanent grant+token tombstone, delayed activation prohibited | local token |
| IMPORT | Authentic consumed exact NEW_CASE or ALIAS fact; alias original must already be imported; retain admission/mapping/evidence; new case adopts original receipt; before-close pending or after-close adjustment determined at selected prefix | central token |
| RECONCILE | Exact terminal disposition plus any consumed import present; permanently reconcile token, return only proven central slack; ID and actual pages remain charged | central token |
| ADVANCE | Through is exactly next allocation; terminal and imported-if-consumed; monotonic allocation cursor. Separately advance contiguous imported original receipt prefix. Original token may import before earlier alias allocation | central token |
| RETIRE_GRANT | Same lock as ISSUE; only registered unclaimed; permanent RETIRED_UNCLAIMED, claim forever barred | central grant |
| LOCAL_TERMINAL | Exact authoritative central retirement/reconciliation proof; no timeout/absence release; retain local permanent terminal; release only unused slack, not used pages/IDs | original local grant |
| BEGIN | One new round, previous terminal installed at every required gateway; requested nonempty newly closed family set; pin mode, finite allocation cutoffs and predecessor; FINISH_ONLY consumes designated protected family bundle; CANCELLABLE buys whole independent bundle | protected or optional round |
| SEAL_BEGIN | Exact active round/predecessor, current writer; serialize with receipt commit, switch OPEN→SEALING; no new local intake | round local seal slot |
| SEALED | All issued tokens through cutoff terminal locally, including undelivered; actual receipt high-water/root includes receipts from above-cutoff adjustment tokens consumed before SEAL_BEGIN; store exact seal | round local sealed slot |
| DRAIN | Exact seal, central allocation cursor>=cutoff and every receipt through actual high-water imported; bind allocation disposition and receipt roots/counts. An allocation count is never a receipt count | round central drain slot |
| READY | Every fenced gateway drained, exact selected cutoffs and actual high-waters; revalidation failure appends nothing | round |
| CLOSE | READY, current authority and all family/prerequisite/entitlement/pool heads; atomic bounded set certificate, complete last-family supplier transitions (including zero held), COMMITTED CAS | round |
| ABORT | CANCELLABLE and DRAINING/READY only; ABORTED CAS. FINISH_ONLY refusal leaves finish path intact. Token progress stays permanent | optional round |
| INSTALL | Exact terminal round/outcome/predecessor; install even if gateway never saw BEGIN, retaining tombstone; stale controls cannot touch R+1; after COMMITTED record actual close-time floor, never clamp clock | round local installation slot |
| ACK_INSTALL | Exact known local terminal installation proof observed at center; mark this gateway acknowledged, allowing next round only after all required acknowledgments | round central acknowledgment slot |
| SUPPLEMENT | Existing pending case; current submit authority, cumulative16 evidence maximum; no changed submission or replacement receipt | optional command |
| DECIDE | Pending imported case; explicit ALLOW/DENY, authority; ordinary requires open eligible family, accepted prerequisites, original terms/windows/supplier hold; adjustment requires unavailable ordinary path, separate assent/roles/pool/caps; DENY finalizes case only | optional decision |
| CORRECT | Existing ALLOW current revision, separate correction authority/window, permitted original replacement code; inverse exact previous signed amount + replacement, no new entitlement | optional correction |
| REPLACE_WRITER | Actual old-writer exclusion and full authoritative journal/grant/token/round recovery proven; strictly greater epoch under same storage boundary; stale or copied journal not a capability | optional handover |
| EXTEND_RESOURCES | Authenticated additive physical backing, own operation costs and counter headroom; never extends numeric M | optional extension |

The single total central lock order extends the existing global order at the
integration seam: admission/identity/namespace scope → capacity/grant/allocation →
gateway/round → family/prerequisite → case → entitlement → supplier → adjustment
pool, preserving preceding existing source/binding authority locks. Within each
class sort complete scoped canonical keys. Discover extra locks: rollback/restart.
Local journal fencing serializes all local commands; no distributed lock is claimed.

## Economics and closure projection

Pending cases are not enumerated in closure. At prefix P a case admitted pending
before its first affecting certificate has immutable transfer `(case,certificate)`.
Later ALLOW/DENY retains that lineage. Pre-close final cases do not transfer. A
later admission to unavailable family starts adjustment and names actual receipt
chronology; it is not backdated into the prior pending set. Projection scans bounded
pages. Certificate size depends only on32 families/4 gateways/8 pools, never cases.
Prerequisite propagation follows original graph through unaccepted prerequisites,
stopping at already accepted facts. Explicit closure differs from unavailability.
Only the last *explicit* supplier-family close releases all remaining held capacity.
No later repair command. No transfer uses adjustment funds or promises an award.

One permanent full-family entitlement is consumed by every first ALLOW including
negative and zero; DENY consumes only its case. Zero has no action/intention.
Independent adjustment magnitude=abs(signed_atoms), funding and gross both charge
that magnitude; positive/negative caps are separate. Roles are directional and
match independent assent; bearer!=payer requires retained delegation. +100,-150
consume250; net-50 never replenishes gross/funding/supplier capacity. Ordinary
supplier consumption similarly remains consumed after correction. Correction
increments accepted revision and stores exact inverse/replacement; it does not
reopen a family or change the entitlement consumer. Original receipt is immutable.

Original retail-net basis is fixed at base acceptance. Supported percentage
calculation is basis*numerator/(denominator*100), reduced bounded exact integer
arithmetic, nearest ties away once. Corrections invert stored rounded atoms.
Customer fixture final10000+1200+500+100-150-500+300=11450; supported resolution-only
alternative1500 yields11750. Actual adjustments/correction/decisions/supplier3000
stay fixed.6000 resolution fails first at its ALLOW against5000 premium cap; no
comparable partial total. Unsupported substitutions are explicit.

## Prefix, coverage and actual proof boundary

Trusted ExpectedPrefix comes from authenticated primary snapshot or external
head pin, never the submitted chain. Exact terminal ordinal,segment,root,store,
scope,target,enrollment must match; removed suffix fails. Without expected head
return VERIFIED_SUPPLIED_PREFIX only. Historical selection is labeled historical.
Incomplete/canceled/budget-exhausted read carries cursor and no complete total.
Whole-registration ingress coverage requires all registered gateways and exact
finite allocation/receipt cutoffs imported at P. Unfenced unknown coverage stays
UNKNOWN even if central replay is complete. No wall-clock-now completeness claim.
Offline retry returns identical receipt plus cached-prefix/current UNKNOWN or all
UNKNOWN; no invented central admission or current ordinary state.

Canonical trace models do not certify actual storage or authority. SQLite/PG17/
PG18 unknown commit, cancellation/reopen, physical pages/WAL/staging/reader pins,
actual two-process exclusion, stale backups, and observed nonposting statements/
effects all remain mandatory independent runtime gates. A capability field in a
fixture is a declared trusted input, never real writer-fencing evidence.

## Interface refinement A (supersedes abbreviated trace fields above)

Each segment explicitly names its owning `host`. Central host is ENROLL.store;
local hosts are logical gateway IDs. Local kinds are LOCAL_GRANT, ACTIVATE,
RECEIVE, RETURN_UNUSED, LOCAL_TERMINAL, SEAL_BEGIN, SEALED, INSTALL and
REPLACE_WRITER. All other kinds commit centrally. Each host has independent
ordinal/previous-segment/replay-root. Authority.head binds that host's prior root.
A trace is only an interleaving schedule of these separate journals. Its aggregate
summary root H(replay,sorted[[host,root]...]) is a test manifest root, not a central
head. No global command position establishes cross-journal authority.

IMPORT/RECONCILE now carry closed source fact proof fields: store/scope/
registration/host, ordinal/segment/root, exact fact kind/full key/body hash/bytes,
and trusted observation reference. Membership is checked against the exact source
segment result/inventory through the externally authenticated named historical
prefix. Later journal activity does not invalidate that proof. Caller-selected
roots and arbitrary equal body hashes are insufficient. Historical reconstruction
starts at the source capability/enrollment anchor or a locally verified cursor.
The cross-journal reference graph must be acyclic: local grant→central claim→local
receipt/disposal→central import/reconciliation. Reject unresolved/forward/cyclic
references. Original allocation2 can import before dependent alias allocation1.

Each segment has `objects`, a bounded typed exact-object inventory. Body is Base64
of strict canonical bytes; bytes and raw SHA256 bind decoded bytes and full key.
Kinds are closed. Previously retained objects are hash dependencies, not recopied.
Imported receipt includes exact submission/evidence and original receipt bodies.
An alias proof identifies the exact mapping plus immutable original receipt/token.
Unused proof identifies the permanent grant-token tombstone. Source segments are
referenced; they are never recursively embedded. Both decoded trust total and
encoded complete segment total are checked before commit. A reader charges source
chain traversal, every fetched object and proof page to its explicit budget.

ENROLL must retain complete original-profile base acceptance companions and the
exact original manifest/receipt in the *same central atomic operation*. Initial
base hashes are expected inputs, not evidence of atomic acceptance. Initial
`original_objects` supplies their exact immutable typed inventory, <=128 objects
and <=1048576 decoded bytes, validated against the inherited frozen profile. No
separately preaccepted target may be enrolled. Runtime must construct the original
validated acceptance plan and enrollment together; absence of this real seam fails
actual enrollment acceptance even if a synthetic canonical fixture is consistent.

A family has an explicit RETAIL or SUPPLIER book. Ordinary amount belongs to that
book: supplier outcomes consume supplier hold, never customer amount. Retail
families use supplier_pool="none". Actions preserve book and exact signed amount.
Adjustment pools contain up to3 independent direction/roles/assent authorizations;
one shared pool can therefore fund +100 and−150 with different consenting roles.
A present payer_delegation is a retained authenticated document hash. Absent is
allowed only when payer==bearer. No fabricated null delegation. UNKNOWN lifecycle
without a verified head omits prefix; it must not invent a zero-filled head.

Resource vectors are per-host, six-dimensional. `initial_resources[host]` gives
provisioned capacity, `initial_counters[host][name]={q,R}` gives prior obligations.
`protocol/resources.json` derives complete retained slot/bundle costs. Reserve all
retained dimensions additively per owner; workspace is the maximum slot workspace
within an owner, held additively across owners. This deliberately conservative
profile gives each live owner its own bounded recoverable staging slot. Only one
operation at a time uses an owner's slot; unknown outcome keeps it held. Local
grant.resources/counters must cover its entire local_grant bundle before export.
Enrollment pre-funds32 finish_central bundles and32 finish_gateway bundles per
gateway; optional cancellable rounds allocate separate complete bundles. Resource
release requires exact terminal proof. Used immutable bytes/pages never return to
free. ACK_INSTALL is a separate central commit after local INSTALL.

`counter_increments` in the worksheet is a per-slot maximum credit vector, not
permission to increment unrelated semantic counters. Actual fields increment only
for their owning transition. RECEIVE creates one receipt position for NEW_CASE,
zero for ALIAS; IMPORT adds one original receipt-import fact or zero for ALIAS;
DECIDE only ALLOW increments accepted economic revision. Head/segment/resource
revision increments occur once per committed transition. An optional operation
cannot consume reserved future headroom. Retrying existing identity changes no
counter, segment, receipt, grant, allocation, epoch or staging identity.

## Interface refinement B: exact source objects and bounded receipt advance

REGISTER_GRANT, ACTIVATE, RETURN_UNUSED, LOCAL_TERMINAL, SEAL_BEGIN, DRAIN,
INSTALL and ACK_INSTALL also carry the same typed source proof. Expected sources
are respectively GRANT, CLAIM, CLAIM, RETIREMENT/RECONCILIATION, BEGIN, SEAL,
TERMINAL and INSTALLATION. Full key and source gateway/center must match the
receiving command. `initial.trusted_observations` pins each exact proof observation:
H(authority,proof with trusted_observation_ref omitted). The pin is supplied by the
trusted host, not derived by accepting submitted bytes. Also independently verify
exact source segment/hash/root/body membership against its already reconstructed
historical journal; an authenticated wrong-body reference still rejects.

Source fact body is canonical `{payload:source_command.payload,effects:source_result.effects}`.
The inventory stores its raw SHA256, length, canonical Base64 and full key. Local
GRANT key=grant.id; central CLAIM key=token.id; RECEIPT/ALIAS/RETURNED_UNUSED/
RECONCILIATION key=token ID; RETIREMENT key=grant ID; BEGIN/SEAL/TERMINAL/
INSTALLATION key=decimal round string. A proof of a receipt uses the exact source
RECEIVE fact, whose payload and effect jointly retain submission/evidence and
original receipt. Local aliases have no new receipt position and retain original
receipt/token in their effect.

RECONCILE inventory contains the copied local disposition fact plus its own new
RECONCILIATION fact. IMPORT contains only the copied local fact. ENROLL contains
exact initial.original_objects only. Other source-producing transitions contain
one source fact; other transitions contain none. Inventory sorts by canonical
object bytes. Any command with proof has dependencies=[proof.segment]; otherwise
empty. All proof paths are immutable historical references, never current-root
substitution. A source segment includes its inventory in its segment digest.

ADVANCE_RECEIPT is a separate central bounded operation `{gateway,through}`.
Through must be exactly the next receipt position and its original token imported.
That original central token pre-funds this slot at ISSUE. IMPORT does not advance
the receipt cursor. ADVANCE_RECEIPT spends one receipt_prefix counter credit;
ADVANCE spends one allocation_prefix credit. A NEW_CASE token returns central
slack only after both required advances; alias/unused tokens need only allocation
advance and then release their unused receipt-advance slot. This avoids unbounded
cursor work or losing prepaid headroom when imports arrive out of order.

The worksheet's actual counter vector equals its reservation maximum except
RECEIVE ALIAS consumes0 receipt increments and DECIDE DENY consumes0 economic
revision increments; those unused per-slot credits discharge as proven branch
slack. No other counter is touched by these exceptions. index_cardinality counts
permanent retained index-version entries (including updates), not live map size.
All separate visible cursors remain independently range-checked. Every increment
is checked before mutation and every exact retry consumes0 in every dimension.

## Interface refinement C: retained cutoffs and exact intake retry

RECEIVE.key equals RECEIVE.payload.delivery. Its stable command digest is
H(command,[kind,key,payload.submission]); assigned gateway/token/epoch/time stay
in the original immutable segment, but are not resupplied as retry identity.
All other command digests remain H(command,[kind,key,payload]).

BEGIN has one ROUND_BEGIN effect with body `{round,predecessor,mode,cutoffs}`;
cutoffs is the canonical sorted set of `{gateway,cutoff}`. SEALED has one SEAL
effect with `{round,gateway,cutoff,receipt_high,disposition_root,receipt_root}`.
The disposition root is H(result,[[allocation,token_id,disposition],...]) in
numeric allocation order for this gateway through the BEGIN cutoff. Disposition
is exactly NEW_CASE, ALIAS or RETURNED_UNUSED; any unresolved token refuses seal.
The receipt root is H(receipt,[[position,full_receipt],...]) in numeric position
order for every actual receipt at this gateway through the actual high-water,
including above-cutoff receipts. These bounded source facts retain the roots;
actual stores reconstruct the streams with bounded paged work. DRAIN retains its
source proof.trusted_observation_ref; CLOSE coverage.observation uses that exact
reference, and copies its sealed cutoffs/roots/high-water plus central prefixes.

An ALIAS source fact retains a RECEIPT effect containing the original full receipt
and original token; its payload retains the alias token. RETURNED_UNUSED retains
its payload token and exact CLAIM proof, which resolves to the unique grant/token.
Neither needs an invented posting or receipt position.

Introduced trusted bytes for a local segment equal canonical command length plus
canonical result length plus decoded object body bytes, counting a repeated exact
body_hash once. Object framing and Base64 expansion count toward segment bytes.
Proof metadata counts inside the command. Referenced source segments remain old
bounded read dependencies, not newly trusted copies. The worksheet reserves a
conservative envelope over this exact measured quantity.

The fresh synthetic trace starts every writer capability journal_head at ZERO.
Each owning journal evolves independently. Nonzero preexisting initial anchors
require a complete retained prefix and are not accepted by this fresh trace form.
`minimal-trace.json` carries six unchanged source documents plus the25 original
accepted rows; `minimal-segment.json` is its exact newly reconstructed envelope.
The original80 fixture is a compatibility control, not the customer10000 story.

## Interface refinement D: authority, actual counters and bounded reads

Initial trace fields are exactly those previously listed plus original_objects,
trusted_observations and authority_observations. Each is required. All digest sets
are unique and sorted canonical bytes. authority_observations pins
H(authority,exact command.authority), including current principal/permission/time,
head and revision; membership of a generic document is insufficient. The trusted
host, not a caller, supplies these observations. Initial counter q values may
represent other prior host activity; R must be0 because no unnamed obligation is
allowed. Every initial resource host and writer capability object is closed.
Fresh writer capability head is ZERO; epoch is positive. Gateway and center host
identities differ. LOCAL_GRANT.journal_head is the exact current local root.
Source proofs must use the owning source host, not a later imported copy.

Permissions: ENROLL=enroll; RECEIVE/SUPPLEMENT=submit; DECIDE=adjust for adjustment
and decide otherwise; CORRECT=correct; BEGIN/CLOSE/ABORT=close;
REPLACE_WRITER=replace; every other listed durable command=capacity. Receipt time
is the genuine authority observation time, at least occurred_at; CLOSE.closed_at
is its authority observation time. No clamping. Family/pool assent documents and
required payer delegations must be retained trusted authority members.

The worksheet's index_updates and index_cardinality increment are **reservations**.
Actual immutable derived index versions use this named inventory. Every command
has four common versions: occupied command identity, journal ordinal, replay head,
resource head. Count each following branch entry in addition, without padding:

| Kind | Additional actual index versions beyond four |
|---|---|
| ENROLL | each original object, each family, each gateway, each namespace, each supplier, each adjustment pool,32 close-owner allocations per center/gateway, one enrollment allocation |
| LOCAL_GRANT / REGISTER_GRANT | grant head, funding owner |
| ISSUE | claim, grant head, allocation position, funding owner |
| ACTIVATE | local grant and token state |
| RECEIVE NEW_CASE | case, receipt position, token disposition |
| RECEIVE ALIAS | token disposition; occupied command index already owns alias delivery→original receipt |
| RETURN_UNUSED | token disposition and grant tombstone |
| IMPORT NEW_CASE / ALIAS | admission-case / no case, token import, import position |
| RECONCILE | disposition reconciliation |
| ADVANCE / ADVANCE_RECEIPT | gateway's corresponding prefix |
| LOCAL_TERMINAL / RETIRE_GRANT | grant terminal |
| BEGIN FINISH_ONLY / CANCELLABLE | round / round plus center and each gateway optional allocation |
| SEAL_BEGIN / SEALED / DRAIN / READY / ABORT / INSTALL / ACK_INSTALL | corresponding round/gateway fact head |
| CLOSE | round, certificate, each explicitly closed family, each supplier transition |
| SUPPLEMENT | case evidence and optional funding owner |
| DECIDE DENY | case and optional funding owner |
| DECIDE ALLOW | case, entitlement, funding pool, economic revision, optional owner, plus obligation and action only when nonzero |
| CORRECT | case revision, economic revision, optional owner, each nonzero inverse/replacement |
| REPLACE_WRITER / EXTEND_RESOURCES | epoch/target-resource head and optional owner |

Only these actual versions advance q.index_cardinality; reserved but unused index
headroom is discharged with the consumed slot. A consumed logical capacity slot
remains charged at its conservative envelope: this is reserved retained capacity,
not a measurement of backend used pages. Actual physical u and reclaimable slack
remain store-proof obligations. Actual q exceptions also include alias receipt0
and DENY economic_revision0. ISSUE releases its registered grant's unused central
retirement slot because CLAIMED and RETIRED_UNCLAIMED are permanently exclusive.

Closed read shapes are expected_prefix, read_budget, read_cursor, read_request,
read_response and comparison_* in schema.json. A read cursor belongs to a bounded
reader session at one authenticated ExpectedPrefix and cannot be imported from a
caller or another session. It records next host ordinal, byte offset and verified
root. Each page is at most4096 bytes; budgets independently cap bytes/pages/complete
segments. INCOMPLETE carries that exact cursor and measured work, no successful
total and no authoritative write. Cancellation drops only external reader state;
retrying a read is not a durable command. Old prefixes remain immutable.

SEALED disposition/receipt construction is a read-only fold over its fixed,
writer-fenced authoritative source prefix. It may yield between pages in the
reserved gateway round workspace; partial hash state is session staging, never a
complete root. Repeated traversal does not append progress records. Its cumulative
work includes every issued cutoff token and every actual receipt through H; the
local grant's one-time reservation funds the corresponding terminal object reads.
Only complete verification yields the bounded SEAL fact. Canonical model scans
reconstruct that fold; adapters must demonstrate paged work and staging enforcement.

`reads.py` supplies canonical read/cursor and comparison vectors over a verified
model snapshot. This is not an implementation of a database reader or evidence of
physical bounded memory/nonposting. Comparison permits only replacement of the
resolution amount, retains actual decisions and adjustment/correction history, and
returns POLICY_FAILURE with no partial total if the original premium cap is hit.
Changing any other policy/decision field is structurally unsupported. Complete
central verification does not assert unknown gateway coverage is complete.

UNKNOWN_GATEWAY_COVERAGE has exactly gateway/status; UNRECONCILED has
only gateway/status/observation. Counts/roots are present only in complete
coverage, never fabricated as zero for UNKNOWN. Comparison requests carry exactly
one coverage entry per enrolled gateway, and comparable output preserves it.
Full replica/read-only snapshots are test artifacts, not bounded journal records.
The replay output includes snapshot with resources/counters/allocations, permanent
grant/token states, cases/transfer lineage, entitlements/pools/suppliers and exact
actions/certificates. Decimal quantities stay strings; terminal owner allocations
remain present with empty slots and zero holds. State maps may contain null for a
grant without a token or case without a transfer; this is diagnostic state, not
an optional canonical-record field.
