# Outbox safety hardening

Assigned branch: `codex/outbox-hardening`, based on
`b35258425970052ed71481eca1f33ef857c61be1`. This supersedes the scan-limit,
rejection and operator-recovery behavior in `PHASE-2-OUTBOX-STATUS.md`.

## Reproduced defects and fixes

1. **Administrative lockout at 1,001 retained intentions.** Before the change,
   `controls_survive_1001_intentions` accepted 1,001 distinct real decisions across
   two seeded chains and failed at pause with `ScanLimit`. The same test now
   checks pause, restore hold, reconciliation and resume at 64, 65, 1,000 and
   1,001 intentions, plus fenced lost-response recovery at 1,001.
2. **Permanent rejection could become runnable again.** Before the change,
   `rejected_survives_restore` and `rejected_survives_unknown_inventory` both
   observed `Unknown` where a permanent `Rejected` was required. Later confirmed
   absence could then become pending. Rejection is now absorbing in the shared
   coordinator and protected by both databases' update triggers.
3. **No explicit escape from an unresolved intention.** A rejected/exhausted key
   prevented every subsequent resume, with no audited operator transition. The new
   `quarantine` operation permanently isolates one named rejected/unknown intention
   while retaining its delivery state, immutable intention and economic records.
   Fresh reconciliation can permit unrelated work. No retry/reset/unquarantine or
   operator assertion of economic settlement is exposed.

Before-fix test logs are retained locally under ignored `work/validation/`.
Tests never call a billing provider. No price, pure-core/oracle rule, Phase 2
canonical contract, production CLI, manifest/lockfile or external provider behavior changes.

## State rules

The seven persisted delivery states are unchanged. Quarantine is an independent
permanent veto, not a replacement state and never proof of delivery.

| Operation/evidence | Transition |
|---|---|
| Claim due work, dependencies delivered and not quarantined | held/pending/retry → leased; attempts increase by one, maximum 20 |
| Matching response for the current attempt | leased → delivered |
| Mismatch or explicit permanent rejection | leased → rejected |
| Confirmed pre-receipt failure, attempts 1–19 | leased → retry, delay 1s, 2s, 4s… capped at 300s |
| Confirmed absence at attempt 20 | leased → rejected |
| Uncertain response, expired delivery lease, replacement or pause | potentially sent lease → unknown |
| Restore hold | every non-rejected mapping → unknown; rejection and quarantine survive |
| Reconcile matching receipt | non-rejected → delivered; rejected remains rejected |
| Reconcile authoritative absence | non-rejected and attempts <20 → pending; attempts ≥20 → rejected |
| Reconcile mismatch | rejected |
| Reconcile unavailable inventory | non-rejected → unknown; rejected remains rejected |
| Operator quarantine | rejected/unknown retains its state and attempts; permanent separate veto is appended |
| Late/stale response | append evidence, return Fenced, no delivery-state change |

The shared reconciliation transition matrix covers all seven prior states, six
outcomes, attempt counts 0/19/20, and presence/absence of quarantine: 252 cases.
Database tests also attempt every rejection downgrade, attempt-count reset and
leasing a quarantined intention through the restricted runtime transaction port.

Quarantine requires a held installation with no dispatcher owner, an exact
IntentionId, the expected current observation digest, a nonempty operator identity
(up to 128 bytes), and a nonempty reason (up to 2,048 bytes). Stale evidence,
repeat resolution, active dispatch and non-unresolved targets reject. The embedding
host remains responsible for authenticating its operator, as with the existing
trusted host outbox API. No new untrusted transport is exposed.

Audit bytes bind store identity, restore generation, intention ID/hash, prior
state, attempt count, observation digest, operator, reason and supplied time.
The digest and full bytes are inserted into `delivery_quarantines`, with the same
body appended to `delivery_observations` in the same transaction. Both have
immutable guards. `Delivery.quarantine` exposes the retained veto digest. Future
inventory evidence remains separate and does not remove the veto. A quarantined
original does not satisfy any dependent intention's delivery requirement.

## Bounded reads and complete reconciliation

- Control reads load installation/head/latest compact report with **zero intention
  bodies**. Pause, acquisition and restore use set-based delivery-state updates
  under the existing installation-first lock order and tracked transaction.
- Keyset pages select at most 64 IDs and lengths before loading blobs, with an
  8 MiB aggregate intention-byte budget and the existing 4 MiB individual bound.
  No installation-wide count/sum gate exists. Send/observe/quarantine read only
  their key. Claim checks due candidates page by page and resolves dependencies
  by key, including dependencies outside the candidate page.
- `deliveries()` retains its original complete-query limit of 1,000 rows and fails
  with `ScanLimit` above it. `deliveries_after(last_id)` supplies bounded pages;
  continue until empty. Independent query pages are not an atomic inventory view.
- Reconciliation and resume stream every local intention under one transaction.
  The fingerprint rolls through ordered ID/hash/state/attempt/schedule/observation/
  quarantine tuples, bound to store, restore and dispatcher generations. They use
  a 60-second maintenance deadline; ordinary operations retain five seconds.
  Per-query deadlines and PostgreSQL SERIALIZABLE/ordered locks remain in force.
  Timeouts/conflicts/cancellation retain the hold; this is not an unlimited
  throughput or lock-duration claim. Persistent multi-transaction scan checkpoints
  remain future scale work if inventories exceed the maintenance time budget.
