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
`authority_observations`, `authority_sources`, `trusted_observations`, `original_objects`,
`original_base_receipt`, `original_base_manifest`, `initial_resources` (one full `resource` vector per host), `initial_counters`
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
| BEGIN | One new round, previous terminal installed at every required gateway; requested nonempty newly closed family set; pin mode, finite allocation cutoffs, global predecessor and each gateway acknowledged predecessor; FINISH_ONLY consumes designated protected family bundle; CANCELLABLE buys whole independent bundle | protected or optional round |
| SEAL_BEGIN | Exact retained BEGIN and local gateway predecessor, current writer; serialize with receipt commit, switch OPEN→SEALING; no new local intake | round local seal slot |
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
Kinds are closed. Previously retained exact typed identities remain bound dependencies, not recopied; a shared body hash alone never establishes that identity.
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
provisioned capacity, `initial_counters[host][name]={q,R}` gives the closed fresh-genesis counter state (q0/R0 except writer_epoch1).
`protocol/resources.json` derives complete retained slot/bundle costs. Reserve all
retained dimensions additively per owner; workspace is the maximum slot workspace
within an owner, held additively across owners. This deliberately conservative
profile gives each live owner its own bounded recoverable staging slot. Only one
operation at a time uses an owner's slot; unknown outcome keeps it held. Local
grant.resources/counters must cover its entire local_grant bundle before export.
Each gateway PREPARE_ENROLL pre-funds32 local finish_gateway bundles; central
ENROLL pre-funds32 finish_central bundles. Cancellable rounds similarly prepare
local optional bundles before central BEGIN. Resource
release requires exact terminal proof. Used immutable bytes/pages never return to
free. ACK_INSTALL is a separate central commit after local INSTALL.

`counter_increments` in the worksheet is a per-slot maximum credit vector, not
permission to increment unrelated semantic counters. Actual fields increment only
for their owning transition. RECEIVE creates one receipt position for NEW_CASE,
zero for ALIAS; IMPORT increments the token-import counter for either branch,
while an alias adds no new original receipt position;
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

Every command with proof considers retaining its exact source fact. ENROLL also
considers every original object and preparation fact; BEGIN considers preparation
facts. Each producer appends its own fact. The typed first-introduction rule below
then filters this inventory and canonical object bytes order it. Dependencies are
the sorted source segment IDs from proof and preparations, or an empty set. All proof paths are immutable historical references, never current-root
substitution. A source segment includes its inventory in its segment digest.

ADVANCE_RECEIPT is a separate central bounded operation `{gateway,through}`.
Through must be exactly the next receipt position and its original token imported.
That original central token pre-funds this slot at ISSUE. IMPORT does not advance
the receipt cursor. ADVANCE_RECEIPT spends one receipt_prefix counter credit;
ADVANCE spends one allocation_prefix credit. A NEW_CASE token returns central
slack only after both required advances; alias/unused tokens need only allocation
advance and then release their unused receipt-advance slot. This avoids unbounded
cursor work or losing prepaid headroom when imports arrive out of order.

Actual counters use the owning transition's applicable increments, with named
actual index-version counts instead of padded maxima. RECEIVE ALIAS consumes0
receipt increments and DECIDE DENY consumes0 economic revision increments;
unused per-slot credits discharge as proven branch slack. index_cardinality counts
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
host, not a caller, supplies these observations. Every initial R is0 and every
initial q is0 except gateway writer_epoch, which is explicitly1 and matches the
trusted capability epoch1. No invented prior activity is accepted at fresh ZERO
journals. Every initial resource host and capability object is closed.
Fresh writer capability head is ZERO; replacement advances epoch by exactly1. Gateway and center host
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
| ENROLL | each newly introduced typed object, each family, each gateway, each namespace, each supplier, each adjustment pool,32 central close-owner allocations, one enrollment allocation |
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
active local round owner retains a reusable bounded scan workspace through INSTALL.
No per-token retained scan slot or one-time read credit is asserted.
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


## Final host-local preparation and provenance contract

PREPARE_ENROLL is a gateway commit with payload store/scope/registration/gateway,
namespace, and intent=H(enrollment,ENROLL payload omitting preparations). It pays
its administrative slot and reserves32 local protected finish bundles named
close:i:gateway. ENROLL requires exactly one current authenticated preparation
per enrolled gateway, preserves those local resources, and atomically accepts the
exact original base companions, immutable terms, central enrollment and32 central
finish bundles. An unknown/refused ENROLL leaves preparations held. This slice
has no timeout or orphan-preparation retirement. There is no distributed commit.

