# Product Phase 2 integration candidate

Status: PASS for the bounded local integration checks; ready for independent
combined-candidate review. This
report supersedes historical lane status only for this combined tree. The pinned
`ROADMAP.md` and Phase 1 freeze records remain immutable historical evidence.
Independent review of the combined candidate remains the owner's next gate.

## Exact inputs and integration

Base: `3e56ff172d4dba326272c66c85512c8a481f7a52`.
Branch: `codex/p2-reviewed-integration`.

Applied locally in the authorized order, without intermediate commits:

| Lane | Reviewed scoped commit |
|---|---|
| Semantic | `73678106f53c5ff857928c17264e1d056dd4eff5` (first-parent delta) |
| Outbox/reliability | `f95773677716776ef7006ac7ce7ac25b9c2c5353` |
| CLI/DX | `7202364fae87765a95baa96d928eb9d085e0fa45` |

The 61-file union applied without unresolved conflicts. The only overlapping file
was `crates/ledgerlab-cli/tests/local.rs`: its combined bytes equal the CLI lane
exactly except for retaining the reliability lane's migration-count expectation
of three. A byte-for-byte provenance check confirms every other imported file
matches its reviewed source. No production edits beyond the reviewed inputs were
needed. Shared Cargo manifests, lockfile and toolchain pins are unchanged.

The candidate combines approved pure outcome semantics, the frozen canonical
contract, outbox fencing/reconciliation/quarantine, explicit schema upgrades,
and local CLI presentation/configuration/onboarding. The persistence coordinator
still handles the bounded original economic path. Outcome/correction persistence,
production historical decoding and CLI submission of those new paths remain
Phase 3 or later work; this candidate does not implement them.

## Combined validation

All results below are from this combined source tree, not inherited lane counts.
Rust 1.98.1, cached locked dependencies, local audit packages, macOS ARM64,
linked SQLite 3.51.3. No dependencies or images were downloaded.

| Gate | Result |
|---|---|
| `sh scripts/check.sh` | PASS: 128 Rust tests; zero failed; 19 default ignored entries |
| Formatting, warnings-denied Clippy, no-default build | PASS |
| Production crate/dependency/source boundaries | PASS |
| Exact approved semantic comparison | PASS: one scalar/Unicode test and six outcome tests |
| Standalone driver/TLS proof | PASS: 16 parent tests; one framework-ignored child exercised by parent; fmt/Clippy/no-default and 150-package audit pass |
| Real PostgreSQL 17.11 | PASS: 17 tests; zero failed/ignored; 676.43 seconds |
| Real PostgreSQL 18.6 | PASS: 17 tests; zero failed/ignored; 679.12 seconds |
| Source provenance, frozen bytes, original migrations | PASS |

The 128 tests comprise 46 facade/store/outbox tests, 18 CLI tests (two unit and
16 integration), 50 core tests, and 14 testkit tests. Default ignored entries
are 17 explicit PostgreSQL tests plus two deferred process-durable independent
fake-destination gates. The PostgreSQL entries are executed separately below;
the two durable-fake gates remain unimplemented and are not passing evidence.
The TLS proof’s one framework-ignored child is invoked by its passing parent.

The independent Python/Node reconstruction preserves all 159 registered frozen
files, including 99 original v1 assets; the freeze registry itself and all four
original backend 0001/0002 migration files are also unchanged. Original first-slice
receipts, 60 hash vectors, 25 accepted immutable rows, 29 manifest members and
80 atoms remain exact. No fixture writer or regeneration was run.

Frozen outcome profile `2-candidate.4`: 27 record kinds, 24 histories, 1,450
records, 43 decisions, 23 retained original Evaluations and 218 identity mappings.
The packaged audit passes 49 fully rehashed semantic attacks (3,570 records/109
decisions), five scalar attacks (205 records/five decisions), 272 negative
assertions, four freeze-metadata attacks, 217 scalar cases and 38 field boundaries.
Python, Node and approved Rust agree across 1,112,064 Unicode scalar values,
2,224,128 text/source checks per runtime. Accepted U+FEFF evidence adds 41 records
and one decision. Exact approved-core comparison verifies 24 complete typed
Evaluation roundtrips and 44 decisions, original IDs/fields/vectors, original
receipt retries, document reuse/rejection, 11 deadline/ordering cases and the
86-attempt/23-history semantic oracle.

