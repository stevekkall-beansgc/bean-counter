# ADR 022: PostgreSQL uses tokio-postgres with an explicit Rustls connector

Status: **selected and source/code-reviewed for the bounded executable proof**,
20 September 2026. Authorized driver-gate amendment; integration-owner review
and real PostgreSQL conformance remain required before adapter integration exits.
This is not a claim of independent human review or production certification.

Amends the PostgreSQL driver choice in ADR 004 and clarifies the PostgreSQL TLS
implementation in ADR 019 / detailed design §§4, 11, 14 and 27. SQLx remains the
SQLite choice subject to its own gates. All economics, canonical contracts,
locks, durability requirements, TLS trust modes and environment restrictions
are unchanged. The earlier ADRs and their frozen index remain byte-identical;
this standalone amendment supersedes only their PostgreSQL driver choice. The
Phase 0 document inventory check now requires the exact frozen ADR set plus
this explicitly authorized ADR, instead of hard-coding a count of 21. All
frozen-digest and fixture checks remain unchanged. No frozen fixture needs amendment: this changes infrastructure,
not a byte, identifier, record count, amount or authority decision.

## Decision and alternatives

Use **tokio-postgres 0.7.18**, **tokio-postgres-rustls 0.14.0**, and an
application-built **rustls 0.23.45 ClientConfig**, selecting Ring explicitly.
Use the connector's supported `MakeRustlsConnect::new(ClientConfig)` API.
Disable default features and the connector's native-certs/webpki-roots helpers.
The application's explicit Public branch alone loads `webpki-roots 1.0.9`.

| Option | Contract / maintenance / complexity | Decision |
|---|---|---|
| Direct tokio-postgres + supported Rustls connector | Caller owns exact roots and verification; typed parameters, tracked SERIALIZABLE transactions and cancellation APIs. Two database driver APIs must be maintained; PG pooling/migrations are no longer supplied by SQLx. Small separate connector adds X.509 parsing for channel binding. | Select for PostgreSQL. No ORM, pool or generic driver layer needed for this proof. |
| Patch/fork SQLx 0.9.0 | Must fix both additive roots and ambient configuration. A local fork also owns upstream security updates, feature combinations, rebases and connector visibility. No supported upstream custom PG connector was found. | Reject for this gate: larger sustained ownership than using supported APIs. Reconsider an upstream released solution later, with a new review. |
| SQLx 0.8.6 or private feature recipes | Predecessor has the same additive-root problem; private features are not a maintained contract. | Reject. |
| Broaden PEM trust, native TLS, proxy workaround, Prefer | Changes the required security/environment boundary or permits plaintext. | Reject. The contract is not weakened. |