PREPARE_ROUND applies only to CANCELLABLE rounds. Its payload is round,
predecessor,gateway,mode=CANCELLABLE,enrollment=H(enrollment,full ENROLL payload),
and an exact ENROLLMENT proof. It pays its local admin slot and the complete
optional-round:n:gateway bundle. BEGIN consumes at most4 such exact preparation
proofs and reserves only optional-round:n at center. FINISH_ONLY requires no new
local preparation and preparations=[]. There is no artificial optional-round
lifetime count; actual remaining resource/counter capacity governs admission.
A preparation left unclaimed by a refused BEGIN remains safely held.

LOCAL_GRANT carries an exact central ENROLLMENT proof. The complete immutable
terms are introduced once at that gateway through this authenticated historical
source. EXTEND_RESOURCES belongs to the journal named by payload.host and changes
only that host. ISSUE uses central grant/round facts and does not read a remote
OPEN flag. Ordinary issuance freezes at BEGIN; an ADJUSTMENT claim after local
seal can still take its prepaid RETURN_UNUSED branch. SEAL_BEGIN uses its retained
BEGIN fact and local installed predecessor; later unseen central ABORT is not a
local oracle. Every remote semantic dependency requires its exact source proof.

Each inventory object has origin={store,scope,registration,host,ordinal}. This
origin names the producer's next journal ordinal and avoids a self-referential
segment hash. A membership proof still binds the exact producer segment/root.
Copied bodies preserve origin; their later receiving segment cannot become a
replacement source. Full typed identity is canonical([origin,kind,full_key,
body_hash,bytes]). Each receiving host retains a typed identity once. Known bytes
with another key or origin are distinct and charged; hashing never substitutes
for source membership. Objects are proposed from payload.proof, preparations,
original companions and the command's new source fact, then existing exact typed
identities are omitted and the remainder sorted by canonical object bytes.

New producers: PREPARE_ENROLL→ENROLL_PREPARATION keyed by gateway;
PREPARE_ROUND→ROUND_PREPARATION keyed by H(namespace,[gateway,round]);
ENROLL→ENROLLMENT keyed by registration. The source body is still exactly
{payload,effects}. Original objects have the atomic center ENROLL's origin.
Per-operation introduced trust counts canonical command+result plus distinct
new decoded body hashes, while every typed identity/envelope counts in segment
and index reservations even when bytes can share. Remote source reads remain
bounded dependency work. Snapshot exports preparations, round_preparations and
object_inventory maps in addition to previous state; inventory values are sorted
full typed identity strings. Snapshot certificates are raw certificate bodies.

CLOSE explicitly binds predecessor=current central root before close,
enrollment=H(enrollment,full ENROLL payload), and canonical-sorted family_heads
for every original family. Each head binds full family, terms=H(enrollment,terms),
pre-close closed/unavailable booleans, and entitlement UNCONSUMED or CONSUMED with
consumer case/revision/head=H(result,{case,state,revision,signed,receipt:
H(receipt,full receipt)}). This bounded original topology determines closure
identity; pending-case cardinality does not change certificate size.

Every new typed object creates one additional object-identity index version.
ENROLL's named count already includes its objects. PREPARE_ENROLL has40 base
versions: four common versions, local preparation, gateway state, local namespace,
its admin owner and32 protected owners. PREPARE_ROUND has7: four common versions,
preparation, admin owner and optional round owner. Other base versions remain in
the table above; object introduction adds its actual count, not a padded maximum.

## Logical index wire codec and page layout

All index keys use the closed binary codec in index-vectors.json: eight ASCII
bytes of uppercase index-kind padded by underscores (unique tags), one-byte
arity, then uint16 big-endian byte length and exact raw UTF-8 per component.
Component types and positions are fixed by the tag and key_components table;
full nested case identities flatten in that specified order. No JSON framing or
escaping is used for index keys. Quote/backslash bytes remain literal bytes.
K=max(9+sum(2+component_bound))=1115, L=8K+1=8921. Path bits are key bits followed
by one terminator bit and zero padding to L. Immutable replacement reserves L+1
nodes. Exact full keys remain in terminal values; no digest-only collision bucket.

