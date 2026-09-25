# Admission charge amendment — admission/1-candidate.1

**Candidate / review pending / not frozen / synthetic only.** This is an explicit
proposal, not a reinterpretation of `content.generated`, a released billing
profile, or a claim of conformance. Existing frozen contract files, manifests,
records and identities are unchanged. Nothing here authorizes real charges.

## Bounded change and ownership

Add a separately versioned, opt-in admission event and outcome acceptance for
one synthetic customer, one immutable agreement and one logical host execution
slot. Up to eight distinctly authorized orders can run sequentially. An
installation has a separate candidate marker and database. Its records cannot
be submitted to `ledger billing`, imported into that profile, or mistaken for
frozen v1/v2 records. All three existing production crates remain: the pure core
validates/evaluates, one facade coordinator replays and commits, the SQLite
adapter only reads/appends bytes, and the CLI only handles input/output.

The feature `zen-charge-candidate` is disabled by default. No new dependencies,
production database migrations, linked-work chains, payment collection, Agency
integration, HTTP service, real provider invocation, or supplier-cost accounting
are included. The fixture worker is a real child process but is not OpenCode.
The purpose is to exercise the commercial boundary before attaching Zen.

## Accepted authorization and immutable binding

Only initialization by the local private-directory operator installs accepted
synthetic authorizations. An event cannot add an authorization. This models a
trusted host's accepted purchase registry; it does not authenticate a remote
customer or prove real assent. Anyone controlling that OS account/database is
inside the trust boundary. `synthetic` must be true and customer must be exactly
`synthetic-customer`.

Setup fields are closed and required: `schema`, `synthetic`, `customer`,
`agreement`, `terms_version`, `admission_atoms`, `outcome_atoms`,
`authorized_at_us`, `admit_before_us`, `outcome_before_us`, `acceptance_rule`,
`authorizations`. Each authorization has exactly `authorization_id`, `order_id`,
`deliverable_version`. Both IDs are unique across this installation. The
deliverable version and acceptance rule are `five-products-exact/v1`.

The complete setup, including the ordered authorization list, is immutable and
hash-bound as `terms_hash = H("terms", setup)`. Adding an order requires a fresh
candidate installation with its own explicitly accepted authorization list; it
does not mutate this installation. This pilot does not promise deduplication
across cloned/new installations or recover an erased host authorization history.

Every command repeats a binding with exactly: `order_id`, `authorization_id`,
`customer`, `agreement`, `terms_hash`, `admission_atoms`, `outcome_atoms`,
`deliverable_version`, `target`. The target is host-derived at initialization:
`H("target", [terms_hash, authorization_id, order_id])`. All fields must match
the saved authorization. An unknown order refuses; changing customer, agreement,
terms hash, amount, authorization, target or version refuses, including on retry.
No fuzzy prompt/content matching or caller-generated replacement purchase exists.

The permanent semantic effect keys are `(order_id, admit)` and `(order_id,
outcome)` within the installation's immutable terms/authorization namespace.
Evidence fields `delivery_id`, `attempt_id`, `session_id`, `model`, `outcome_id`
never create authority, identity, price or eligibility. Mutating any or all of
them returns the same committed receipt. No `operation_id` economic override,
caller-supplied success flag, cost, principal, price selection, or dynamic rule
is accepted. These subordinate labels need not be globally unique; they cannot
name or select a purchase. Duplicate attempts are not appended to economic
history and this journal is not an exhaustive execution/audit log.

## Lifecycle and monetary effects

`authorized → reserved → admitted → completed | failed`

`reserved → failed` is also allowed and releases the slot without a charge.
Completed/failed orders cannot restart; a new purchase needs another accepted
authorization. A new attempt for the same order is not a new purchase.

| Command | Preconditions | Durable result | USD atoms |
|---|---|---|---:|
| reserve | Authorized order; free host slot; admission window | Order owns sole slot | 0 |
| admit | Previously committed reservation for that order | Admission receipt references reservation | A |
| outcome | Admitted order; exact verified artifact; outcome window | Accepted outcome references admission; slot released | B |
| fail | Reserved or admitted order | Terminal failure; slot released | 0 |

