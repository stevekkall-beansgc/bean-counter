# Ledger Lab: platform, cloud and hardware neutrality

Founder review · September 20, 2026 · Planning only; no product implementation or platform certification was performed

This review narrows the operating and distribution promises in the [Rust foundation plan](LEDGER-LAB-RUST-FOUNDATION-PLAN.md) and [adoption review](LEDGER-LAB-ADOPTION-COST-REVIEW.md). It preserves the atomic economic engine, both databases, the three production crates, local development without a hosted service, and the fake export adapter. Its support matrix and deployment contract supersede less specific platform language in those reports.

## 1. Verdict: portable design, incomplete operating contract

**Approve the direction with the concrete changes below. The plans are not yet sufficient to call the product cloud-neutral or hardware-portable.** Rust, PostgreSQL and an OCI image make that achievable; none independently proves it. The founder's Mac should be one development environment, never the reference authority for economic behavior or production operation.

The important hidden workstation assumptions are:

| Existing assumption | Required correction |
|---|---|
| Commands run from a writable project containing `.ledger/`. | Keep that for local development. A PostgreSQL server must run with a read-only root filesystem, explicit configuration and no writable project or home directory. |
| A local Unix-socket PostgreSQL URL is the first migration example. | Make TCP the portable example. Label sockets as an optional Unix convenience. |
| `ledger dev` generates local credentials and launches a child app/browser. | Separate development lifecycle from `ledger serve`. Production gets stable operator-provisioned identities and secrets; no browser, child process or per-replica identity generation. |
| “Linux and macOS” is a release matrix. | Name CPU, operating-system reference, backend coverage and artifact. State exactly what is untested. |
| npm is optional, but the headline quickstart depends on it. | Make the native archive the primary route; an npm launcher is convenience work that can be deferred. |
| Installing on the founder's Mac establishes install quality. | Test a downloaded release on a clean account. Separately record runtime conformance and operating-system download/security friction. |
| A container suggests PostgreSQL is mandatory. | Storage topology determines safety. A single container with appropriate persistent storage can use SQLite. |
| A PostgreSQL connection makes every cloud deployment viable. | Process lifetime, migration ownership, outbox execution, shutdown and connection budgets also matter. |

No platform-specific API belongs in the evaluator. No Keychain, launchd, Apple filesystem behavior, GPU, paid model endpoint, proprietary cloud identity service or company-operated control plane is required. Platform-specific install instructions and operating-system boundary code are acceptable. A single foreground service supervised by the operator is the common production model.

## 2. Exact v0 support policy

“Certified” below is a **proposed release gate**, not a certification already earned. It means the published artifact passes the defined native conformance, lifecycle and recovery checks on the named reference. It is not a support SLA or a promise that every host, filesystem, cloud or PostgreSQL proxy works.

| Target / environment | Proposed v0 status | Scope and evidence required |
|---|---|---|
| `x86_64-unknown-linux-gnu`; Ubuntu 22.04 reference, Ubuntu 24.04 compatibility smoke | Certified after gates | Native CLI, embedded Rust integration, HTTP server, SQLite single owner, PostgreSQL 17/18, install/upgrade/export/restore tests. Build on 22.04; inspect actual linked ABI requirements. |
| `aarch64-unknown-linux-gnu`; same Ubuntu references | Certified after gates | Same functional promises, run natively on ARM64. No “cross-compiles successfully” substitute. |
| `aarch64-apple-darwin`; macOS 15 | Certified local development after gates | Native CLI, embedded use and local HTTP/inspector; SQLite full suite, PostgreSQL TCP compatibility against a tested server. No macOS production-host certification or daemon installer. Download onboarding is a separate gate. |
| OCI `linux/amd64` and `linux/arm64` | Certified artifacts after gates | One versioned multi-platform index, two natively tested images, pinned Debian 12 slim base digests, non-root/read-only-root checks. Reference hosts are the tested Linux environments; this is not certification of every orchestrator. |
| `x86_64-pc-windows-msvc`; Windows Server 2022 CI | Build-only | Release-gated compilation of all production crates, including database/TLS dependencies. No downloadable Windows binary or runtime-support claim. |
| Other Linux distributions with a compatible GNU ABI; macOS 26; Intel Mac; Windows ARM64; Linux musl | Source / best effort | Public source and portable interfaces; no v0 binary, release gate or operational guarantee unless explicitly listed above. The Linux OCI artifact is an alternative on a compatible Linux host. |
| 32-bit CPUs, big-endian machines, browser/WASM/edge isolates | Unsupported v0 engine | Fixed-width wire data still matters, but do not promise build or runtime support. A compatible HTTP client may call a supported engine. |

