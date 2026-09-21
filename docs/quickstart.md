# Local developer quickstart

Ledger Lab records work, applies its agreed price, and explains the immutable
receipt. The `ledger` binary runs locally with SQLite. Running it requires no
Docker, Node, cloud account, hosted service, model key, or paid provider.

The local commands support the completed Phase 1 generation slice. The product story
is an AI generation charged initially, followed by a linked outcome that can add
a premium or discount. Optional paid tools and BYOK/platform-funded responsibility
belong in the same chain. The branch includes a pure Phase 2 evaluator, but its
results cannot yet be saved or previewed through these commands. Linked events
return `UNSUPPORTED_SLICE` without changing stored state. The demo's BYOK context
creates no host supplier payable. See the [integration status](../PHASE-2-INTEGRATION-STATUS.md)
for the unresolved discount semantics and missing persistence definitions.

## Build and run

From a repository checkout with the verified Rust development toolchain and
cached dependencies:

```sh
cargo build -p ledgerlab-cli --locked --offline
export PATH="$PWD/target/debug:$PATH"
ledger init ledger-demo --demo
cd ledger-demo
ledger preview examples/generated.json
ledger accept examples/generated.json
ledger explain --chain demo-slice
ledger accept examples/generated.json --format json
cat examples/generated.json | ledger accept - --format json
```

`ledger init --demo` also works inside an empty directory. Initialization refuses
nonempty destinations, including existing ledgers. It creates `ledger.yaml`,
`examples/generated.json`, a short README, `.gitignore`, and private `.ledger`
SQLite state. No event has been accepted until you run `accept`.

Expected postings are **100 USD atoms charged, −20 discounted**, with one
**80-atom intention held for fake export**. Scale 2 means 100 atoms is $1.00.
Retrying returns the original receipt without a new economic decision. This is
the frozen first-slice example; it is separate from the later $1.20 onboarding
chain in the design. No exporter or payment call runs.

`explain generation-1` accepts an external event ID in the configured source;
canonical `ev_…` IDs and semantic delivery aliases also work. `explain --chain
 demo-slice` returns accepted events in revision order. The JSON view includes
source events, immutable decision manifests, postings, links, intentions,
explanations, original snapshot/input documents, and receipts. It never reprices
history. The text view summarizes these records and prints the full source event.

Use a new delivery ID **and** new `operation_id` for genuinely distinct work.
Reusing the operation with matching facts is a semantic duplicate. Reusing an
identity with changed content is a conflict. The current facade requires a
provisioned chain: changing `chain` to an unknown value returns Waiting without
reserving anything. Later outcomes, links, supplier fees and real assent remain
unsupported; they must be added to the core/coordinator rather than the CLI.

## Configuration and local ownership

`ledger.yaml` has schema `ledger/v1`. This bounded CLI accepts **strict JSON
syntax**, a YAML 1.2 subset, using the existing parser. General YAML syntax,
unknown/duplicate fields, environment expansion, shell interpolation, real mode,
PostgreSQL config, and enabled dispatch are rejected. JSON avoids a new parser
or dependency version. `--config PATH` is explicit and defaults to
`./ledger.yaml`; there is no ambient home/environment config search.

The generated fields are `schema`, `mode`, `identity`, `storage`, `auth`, and
`dispatch`. `auth` contains trusted local host selectors (`principal_id`,
`source`, `authority_head`, `binding_selector`), **not credentials**. Local file
access supplies the host identity; acceptance still checks retained grants under
lock. This local convenience read API must not become a remote read endpoint.
Prices and accepted synthetic terms live in the database, not the config.

Storage paths resolve relative to the configuration file. They must be relative
and contain no parent traversal. Input/config/storage paths reject symlinks;
regular files are required. Input events are limited to 256 KiB and configuration
to 64 KiB. On Unix, generated directories are 0700 and files 0600; opening rejects
group/world-accessible data directories or database files. Native Windows ACL
hardening and certification remain a platform gate. Existing parent directory
permissions are not changed. The supplied destination's parent must exist.

Each command owns the SQLite directory exclusively and closes it before exit.
Concurrent owners return busy/unavailable. Keep the entire `.ledger` directory
together; do not copy a live database file without its WAL. Demo installations
intentionally preserve the frozen `demo/sandbox` scope and `store-demo-slice`
identity for exact fixture compatibility. They are isolated disposable examples,
**not distinct namespaces to merge or export to a shared destination**.