Reservation and admission are separate process calls and separate commits.
Admission cannot book A in the same uncommitted transaction that first reserves
the slot. Work is launched only after the admission receipt. The logical slot
serializes this pilot's participating host jobs; it is not a RAM/CPU reservation,
Zen quota reservation, or completion guarantee. B is not earned by admission,
provider response, timeout, model assertions, valid JSON alone, or artifact
generation without submission to the authorized acceptance rule.

A and B are positive canonical integer atom strings, each at most 10,000; money
is USD scale 2. Fixture A=1 and B=499. Refusal before admission yields 0; failure
after admission retains 1; accepted outcome yields 1+499=500. Two separately
authorized accepted purchases yield 1,000. The core selects the fixed amounts;
the adapter/CLI never computes authoritative prices or balances. No corrections,
refunds, taxes, payment execution, or provider cost are implemented in this
candidate. Provider cost must be retained separately by a future executor.

## Deliverable and authorized acceptance

Input: `Products: Elm, Cedar, Amber, Delta, Birch. Return their names as a sorted JSON array.`

The only accepted artifact is the exact array
`["Amber","Birch","Cedar","Delta","Elm"]`. No duplicates, extras, changed
case, reversed order, prose or omissions qualify. Setup delegates outcome
acceptance to this deterministic exact-content rule. The host's pure core
verifies the returned artifact against its own immutable specification, outside
the worker process. A caller cannot assert that the verifier passed. This is
preauthorized automatic acceptance, not proof of a later human approval. A
future human-acceptance mode requires its own trusted approval receipt/contract.

Successful verification retains the artifact hash and acceptance rule in the
outcome receipt; the exact artifact is retained in the journal command. A
changed artifact under the same order refuses even after completion. Correct
artifact replay returns the original outcome receipt.

## Time and retry semantics

Times are canonical nonnegative integer strings representing UTC Unix
microseconds, no later than `i64::MAX`; the candidate does not reuse the frozen
billing timestamp DTO. Setup requires
`authorized_at_us < admit_before_us < outcome_before_us`.

For new reserve/admit decisions, `authorized_at_us <= host_decision_time <
admit_before_us`. For a new outcome, the previously committed admission time
must be no later than host decision time, which is `< outcome_before_us`.
There is no backdated synthetic work timestamp and no rolling deadline.
`accepted_at_us` is the host's decision observation under the writer lock,
retained before commit, not a claim about exact disk-commit time or model
generation time. Every new decision must be no earlier than the latest saved
decision or setup authorization; a backward host clock refuses new work.
Failure/release is allowed after a deadline, subject to that clock floor.

Existing semantic receipt lookup precedes new-decision time eligibility and
phase checks, after immutable binding/artifact validation. Thus an ordinary
retry after failure/completion or after a deadline retrieves an existing
reserve/admit receipt, without reacquiring capacity or booking money. Replaying
`fail` retrieves the existing failure receipt. Replaying outcome never accepts
a different artifact. No automatic lease expiry or recovery relaunch is implied.

## Candidate bytes, receipts and storage

Inputs use the existing strict integer-only JSON parser: reject duplicate keys,
nulls, unknown fields, invalid UTF-8, unsafe/fractional JSON numbers, and inputs
over 16 KiB. Command fields: `schema`, `operation`, `binding`, `evidence`, plus
`artifact` only for outcome. Labels are 1–96 ASCII bytes from letters, digits,
`._/-:`. All schema discriminators here equal `admission/1-candidate.1`.

Use existing canonical JSON serialization (UTF-16 key ordering, no terminal
newline), with a NEW hash domain; no frozen hash profile changes:

`H(domain, value) = "sha256:" + hex(SHA256(UTF8("ledgerlab/" + domain + "/admission/1-candidate.1") || NUL || canonical(value)))`.

Receipt body has `schema`, full `binding`, `kind`, `currency`, `scale`, `atoms`,
`accepted_at_us`. Admission additionally has `reservation_receipt`; outcome
additionally has `admission_receipt`, `artifact_hash`, `acceptance_rule`.
`artifact_hash = H("artifact", artifact)` and receipt is
`{id:H("receipt",body),body}`. Subordinate evidence is retained in the first
command but excluded from the economic receipt. Response is
`{candidate:true,status:"accepted"|"duplicate",receipt}`. Missing optional
input/receipt fields are omitted, never null. The noncanonical statement's
`slot_owner` is null when idle.

