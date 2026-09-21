# PostgreSQL Phase 1 — driver/TLS architecture gate resolved

Status: **compliant driver path selected; bounded executable proof passes**.
A completed PostgreSQL adapter, real-store conformance and SQLite parity are
not claimed. [ADR 022](docs/adr/0022-postgres-driver-rustls.md) amends ADR 004/019
for PostgreSQL. The original SQLx diagnostic evidence is retained below.

## Selected path and validation

Use tokio-postgres **0.7.18** with tokio-postgres-rustls **0.14.0**, Rustls
**0.23.45**, explicit Ring and explicit per-connection roots. SQLx stays available
for SQLite. The PG connector accepts a supported custom ClientConfig, so no
fork, native roots, Prefer/plaintext fallback or ambient PG configuration is
needed. Exact direct/transitive versions, source review, alternative comparison
and dated RustSec review are in ADR 022 and the standalone proof lockfile.

The proof files are under `crates/ledgerlab/src/store/postgres/proof/`.
Sixteen parent tests pass; an additional ambient-environment child executes
inside its parent test (listed as ignored in the outer run). Evidence includes:

- Exact supplied-anchor membership and exclusion of **every** bundled public
  anchor in PEM mode, using the same builder passed into Rustls. This is the
  deterministic equivalent requested for public-root exclusion; a public-CA
  server handshake is not claimed.
- Private CA success; wrong host, expired leaf/intermediate CA, unrelated issuer,
  malformed/empty/oversized PEM and plaintext refusal.
- Actual startup immunity to synthetic PG*, HOME and SSL_CERT_* variables.
- SERIALIZABLE begin message, typed INT8 parameter encoding/decoding, explicit
  and drop rollback, cancellation/discard and connection-task termination.
- Verified TLS cancellation request; refusal of plaintext and unrelated-CA
  cancellation peers before any cancellation key is sent.
- Acknowledged commit, explicit serialization/deadlock abort classification,
  lost commit response and commit deadline returning Unknown.

All network tests use in-memory generated certificates, synthetic credentials
and bounded loopback peers. These peers implement selected PostgreSQL protocol
messages; **they do not execute SQL, authenticate with SCRAM, or persist data**.
The tests prove driver/TLS behavior, not server isolation or durability.

Formatting, Clippy with `-D warnings`, proof no-default check, dependency/source
audit, and `sh scripts/check.sh` pass on Rust 1.98.1 / macOS 26 Apple Silicon.
The workspace remains the Phase 0 scaffold, so its zero product tests are not
adapter evidence. The first loopback run was sandbox-blocked; the authorized
rerun passed. The proof audit records 150 resolved packages including the proof
package and target/test dependencies. Production Cargo files are untouched. The document inventory check is narrowly
updated to require the frozen ADR set plus ADR 022; the freeze manifest and
all earlier ADR bytes remain unchanged.

## Reproduce and integrate

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
sh crates/ledgerlab/src/store/postgres/proof/run.sh
PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps \
  sh scripts/check.sh
