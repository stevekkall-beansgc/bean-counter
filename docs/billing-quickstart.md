# Local SQLite billing

This is a source-installable, bounded local profile for one operator-controlled customer/agreement per installation. It uses the existing pure economic engine and frozen v2 outcome records. It accepts independent successful `content.generated` work at one fixed USD price, one fixed-amount outcome family, and corrections to that family's claim. A correction appends the exact inverse of the current adjustment plus the permitted replacement. It does not edit the original charge. Supplier obligations, predecessor chains, remote authentication, payments and tax invoices are outside this profile.

Local implementation checks are not independent acceptance. Independent review remains platform-blocked; this delivery does not complete Phase 4 or establish production readiness. The earlier demo and Phase 4 records remain historical evidence.

## Install

The tested native environment is macOS 26.6.2 on Apple silicon. Use the repository's pinned Rust 1.98.1 development toolchain and native C/linker tools. From the checked-out source:

```sh
cargo install --path crates/ledgerlab-cli --locked --root ./work/billing-install
export PATH="$PWD/work/billing-install/bin:$PATH"
ledger --help
```

The first build may download locked dependencies. Add `--offline` only after populating the cache. This command installs from the checked-out source. Record its exact commit; [GitHub Releases](https://github.com/stevekkall-beansgc/bean-counter/releases) identify published versions. The installed `ledger` binary needs no Cargo, Python, Node, Docker, cloud account, model key or network service for billing. Preserve the source commit and binary SHA-256 with operational records. See [resources and costs](resources-and-costs.md).

## Set up explicit terms

Copy `examples/billing/setup.json` and edit it before using actual customer information. The example is illustrative input, not real external customer assent. Supply fresh scope, store, customer, agreement and binding identifiers; an absolute source URI; the exact agreed price; the acceptor; retained assent evidence; and truthful operator attestations of authority and finality. Match the binding and source throughout the outcome policy. The program retains these assertions but does not obtain or independently authenticate customer consent.

The operating-system account and private local directory are the administrator/authentication boundary. Anyone controlling that account or database is inside this trust boundary. There is no remote caller identity, credential service or tamper-proof signature. Application events cannot choose grants, authoritative prices or permissions. Keep the installation private, on durable local storage with working filesystem locks and fsync. Shared/network filesystems and concurrent copied writers are unsupported.

Policy input is strict: exactly one outcome family, 1–32 fixed USD/scale-2 codes, explicitly permitted replacement codes, explicit reversal permission, evidence required, and one nonnegative premium ceiling. Percentage adjustments and unknown fields refuse. The existing core enforces discount/premium bounds. Ordinary and correction windows are immutable absolute timestamps: start < occurrence deadline <= receipt deadline <= acceptance deadline. **Base occurrence must be no later than both window starts.** Windows do not slide relative to acceptance; choose terms covering the intended work and outcome reporting period. The checked-in example uses September 2026 work and expires in early 2027.

```sh
ledger billing init ./billing --setup examples/billing/setup.json
ledger billing --directory ./billing accept examples/billing/event.json --json
```

Initialization books nothing. Successful acceptance returns a `base-acceptance` receipt whose `body.target` is the target ID. With the unchanged example, the customer owes 250 atoms = USD 2.50. Copy the target ID into `TARGET`:

```sh
TARGET='ev2_REPLACE_WITH_THE_RETURNED_TARGET'
ledger billing --directory ./billing explain "$TARGET" --json
ledger billing --directory ./billing accept examples/billing/event.json --json
ledger billing --directory ./billing statement --customer customer-1 --json
```

Each invocation opens and closes the store. The retry returns the original receipt and books nothing. A different delivery `id` with the same `operation_id` and economic facts is a semantic duplicate; its alias is permanently reserved. Reusing either ID with different content refuses. Omitting `operation_id` makes it default to the delivery ID, so keep it explicit and stable across renamed deliveries. Never replace an ID just because a response was lost.

## Adjust and correct

Copy `examples/billing/outcome.json` and `examples/billing/correction.json`, replacing their `target` with the returned target ID and their timestamps/evidence with the actual observed facts. Then:

```sh
ledger billing --directory ./billing outcome ./outcome.json --json
ledger billing --directory ./billing explain "$TARGET" --json
ledger billing --directory ./billing correct ./correction.json --json
ledger billing --directory ./billing statement --customer customer-1 --json > statement.json
```

The sample `rebate` is -50 atoms: balance 200. The correction expects claim revision `1` and replaces the rebate with code `none`: +50 inverse and zero replacement, balance 250. Original records remain present. A later correction must name the current revision. `{"kind":"reverse"}` removes the current adjustment when allowed; it does not reverse the base charge. Exact retries return saved receipts even after a subsequent revision; a fresh correction with a stale expected revision refuses. Duplicate ordinary outcomes require the same family, target, code, occurrence and evidence.

Statements contain all accepted decisions at their cutoff, full retained records, stable IDs, original and correction postings, integer-atom totals and a receipt-root snapshot hash. They replay/verify retained decisions and project their saved postings; editing a draft setup cannot reprice history. `complete:true` means the entire installation history, or requested target history, at that snapshot is included. The receipt-root hash describes economic history, not a backup or a hash of administrator changes. It is a billing statement, not a tax/legal invoice or proof of payment. No money is collected or dispatched.

## Permissions and revocation

Initial rights are `read`, `submit` and `correct`. A private-directory administrator can revoke or restore only rights listed in the original setup. Inspect current rights even when event access has been revoked:

```sh
ledger billing --directory ./billing permissions --json
ledger billing --directory ./billing permissions examples/billing/revoke.json --json
```

The sample change expects revision `1`, retains only `read` and includes a reason. New submissions and corrections then refuse. Read-authorized duplicate retries still return original receipts. Removing `read` blocks reports and duplicate resolution. To restore, supply the current `expected_revision`, a reason, and a subset of the original rights. Controls are checked and recorded inside the same immediate transaction as relevant reads/writes. Accepted evidence records the permission revision used. The `permissions` status command is an administrator control, not an event read authorization bypass for remote users; there are no remote users in this profile.

Terms and the permission ceiling cannot be edited in place. A new agreement needs a separate installation with new identifiers; explicitly reconcile its separate statements. No migration/import between billing installations or repricing of already booked work is provided. Schema 8 is required by this candidate; it refuses older billing snapshots instead of silently migrating them. Preserve an older binary with its older snapshot.

## Bounds and failures

Hard admission bounds are 1,000 accepted decisions (bases plus outcomes/corrections), 1,000 delivery aliases and 1,000 permission changes. Economic payloads are limited to 32 MiB in aggregate; alias ingress separately to 32 MiB; each permission input to 64 KiB. Event input is at most 256 KiB, setup at most 64 KiB, and each accepted bundle at most 8 MiB. Reads verify complete history under these bounds and never silently truncate. The limits are ceilings, not tested throughput or hardware sizing guarantees. Reserve headroom and start a reconciled new agreement before exhausting a store; there is no compaction or automatic rollover.

Use `--json` for stable machine-readable status. Exit 0 means accepted/duplicate or a successful control/read; 2 is usage/local file input, 3 rejected input or stale revision, 4 conflicting identity/facts, 6 unauthorized/wrong customer or source, 7 unavailable storage, 8 unknown commit outcome, 9 failed retained-data integrity. An unknown result is not evidence of rollback. Reopen and retry the identical event through the same command and original source/ID. A permission-control unknown result requires inspecting `permissions` and its revision/change history before deciding whether to retry. Do not delete the owner lock file or bypass an integrity error.

There is no capacity reservation or unconditional completion guarantee. Disk/RAM exhaustion may refuse work. Accepted atomic writes rely on SQLite, the OS and local storage honoring their interfaces. The adapter uses WAL, FULL synchronous durability and one owning process; no parallel CLI invocations may own the same store. See [backup and recovery](billing-recovery.md) before relying on the data.
