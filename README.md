# Ledger Lab

Chain events. Compose pricing. Export the result anywhere.

Ledger Lab is an Apache-2.0 event-native economic engine in development. **Bounded product Phase 3 persistence is complete and independently reviewed PASS; the product is not production-ready.** The reviewed SQLite/PostgreSQL path covers one final base with no predecessor chain, authorized outcome adjustment, correction and supplier reservation closure. Phase 4, isolated nonposting policy comparison, is next. See the [current Phase 3 status](PRODUCT-PHASE-3-STATUS.md) for the exact reviewed commit, evidence and remaining limits. [ROADMAP.md](ROADMAP.md) is a frozen historical contract overlay: its “not started” progress text is stale and remains unchanged; the current status document is the progress pointer.

The repository also contains frozen contracts, the bounded first acceptance slice, a typed pure core, reference histories, bounded outbox primitives and a local SQLite developer CLI. `ledger init --demo`, `accept`, `preview`, and `explain` run without Docker, Node, a cloud account, or a paid provider. Those commands still save and preview only the original generation slice; they do not expose the new outcome lifecycle. No HTTP service or payment execution is included. See the [local quickstart](docs/quickstart.md) for the runnable CLI workflow.

The frozen first slice shows demo-customer owing demo-host USD 0.80 for generation: USD 1.00 charge less USD 0.20 customer discount, with dispatch held. Supplier obligations and provider cost observations are separate; neither is recorded in this demo. The separate design onboarding chain totals 120 atoms. The [upcoming linked-work story](docs/upcoming-story.md) is documentation only; it creates no ledger history.

## Validate locally

Use Rust 1.98.1 with rustfmt/Clippy, Python 3.11+ and Node. The Python minimum is for test tooling only; no product MSRV is declared. Install the pinned document-test requirements into a local virtual environment if they are not already present:

```sh
python3 -m venv work/check-venv
. work/check-venv/bin/activate
python3 -m pip install -r scripts/requirements-contracts.txt
sh scripts/check.sh
```

The Rust workspace uses pinned dependencies and builds offline once its dependency cache is populated. Python packages may need an initial download; subsequent contract checks do not use the network. On the prepared development machine, source `work/toolchain/activate.sh` and select the installed verified compiler alias with `export RUSTUP_TOOLCHAIN=stable`; checks require its actual version to match 1.98.1. The `work/` toolchain/cache is intentionally untracked and is not a portable repository prerequisite.

## Start contributing

- [Implementation boundaries and Phase 1 lanes](docs/implementation.md)
- [Frozen contract guide](contracts/README.md)
- [Canonical record addendum](docs/design/CANONICAL-RECORDS-V1.md)
- [Phase gates](docs/phase-gates.md)
- [Release support contract](release/README.md)
- [Validation evidence](docs/PHASE-0-VALIDATION.md)

One coordinated release train: `ledgerlab-core` → `ledgerlab` → `ledgerlab-cli` (binary `ledger`). `ledgerlab-testkit` is unpublished and never a production dependency. Version 0.0.0 is an internal scaffold anchor; no package has been published. Public release notes will be the changelog. Project owner: BeanLabs.
