# Local SQLite billing

This is a source-installable, bounded local billing profile for one operator-controlled business per installation, with multiple customers and sources inside that business. Each customer/source pair has explicit permissions and an immutable agreement timeline. It accepts billable `content.generated` work at the active fixed USD price, one fixed-amount outcome family, and corrections to that family's claim. A correction appends the exact inverse of the current adjustment plus the permitted replacement; it never edits the original charge. Payments, tax invoices, remote authentication, and multi-business tenancy are outside this phase.

The reduced local SQLite Phase 4 is complete. Its author checks and owner acceptance are not independent technical review: the owner waived that separate requirement for the amended local Phase 6, and no independent PASS is claimed. The original full Phase 4 and production host certification remain outside this delivery. See [current status](../STATUS.md); earlier demo and phase records remain historical evidence.

## Install

The tested native environments are macOS 26.6.2 on Apple silicon and Ubuntu 24.04.5 x86-64/glibc 2.39. The [v0.3.0 native package guide](../examples/integration/README.md) covers downloaded installs. For a source install, use the repository's pinned Rust 1.98.1 development toolchain and native C/linker tools:

```sh
cargo install --path crates/ledgerlab-cli --locked --root ./work/billing-install
export PATH="$PWD/work/billing-install/bin:$PATH"
ledger --help
```