The Rust-Postgres release history shows 0.7.18 on 12 June 2026, 0.7.17 on
30 March, and 0.7.16 on 14 January. The connector's 0.14.0 was published
21 May 2026. Both are released open-source packages (driver MIT OR Apache-2.0,
connector MIT), with source and tests available. This supports selecting a
maintained free path; release activity alone is not a security guarantee.
[Driver releases](https://docs.rs/crate/tokio-postgres/0.7.18),
[connector release/source](https://docs.rs/crate/tokio-postgres-rustls/0.14.0),
[connector repository](https://github.com/jbg/tokio-postgres-rustls).

## Trust and configuration boundary

`proof/tls.rs` constructs an empty RootCertStore. PemOnly adds only certificates
parsed from the explicitly supplied, bounded in-memory PEM. Empty, malformed,
unparseable or oversized inputs fail. Public loads exactly the pinned bundled
WebPKI anchor set. Neither branch reads a file, HOME, Keychain, OpenSSL variables
or a platform certificate store. The dependency graph contains no native-tls,
OpenSSL, rustls-native-certs or security-framework package. Public roots may be
linked in the same binary without belonging to a PemOnly connection's store.

The standard Rustls verifier is retained; no dangerous verifier or hostname
bypass is used. The connector clones the supplied ClientConfig and converts the
configured hostname through `ServerName` into tokio-rustls. All connections use
`SslMode::Require` plus that verified connector. Require alone is insufficient
without the verifier; the combination implements the external verify-full
contract. A server's `N` answer to SSLRequest is an error with no startup fallback.
[Supported TLS API](https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/),
[connector source](https://docs.rs/crate/tokio-postgres-rustls/0.14.0/source/src/lib.rs).

`proof/connect.rs` uses Config::new followed by explicit TCP host, port, user,
password, database, application name, fixed startup options and three-second
connect timeout. No URL/string parsing or ambient PG input is used. The source
constructor only initializes fields; it does not consult the environment.
Although connect_raw has a whoami fallback for an absent user, the proof rejects
an absent user and supplies it explicitly. Host validation rejects Unix paths,
empty/default hosts and combined/query-bearing hosts. A whole connect deadline
also bounds DNS/TLS/authentication. The synthetic child-process test verifies
actual startup with hostile PG*, HOME and SSL_CERT_* settings.

The proof does not implement the public configuration resolver. Integration must
map only resolved allowlisted Ledger settings into these fields and reject URL
query settings conflicting with TLS/timeouts. It must not pass unfiltered URLs
to a driver parser. Explicit insecure-local, if later implemented, remains a
separate sandbox+loopback-only profile; this proof offers only verified TLS.

Root membership is proved on the exact root-store builder used by the connector:
PemOnly's anchors equal precisely the supplied private CA, have no intersection
with any pinned public anchor, and two supplied CAs produce exactly two anchors.
This is the authorized deterministic equivalent of using a public-CA-issued
server certificate. It does not claim a real public-CA handshake. Wrong names,
expired leaves, expired intermediate CAs and unrelated issuers are rejected.
Rustls treats supplied root certificates as trust anchors; anchor self-signature
and validity intervals are not an independent root-expiry policy. This proof's
expiry claims concern the verified leaf/intermediate chain. Operators control
which anchors are supplied; no extra root-expiry promise is introduced here.

## Transactions, cancellation and unknown outcomes

The bounded executable proof starts `START TRANSACTION ISOLATION LEVEL
SERIALIZABLE` through TransactionBuilder, sends an INT8 parameter with a fixed
parameterized query, decodes an i64 and commits only after successful work.
It exercises the real driver's protocol encoder/decoder and tracked transaction
API against a synthetic TLS peer. The peer never executes SQL or stores data.

Source review and wire tests establish:

- TransactionBuilder has a cancellation cleanup guard while BEGIN is pending.
  Transaction drop queues ROLLBACK. Explicit rollback awaits its response.
  Queued cleanup alone is not proof that a real server rolled back.
- Dropping a normal Tokio JoinHandle would detach the connection task. Session
  instead aborts its driver on drop; explicit discard aborts and joins it.
  The proof consumes/discards each session and has no pool or next borrower.
- CancelToken reuses the saved SSL mode, address and hostname. Tests transmit a
  synthetic backend cancellation key only after verified TLS, and reject a
  plaintext or unrelated-CA cancellation peer. A successful cancel send is
  **not** confirmation of query cancellation, rollback or commit outcome.
- The driver's commit marks its guard done before awaiting COMMIT. Loss of the
  response or cancellation of that future must not be interpreted as rollback.
  The proof returns Unknown for transport errors and commit drain deadlines.
  Only explicit 40001/40P01 commit errors are classified as retryable aborts;
  other errors remain conservatively unknown for this bounded example.

The synthetic peer proves message order, typed parameter bytes, explicit/drop
rollback traffic, pre-commit cancellation discard, acknowledged commit, lost
reply classification and the drain timeout. It cannot prove SERIALIZABLE
isolation, durable commit, actual server cancellation, lock release or pooling.
The driver's commit API returns `()`; the integration must not commit an already
failed transaction and mistake an aborted transaction's command completion for
an accepted decision. Stop work on statement failure and rollback/discard.

Integration must keep the coordinator's pre-commit cancellation check and shield
started commits from request cancellation for the bounded drain. If a task is
aborted during commit, its supervising coordinator must retain OutcomeUnknown.
The proof does not implement that supervisor, retry policy or primary lookup.
No automatic retry is performed. Resolve through the same original identity and
ordered locks on the authoritative primary; an absent row alone proves nothing
while the old transaction may still be active.

## Exact dependencies and source/security review

Pinned direct proof dependencies:

| Package | Version | Explicit features |
|---|---|---|
| tokio-postgres | 0.7.18 | runtime, defaults off |
| tokio-postgres-rustls | 0.14.0 | ring, defaults off |
| rustls | 0.23.45 | ring, std, tls12, defaults off |
| tokio | 1.53.1 | rt, macros, time, net, io-util, sync, defaults off |
| webpki-roots | 1.0.9 | defaults off |
| tokio-rustls (test) | 0.26.5 | ring, tls12, defaults off |
| rcgen (test) | 0.14.10 | ring, pem, defaults off |

Important resolved transitive versions: postgres-protocol 0.6.12,
postgres-types 0.2.14, rustls-webpki 0.103.15, rustls-pki-types 1.15.1,
Ring 0.17.14, x509-cert 0.2.5, sha2 0.11.0, whoami 2.1.3.
The committed standalone lockfile fixes the entire graph with registry
checksums. `proof/source_audit.py` records versions, licenses, active features
and SHA-256 of the inspected upstream configuration, connector, transaction,
cancellation and verification files. It also rejects prohibited TLS packages
and first-party environment/trust bypasses. Audit assertions deliberately force
review on dependency changes; they are not a substitute for code review.

Selected upstream source fingerprints (registry package files):

| Package/file | SHA-256 |
|---|---|
| tokio-postgres 0.7.18 / src/config.rs | `d06bffc37f9aa290ca72961508dcbcfdcab6d05e76fbf52fe450a268a0043b89` |
| tokio-postgres 0.7.18 / src/connect_tls.rs | `d407bd286a40348570c887139eb149340f835ae96f624668c6b04a99ccc95b00` |
| tokio-postgres-rustls 0.14.0 / src/lib.rs | `40669c92184bbbc438cbe56e4448c978916d959b8eeb6fc21daeb3d0fffd9d49` |
| rustls 0.23.45 / src/client/builder.rs | `aa2df5ce333599e3acd9cbf6f7758eb5444f6d9cf4b66c0a84130fc7e6a55f4b` |

A dated manual advisory review used RustSec advisory-db commit
`d5c17953a895cf19e8d3ce66eaa42b6fcfe1fb16` (19 September 2026), fetched from the
public repository. All 51 advisories matching package names in the standalone
lockfile (including both locked base64 versions) were checked against its patched/unaffected/withdrawn ranges; no active
affected version was identified. In particular, the selected versions meet the
patched thresholds for tokio-postgres RUSTSEC-2026-0178, postgres-protocol
RUSTSEC-2026-0179/0180, Rustls RUSTSEC-2026-0285 and WebPKI
RUSTSEC-2026-0049/0098/0099/0104. Ring 0.17.14 is beyond the 0.17.12 fix;
the blanket unmaintained notice was withdrawn and the older-version notice
excludes >=0.17. This was a manual range review, **not cargo-audit execution or a
complete cryptographic/security assessment**. Repeat advisory checks at
integration/release. [RustSec database](https://github.com/RustSec/advisory-db/tree/d5c17953a895cf19e8d3ce66eaa42b6fcfe1fb16).

Rust 1.98.1 on macOS 26 / Apple Silicon compiled and exercised this proof. That
is a development observation, not a selected/tested MSRV. No native Linux/Windows
or read-only-root container certification is implied.

## Review conclusion and integration gates

The supported API and executable evidence close the **driver/TLS feasibility
blocker**. Self-review explicitly checked additive-root regressions, Prefer,
ambient user/options, Unix-host bypass, detached connection tasks, cancellation
credentials, commit ambiguity and the distinction between wire and store tests.
No TLS/environment weakening is needed. SQLite remains on SQLx.

The proof lives only under `crates/ledgerlab/src/store/postgres/proof/`, outside
the production workspace graph. It must be wired deliberately by the integration
owner, not treated as a completed PostgresStore. Remaining gates include real
PG18/17 authentication, primary/durability/role checks, migrations, ordered locks,
per-attempt deadline clamps, bounded pooling with no leaked transaction, all
cancellation positions and actual ambiguity resolution, reopen/readback, the
27 writes/54 failure positions, exact 25 new immutable rows/29 manifest
members/80 atoms, and SQLite parity. Public trust handshake and the full native
platform/read-only-root matrix remain release gates. See
`POSTGRES-PHASE1-INTEGRATION.md` for commands and integration steps.