- Per-key reconciliation evidence is retained in bounded observation pages.
  The final version-2 report contains full counts, a complete rolling fingerprint
  and samples of at most 64 unresolved keys, orphan keys and observations. Samples
  never determine completeness. Resume recomputes the complete fingerprint.
- The independent fake offers direct lookup, key pages and an inventory digest
  computed without cloning the entire receipt collection. Reconciliation compares
  inventory before/after scanning; resume compares it again. Unknown inventory or
  any orphan remains a hold, even when every local unresolved key is quarantined.
- New acceptance, a changed state/observation/quarantine, restore generation or
  remote inventory invalidates a prior report. Old report formats cannot resume.

The in-memory fake necessarily retains its receipts; its public diagnostic
`receipts()` helper still returns them for tests. Production reconciliation no
longer calls that unbounded diagnostic helper.

## Schema and upgrade implications

Both backends add **0003_outbox_safety.sql** and initialize backend schema 3.
Logical economic schema remains 1. Migrations 0001/0002 retain their exact bytes.
The new table has one permanent quarantine per scoped intention, a foreign key,
canonical audit bytes and digest. SQLite UPDATE/DELETE and PostgreSQL
UPDATE/DELETE/TRUNCATE guards reject mutation. Delivery update guards forbid
rejection downgrades, decreasing attempts and leasing quarantine.

The existing CLI coexistence test now expects three installed migrations; no
CLI behavior or command implementation changes.

The PostgreSQL runtime role gains only SELECT/INSERT on quarantine evidence via
initialization, with no DDL/ownership/DELETE/TRUNCATE privilege. Normal open
verifies all three migration checksums and never migrates. Older schema-1/2 stores
are refused until an explicit owner-driven, admission-fenced upgrade is supplied.
That upgrade must review any rejection already downgraded by older software;
this migration does not infer or repair past economic/delivery history.

## Validation

Final `sh scripts/check.sh` passed on the prepared Rust 1.98.1 development
toolchain with locked offline dependencies: **117 passed, zero failed, 18 ignored**.
The ignored entries are 16 opt-in real-PostgreSQL tests (executed separately) and
the two remaining process-durable fake gates. Formatting, warnings-denied Clippy,
no-default-feature build, resolved dependency/source boundaries and the complete
frozen document/canonical audit all passed. The audit retains 99 frozen files,
60 hash vectors, 25 accepted immutable records, 29 manifest members and 80 atoms.

Real PostgreSQL **17.11** (`170011`) and **18.6** (`180006`), aarch64 Linux,
passed all **16 unique opt-in integration entries on each major**. Each major ran
the full 15-entry suite plus the final two paging tests (one repeated with tighter
byte-bound assertions, one newly added rollback test). These include the 83-case
acceptance suite, 45 cancellation cases, aliases, actual competing connections,
immutable guards, exact reopen readback, preview isolation, direct driver
transport cuts and production-supervisor ambiguous-commit/overlapping-drain cuts.
Eleven outbox histories per backend preserve the immutable journal/index bytes;
quarantine and 1,001-intention cases add their independent adversarial evidence.

The PostgreSQL runs used fresh disposable local containers from already cached
images, new synthetic credentials and a newly generated test CA. No existing
credentials were extracted. Services are stopped after validation; no remote
infrastructure or billing provider is involved.

Reproduction with a prepared offline toolchain and explicit synthetic test
port/password/CA/major environment:

```sh
sh scripts/check.sh
cargo test -p ledgerlab --lib --all-features --locked --offline -- --ignored --test-threads=1 --nocapture
sh crates/ledgerlab/src/store/postgres/proof/run.sh
```

The standalone driver/TLS proof passed **16 tests**, its compile/lint/source audit,
and its parent-launched synthetic ambient-environment child test. Its single
framework-ignored entry is that child, invoked by the passing parent test; it is
not a missing transport test. Positive private trust, wrong-host/expired/unrelated
CA/plaintext rejection, cancellation, rollback, commit ambiguity and discard were
exercised using synthetic localhost peers.

The final adversarial checks include second-page reconciliation failure after
first-page state/evidence writes: every physical cell remains unchanged after
rollback and reopen, with dispatch held. The same check passes on SQLite and both
PostgreSQL majors. Rejected/unknown/exhausted quarantine histories preserve
original economic rows, prove stale resolution refusal, continue unrelated work,
and retain audit/veto data through reopen and restore.
The adversarial storage-only tests insert separately labeled synthetic operational
fixtures to probe >12 MiB retained bytes and cross-page dependency traversal.
These fixtures are not canonical economic conformance evidence. The 1,001-item
regression uses the actual acceptance coordinator, with no synthetic row shortcut.

## Remaining process-durable gates

The fake is still in memory. Independent durable SQLite/PostgreSQL fake receipt
and fencing storage, process-restart adapters and the two explicitly ignored
process-durable delivery tests remain open. Same-instance reopen/restore evidence
must not be represented as process durability or arbitrary-provider exactly-once.

Full backup production, PostgreSQL restore drills, portable byte-preserving
export/import/cutover, explicit owner upgrade orchestration, continuous scheduling,
PostgreSQL session advisory ownership/connection budgets, power-loss/disk-full
certification and the native platform matrix remain outside this slice. No
merge, push, deployment, publication, paid service or network billing call occurs.

## Phase 2 integration follow-up

The explicit schema-1/2 upgrade gate described above is now implemented and tested in [PHASE-2-OUTBOX-MERGE.md](PHASE-2-OUTBOX-MERGE.md). The validation counts above remain the original source-lane evidence; the integration report records the current runs separately.