The first build may download locked dependencies. Add `--offline` only after populating the cache. This command installs from the checked-out source. Record its exact commit; [GitHub Releases](https://github.com/stevekkall-beansgc/bean-counter/releases) identify published versions. The installed `ledger` binary needs no Cargo, Python, Node, Docker, cloud account, model key or network service for billing. Preserve the source commit and binary SHA-256 with operational records. See [resources and costs](resources-and-costs.md).

For machine callers, use the [M2 billing CLI contract](billing-m2-cli-contract.md) and its durable Python or Node.js outbox examples. The [historical v0.3.0/v0.4.0 contract](billing-cli-contract.md) records the older single-customer CLI behavior.

For v0.3.0 Mac and Linux archive checks, guided setup and machine-readable product examples, see [local product integration](../examples/integration/README.md). The packages are unsigned. Other OS versions and Linux distributions remain unverified; Windows is deferred.

## Set up explicit terms

Copy `examples/billing/setup.json` and edit it before using actual customer information. The example is illustrative input, not real external customer assent. Supply fresh scope, store, customer, agreement and binding identifiers; an absolute source URI; the exact agreed price; the acceptor; retained assent evidence; and truthful operator attestations of authority and finality. Match the binding and source throughout the outcome policy. The program retains these assertions but does not obtain or independently authenticate customer consent.

The operating-system account and private local directory are the administrator/authentication boundary. Anyone controlling that account or database is inside this trust boundary. There is no remote caller identity, credential service or tamper-proof signature. Application events cannot choose grants, authoritative prices or permissions. Keep the installation private, on durable local storage with working filesystem locks and fsync. Shared/network filesystems and concurrent copied writers are unsupported.

Policy input is strict: exactly one outcome family, 1–32 fixed USD/scale-2 codes, explicitly permitted replacement codes, explicit reversal permission, evidence required, and one nonnegative premium ceiling. Percentage adjustments and unknown fields refuse. The existing core enforces discount/premium bounds. Ordinary and correction windows are immutable absolute timestamps: for each window, `starts_at < occurs_before <= received_by <= accepted_by`. **The base event's `occurred_at` must be no later than both windows' `starts_at`.** An outcome's occurrence time must be inside its ordinary window; its receipt and acceptance must arrive by the configured cutoffs. A correction uses the separate correction window and must name the current claim revision. Windows do not slide relative to acceptance; choose terms covering the intended work and outcome reporting period. The checked-in example uses September 2026 work and expires in early 2027.

```sh
ledger billing init ./billing --setup examples/billing/setup.json
ledger billing --directory ./billing accept --customer customer-1 --source urn:example:work examples/billing/event.json --json
```

The setup file creates the first customer and source. Register another customer/source pair with `ledger billing --directory ./billing agreement --customer CUSTOMER --source SOURCE registration.json`. The `/2` registration contains a stable `change_id`, `expected_revision: "0"`, `effective_at`, and a complete setup `/1` document for that customer/source. The customer and source in the command must match the request. New customers receive a separate derived billing scope; a setup cannot choose or overwrite it. To add a source for a customer, register that pair with the same customer ID. Agreement changes use new `/2` amendment and ending controls, a strictly increasing revision, and explicit effective time. Amendments keep the original permission ceiling. An end blocks new billable work after its effective time and leaves prior work and exact retries readable.

For an interactive setup with no hand-authored JSON, use `ledger billing setup ./billing`. It asks for the actual parties, agreement, exact price, fixed outcome codes, UTC reporting/correction windows, retained assent evidence, and truthful authority/finality attestations. Read and submit permissions are required; correction permission is optional and separately confirmed. It displays the validated terms and entered assertions for review, then requires `CREATE`; `cancel`, invalid terms, or an existing destination do not initialize a store. The generated configuration is retained as `./billing/setup.json` with private file permissions for repeat use. The program does not obtain assent or verify authority. `ledger billing setup DIR --setup FILE` remains available when terms were prepared separately. For scripts, agents and products, use `billing init DIR --setup FILE --json`; this remains noninteractive. The native packages target Apple-silicon macOS and Linux x86-64. Setup was exercised on macOS 26.6.2 and Ubuntu 24.04.5 x86-64/glibc 2.39; other versions and distributions are untested. The macOS artifact’s declared deployment minimum is a loader/build declaration, not a certification of every later version.

Initialization books nothing. Successful acceptance returns a `base-acceptance` receipt whose `body.target` is the target ID. With the unchanged example, the customer owes 250 atoms = USD 2.50. Copy the target ID into `TARGET`:

```sh
TARGET='ev2_REPLACE_WITH_THE_RETURNED_TARGET'
ledger billing --directory ./billing explain --customer customer-1 "$TARGET" --json
ledger billing --directory ./billing accept --customer customer-1 --source urn:example:work examples/billing/event.json --json
ledger billing --directory ./billing statement --customer customer-1 --json
```

Each invocation opens and closes the store. The retry returns the original receipt and books nothing. A different delivery `id` with the same `operation_id` and economic facts is a semantic duplicate; its alias is permanently reserved. Reusing either ID with different content refuses. Omitting `operation_id` makes it default to the delivery ID, so keep it explicit and stable across renamed deliveries. Never replace an ID just because a response was lost.

## Adjust and correct

Copy `examples/billing/outcome.json` and `examples/billing/correction.json`, replacing their `target` with the returned target ID and their timestamps/evidence with the actual observed facts. Then:

```sh
ledger billing --directory ./billing outcome --customer customer-1 --source urn:example:work ./outcome.json --json
ledger billing --directory ./billing explain --customer customer-1 "$TARGET" --json
ledger billing --directory ./billing correct --customer customer-1 --source urn:example:work ./correction.json --json
ledger billing --directory ./billing statement --customer customer-1 --json > statement.json
```

The sample `rebate` is -50 atoms: balance 200. The correction expects claim revision `1` and replaces the rebate with code `none`: +50 inverse and zero replacement, balance 250. Original records remain present. A later correction must name the current revision. `{"kind":"reverse"}` removes the current adjustment when allowed; it does not reverse the base charge. Exact retries return saved receipts even after a subsequent revision; a fresh correction with a stale expected revision refuses. Duplicate ordinary outcomes require the same family, target, code, occurrence and evidence.

For runnable success, unsuccessful-by-cutoff, retry and correction examples using fresh private stores, run the extended synthetic walkthrough in [finance-e2e.md](finance-e2e.md) with `--checks`. It demonstrates a 2-atom completed-work base, a +98 success premium, a -2 explicit unsuccessful adjustment, idempotent retry, and exact reversal plus replacement on correction. No outcome means the completed-work base remains 2 atoms; it does not cause an automatic credit. These examples are operator attestations over synthetic data, not automatic verification of a real LLM result or a capacity reservation.

Statements use `ledger-billing-statement/2` and cover all accepted decisions for the requested customer across every registered source at their cutoff. They include full retained records, stable IDs, agreement versions, original and correction postings, integer-atom totals and a receipt-root snapshot hash. They replay/verify retained decisions and project their saved postings; editing a draft setup cannot reprice history. A statement refuses rather than returning a partial result if read permission is missing for any source. `complete:true` means the whole customer history, or the requested target history, at that snapshot is included. Customer statements and CSV exports have customer-local ordinals and hashes. The receipt-root hash describes economic history, not a backup or a hash of administrator changes. It is a billing statement, not a tax/legal invoice or proof of payment. No money is collected or dispatched.

## Permissions and revocation

Initial rights are `read`, `submit` and `correct`. A private-directory administrator can revoke or restore only rights listed in the original setup. Inspect current rights even when event access has been revoked:

```sh
ledger billing --directory ./billing permissions --customer customer-1 --source urn:example:work --json
ledger billing --directory ./billing permissions --customer customer-1 --source urn:example:work examples/billing/revoke.json --json
```

The `/2` sample change expects revision `1`, has a stable `change_id`, retains only `read` and includes a reason. New submissions and corrections then refuse. Read-authorized exact retries still return original receipts after revocation or agreement end. Removing `read` blocks reports and duplicate resolution. To restore, supply the current `expected_revision`, a new `change_id`, a reason, and a subset of the original rights. Controls are checked and recorded inside the same immediate transaction as relevant reads/writes. Accepted evidence records the permission revision used. The `permissions` status command is an administrator control; remote callers are not part of this local profile.

Terms and the permission ceiling cannot be edited in place. Use an immutable amendment or a new agreement start; already accepted work keeps its original price and policy. Schema-8 installations require the explicit command `ledger billing --directory ./billing upgrade --json`; ordinary open refuses schema 8 rather than migrating it silently. The upgrade is limited to the source-built v0.4.3 schema-8 path and preserves schema-8 record, receipt, alias, permission, and setup bytes. Keep a quiescent whole-installation backup before upgrading and validate the resulting customer statement and exact retries. This does not qualify native artifacts or other historical stores.

## Bounds and failures

Hard admission bounds are 1,000 accepted decisions (bases plus outcomes/corrections), 1,000 delivery aliases and 1,000 permission changes. Economic payloads are limited to 32 MiB in aggregate; alias ingress separately to 32 MiB; each permission input to 64 KiB. Event input is at most 256 KiB, setup at most 64 KiB, and each accepted bundle at most 8 MiB. Reads verify complete history under these bounds and never silently truncate. The limits are ceilings, not tested throughput or hardware sizing guarantees. Reserve headroom and start a reconciled new agreement before exhausting a store; there is no compaction or automatic rollover.

Use `--json` for stable machine-readable status. Exit 0 means accepted/duplicate or a successful control/read; 2 is usage/local file input, 3 rejected input or stale revision, 4 conflicting identity/facts, 6 unauthorized/wrong customer or source, 7 unavailable storage, 8 unknown commit outcome, 9 failed retained-data integrity. An unknown result is not evidence of rollback. Reopen and retry the identical event through the same command and original source/ID. A permission-control unknown result requires inspecting `permissions` and its revision/change history before deciding whether to retry. Do not delete the owner lock file or bypass an integrity error.

If a valid outcome returns `OUTCOME_WINDOW`, compare the base event's `occurred_at` with both configured window starts, then check the outcome's own occurrence, receipt, and acceptance times against the ordinary cutoffs. Correct the terms before initializing a fresh synthetic store; accepted terms cannot be edited in place. If correction returns `STALE_REVISION`, read the target statement/explanation and submit a newly prepared correction with the current revision and factual evidence. For exit 8, preserve the original IDs and files, reopen the same store, and retry the identical request; never invent a replacement identity to guess whether it committed. Keep command output and errors visible while diagnosing failures.

There is no capacity reservation or unconditional completion guarantee. Disk/RAM exhaustion may refuse work. Accepted atomic writes rely on SQLite, the OS and local storage honoring their interfaces. The adapter uses WAL, FULL synchronous durability and one owning process; no parallel CLI invocations may own the same store. See [backup and recovery](billing-recovery.md) before relying on the data.