A128-byte logical node is tag1,flags1,depth2,reserved4,left hash32,right hash32,
value hash32,reserved24. The4096-byte logical value page has a64-byte header
(type1,flags1,length2,next hash32,ordinal8,reserved20) and4032 payload bytes.
Values span ceil(max_value_bytes/4032) pages. These closed logical layouts are
capacity envelopes, not claims about SQLite/PostgreSQL page packing or WAL.
Actual adapters must implement/bound these or a no-larger representation.
SEALED's active gateway round workspace owns a reusable32768-byte scan lane:
two4096-byte pages, bounded8192-byte entry staging and8192-byte cursor/hash/control
state with8192 spare bytes. Its full reserved peak is larger. The fixed source prefix, stream position,
byte offset and incremental hash state belong to an external reader session.
Partial traversal can yield, abort and restart without authoritative progress or
success roots; only both completed folds permit SEALED. Repeated read work does
not consume an invented monotonic counter. Physical workspace enforcement remains
an actual-store obligation.

Grant IDs use `gr1.<32hex namespace tag>.<suffix>` with1..91 raw UTF-8 suffix
bytes and128 total bytes. Enrolled namespaces have unique tags and immutable
owners, so equal suffixes generated independently remain separate full IDs.
LOCAL_GRANT validates the codec before registry lookup. RECEIVE validates its
delivery namespace at the arrival gateway before occupied-key retry lookup; an
exact retry at another gateway refuses and does not reveal that gateway's receipt.
Dormant protected close owners remain held after other owners close their families;
this conservative slice releases only the active local owner at INSTALL and
active central owner after final ACK_INSTALL, with no cross-host slack mutation.

Saved control outcomes and their optional owner keys are scoped by canonical
[owning_host,full_control_key]. Same textual control keys on distinct journals
are independent. The control/resource-owner binary index key flattens host,
scope.tenant,scope.environment,source,external ID (128/128/128/256/128 bytes).
The object index key flattens origin.store,scope.tenant,scope.environment,
registration,host,ordinal,kind,full_key ID,body_hash,bytes (128/128/128/128/128/
30/32/128/64/30). Producer fact keys in this slice are IDs; arbitrary full-case
source keys are structurally representable but cannot pass producer membership.
Grant index keys include store,scope.tenant,scope.environment,registration,
gateway and full owner-namespaced grant ID. None exceeds the revision K.
SEALED derives terminal coverage only from locally retained CLAIM and local
disposition facts. It requires their numeric allocations to be exactly1..cutoff.
An undelivered central ISSUE leaves a local hole; the numeric cutoff suffices to
refuse without discovering an unobserved token identity.

Reader is explicitly a projection over a previously semantically verified cursor.
`reconstruct_stored` performs fresh command/authority/source/economic replay and
compares every provided full segment, then checks external ExpectedPrefixes. It
rejects changed effects/results/dependencies and removed/extra suffixes. Reader
does not replace that semantic reconstruction with a shape/hash scan. Each commit
materializes immutable canonical bytes and per-host ordinal lookup as part of
its bounded segment work. Reader looks up only the current ordinal, streams at
most4096 bytes per charged page into a constant hash state, and checks the pinned
terminal identity only after all requested bytes verify. It never constructs a
host-history list or reserializes the segment per page. One Reader owns at most
one live continuation; progress replaces it, cancellation/completion releases it,
and stale/foreign cursors cannot skip work. Model immutable byte caches are
preverified source storage, not charged active reader workspace. Runtime must
provide the same bounded indexed/page source and demonstrate physical behavior.

A completed bounded read labels selection CURRENT_AT_READ only when its verified
ExpectedPrefix still equals that journal head at completion; otherwise it labels
HISTORICAL_PREFIX. Scope is explicitly CENTRAL_PREFIX or GATEWAY_PREFIX. Every
complete read includes one UNKNOWN_GATEWAY_COVERAGE entry per enrolled gateway;
this deliberately conservative projection never implies complete durable ingress.
A comparison can instead carry exact retained certificate coverage, which must
match the committed fields, not merely reuse a trusted observation digest.

## Final read and subset-round refinements

ExpectedPrefix explicitly includes logical store, scope, target, profile,
enrollment digest, registration, host, ordinal, segment and root. Every binding is
checked; registration never substitutes for target/profile/enrollment. The immutable
enrollment digest is computed at ENROLL, so prefix validation does not reserialize
terms before its budget starts.

Routing uses the immutable ordered ENROLL.gateways array. Preparation arrival order
cannot influence ownership. The permuted-preparation vector covers every owner.

