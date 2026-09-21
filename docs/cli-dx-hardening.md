# Local CLI DX handoff

Branch: `codex/cli-dx-hardening`.
Base integration commit: `b35258425970052ed71481eca1f33ef857c61be1`.

The local CLI now presents who owes whom, the exact decimal amount and the
reason before internal identifiers. Preview marks the amounts as estimates;
acceptance and explanation read the verified stored projection. The CLI places
a decimal point in the facade's amounts and reads its net intention; it does
not sum, round, select authority or evaluate prices. JSON receipt/record schemas,
status codes, exit codes and original retry receipts remain intact. Error
messages gain input/config paths and recovery guidance.

## Five-minute workflow after prerequisites

From the repository root, with Rust 1.98.1, a working native compiler/linker/SDK
and the locked dependency cache already installed:

```sh
cargo build -p ledgerlab-cli --locked --offline
export PATH="$PWD/target/debug:$PATH"
ledger init ./ledger-demo --demo
cd ledger-demo
ledger preview ./examples/generated.json
ledger accept ./examples/generated.json
ledger explain --chain demo-slice
ledger accept ./examples/generated.json --json
```

The last command returns the original receipt. The initial build and prerequisite
download duration are machine dependent. The hands-on workflow is five minutes
with the binary available. The [quickstart](quickstart.md) documents fresh
preparation and an optional offline `cargo install --path` into `work/install`.
No Docker, account, credential, telemetry, provider call or paid service is
needed. Node/Python are full-audit tools, not CLI runtime requirements.

## Output and local files

- Text shows demo-customer → demo-host, USD 0.80 net; USD 1.00 generation charge
  and USD −0.20 customer tier discount. Supplier obligations and observed provider
  costs have their own sections, empty in this slice. IDs, atoms, source events
  and retained explanation codes remain in JSON.
- Human acceptance retrieves the stored breakdown after acceptance. If this read
  fails, it still reports successful acceptance/original receipt and tells the
  caller to retrieve it with an identical JSON retry; it does not claim rollback.
- Initialization creates strict JSON named `ledger.json`. `--config ledger.yaml`
  explicitly opens a legacy strict-JSON file; there is no implicit fallback or
  YAML parser. SQLite stays the only local profile, with dispatch disabled/held.
- Explicit relative input/config/init paths normalize `.` and `..`. Storage stays
  in a dedicated directory inside the config directory. Every original component
  is checked before parent collapse, rejecting hidden symlinks and traversal
  through regular files. Unix private modes and owner-lock behavior are retained.
- Config validation identifies missing/unknown/invalid fields and a next action.
  File errors name the supplied path. Event rejections retain the facade's code
  and identify the input with example/schema guidance; the current facade does
  not expose exact parser field locations.

## Evidence

The complete offline `sh scripts/check.sh` passed: **117 tests passed, 13
explicit PostgreSQL/TLS tests ignored**, formatting, warnings-denied Clippy,
all-target/all-feature and no-default builds, resolved dependency/source checks,
and independent Python/Node contract verification. The audit preserved all 99
frozen files, 60 hash vectors, 25 immutable accepted records and 29 manifest
members. No live PostgreSQL/TLS run is claimed.

CLI coverage includes 16 integration tests, five text snapshots (help, preview,
accept, explain, config error), exact signed/large decimal display, stable
receipt retries, unchanged database cells after preview/explain, zero-action
acceptance, rejection/unsupported paths, legacy config selection, normalization,
storage containment, symlinks, privacy and concurrent ownership. A final targeted
CLI rerun covers the added damaged-breakdown retry assertion; formatting and CLI
Clippy were rerun. The documented offline local install and installed-binary
init/preview/accept/retry/explain workflow also passed.

Checks used the existing machine-local Rust alias after verifying actual
version 1.98.1 and existing cached Python audit packages. Those untracked caches
are not checkout prerequisites and are not committed.

## Phase 2 boundary and integration seam

The [upcoming story](upcoming-story.md) is a documentation-only contract
illustration: generation → publication → acquisition, separate customer discount,
paid tool obligation and explicit BYOK/platform-funded responsibility. It is not
an event bundle, a receipt or executable preview, and creates no ledger history.
Phase 2 persistence and CLI preview remain unavailable. Its unresolved discount
semantics are not changed by the story.

Changes are limited to `ledgerlab-cli`, CLI tests/snapshots, onboarding docs and
the existing local facade's config/path/initializer presentation helpers in
`crates/ledgerlab/src/local.rs`. The latter now includes an owned diagnostic
variant in `LocalError`; exhaustive internal consumers must handle it.
No core pricing, canonical contracts, fixture bytes, store/outbox behavior,
migrations, dependency manifests or lockfile changed.

Future work belongs behind `LocalLedger`/the shared coordinator: reviewed Phase 2
record encodings, preparation/persistence and stored-read projections. Only then
should the CLI format additional facade results and accept linked demo inputs.
No new economic model was added to the CLI. Nothing was merged, pushed, deployed
or published; the install was local to the ignored `work/` directory.