The Linux build reference is intentionally older than the compatibility smoke reference. Set the intended GNU ABI floor to the Ubuntu 22.04 toolchain environment, and verify it from each finished artifact rather than assuming Rust's target triple establishes it. Do not advertise a universal Linux executable, Alpine compatibility or an untested minimum kernel. Record the actual compiler, glibc dependencies and host kernels in release evidence. The OCI base is an explicit additional runtime test, not a reason to extend native-archive promises to Debian generally.

**Three binary targets are manageable only with one shared test suite and this narrow support policy.** They are too broad if “support” means native installers, service managers, every OS release, every database version and every cloud combination. Maintain one GNU Linux build family, one OCI base, one Mac reference and two PostgreSQL majors. If native ARM execution cannot be obtained, withhold ARM certification; do not relabel an emulated build as tested production support.

Rust currently lists the selected native targets and Windows MSVC among its stronger supported platforms, but those tiers concern Rust itself. They do not validate Ledger Lab's migrations, locking, installation, TLS, process handling or recovery. [Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html)

## 3. Release artifacts: small, complete and independently usable

The following names are proposed patterns, not existing downloads or registered package names. Each public version should provide:

| Artifact | v0 requirement |
|---|---|
| Three native archives | `ledger-v<V>-x86_64-unknown-linux-gnu.tar.gz`, `ledger-v<V>-aarch64-unknown-linux-gnu.tar.gz`, `ledger-v<V>-aarch64-apple-darwin.tar.gz`. Contain the executable, license/notices, minimal README and the versioned public schema or a documented embedded schema-export command. Bundle inspector assets if the inspector ships. No Node requirement to run the engine. |
| OCI distribution | One immutable release tag/index for `linux/amd64` and `linux/arm64`, with index and platform digests documented. Publish a public image; support operators mirroring it. Deployments should pin digests. No Docker Desktop or Docker Hub account requirement. |
| Source release | Source archive, lockfile, pinned Rust toolchain, build instructions and dependency/license inventory. A network-free build requires a separately prepared dependency cache/vendor bundle; source alone does not establish an offline build. |
| Checksums and release manifest | SHA-256 for archives and source; image digests; commit, toolchain, target, dependency/backend versions and test-evidence references. Checksum verification detects substitution only when the expected checksum comes through a trusted channel. |
| SBOM | SPDX JSON per native artifact and per platform image. Include bundled SQLite and shipped native/base-image packages, not just Rust crates. Tie each SBOM to its artifact digest. |
| Build provenance | Signed CI provenance attestations for binaries/images and downloadable verification material. Verification instructions must identify the intended repository/workflow identity, not merely accept any valid signature. This is feasible without buying a signing certificate. |
| TypeScript client | Retain the thin HTTP SDK/types from the adoption plan. It is separate from distributing the Rust executable. |
| npm binary launcher | Optional for v0. If provided, pin exact platform artifacts and verify trusted release digests, fail clearly on unsupported targets, and avoid compiling Rust as a silent fallback. Do not make the primary quickstart depend on it. |
| Installers / package managers | Defer `.deb`, `.rpm`, Homebrew tap, Windows MSI, OS service installers and universal Mac binaries. No auto-updater in v0. |

