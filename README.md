# Bean Counter

Local billing with durable receipts, explained corrections and complete statements.

Bean Counter is an Apache-2.0 local SQLite billing CLI, developed as Ledger Lab. The command remains `ledger` and the Rust crates remain `ledgerlab*`. Configure explicit customer terms, record fixed-price work, apply authorized adjustments and corrections, and export reconciled JSON statements. No hosted account, model API, runtime subscription or payment processor is required.

**Current scope:** one customer/agreement per installation, local OS/filesystem administration and tested macOS 26.6.2/Apple silicon and Ubuntu 24.04.5/x86-64 behavior. Users supply compute, memory, storage and backups. Resource reservations and completion guarantees, PostgreSQL product support, multi-host writer replacement and advanced comparison are deferred. The amended local phases are complete with owner acceptance; the owner waived separate independent review, which did not occur or pass. Read the [public status](STATUS.md) and [supported/deferred matrix](CURRENT-REQUIREMENTS.md).

The repository also contains frozen contracts, the bounded first acceptance slice, a typed pure core, reference histories, bounded outbox primitives and a local SQLite developer CLI. `ledger init --demo`, `accept`, `preview`, and `explain` run without Docker, Node, a cloud account, or a paid provider. Those commands still save and preview only the original generation slice; they do not expose the new outcome lifecycle. No HTTP service or payment execution is included. See the [local quickstart](docs/quickstart.md) for the runnable CLI workflow.

The frozen first slice shows demo-customer owing demo-host USD 0.80 for generation: USD 1.00 charge less USD 0.20 customer discount, with dispatch held. Supplier obligations and provider cost observations are separate; neither is recorded in this demo. The separate design onboarding chain totals 120 atoms. The [upcoming linked-work story](docs/upcoming-story.md) is documentation only; it creates no ledger history.

## Ordinary local billing

The `ledger billing` profile accepts operator-configured fixed-price retail work, outcome adjustments and authorized corrections, with durable receipts, duplicate protection, revocable local permissions and complete JSON statements. Start with the [billing quickstart](docs/billing-quickstart.md), [recovery procedure](docs/billing-recovery.md) and [resource/cost manifest](docs/resources-and-costs.md). It uses a separate installation with explicit terms. The repository includes illustrative inputs, never customer assent obtained by the program. No payment is collected and a statement is not a tax/legal invoice.

The [v0.3.0 native packages and integration guide](examples/integration/README.md) provide `ledger billing setup DIR`, a guided terminal setup without hand-authored JSON, and a strict JSON interface for products. The published native packages are unsigned and have only the tested platform coverage stated above.

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
