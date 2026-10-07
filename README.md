# Bean Counter

**Local billing with durable receipts, explained corrections and reconciled statements.**

Bean Counter is an Apache-2.0 billing engine for applications that produce completed generated work. Configure an agreed fixed price or quantity rate, record the completed work, and retain an exact receipt. It helps your application reconcile retries and corrections without rebuilding billing history for every product.

**[Install v0.9.6 Open Beta and get a first synthetic receipt →](START-HERE.md)**

These instructions match **v0.9.6 Open Beta** with unchanged billing contract v0.3 and schema 11. The [matching release page](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.6) and [version-bound qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.6/QUALIFICATION.json) determine actual publication, exact source and qualified archive identities. A source checkout or native candidate alone does not establish qualification or publication. Use these instructions with matching published assets; older releases retain their own instructions and immutable bytes. The package includes the executable, verification/install tools, documentation, examples, full bounded-cohort fixtures and checker.

Historical **[v0.9.5 Open Beta](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.5)** was released from `3406c1782b45a28ef39b974bf23978c72b44764e`. Its [version-bound qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.5/QUALIFICATION.json) identifies that release's actual checks and immutable assets. Use its matching instructions for those bytes; the v0.9.6 fixes require their own qualification.

Historical **[v0.9.4 Open Beta](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.4)** was released from `e897ef108753447371282cb5253fc88634e5977d`. Exact-source QA/CI, both native installed journeys and matching public-download checks passed for that version; see its [qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.4/QUALIFICATION.json). Its evidence applies only to those immutable assets.

The native package includes the `ledger` executable, verification/install tools, documentation and examples. Historical v0.9.4 native qualification covered Apple-silicon macOS 26.6.2 and Ubuntu 24.04.5 x86-64/glibc 2.39. Install/example prerequisites are Python 3.11+ and curl; no hosted account, model API, runtime subscription, Rust compiler or payment processor is needed. Packages are unsigned/unnotarized; other platforms and clean-account launch remain unverified.

One business owns each private local SQLite installation, with multiple customers and sources. Supported operations include fixed and exact scale-18 usage, explicit effective agreements, authorized corrections, immutable close statements, recurrence records, fiscal reports and finance CSV. You supply customer assent/authority evidence and operate the computer, storage and backups. Hosted APIs, payment collection, PostgreSQL billing and multi-host guarantees remain outside this beta.

[Release/downloads](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.6) · [Supported scope](CURRENT-REQUIREMENTS.md) · [Operator checklist](docs/billing-operations.md) · [Integration guide](docs/integration-agent-guide.md)

## Ordinary local billing

The `ledger billing` profile accepts operator-configured fixed-price retail work, outcome adjustments and authorized corrections, with durable receipts, duplicate protection, revocable local permissions and complete JSON statements. Start with the [billing quickstart](docs/billing-quickstart.md), [recovery procedure](docs/billing-recovery.md) and [resource/cost manifest](docs/resources-and-costs.md). It uses a separate installation with explicit terms. The repository includes illustrative inputs, never customer assent obtained by the program. No payment is collected and a statement is not a tax/legal invoice.

The [version-matched package and integration guide](examples/integration/README.md) describe guided terminal setup and the strict CLI/JSON interface. Use [START-HERE.md](START-HERE.md) for the complete download/install/example path. Published v0.9.2 and earlier releases retain their own immutable bytes and version-matched instructions. No Rust crates are published by the native release.

For a source build:
```sh
cargo build --release --locked -p ledgerlab-cli
./target/release/ledger billing --help
```

Build with the pinned Rust 1.98.1 development compiler and native C/linker tools. Build dependencies need an initial download; an installed binary does not need Rust, Python or Node. No minimum supported Rust version or untested OS support is claimed.

## Local finance CSV

[WORKFLOW.md](WORKFLOW.md) provides the installed synthetic billing-to-finance example, explicit account mapping, stable repeat export and reconciliation. See the [CSV contract](docs/finance-csv.md) and [owner end-to-end walkthrough](docs/finance-e2e.md). The [public status](STATUS.md) distinguishes owner acceptance from the waived independent-review gate.

## Validate locally

Use Rust 1.98.1 with rustfmt/Clippy, Python 3.11+ and Node. The Python minimum is for test tooling only; no product MSRV is declared. Install the pinned document-test requirements into a local virtual environment if they are not already present:

```sh
python3 -m venv work/check-venv
. work/check-venv/bin/activate
python3 -m pip install -r scripts/requirements-contracts.txt
sh scripts/check-local-billing.sh
sh scripts/check-local-billing-e2e.sh
```

The Rust workspace uses pinned dependencies and builds offline once its dependency cache is populated. Python packages may need an initial download; subsequent contract checks do not use the network. On the prepared development machine, source `work/toolchain/activate.sh` and select the installed verified compiler alias with `export RUSTUP_TOOLCHAIN=stable`; checks require its actual version to match 1.98.1. The `work/` toolchain/cache is intentionally untracked and is not a portable repository prerequisite.

## Start contributing

- [Contributor rules and exact test commands](AGENTS.md)
- [Current local SQLite requirements](CURRENT-REQUIREMENTS.md)
- [Implementation boundaries and Phase 1 lanes](docs/implementation.md)
- [Frozen contract guide](contracts/README.md)
- [Canonical record addendum](docs/design/CANONICAL-RECORDS-V1.md)
- [Phase gates](docs/phase-gates.md)
- [Release support contract](release/README.md)
- [Validation evidence](docs/PHASE-0-VALIDATION.md)

One coordinated release train: `ledgerlab-core` → `ledgerlab` → `ledgerlab-cli` (binary `ledger`). `ledgerlab-testkit` is unpublished and never a production dependency. [GitHub Releases](https://github.com/stevekkall-beansgc/bean-counter/releases) are the changelog; source and native-archive publication do not publish Rust crates. See the [local release profile](release/local-sqlite.md). Project owner: Legume Labs.

For the finance CSV workflow and expected $5.00 synthetic result, see [WORKFLOW.md](WORKFLOW.md) and the [owner end-to-end walkthrough](docs/finance-e2e.md).

## Historical development evidence

The [Phase 3 status](PRODUCT-PHASE-3-STATUS.md), incomplete [Phase 4 foundation](PRODUCT-PHASE-4-FOUNDATION.md), design sources and frozen [roadmap](ROADMAP.md) preserve their original scope and verdicts. The bounded Phase 3 persistence review does not certify the current billing profile or full Phase 4. Historical documents can contain local evidence links unavailable in this public checkout; private evidence files and session transcripts are not bundled. Current support is defined by [CURRENT-REQUIREMENTS.md](CURRENT-REQUIREMENTS.md).