An OCI index describes platform-specific manifests; publishing an index does not demonstrate either image executes correctly. Both images must run their own release acceptance checks. [OCI image index specification](https://github.com/opencontainers/image-spec/blob/main/image-index.md)

GitHub provides public-repository attestations using Sigstore, including build provenance and SBOM support. This supplies artifact-origin evidence, not a claim of reproducible builds or an audit of application correctness. Keep the build from a reviewed commit, pin workflow dependencies, and retain the evidence alongside the release. [GitHub artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations), [offline verification](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations-offline)

Mac Developer ID signing/notarization is a separate distribution concern. CI provenance does not satisfy Gatekeeper. Under a strict $0 incremental budget, publish an honestly labeled unsigned artifact and a source-build path, test the actual supported launch experience, and do not promise frictionless downloaded installation. Never make disabling operating-system security the five-minute procedure. Paid signing can improve that experience later without changing the engine. [Apple Developer ID](https://developer.apple.com/developer-id/)

## 4. Cloud-neutral runtime contract

Add this contract before implementing the server. Names and defaults below are proposed public behavior to freeze in design.

| Concern | Required behavior |
|---|---|
| Process | `ledger serve` is one foreground process. No daemonization, login session, shell profile, system service registration or browser dependency. The operator owns restart policy. |
| Listener | One HTTP port. Local default binds loopback. Server/container examples explicitly bind `0.0.0.0:8080`; configurable address/port, IPv6 tested where promised. Never infer permission to expose unauthenticated APIs from a non-loopback bind. |
| Configuration | Explicit YAML path plus documented environment variables/flags. Precedence: flags, environment, YAML, defaults. Validate once and report redacted effective settings. Unknown keys and conflicting secret sources fail startup. No shell evaluation in YAML. |
| Secrets | Environment or mounted secret-file references, with explicit `_FILE` alternatives and an error if both are supplied. No Keychain/cloud SDK requirement. Never print database URLs with passwords, bearer tokens or private keys. Production identity/authority is stable across replicas and provisioned by the operator. |
| HTTP authentication | Required for non-loopback exposure; production must refuse missing credentials. A reverse proxy may terminate TLS, with the private upstream boundary documented. Public cleartext exposure is not an endorsed deployment. Health probes return no economic or secret data. |
| Liveness | `/health/live`: process and request loop alive; no database round trip. Do not restart a healthy process merely because PostgreSQL is temporarily unavailable. |
| Readiness | `/health/ready`: validated config, compatible schema, initialized authority and usable backend; draining or backend failure makes it unready. Bounded/cached backend check. Readiness is not proof that an external export succeeded. |
| Startup and migrations | Local init may initialize an empty SQLite store. Production schema migration is an explicit operator step with a database migration lock. API replicas check compatibility rather than racing to upgrade the database on startup. |
| Shutdown | SIGTERM/SIGINT mark unready, stop admitting work and new dispatch attempts, then drain within a configurable deadline, proposed default 20 seconds. Let the host allow at least 30 seconds. Interrupted/ambiguous commits resolve through the existing identity/retry contract; never fabricate failure or success. Expired leases recover work after forced termination. |
| Logs | Structured stdout logs with request/event correlation, severity and redaction; stderr for startup/fatal diagnostics. No required log directory, desktop console or proprietary telemetry collector. |
| Filesystem | PostgreSQL mode runs with read-only root/config and no writable HOME or working directory; bounded temporary space only when an operation needs it. SQLite requires an explicit writable persistent data directory. Export/backup writes use an explicit destination. |
| Privileges | Non-root OCI user; no privileged container, host socket mount or hardware access. Writable mounts must have compatible ownership. Configuration readability is explicit, not dependent on the founder's UID. |
| Resources | Bound request size, acceptance concurrency, pending backlog, retries, database pool and temporary disk use. Publish measured memory/disk observations when implemented; do not invent minimum RAM or throughput guarantees now. |
| Network | No startup call home, license check, model inference or company service dependency. With SQLite, the local engine works without network access. PostgreSQL needs only its configured database network; configured export destinations add their own requirements. |

Retain one outbox dispatcher per installation in v0. For multiple PostgreSQL API processes, choose one explicit worker-enabled process and start other replicas with dispatch disabled. Enforce exclusive active-worker ownership in PostgreSQL, with a fencing generation for database claim/update operations; fail closed on loss of ownership. A deployment label alone is insufficient. Do not promise multiple active dispatchers or exactly-once remote effects. Downstream idempotency and uncertain-result reconciliation remain necessary even with fencing.

The acceptance transaction always records downstream intentions durably. The worker reads that database state, never an in-memory queue. Readiness may remain true when dispatch is deliberately paused; expose paused state, age/backlog and errors separately. Restore/import starts with dispatch disabled. The fake adapter remains a test/development destination; v0 must not imply a production payment service exists. PostgreSQL production startup must not silently require a local file for fake receipts or inspector state.

## 5. Deployment profiles: what is supported and what merely fits

| Profile | v0 judgment | Operator conditions |
|---|---|---|
| Local Mac/Linux development | Supported on certified references | SQLite default; optional inspector; CLI and server share the single-owner rule. No Docker or cloud account required. |
| Linux VM or bare-metal server | Supported reference profile | Foreground binary supervised externally; durable local disk for SQLite or reachable compatible PostgreSQL. User owns TLS, backups and host patching. No dependency on systemd, though an example can use it. |
| One long-lived Linux container | Supported reference profile | PostgreSQL, or SQLite with the explicit storage conditions in section 6. Test stop/restart, read-only-root behavior and mounts. |
| Several API processes / replicas | Supported application topology only after concurrency gates | PostgreSQL mandatory; shared authority, bounded total connections, explicit migrations and one fenced dispatcher. No automatic HA or zero-downtime-upgrade promise. |
| Kubernetes | Compatible deployment recipe; cluster operation not certified | PostgreSQL recommended and the only v0 documented cluster profile. Explicit migration job, probes, termination grace and worker ownership. Defer Helm chart/operator and managed-cluster test matrix. |
| ARM64 edge computer | Best effort outside the Linux reference | Standard ARM64 Linux, sufficient measured resources and a trustworthy persistent filesystem. No promise for all Raspberry Pi OS versions, SD cards, industrial devices or 32-bit builds. |
| Always-running managed container service | Potentially compatible, provider qualification required | Satisfy lifecycle, background CPU, database connectivity, connection limits, secrets and shutdown contract. A generic image is not a provider endorsement. |
| Request-driven functions / scale-to-zero execution | Unsupported as the complete v0 engine | Suspension/termination and intermittent CPU undermine the continuously running outbox and complicate connection budgets. PostgreSQL alone does not fix that. Functions can be clients of a supported always-running Ledger Lab service. |
| Browser/WASM or provider edge isolate | Unsupported engine | No equivalent claim for embedded SQLite files, OS process APIs or the current database/runtime dependencies. HTTP integration remains available. |

For example, Cloud Run documents an in-memory writable filesystem and lifecycle conditions; those are not equivalent to a persistent local server. This is evidence that the contract must be checked per deployment, not a blanket assertion that managed containers can never work. [Cloud Run container contract](https://docs.cloud.google.com/run/docs/container-contract)

“Cloud-neutral” should mean deployable through ordinary process/container, HTTP, filesystem and PostgreSQL interfaces. It must not mean “runs safely under every provider's default settings.”

## 6. SQLite: containers are acceptable, unsafe storage is not

The product policy is **one application process owning a SQLite database**, with one write connection and bounded internal queues. This is a deliberately smaller support surface than everything SQLite itself permits. Concurrent HTTP requests inside that process do not automatically require PostgreSQL.

A supported SQLite container mounts the **whole data directory**, including the database and WAL sidecars, on durable storage accessible to exactly that application owner. A container restart must see the same directory. A normal Linux filesystem on a correctly attached block volume can qualify even if the underlying block service is remote; the important distinction is filesystem semantics and exclusive ownership, not whether the storage product is marketed as cloud storage.

Do not support SQLite on the disposable container writable layer, NFS/SMB shares, object-storage filesystem adapters, synchronized desktop folders or a volume concurrently attached to independent writers. SQLite WAL requires same-host shared-memory coordination; network filesystem deployments are outside this contract. Preserve the foundation's WAL, FULL synchronization, foreign-key checks and immediate write-transaction discipline. [SQLite WAL documentation](https://www.sqlite.org/wal.html)

Verify persistence across replacement and permission changes, forced termination during acceptance, recovery with a WAL present, disk-full errors and backup/restore. Passing process-kill tests is useful evidence, but not proof against arbitrary power loss or storage that lies about flush completion. The operator's durable storage and backups remain part of the system.

One real Mac-specific durability detail belongs inside the SQLite adapter: set and verify `fullfsync=ON` on its connections on macOS, alongside `synchronous=FULL`. SQLite documents that this selects `F_FULLFSYNC` where supported; with it enabled, the separate checkpoint setting is irrelevant. Read back required pragmas because unknown pragmas can be silently ignored. Using a maintained database's appropriate OS synchronization behavior preserves portability; importing Apple APIs into the economic core would not. [SQLite synchronization pragmas](https://www.sqlite.org/pragma.html#pragma_fullfsync)

Use a database-aware backup procedure or quiesced, verified export. Do not teach copying a live `.db` file by itself. Restoring must preserve economic identities and keep external dispatch paused until reconciliation.

**PostgreSQL becomes mandatory for multiple independent Ledger Lab writer processes, horizontal API replicas, a deployment with no suitable durable single-owner filesystem, or operational requirements outside this SQLite profile.** Recommend it for most cloud services and the v0 Kubernetes recipe. “Container” alone is not the deciding factor, and PostgreSQL is not a substitute for database backups.

Kubernetes `ReadWriteOnce` means mounting by one node, not necessarily one pod or one process. It is not an application writer lock. `ReadWriteOncePod` offers a stronger mounting constraint where supported, but does not establish the rest of the SQLite durability contract. Avoid a v0 Kubernetes/SQLite certification project. [Kubernetes persistent-volume access modes](https://kubernetes.io/docs/concepts/storage/persistent-volumes/)

## 7. Determinism must be tested above the compiler

The intended invariant is: **identical validated inputs, accepted bindings/policy versions and ordered prior economic state produce identical action IDs, amounts, canonical hashes and structured explanations on every certified target/backend.** It does not mean independently generated receive times or different concurrent serialization orders magically match. Capture such context explicitly; compare decisions under the same context. Display layout, ANSI coloring and localized UI formatting are outside the canonical economic record.

| Risk | Required design / evidence |
|---|---|
| Money and intermediate arithmetic | Keep fixed-width checked integer atoms, decimal strings and the bounded exact arithmetic/rounding contract from the foundation. No float economics, native-width `usize` persistence, unchecked release-mode overflow or platform math-library rounding. Test boundaries and negative rounding on both CPUs. |
| JSON canonicalization | Reject duplicate keys and invalid numeric/text inputs before interpretation. Preserve the versioned canonical profile and exact canonical bytes. Include non-ASCII property keys and supplementary-plane characters in cross-language fixtures. |
| Unicode | Decide normalization at the field/schema boundary. Do not silently normalize arbitrary identifiers or signed payloads using OS filename conventions. Equivalent-looking Unicode strings are not automatically the same identity. |
| Ordering | Explicit stable ordering for action generation, policy evaluation ties, graph traversal, SQL results and exports. No hash-map iteration, directory listing order or database collation as economic authority. |
| Time | Explicit UTC timestamp format, precision and boundary rules. Clock injected into tests; no local timezone/DST interpretation of pricing dates. Use monotonic time only for operational deadlines, not persisted event identity. |
| Locale | Locale-independent numeric parsing, sorting and canonical explanation codes. A machine's decimal comma or language cannot change a price. |
| Filesystems | IDs stay database values, not case-sensitive path identities. Test spaces and Unicode in config paths. No assumed case sensitivity, APFS normalization, executable lookup from the current directory or fixed `/Users/...` paths. |
| Binary layout | Encode fields with specified widths/byte order; hash canonical wire bytes, never Rust memory layouts or architecture-specific serialization. Reject unsupported persisted versions. |
| CPU/build flags | Generic target CPU baseline. No `target-cpu=native` in distributed artifacts. Accelerated hashing must yield identical bytes; do not require a founder-machine instruction extension. |
| Explanations | Store stable structured reason codes, references, basis and booked values in the same atomic decision. Render text from those facts; never rerate to explain. Golden comparison covers canonical explanation data. |

JCS sorts property names by UTF-16 code units and emits UTF-8; ordinary Rust string-map ordering is not sufficient for every valid key. JCS also preserves string data rather than applying Unicode normalization. Verify the chosen implementation against the specification and independent fixtures. [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)

For release comparison, each native lane emits the same golden decision bundle. A separate comparison job checks byte equality and expected independent oracle values across both stores and all three targets. Include import/export through a different architecture, replay under the original policy, exact reversal of booked amounts, and retry after unknown commit outcome. Matching two copies of the same flawed evaluator is insufficient without hand-reviewed expected results.

## 8. CI and release matrix without mandatory new cash spend

Use public-repository standard hosted runners for native execution. Current GitHub documentation lists native Ubuntu x64/ARM64 and Apple Silicon macOS runners; public repositories receive standard hosted runner compute without charge. Availability and labels must be verified during the initial pipeline spike. This does not make all storage, larger runners or private-repository usage free. [GitHub hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), [Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions)

Avoid a Cartesian product of every OS, database and configuration:

| Lane | Pull requests | Every public release |
|---|---|---|
| Ubuntu 22.04 x64 | Format/lint, core/property goldens, full SQLite contract, PostgreSQL 18 contract/concurrency | Same plus PostgreSQL 17, migration/export/restore and targeted crash/cancellation suite; build archive and amd64 image |
| Ubuntu 22.04 ARM64 | Core goldens, SQLite contract, PostgreSQL 18 contract | Native same-backend correctness suite, PostgreSQL 17, archive/image lifecycle and recovery checks |
| macOS 15 ARM64 | Core goldens, SQLite contract, CLI smoke | Full local SQLite recovery/lifecycle, PostgreSQL 17/18 TCP contract, local API and inspector smoke, downloaded-archive install check |
| Ubuntu 24.04 x64 + ARM64 | Only when packaging/runtime dependencies change | Run the actual released native archives; smoke acceptance, retry, explain and shutdown. No separate rebuild that hides binary incompatibility. |
| Both OCI architectures | Packaging changes | Run the exact image digests natively: SQLite persistent mount restart; PostgreSQL connection/TLS; non-root/read-only root; health, shutdown and outbox recovery |
| Windows Server 2022 x64 | Compile production workspace when portable boundary code changes | `cargo check --locked` for production crates including selected SQLite/PostgreSQL/TLS features. Build-only label remains explicit. |
| Cross-target comparison | Golden bundle comparison | Exact canonical bundle equality, source/lockfile checks, SBOM/provenance verification and release evidence manifest |

Native PostgreSQL can be installed on the Mac runner for the client contract tests; do not make Docker Desktop a CI dependency. Pin/record the actual server version and verify it at runtime. PostgreSQL's official Mac download page identifies native installation routes including Homebrew. If a desired older major is unavailable through a chosen package route, use a verified alternative or reduce the promise before publishing. [PostgreSQL macOS packages](https://www.postgresql.org/download/macosx/)

Keep long fuzz campaigns and broader dependency-update checks periodic or manually triggered, with focused seeded regressions in release gates. Do not require a 24/7 paid runner, a second founder computer or cloud-provider accounts. Emulation can help build/debug; it cannot replace the two native Linux release runs. Pin OS labels rather than `latest`, pin the Rust toolchain and action revisions, and record runner-image versions because hosted images still change.

Retain only short-lived intermediate CI artifacts; place final evidence with the release and keep caches bounded. Standard runner disk space is finite, so sequence heavyweight builds or remove reproducible intermediates. Configure spending controls before enabling any paid feature. The time needed to maintain three target lanes and two databases is real even when the bill is zero.

Public GHCR images can be pulled anonymously, and GitHub currently states container image storage/bandwidth are free. This is a present distribution policy, not a permanent economic guarantee. A registry mirror or source build must remain possible, and no running ledger should contact GHCR after its image is installed. [Container registry access](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry), [Packages billing](https://docs.github.com/en/billing/concepts/product-billing/github-packages)

## 9. Windows: preserve portability, defer certification

**Windows is build-only in v0 and a candidate for v0.1, not a promised v0.1 deliverable.** Users can attempt source builds, with the unsupported runtime status visible. Do not ship a release zip that implies production support merely because `cargo check` passes.

Promotion requires a concrete Windows x64 runtime lane and demand sufficient to justify maintaining it. Test SQLite WAL/locking and crash recovery, PostgreSQL/TLS, path/Unicode/case behavior, exclusive file creation and replacement, local credential ACLs, child termination/Ctrl-C, service shutdown, port binding, archive extraction and fresh-user installation. Add an actual Windows process lifecycle adapter before promising the `ledger dev -- COMMAND` experience there. Decide signing/installer scope separately.

No NTFS-specific behavior should leak into economics any more than APFS behavior should. Maintain source-level portability now through narrow boundaries, not a speculative Windows service framework. Windows ARM64 and Intel Mac do not enter the release matrix merely because Rust can compile them.

## 10. Database and TLS portability

Bundle one verified, patched SQLite build in every official binary. The foundation's 3.51.3 minimum addressed a specific WAL-reset defect; the release should use the latest tested patched line, not freeze permanently at that minimum. Record `sqlite_version()`, `sqlite_source_id()` and relevant compile options in diagnostics/release evidence. Fail certification if any artifact silently links the host's older SQLite instead. [SQLite WAL-reset bug and fixes](https://www.sqlite.org/wal.html#wal_reset_bug)

Keep PostgreSQL 17 and 18 as the initial supported major lines, with exact tested current minors listed per release. The earlier 17.11/18.6 values are dated initial references, not perpetual recommended patch levels. Test stock PostgreSQL protocol/transaction behavior; managed offerings with proxies, transaction pooling or restricted extensions are not automatically certified. The engine must require no optional server extension or shared server/client CPU architecture.

Use TCP URLs in all portable setup examples, explicit connection and statement timeouts, and an installation-wide connection budget. Start with a small bounded pool, proposed maximum five per API process; account for all replicas, dispatcher and migration connections before setting the server limit. Keep the existing whole-transaction retry and primary-read resolution for ambiguous commits.

Select a Rustls-based SQLx TLS feature set and document its trust-source behavior explicitly. Support a supplied PEM root-CA file for private databases and a defined public trust source; do not rely on a certificate imported into the founder's Keychain. Production remote connections use full certificate-chain and hostname verification. SQLx's documented default is `Prefer`, which can fall back to plaintext; set the mode explicitly instead. Plaintext local development must be an explicit profile choice. [SQLx PostgreSQL SSL modes](https://docs.rs/sqlx/latest/sqlx/postgres/enum.PgSslMode.html)

Release tests include trusted and private CA success, unknown CA rejection, wrong-host rejection, expired certificate rejection and failed plaintext fallback. Validate on both Linux images and Mac. Do not claim every cloud IAM database-auth plugin or client-certificate setup is supported; providers can supply conventional credentials through the documented secret contract. No native OpenSSL/Keychain integration should become an accidental runtime prerequisite.

Database storage must preserve canonical bytes identically: SQLite BLOB and PostgreSQL BYTEA remain authoritative; JSON query conveniences cannot rewrite identity material. SQL collation, numeric coercion, timezone defaults and row retrieval order must not determine domain behavior. The existing separate SQL/migrations and shared acceptance coordinator remain the right design.

## 11. Only a few operating-system boundaries are needed

Keep these modules inside the existing facade/CLI crates; do not add a platform framework or new crate per OS:

1. **Paths and local state:** resolve an explicit data/config directory; development defaults may be project-relative. Validate permissions and expose actionable errors. Production PostgreSQL requires neither home-directory discovery nor writes there.
2. **Exclusive ownership and safe file publication:** OS-backed local ownership guard plus database transaction enforcement; atomic temporary-file publication where used, synchronization and close semantics, backup/import fencing. A PID file alone cannot establish ownership after crashes/PID reuse.
3. **Process lifecycle:** signals, child-process handling for development only, orderly shutdown and exit codes. Keep POSIX handling out of core code so a later Windows adapter is possible.
4. **Clock/entropy/environment inputs:** injectable operational clock/random source and explicit environment loading. These inputs must not invisibly alter deterministic economic outputs.
5. **Optional presentation integration:** browser opening, terminal color/width and OS install hints. Headless operation and plain text/JSON must always work.

HTTP, TLS, SQLite and PostgreSQL use maintained libraries behind the existing boundaries. No generalized filesystem/database/cloud plugin layer, secret-manager integration framework or service-manager abstraction is warranted in v0. SQLite and PostgreSQL remain different persistence implementations with one economic decision model.

## 12. Three documentation paths, one product

**Local developer path:** choose one of the three native archives, verify it, run the executable from a user-writable directory and initialize the synthetic template. Show Linux first as the clean downloaded-binary reference; show Mac download/security behavior honestly. Then accept generation, accept linked publication, explain the unchanged $1.20 after a retry. Browser and Node are optional. No cloud signup, Docker, PostgreSQL or model key appears before this result. Measure the proposed five-minute target independently on each advertised install route.

**Single-server self-host path:** select Linux binary or OCI image; choose single-owner SQLite with persistent directory or PostgreSQL; provision stable credentials; initialize/migrate explicitly; configure listener and TLS boundary; supervise the process; check probes; perform backup/restore with dispatch paused. One page should show where state lives and what survives replacement. A standalone service must not require a cloned source repository or development workspace.

**Cloud/container path:** image digest, PostgreSQL secret, explicit configuration, one port, probes, graceful termination, replica/connection budget and one dispatcher. State which filesystem paths are optional and which profile requires persistence. Give a generic process/container recipe first; provider examples, if any, must name their assumptions and tested status. Never make a provider CLI, SDK, metadata endpoint, volume product or proprietary secret store part of the core contract.

Replace the current local migration example with a TCP-based procedure. Use an already provisioned empty database and operator-supplied `LEDGER_POSTGRES_URL`, such as the illustrative shape `postgresql://USER@127.0.0.1:5432/ledgerlab`; supply credentials through the documented secret mechanism and choose the explicit local TLS profile. For remote databases, require verified TLS and the correct hostname/CA. Do not put a real password in a shell example. `ledger storage move --to postgres --url-env LEDGER_POSTGRES_URL` still performs the verified frozen-source cutover; changing configuration is not migration.

Every guide should distinguish economic acceptance, export intention and confirmed delivery. None should imply payment movement, automatic supplier consent or automatic PostgreSQL provisioning. Preserve the adopted small pricing presets and the six economic roles; platform neutrality should not expand the product model.

## 13. Exact changes required in the previous plans

This report is the authoritative amendment; the earlier documents receive prominent pointers rather than a wholesale rewrite. Before turning them into an implementation specification, apply the following section-level changes:

| Existing report / section | Required edit |
|---|---|
| Foundation §1, §16 — recommendation/final stack | Add the three-target support policy and foreground Linux production contract; remove any implication that a portable Rust core establishes broad OS support. |
| Foundation §2 — workspace | Keep the adoption review's three production crates; place the five small OS boundaries from §11 here inside them. No platform crate explosion. |
| Foundation §4 — representation | Add the cross-architecture determinism conditions, Unicode/JCS fixtures, explicit ordering and CPU-baseline rules from §7. |
| Foundation §5 — persistence | Specify SQLite container volume/one-owner conditions, PostgreSQL multi-process threshold, production migration ownership and exclusive dispatcher enforcement. |
| Foundation §6 — SQLx | Pin bundled SQLite provenance and Rustls/TLS trust/verification behavior; do not inherit `Prefer`. |
| Foundation §7 — API | Separate `dev` from `serve`; adopt §4's listener, secrets, probes, read-only-root and shutdown requirements. |
| Foundation §8 — migration/export | Use TCP-first examples; cross-architecture export/import test; explicit output path and no working-directory requirement. |
| Foundation §9 — public contracts | Publish certified/build-only/best-effort statuses and digest/version compatibility policy. |
| Foundation §10 — tests | Replace the vague Linux/macOS release sentence with §8's native matrix and artifact-level evidence. |
| Foundation §11 — costs | Public standard CI can cost $0 under current policy; distinguish storage/paid runners and Mac distribution signing. |
| Foundation §13 phases 0 and 7; §14 ADRs | Freeze support/ABI/deployment/TLS contracts in phase 0; require native artifacts and recovery evidence in phase 7. Windows remains a conditional later decision. |
| Adoption §2 — first commands | Native archive primary, npm launcher optional; separate Mac runtime support from clean-download onboarding. Keep the synthetic two-event explanation. |
| Adoption §3 — JS and PostgreSQL | Use installed `ledger` in base examples instead of depending on `npx ledger`; document the engine process; replace Unix-socket-first PostgreSQL setup with the portable procedure. |
| Adoption §4 — minimum configuration | Add explicit production identity, database, listener and secret inputs without adding them to the first local demo. |
| Adoption §5 — costs | Add bounded native public CI/registry assumptions, optional notarization, maintenance and user-owned production storage costs. |
| Adoption §6 — simplification | Preserve three crates and one worker; specify enforceable worker ownership for PostgreSQL multi-process service. Defer installers/provider adapters. |
| Adoption §7 — release gates | Measure installation independently per target; gate exact downloaded archives/images and native CPU behavior, not just repository tests. |
| Adoption §8 — final answer | Add that $0 founder cash depends on distribution choices and current public-service policies, while production infrastructure remains user supplied. |

## 14. Final policy and decisions before detailed design

**Support policy:** certify Linux x86-64 and ARM64 for the named server references, and Apple Silicon macOS 15 for local development. Publish one two-architecture Linux OCI image. Treat Windows x64 as build-only, other plausible native targets as source/best effort, and request-driven/serverless engine hosting as unsupported v0. A platform gains support through native product evidence, never through a compiler tier or successful cross-build alone.

**Required release set:** three native archives, source/lockfile, one multi-platform OCI index, checksums/manifest, artifact-specific SBOMs and signed CI provenance. Keep the thin TypeScript SDK; defer the npm executable launcher if necessary. No mandatory OS installer, Apple signing purchase, provider account or company runtime service.

**CI strategy:** native public x64 Linux, ARM64 Linux and ARM64 Mac lanes; two PostgreSQL majors exercised without multiplying every configuration; exact artifact smoke tests; cross-platform canonical comparison. Standard hosted compute and current public container distribution can keep incremental cash at $0. Limit intermediate artifact storage and do not enable paid runners accidentally. A private repository or future policy change requires reassessing that budget. Engineering time, user production compute/storage and optional signing are separate costs.

Before detailed implementation design, freeze these six choices:

1. Accept the exact reference OS/ABI matrix and distinguish certified runtime from unsigned Mac download experience. If the Mac install gate cannot pass within budget, document source-build onboarding honestly and remove the Mac five-minute claim.
2. Approve the server contract: no writable project in PostgreSQL mode, stable provisioned identity, explicit TLS/secret sources, bounded shutdown and probes.
3. Approve SQLite's one-owner filesystem profile and PostgreSQL's mandatory multi-process threshold, including explicit migration and worker ownership.
4. Freeze canonical representation and deterministic context boundaries, including Unicode ordering, exact arithmetic and structured explanation comparison.
5. Confirm access to native free public runner lanes and select a pinned image/toolchain/TLS/SQLite build recipe. This requires a bounded feasibility spike; no such spike or release test was run in this review.
6. Confirm artifact/package names, provenance/SBOM generation, the clean install procedure and how release evidence is retained without unbounded storage costs.

These are decisions and narrowly scoped feasibility checks, not a request to build a cross-platform infrastructure platform. The economic core can stay small: one atomic acceptance path, two durable stores, ordinary HTTP/process interfaces and portable canonical records. The founder's Mac remains useful, while Linux artifacts and shared conformance evidence establish the production promise.