Candidate SQLite uses the existing OS owner-lock mechanism and a separate
private directory with an exact candidate marker. Writes use BEGIN IMMEDIATE,
WAL and synchronous FULL. One immutable setup row and up to 32 sequential
command/time/response rows are retained; SQL triggers reject update/deletion.
The coordinator rebuilds state from setup and every command, compares exact
canonical response bytes, and refuses inconsistent history before any action.
Economic amount/receipt and lifecycle transition share one journal commit.
Statements replay all retained entries and project the pure core's exact totals.
There is no production migration, remote authorization, signed external root,
rollback detection, or defense against an administrator coherently rewriting
the entire store. SQLite/OS/filesystem durability assumptions still apply.

Commit failure returns `OUTCOME_UNKNOWN` (exit 8), never assumed rollback. Reopen
the same installation and submit the same bound semantic operation. Owner
contention/storage failure returns 7; malformed/invalid input 3; binding
conflict 4; invented order 6; replay integrity failure 9. CLI file/usage errors
remain 2. Incomplete initialization is visible and not silently reinitialized.

## Review evidence and unresolved decisions

`setup.json`, `bindings.json`, `outcome.json`, and `expected.json` are newly
authored candidate fixtures, not frozen or independently verified. `inventory.json`
is an author-generated byte inventory, not a reviewed freeze manifest. Expected
receipt fixtures use fixed observation times for independent byte review;
the separate E2E-hooks build injects those exact times and compares entire
implementation receipts and saved canonical response bytes against `expected.json`.
Other scenarios use the real host clock and independently check receipt hashes.
The expected fixture is not regenerated or rewritten by the E2E runner.

Private time/crash injection is gated by **`zen-charge-e2e-hooks`**, a separate
build feature that implies the candidate but is never implied by it. The default
binary and ordinary `zen-charge-candidate` build compile out both environment
hooks. The CLI has no caller-supplied billing-time argument or command field.
Only the private build recognizes `LEDGER_ZEN_E2E_TIME_US` (canonical UTC Unix
microseconds) and `LEDGER_ZEN_CANDIDATE_CRASH`. The dedicated gate builds three
separate binary artifacts and exercises their runtime boundaries. Never ship
the private hooks artifact. This adds no production clock authority.

Authored process cases include cross-order reservation and outcome/failure
races, concurrently renamed evidence retries, exact lower/inclusive and
upper/exclusive time boundaries, backward-clock rejection, original receipt
lookup after expiry/backward time, malformed/duplicate-key/null input, duplicate
authorizations, price limits, and corrupted replay. Races use a common process
start barrier and reconcile owner-lock refusals; they do not force a particular
scheduler winner or claim measured overlapping SQL critical sections. Corruption
is injected by a separate SQLite process only after all owners of a throwaway
store have exited. Default-feature compatibility also exercises the existing
local billing init/accept/retry/reopened-statement path, not just help/version.

Before freeze: independently verify fixture bytes, target/receipt domains and
math; review acceptance delegation and A-retention/cancellation semantics;
review persisted setup as the local authority root, cross-installation purchase
identity limits, sequential host-slot ownership and crash cleanup; decide how a
real host registers/revokes authorizations; review fixed windows and clock policy;
run the process E2E with the pinned toolchain, including feature-off compatibility;
review the exact candidate inventory and publish a separate reviewed freeze
manifest. No automatic amendment to the released local profile is proposed.

Remaining implementation limits: no live Zen/provider check, automatic job
restart, expiry/reaper, refund/correction, authorization revocation, physical
resource proof, crash-during-COMMIT/power-loss injection, or cross-store exactly-once
guarantee. The author compiled all three build variants and passed the dedicated
process E2E gate (270 counted CLI checks) using the user-provided pinned Rust
1.98.1 toolchain/cache. See `REVIEW-STATUS.md` and `E2E-RESULT.json`. This does not
replace separate review or establish a reviewed freeze.
