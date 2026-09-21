# Ledger Lab

Chain events. Compose pricing. Export the result anywhere.

Ledger Lab is an Apache-2.0 event-native economic engine in development. This repository contains the **Phase 0 contract freeze and a compiling Rust scaffold**. It does not yet accept events, evaluate policies, open databases, serve HTTP, or move money. The `ledger` executable reports that status and exits 2.

The frozen first slice is a synthetic generation event: base 100 USD atoms, enterprise discount −20, net 80, one held fake-export intention. The separate onboarding chain totals 120 atoms. Do not conflate them.

## Validate locally

Use Rust 1.98.1 with rustfmt/Clippy, Python 3.11+ and Node. The Python minimum is for test tooling only; no product MSRV is declared. Install the pinned document-test requirements into a local virtual environment if they are not already present:

```sh
python3 -m venv work/check-venv
. work/check-venv/bin/activate
python3 -m pip install -r scripts/requirements-contracts.txt
sh scripts/check.sh
```

The Rust scaffold has no external dependencies and builds offline. Python packages may need an initial download; subsequent contract checks do not use the network. On the prepared development machine, source `work/toolchain/activate.sh` and select the installed verified compiler alias with `export RUSTUP_TOOLCHAIN=stable`; checks require its actual version to match 1.98.1. The `work/` toolchain/cache is intentionally untracked and is not a portable repository prerequisite.

## Start contributing

- [Implementation boundaries and Phase 1 lanes](docs/implementation.md)
- [Frozen contract guide](contracts/README.md)
- [Canonical record addendum](docs/design/CANONICAL-RECORDS-V1.md)
- [Phase gates](docs/phase-gates.md)
- [Release support contract](release/README.md)
- [Validation evidence](docs/PHASE-0-VALIDATION.md)

One coordinated release train: `ledgerlab-core` → `ledgerlab` → `ledgerlab-cli` (binary `ledger`). `ledgerlab-testkit` is unpublished and never a production dependency. Version 0.0.0 is an internal scaffold anchor; no package has been published. Public release notes will be the changelog. Project owner: BeanLabs.