The center retains an acknowledged-round cursor per gateway, changed only by its
exact ACK_INSTALL. Each BEGIN ROUND_BEGIN.cutoffs item binds gateway_predecessor
from that cursor, independently of the global predecessor. PREPARE_ROUND binds the
requested global round and precise locally installed predecessor; gaps are allowed
for skipped gateways. BEGIN checks those preparations against central acknowledged
cursors. SEAL_BEGIN and INSTALL compare the bound gateway predecessor with local
installed state. INSTALL carries both `proof` (TERMINAL) and `begin` (BEGIN); both
exact source facts/dependencies are funded even if the gateway never saw BEGIN.
The terminal outcome cannot smuggle a current global round into local knowledge.
ACK_INSTALL has one additional actual/index-reserved version for its central
per-gateway acknowledged cursor. Alternating subset rounds remain possible and
unselected gateways are never silently reported as covered.

SealReader now walks immutable local disposition/allocation and receipt/position
indexes maintained by the corresponding successful local commits. Constructor
work is constant: it captures prefix, cutoff, actual H and funded owner. Every
entry is fetched by its exact next numeric position during charged traversal;
a missing position is a hole, never an omitted token. No whole-cutoff list or sort
is constructed. One bounded entry (at most8192 bytes), two page buffers and hash/
cursor state fit the32768-byte logical lane. The SEALED transition uses this same
fold, rather than a separate unbounded history hash helper. Partial source reads
cannot produce a terminal root. Derived index rollback is covered along with all
authoritative state; actual physical index persistence remains a runtime proof.

ComparisonReader folds ACTION effects and requested coverage as each charged
segment finishes verification. These fields are bounded by the same segment bytes;
there is no second uncharged sweep over action/certificate history. Its state is
premium/delta, first failure, at most4 coverage flags and one verified read cursor.
INCOMPLETE includes exact cursor and measured work with no successful total; resume
must keep ExpectedPrefix, policy and coverage unchanged in that reader session.
Completion/cancellation releases the cursor. Wrong or merely hash-known coverage
cannot match a retained exact closure observation. The ordinary comparison helper
is one-shot; resumable callers use ComparisonReader explicitly.

Each successful command checks all six host held totals against the sum of exact
owner allocations and every counter R against the remaining named slots, with
q+R<=M. Refusal and exception restore authoritative maps, immutable-read-index
membership, local scan-index membership and diagnostic peak accounting. Exact
retry appends no segment, index, resource debit or counter increment.

Seal fold page/cursor rule: phases are DISPOSITIONS then RECEIPTS, with numeric
entries1..cutoff and1..H respectively. Entry0 is the single byte `[`, entry i is
its canonical array value prefixed by `,` exactly when i>1, and entry N+1 is `]`.
Those canonical entry fragments are materialized in the corresponding local
immutable index during the already funded disposition/receipt commit. Scanning
looks up a fragment by position and never reserializes it per continuation.
Each read step consumes min(4032, remaining fragment bytes, remaining byte budget)
bytes and one page credit; fragments do not share that logical page credit.
A partially read fragment retains byte_offset. On fragment completion, entry
increments and offset becomes0. Completing `]` advances phase and resets entry0;
completing receipt `]` yields COMPLETE. Opening/closing brackets each cost one byte
and one page. A zero byte/page budget makes no progress. Only complete phases
expose roots. Cursor binds exact ExpectedPrefix/round/phase/entry/byte_offset and
is usable only within its live private session; abort drops it. Hash state uses
the existing domain+NUL prefix followed by these exact canonical array bytes.
The three-unused sample therefore costs102 bytes and7 logical pages when read
without a smaller budget; repeated7-byte/two-page calls have explicit measured
partial work and the same final roots.

A terminal logical value leaf stores the full binary key plus72 bytes of fixed
reference framing: version1,flags1,key-length2,record-length4,record-hash32 and
source-segment32. The bounded canonical record/object is already retained in its
charged segment inventory; value references never substitute for typed membership.
K+72=1151 fits one4032-byte payload page. The worksheet conservatively reserves
ceil((maximum command+result+introduced bodies)/4032) pages per changed value,
which is at least this full-key/reference minimum for every transition. It does
not require copying the entire source archive or using unbounded collision buckets.

## Scoped correction: exact authority sources (four blockers)