```

The proof runner stages its pinned manifest/lockfile into ignored
`work/postgres-driver-proof` and runs offline. On a fresh machine, first copy
`proof/driver-proof.toml` and `proof/driver-proof.lock` there as `Cargo.toml` and
`Cargo.lock`, then fetch that manifest with Cargo using `--locked`. Run in an environment allowing 127.0.0.1 listeners. The source audit
writes a local versions/features/licenses/source-hash inventory; it is not a
vulnerability scanner. Historical `diagnostics/run.sh` still intentionally
ends with exit 2 for the unchanged SQLx blocker.

Integration owner steps:

1. Review ADR 022 and cherry-pick this commit onto the assigned Phase 1 base.
   This branch starts at diagnostic commit `530b3e9`; it changes no shared
   ports, production manifests/lockfile, core, SQLite or frozen contracts.
2. Resolve/pin the exact proof dependencies in the facade's production manifest,
   retaining only SQLite features for SQLx. Re-audit the **unified production**
   graph and advisory state; do not assume this standalone graph proves it.
3. Move/adapt the small PG proof modules behind the existing private concrete
   store boundary. Keep explicit resolved settings and per-mode roots. Select
   and prove a bounded PG pool separately; no pool is introduced here.
4. Adapt the agreed transaction port/GAT lifetime, typed parameterized SQL,
   ordered locks, migrations and runtime/primary/durability checks. Own PG
   migration checksums/history independently of SQLx's migration runner.
5. Preserve pre-commit cancellation and bounded commit drain at the coordinator.
   Abort/discard uncertain sessions, retain original identity and OutcomeUnknown,
   and resolve against the authoritative primary. Do not mistake CancelToken
   send, queued rollback or connection close for confirmation of rollback.
6. Run the complete real PG18 slice and failure/concurrency/cleanup suite, then
   PG17 and the native matrix before making parity or release claims.

No postgres/initdb/pg_ctl/psql executable was available on PATH during this
review; the diagnostic lane also recorded no running Docker daemon. This task
started no server, container, service or cloud resource. Real-PG validation was
optional for the driver proof and remains unexecuted. No production module is
wired, no migration is applied and no release/push was performed.

Still required: actual authentication, schema/migrations, fsync/full_page_writes,
synchronous_commit and primary/role checks, ordered-lock races, rollback/drop/
cancel/pool cleanup at every await, durable commit ambiguity resolution,
reopen/readback, all 27 writes/54 failure positions, exact 25 immutable rows/
29 manifest members/80 atoms, SQLite/PG parity, public-CA handshake, PG17/18 and
Linux/Mac/Windows/read-only-root gates.

---

## Historical SQLx gate report (unchanged evidence)

# PostgreSQL Phase 1 — stopped at the SQLx TLS gate

Status: **BLOCKED, not a completed PostgreSQL adapter.** Inspected and tested on
20 September 2026, macOS 26 / Apple Silicon, Rust 1.98.1. The assigned stop
condition and detailed design §14 require stopping when the driver cannot meet
PEM-only trust. No production PostgreSQL module, migration, shared port, facade
wiring, or workspace dependency change is included in this branch.

## Finding and exact evidence

SQLx 0.9.0's supported Rustls configurations do not express the required choice
between bundled public roots and *only* an operator-supplied PEM root set.
The registry reported 0.9.0 as the current release (`cargo info sqlx`). This is
about the selected driver interface, not a Rustls limitation.

The fetched, unmodified `sqlx-core-0.9.0/src/net/tls/tls_rustls.rs`:

- Line 140 initializes its certificate store by calling `import_root_certs()`.
- Lines 142–150 parse the supplied CA, then **add** each certificate to that
  existing store. Neither file nor in-memory PEM input replaces the store.
- Lines 211–213 import all bundled WebPKI roots when native roots are absent.
- Lines 216–230 import system roots when the native-roots feature is enabled.
- Lines 233–236 contain an empty-root implementation, but it has no supported
  public no-roots feature. The supported provider features enable a root set.
  The internal `_tls-rustls` switch alone does not select a crypto provider.

The complete supported Rustls feature comparison, from the fetched manifests:

| SQLx feature | Root selection in 0.9.0 | PEM-only contract |
|---|---|---|
| `tls-rustls-ring-webpki` | Bundled WebPKI, then supplied CA | Fails |
| `tls-rustls-aws-lc-rs` | Bundled WebPKI, then supplied CA | Fails |
| `tls-rustls-ring-native-roots` | System roots, then supplied CA | Fails; system trust also prohibited |
| `tls-rustls-ring`, `tls-rustls` | Alias to Ring/WebPKI | Fails |
| `tls-none` | No TLS | Cannot verify a remote connection |
| `tls-native-tls` | Different TLS implementation | Violates required Rustls/no-native-TLS boundary |

Enabling both native and bundled root features does not remove roots; native
roots take precedence in the 0.9.0 import function. Cargo feature unification
cannot subtract the bundled-root dependency. No custom TLS connector/root-store
setter is exposed by `PgConnectOptions` or the public `PgConnection` construction
API. `PgConnection::establish` is crate-private; its connection state and stream
are also crate-private. These were inspected directly, not inferred from a
failed network connection.

The official [0.8.6 Rustls source](https://raw.githubusercontent.com/launchbadge/sqlx/v0.8.6/sqlx-core/src/net/tls/tls_rustls.rs)
also imports WebPKI/system roots before adding custom certificates (lines
128–140 in that source). Its [feature aliases](https://docs.rs/crate/sqlx/0.8.6/features)
select the same root categories. Downgrading to that predecessor does not fix
the contract. It was source-inspected, not selected or claimed as a maintained
replacement. No unpublished/custom feature recipe or vendor patch was used.

There is a second configuration concern. In
`sqlx-postgres-0.9.0/src/options/mod.rs`, lines 56–99,
`new_without_pgpass` still reads ambient host/user/database/password, SSL root,
client certificate/key, SSL mode, app name, and startup options. The method only
bypasses the password file. `options()` at lines 436–452 appends to existing
startup options. Public setters do not clear ambient client certificate/key
fields. A synthetic subprocess test reproduces that behavior. Production must
not assume this constructor is environment-free. The
[official options documentation](https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgConnectOptions.html)
also documents ambient settings and the default `Prefer` mode.

Source fingerprints from the exact registry packages:

| File | SHA-256 |
|---|---|
| `sqlx-core-0.9.0/src/net/tls/tls_rustls.rs` | `498c9e5862118c79e773c7e20f1b7a5193d0e182c2294be89e3ec5f4b93c4cf0` |
| `sqlx-core-0.9.0/Cargo.toml` | `d62bb81e87b97948b8a24c75d5c17519fb183f1a837085d76ddf30f0ab9b65be` |
| `sqlx-postgres-0.9.0/src/options/mod.rs` | `1886a4cf46dfdf57d33c78403f9588c3e2f429641dcbd5837f4c8b29273fbc36` |

The diagnostic lockfile retains registry checksums. `source_audit.py` derives
additional file fingerprints, exact active features, and resolved versions
from locked Cargo metadata; its generated report stays under ignored `work/`.

## What was validated

The standalone diagnostic uses SQLx itself against a bounded loopback TLS peer
with certificates generated in memory. The peer implements only PostgreSQL's
SSLRequest and startup rejection. It is **not a database**. A distinctive
startup error after TLS proves the handshake succeeded without pretending that
a real PostgreSQL session or persistence test ran.

Seven parent tests passed, with one additional child test executed by the
ambient-settings parent (shown as ignored in the outer test list):

1. Supplied private CA permits a verified handshake and reaches startup.
2. Wrong hostname rejects before startup.
3. Expired certificate rejects before startup.
4. An unrelated supplied private CA rejects before startup.
5. Bundled public roots reject the generated private CA.
6. `VerifyFull` rejects a server answering `N` to SSLRequest, with no plaintext
   startup bytes sent afterward.
7. `new_without_pgpass` imports synthetic ambient PostgreSQL settings, including
   client/root certificate paths and startup options.

These probes do **not** prove PEM-only isolation. The source audit proves that
the selected implementation includes public roots; the test harness deliberately
reports the overall gate as blocked. No public-CA-issued server certificate,
ambient HOME/Keychain trust, real PostgreSQL authentication, or read-only-root
deployment was tested. The Ring/WebPKI dependency graph excludes native TLS and
native roots. No real credentials were read or sent.

Validation completed:

- Diagnostic formatting and Clippy with `-D warnings`: pass.
- Diagnostic compile includes SQLx's tracked custom `BEGIN ISOLATION LEVEL
  SERIALIZABLE` / rollback signature: pass **at compile time only**.
- `source_audit.py`: **exit 2, expected hard gate**, after reproducing additive
  trust and supported-feature evidence. Exit 2 is not product conformance.
- `sh scripts/check.sh`: pass with the existing Python document dependencies
  supplied through `PYTHONPATH`. Includes workspace fmt/test/Clippy/no-default
  checks, source/dependency boundaries, and frozen-contract checks. The scaffold
  has zero product Rust tests; its pass does not establish store behavior.

The first diagnostic run was blocked from opening loopback sockets by the
execution sandbox; the bounded loopback rerun was authorized and completed.
The initial workspace document check lacked `jsonschema`; using the already
available document-check dependency directory resolved that tooling issue.

## Dependencies and reproduction

Only the diagnostic harness has dependencies. SQLx 0.9.0 defaults are disabled;
requested features are `runtime-tokio`, `tls-rustls-ring-webpki`, `postgres`.
Direct test dependencies are Tokio 1.53.1, Rustls 0.23.45 (Ring, std, TLS 1.2),
tokio-rustls 0.26.5, and rcgen 0.14.10 (Ring, PEM). Resolved TLS dependencies
include rustls-webpki 0.103.15, webpki-roots 1.0.9, Ring 0.17.14. All are free
local test dependencies and are actually exercised. SQLx's transitive core
features include `any`, but no Any API is used; MySQL/SQLite are absent from the
active diagnostic graph. No production dependency or MSRV is selected here.

The manifest template and locked graph live under
`crates/ledgerlab/src/store/postgres/diagnostics/`. The runner materializes an
unpublished test harness in ignored `work/postgres-driver-gate`, preserving the
four-member production/testkit workspace and the integration owner's root
manifest/lockfile. After the first dependency fetch, validation is offline.

```sh
. /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0/work/toolchain/activate.sh
export RUSTUP_TOOLCHAIN=stable
# This installed channel reports exactly rustc 1.98.1; the named 1.98.1 rustup
# alias is absent and otherwise attempts a download. No toolchain files changed.
rustc --version
sh crates/ledgerlab/src/store/postgres/diagnostics/run.sh
# Expected final status: 2 (BLOCKED), after seven passing parent tests + child.

PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps \
  sh scripts/check.sh
```

On a new machine, stage the diagnostic manifest/lockfile as `run.sh` does and
run `cargo fetch --manifest-path work/postgres-driver-gate/Cargo.toml --locked`
before the offline runner. The network is only for free package retrieval;
all handshake probes bind loopback and use synthetic data.

## Server availability and integration seams

No `postgres`, `initdb`, `pg_ctl`, or `psql` was on PATH. No Homebrew PostgreSQL
installation or Postgres.app was present. Docker CLI and a Colima context exist,
but an authorized engine check reported that the daemon was not running. No
server was started, container pulled, system service changed, cloud resource
created, or paid infrastructure used. PG18/17 real-store validation is blocked.

There is no migration to apply and no adapter to wire into the coordinator yet.
The SQLite lane proposed crate-private `Scope`, `CanonicalRecord`, typed
`JournalRecord`, seed/head rows and GAT transaction ports; this branch does not
redefine or change them. Typed runtime parameterized queries remain the planned
PG query strategy once a conforming driver is selected.

Unexecuted gates include schema/migration execution, ordered-lock races,
durability settings/role checks, rollback/drop/cancellation/discard behavior,
commit ambiguity resolution, reopen/readback, all 27 writes/54 failure
positions, the exact 25 immutable rows/29 manifest members/80-atom result,
SQLite/PostgreSQL parity, PG17/18, and the native platform matrix.

Recommended next decision: review an ADR choosing either a maintained SQLx
change exposing an explicit replace-only root store and environment-free
connection options, or a different PostgreSQL driver with a supported custom
Rustls connector. Assess ownership/update burden before approving a maintained
SQLx patch. A Rustls bypass, native trust switch, TLS proxy workaround, or
silently broadened PEM trust does not close this gate. After that decision,
resume the original persistence slice and prove it on a real local primary.
