# Local CLI handoff

21 September 2026. Branch `codex/dev-cli`, based on
`dce3ec4feda4025ab6e98ef23608b9e6f812aeb1`. Work was confined to the supplied
`ledger-lab-v0-cli` worktree. No merge, push, publication or deployment.

Implemented one native Rust executable with `init --demo`, file/stdin `accept`,
`preview`, and event/chain `explain`; plain text and `ledger-cli/1` JSON; documented
exit codes; private local files and SQLite ownership. See `docs/quickstart.md`.

## User workflow

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

The frozen generation example yields +100/−20 USD atoms and one held 80-atom fake
export intention. Retries return the original receipt. No event is preaccepted by
initialization. No network service, model call, exporter or payment is executed.

## Executed validation

```sh
source work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
scripts/check.sh
```

The activation file and prepared Python dependency path are ignored machine-local
tooling, not runtime prerequisites. `scripts/check.sh` is now executable; its
contents and gates are unchanged. The final full check passed:

- 77 Rust tests passed, 0 failed, 11 pre-existing opt-in/later-gate tests ignored.
- Eight native CLI integration tests plus output/clock unit coverage. Three text
  snapshots cover help, preview and explanation. JSON receipt equality uses the
  independent frozen fixture; stdout is one JSON object including errors.
- All-cell database comparisons preserve immutable bytes and operational state
  across preview, identity duplicate, rejected/conflict/waiting and read paths.
  Write-denial triggers prove preview does not even attempt fresh or alias writes;
  acceptance negative controls hit those same triggers.
- Read tests cover canonical and external IDs, semantic aliases, chain reads,
  original document snapshots, retained history after authority revocation,
  and corrupted-record detection. Path/config tests cover no overwrite, byte
  bounds, strict fields, source authorization, identity mismatch, symlinks,
  private permissions and competing owner locks.
- Existing real SQLite acceptance, cancellation, race, write-boundary, unknown
  outcome and recovery suites passed. No new PostgreSQL server run was claimed.
- Formatting, warnings-denied Clippy, all-target/all-feature tests, no-default
  compilation, resolved dependency/source boundary checks, and Python/Node audits
  passed. All 99 frozen files, 60 hash vectors, 25 immutable accepted records and
  29 manifest members are preserved. `ledgerlab-core`, migrations, contracts,
  fixtures, design sources and ADRs are unchanged.

Initial development runs found the input-document parser cannot read retained
snapshot documents, WAL checkpointing invalidates raw database-file comparisons,
and three Clippy style findings. The read path now verifies retained canonical
hashes without compiling current policy; tests compare logical cells; style
findings are fixed. No frozen oracle was changed or assertion weakened.

## Compromises and integration notes

- Phase 2 is absent on this base. The CLI passes generic event bytes to the
  existing facade and displays its results. Later linked outcomes, supplier
  authority, paid tools and platform funding remain explicit integration work.
  The running BYOK demo is the first generation slice, not the $1.20 future chain.
- `ledger.yaml` is strict JSON syntax, a YAML 1.2 subset, with schema `ledger/v1`.
  This bounded local profile documents its host-selector fields and rejects
  unsupported settings. No general YAML parser or new dependency version was
  introduced; the lockfile adds only direct edges to already pinned packages.
- Synthetic initialization embeds frozen demo seed documents/rows, uses the
  existing migrations and private provisioning writes, and preserves the fixed
  demo namespace. No public raw action append or real assent API was added.
- `Ledger::preview` returns a separate `PreviewResult` from shared coordinator
  preparation. It skips all writes and rolls back; it never exposes a receipt.
  It still takes the existing locks. Opening/checkpointing can touch SQLite files,
  so this is not a physically read-only/offline preview. Both library backends
  are wired; new executable preview evidence is SQLite-only.
- `LocalLedger::explain` is local filesystem-authorized and SQLite-only. It reads
  retained bodies from one snapshot and checks hashes, displayed membership and
  receipt links. It is not a complete ledger verifier/replay or remote auth API.
  PostgreSQL remains available through the existing library API.
- No signal-drain handler or new platform certification. Interrupted/lost output
  requires retrying the same identity; it never implies rollback. Unix file modes
  are covered; Windows ACL certification remains a later platform gate.
- Phase 2 integration should extend shared preparation, reviewed record families,
  and the stored-read projection, then add linked demo inputs. Preserve CLI→facade
  ownership and the existing economics; no new transport-side economic types.