## Preview and automation

Preview uses the same normalization, identity checks, locked authority resolution
and pure evaluation as acceptance. It skips all journal writes, including delivery
aliases, and ends with rollback without commit. It returns `PreviewResult`, never
an accepted receipt. Predicted canonical records exclude the candidate receipt.
Acceptance rechecks the context, so the estimate does not reserve a price,
identity, revision, or authority.

This is a **no journal write/no commit** guarantee, not a filesystem immutability
guarantee: opening the existing store and taking its locks may create/update
owner/WAL/shared-memory files. Preview uses the existing transaction locks and
can contend with acceptance. A physically read-only/offline preview is not
implemented. Missing authority is reported honestly as rejected/unauthorized;
this narrow preview does not invent a hypothetical context.

`--format json` (or `--json`) emits one `ledger-cli/1` object on stdout, including
failures. Text is the default; text file/config errors go to stderr. Help/version
are plain text. JSON objects use deterministic key ordering. The wrapper status
is one of `initialized`, `accepted`, `duplicate`, `conflict`, `rejected`, `waiting`,
`preview`, `explained`, `outcome_unknown`, or `error`. Nested receipts and records
retain their existing schema versions. Money stays string atoms plus scale.

| Exit | Meaning |
|---|---|
| 0 | Initialized, accepted, duplicate, explanation, or valid accepting/duplicate preview |
| 2 | Usage, configuration, unsupported config schema, file/path/size error |
| 3 | Rejected event/preview or no accepted event found |
| 4 | Identity/semantic conflict, including preview |
| 5 | Waiting for a dependency, including preview |
| 6 | Unauthorized source/context, including preview |
| 7 | Busy, retryable, or unavailable store |
| 8 | Commit outcome unknown; retry identical input and identity |
| 9 | Retained-data integrity failure |

Malformed event bytes reach the existing boundary and return exit 3. Input-file
and pre-read size errors return 2. Process termination and broken output pipes
cannot prove rollback: if no receipt was received, retry the exact request.
There is no signal-drain handler in this bounded CLI.

## Integration seams and validation

- `LocalLedger::init_demo` is synthetic-only provisioning; it uses the existing
  backend-schema-2 migrations and private typed store writes. Normal open verifies
  both migration checksums; it does not upgrade older schema-1 installations.
- `LocalLedger` loads host context and wraps `Ledger::accept`; the CLI does no
  normalization, authority decisions, pricing, rounding, or totals.
- `Ledger::preview` is a separate result type over the same coordinator preparation
  path for both library backends. SQLite CLI integration and fresh/duplicate/alias/
  unsupported preview on real PostgreSQL 18 and 17 preserve all database cells,
  including the new outbox tables.
- Local explanation reads have an explicit SQLite seam. They check canonical
  bytes/hashes, displayed manifest membership and receipt references, but are not
  a full-ledger verifier or original-semantics replay. PostgreSQL remains available
  through the existing library API, not required for onboarding.
- The library outbox can dispatch and reconcile the existing held intention using
  an independent in-memory fake. The CLI requires dispatch to remain held and has
  no export command. An explicit library pause is required before local commands
  reopen a ledger whose dispatch was enabled. This fake is not process-durable.
- The next Phase 2 bridge needs reviewed canonical records, coordinator preparation,
  and stored-read projections. Then add reviewed linked demo inputs and tests. Keep
  the CLI a formatter of facade results; do not add another economic model.
- Only existing pinned Tokio/serde_json/tempfile/SQLx packages were added as direct CLI
  dependencies; no new package/version was selected. The comparison adapter also
  adds a test-only edge to the pinned serde_json package. Frozen contracts/fixtures
  remain unchanged; the new core API is a separate typed evaluation surface.

Validation commands on the prepared development machine:

```sh
source work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
scripts/check.sh
```

The full repository audit also uses its prepared Python packages and Node as
**development test tools**, not product runtime dependencies. CLI integration
invokes the built native binary, compares the original frozen receipt, and covers
text snapshots, JSON, stdin, duplicates, semantic aliases, conflicts, waiting,
unauthorized/invalid/zero-action events, config/path/permission/ownership failures,
and unchanged logical database cells (including canonical bytes, aliases, and heads)
after preview/read/duplicate operations. SQLite WAL checkpointing may change the
main database file without a journal write.