Both real PostgreSQL runs used fresh localhost-only containers from cached
17.11/18.6 aarch64 images with synthetic passwords and private test CAs. Each
major passed the 83-case acceptance harness, cancellation/real contention,
1,001-intention controls, eleven delivery-recovery histories, quarantine,
cross-page dependencies and rollback, storage guards, 12 driver commit-cut
trials and 12 production-supervisor transport-cut trials. Both final upgrade
tests ran against this exact combined code. The SQLite equivalent gates are
in the 128-test offline run.

## Upgrade, rollback and unknown-outcome scope

Explicit owner maintenance upgrades exact backend schema 1/2 to schema 3 while
logical schema stays 1. Normal open refuses legacy stores and never migrates.
Tests build populated legacy databases from original migrations plus frozen
independent seed/accepted rows; they do not simulate age by downgrading a new DB.
Each backend runs four populated upgrade histories: schema 1/2 crossed with
ordinary acknowledgement/lost application acknowledgement after durable commit.

Coverage includes wrong-store refusal; admission/dispatch/lease fencing;
checksum rejection; partial-DDL failure rollback; exact retained physical rows,
canonical blobs and receipts; migration-history/version atomicity; reopen and
idempotent owner retry; retained nonzero dispatcher revision; permanent rejected
delivery and attempt-count guards; and original acceptance receipt retry without
duplicate economics. SQLite additionally checks exclusive directory ownership;
PostgreSQL checks runtime-role refusal, restrictive operational grants and
malformed metadata. Dispatch remains held after upgrade.

The upgrade lost-ack injection is application-boundary evidence. It is not a
physical network cut during migration COMMIT. Separate acceptance/store suites
and the driver proof cover real transport cuts, ambiguous acceptance commits,
cancellation, rollback/discard and next-borrower cleanup. Upgrade APIs require
stopped application processes, a verified restorable backup and durable owner
fences; the API does not create those prerequisites or certify power loss.

## Reproduction and evidence

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
export PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps
sh scripts/check.sh
python3 scripts/contract_checks/v2_candidate/compare_semantic.py --source /Users/stephenkall/Documents/Codex/2026-09-21/ledger-lab-v0-semantic-freeze
# Supply synthetic localhost LEDGERLAB_PG_TEST_* variables for each major.
cargo test -p ledgerlab --lib --all-features --locked --offline -- --ignored --test-threads=1 --nocapture
sh crates/ledgerlab/src/store/postgres/proof/run.sh
```

Logs are retained locally under ignored `work/validation/`: `combined-offline.log`,
`semantic-comparison.log`, `pg17-all.log`, `pg18-all.log`, `driver-tls.log` and
`source-provenance.json`. Databases, synthetic credentials/certificates, local
toolchains and operational evidence are not committed. Initial PostgreSQL/TLS
attempts failed because the sandbox denied localhost sockets; approved reruns use
local socket access. Initial fixture startup failed because the VM did not mount
the workspace; copying synthetic fixtures into disposable containers corrected
that setup issue without code changes.

## Remaining gates and scope

- Fresh independent review of this exact combined candidate.
- Two process-durable independent fake-destination restart gates remain deferred.
  In-memory destination histories do not establish destination process durability.
- General backup/restore orchestration, arbitrary process/power-loss behavior,
  native release matrix, MSRV certification and production readiness remain open.
- Phase 3 outcome persistence, authority/reservation locking for that new path,
  historical decoding/bridging, and later CLI/finance-adapter work remain deferred.

The two disposable integration containers and their volumes were removed. The
pre-existing PostgreSQL containers were left unchanged, and the test VM was
returned to its original stopped state.

No main merge, push, tag, release, publication, deployment, service registration
or paid work was performed. Total spend: $0.
