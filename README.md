# Bean Counter

<img src="docs/assets/bean-counter.png" alt="Bean Counter: an orange bean wearing an accountant’s green visor" width="280" height="280">

**Give your application a billing history you can explain.**

Bean Counter is an Apache-2.0 local billing engine for applications that produce completed work. Set an agreed price or quantity rate, record the work, and keep a durable receipt. When a request is retried or a charge needs correcting, the earlier records stay intact and the statement explains what changed.

## A small billing story

Your app finishes a report. The agreed price is **$1.00**, so it saves a receipt. A later quality review earns the customer an agreed **$0.20 discount**. The statement shows **$0.80 owed**, with both records intact.

| What happens | What Bean Counter keeps |
| --- | --- |
| Agree the terms before work begins | The price, customer and permissions |
| Finish and save the report | A base charge and its receipt |
| Perform the agreed later assessment | A linked adjustment and its explanation |
| Retry a request after losing the response | The original receipt, without another charge |
| Correct a mistake | A new correction that preserves the earlier history |

**Agree → complete → record → assess → reconcile.**

Your app supplies the facts and authority. Bean Counter keeps the billing history straight. A failed attempt is not completed work; an assessment error is not a quality verdict. The [live-app recipe](docs/live-app-integration.md) shows how to keep those cases separate.

## Try the open beta

**[Download v0.9.7 and get your first synthetic receipt →](START-HERE.md)**

The native kit includes the `ledger` executable, installation and verification tools, documentation, examples, fixtures and a 30-check bounded-cohort checker. You need **Python 3.11+ and curl** for installation and examples. No Rust compiler, Node, hosted account or model API is required.

Qualification covers **Apple-silicon macOS 26.6.2** and **Ubuntu 24.04.5 x86-64/glibc 2.39**. Packages are unsigned and unnotarized; other platforms and unfamiliar-user/clean-account launch remain unverified. Start with synthetic records in a fresh private directory.

[Release and downloads](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.7) · [What is supported](CURRENT-REQUIREMENTS.md) · [Qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.7/QUALIFICATION.json)

## What you can do

- **Record fixed-price or usage charges.** Work with exact integer amounts, including the released scale-18 usage profile.
- **Retry safely.** Reconcile original requests and receipts after a dropped response or restart; changed requests under an existing identity refuse.
- **Explain adjustments.** Record agreed outcomes and authorized corrections while retaining the original charge and history.
- **Close and export.** Produce immutable period statements, fiscal reports and finance CSV with explicit account mapping.
- **Operate locally.** One business owns a private SQLite installation, with multiple customers and sources. You manage the computer, storage, permissions and backups.

The engine does not collect payments, obtain customer assent, authenticate provider evidence or produce certified tax/legal invoices. Hosted APIs, PostgreSQL billing, multi-host guarantees and resource guarantees are deferred. The [current requirements](CURRENT-REQUIREMENTS.md) define the supported scope.

## Connect an application

| What you want to do | Start here |
| --- | --- |
| Install and verify the kit | [First receipt walkthrough](START-HERE.md) |
| Adapt a live application to completed work and later assessment | [Live-app integration recipe](docs/live-app-integration.md) and [runnable bounded-cohort example](examples/integration/bounded-cohort/README.md) |
| Call the CLI and reconcile each operation | [Integration guide](docs/integration-agent-guide.md) |
| Configure parties, prices, terms and permissions | [Billing quickstart](docs/billing-quickstart.md) |
| Run and recover a business installation | [Operator checklist](docs/billing-operations.md) and [recovery procedure](docs/billing-recovery.md) |
| Export a statement to finance CSV | [Workflow](WORKFLOW.md) and [CSV contract](docs/finance-csv.md) |

The outcome profile uses **bounded cohorts**: agree terms before work, finish work by both window starts, then perform the agreed later assessment inside its window. It does not cover indefinitely arriving work under one static agreement. Use actual completion and assessment times. A provider error is unknown evidence, not automatically a negative billing result. See the [live-app recipe](docs/live-app-integration.md) before adapting the example.

On macOS, `/tmp` and `/var` are symlink aliases. Use a private physical directory and set installation and input paths from it; changing the working directory alone does not rewrite an existing path variable. [START-HERE](START-HERE.md#use-private-physical-paths) gives the commands.

## Develop and contribute

Read [AGENTS.md](AGENTS.md) and [CURRENT-REQUIREMENTS.md](CURRENT-REQUIREMENTS.md) before changing implementation or contracts. For a source build, use the pinned **Rust 1.98.1** development compiler and native C/linker tools:

```sh
cargo build --release --locked -p ledgerlab-cli
./target/release/ledger billing --help
```

Build dependencies need an initial download. No minimum supported Rust version is declared. An installed executable does not need Rust, Python or Node; installation and example scripts do need Python.

To validate source, install Python 3.11+, Node and Rust 1.98.1 with rustfmt/Clippy. Populate the locked Cargo dependency cache before the offline checks, then run:

```sh
python3 -m venv work/check-venv
. work/check-venv/bin/activate
python3 -m pip install -r scripts/requirements-contracts.txt
sh scripts/check-local-billing.sh
sh scripts/check-local-billing-e2e.sh
```

[Implementation boundaries](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/docs/implementation.md) · [Frozen contracts](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/contracts/README.md) · [Canonical records](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/docs/design/CANONICAL-RECORDS-V1.md) · [Resources and costs](docs/resources-and-costs.md) · [Release profile](release/local-sqlite.md)

## Release evidence and history

The current published package is **v0.9.7 Open Beta**, with billing contract v0.3 and SQLite schema 11. Its [release page](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.7) and [qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.7/QUALIFICATION.json) identify the exact source, archives and actual qualification. A newer documentation checkout does not replace the instructions or evidence shipped in an older package. Earlier releases retain their immutable bytes and matching instructions.

[GitHub Releases](https://github.com/stevekkall-beansgc/bean-counter/releases) are the changelog. The [current requirements](CURRENT-REQUIREMENTS.md) retain version history; [STATUS.md](STATUS.md) distinguishes current support, historical owner acceptance and the historical independent-review waiver. Source and native publication do not publish Rust crates.

The [Phase 3 status](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/PRODUCT-PHASE-3-STATUS.md), [Phase 4 foundation](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/PRODUCT-PHASE-4-FOUNDATION.md), [phase gates](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/docs/phase-gates.md), [validation record](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/docs/PHASE-0-VALIDATION.md) and [roadmap](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/ROADMAP.md) preserve historical scope and verdicts. Historical local evidence links are not bundled public acceptance evidence.

One coordinated release train: `ledgerlab-core` → `ledgerlab` → `ledgerlab-cli` (the `ledger` command); `ledgerlab-testkit` stays unpublished. [Release support](https://github.com/stevekkall-beansgc/bean-counter/blob/4014764fee92239fb32558e60fb1379a3bd00071/release/README.md). Project owner: Legume Labs.