The existing trusted-host boundary is unchanged. `initial.authority_documents`
remains the externally approved set of raw SHA256 document hashes;
`authority_observations` still pins the exact current command authority decision.
Both are necessary. Add `initial.authority_sources`, a canonical-byte-sorted array
of closed `authority_source` records `{body,body_hash,bytes}`. `body` is canonical
Base64 of exact canonical JSON conforming to `authority_source_body`; decoded
bytes must equal its canonical re-encoding, length must equal `bytes`, and raw
SHA256 must equal `body_hash`. Each source is at most16384 decoded bytes. Source
hashes must equal the approved document set exactly. Missing/extra/duplicate
bodies reject. Full identity is `[source,id,revision]`, with source and id each
at most128 UTF-8 bytes (distinct from event source256). One such identity has one
immutable body: a second body at that identity rejects even if both digests were
approved. No signatures, identity provider, or production authentication is added.

Closed body variants share `source,id,revision,scope,target`:

* AUTHORIZATION adds `principal,permissions,starts_at,ends_at`. Exact principal,
  permission and revision must match `command.authority`; scope matches the full
  command key and target matches enrollment (ENROLL uses its payload). Observation
  time lies in `[starts_at,ends_at)`. PREPARE_ENROLL has no accepted enrollment;
  its document target is additionally checked against the eventual ENROLL target
  when that preparation proof is consumed. The exact current host observation
  remains mandatory even when a valid retained source exists.
* ASSENT adds `roles,terms`. Roles match exactly. `terms=H(authority,X)` where X
  is a complete family_terms object with only its self-referencing `assent` removed,
  or `{id,funding,positive,negative,gross,direction,roles}` for the exact pool and
  directional authorization. Thus signed direction, funding caps, original terms,
  optional delegation reference, scope and target are bound; no self-hash cycle.
* DELEGATION adds `roles` without payer_delegation, canonical unique
  `agreement_ids` (at most32), `maximum_exposure`, `starts_at,ends_at,acceptor`, and
  embedded `assent:{accepted_at,terms}`. `terms=H(authority,complete delegation
  body without assent)` and acceptor equals payer. Exact roles, target/scope and
  applicable agreement IDs must match. Acceptance precedes use; validity is
  half-open. At ENROLL the family correction window must also finish before the
  delegation expiry. Each family reserves maximum absolute original/correction
  result; each pool direction reserves min(funding,gross,directional cap), zero
  reserves0. Sum those exposure bounds for each shared delegation and require it
  not exceed maximum_exposure. Every actual economic use rechecks validity and
  its absolute amount. Bearer!=payer requires a delegation; a present delegation
  is checked even when bearer==payer. The embedded assent is retained evidence,
  with authenticity supplied by the existing host-approved source digest.

Reference collection: every command resolves its AUTHORIZATION body. ENROLL also
resolves all family and pool ASSENT bodies and their present DELEGATION bodies;
DECIDE/CORRECT resolve their exact ASSENT and present DELEGATION. Other commands
resolve one source. ENROLL resolves at most83 distinct sources with aggregate
source decoded bytes<=524288; DECIDE/CORRECT at most3; other commands at most1.
Bounds are checked before committing any resulting promises, with atomic rollback
on refusal/exception. Exact retries still verify the current command source and
host decision, but introduce no records, dependencies, counters or resources.

First reference in an owning journal introduces one object of kind AUTHORITY,
full_key=[source,id,revision], original source body/hash/length and the same origin
format as other newly produced objects (own store/scope/registration/host and next
ordinal). Retain it once in that host. Subsequent uses bind the immutable first
introduction's segment hash in segment.dependencies; dependencies are unique and
canonical-byte sorted, including existing protocol dependencies. Never substitute
a same-hash object at another identity or host. The derived snapshot adds
`authority_retained:{host:{canonical_full_identity:{hash,bytes,segment}}}`. This is
an index over retained source objects, not another copy of their bodies. Every
stored replay must reproduce exact introductions and dependencies; initial source
availability cannot excuse an omitted persisted object.

Each source introduction consumes its own object/record/index version and decoded
trusted bytes. The worksheet reserves maximum source resolution reads even when
already retained: ENROLL512KiB, economic commands49152bytes, other16384bytes.
ENROLL permits up to216 total objects. Exact per-object Base64 and full-key
metadata are included in segment maxima; command/effect bytes remain separately
charged. All original2MiB-trust/8MiB-segment/256KiB-command limits remain unchanged.
The `authdoc` index tag is `AUTHDOC_`; components are origin store, scope tenant,
scope environment, registration, host, ordinal, kind, source, id, revision,
body_hash, bytes, with bounds in resources.json. Its full binary key bound1115
raises K to1115 and L to8921; each changed path has8922 pages. Existing revision
keys remain1079 bytes. This is the same existing binary codec with a newly bounded
source identity; no index/backend redesign.
