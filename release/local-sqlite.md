# Bean Counter local SQLite release profile

This profile implements [the current owner amendment](../CURRENT-REQUIREMENTS.md); [STATUS.md](../STATUS.md) gives the concise public status. The reduced local phases are complete with owner acceptance, and the separate independent-review requirement was waived rather than passed. The historical `targets.toml` and `artifact-manifest.schema.json` describe the broader platform contract. They remain unchanged and are not evidence that PostgreSQL, a multi-platform matrix or reserved-resource completion has passed.

The public destination is `stevekkall-beansgc/bean-counter`. Release notes on GitHub are the changelog. Rust crates and the executable retain their existing names; no crates.io, npm, container or hosted-service publication is part of this release. The package version is independent of frozen economic profile identifiers. `ledgerlab::billing::CONTRACT_VERSION` identifies the current local billing facade and JSON contract family.

## Validation

`sh scripts/check-local-billing.sh` checks formatting, pure-core tests, billing service/store tests (including real SQLite full/failure and commit uncertainty), testkit, strict workspace Clippy, no-default-features compilation, architecture boundaries, frozen contracts and settlement audits. `sh scripts/check-local-billing-e2e.sh` runs the CLI integration tests, including ordinary billing, aliases, permissions, reopen, correction, quiescent recovery and the synthetic finance CSV demonstration. It also preserves the original developer CLI regressions. The commands use locked dependencies and require the verified Rust 1.98.1 development compiler, Python 3.11+ with `scripts/requirements-contracts.txt`, Node and native build tools. Populate the Cargo cache before offline checks.

These are author-run acceptance checks for the supported profile. The owner accepted the amended local Phase 6 and waived its separate independent-review requirement; no independent PASS is claimed. Existing historical test results and partial/failed runs keep their original status. In particular, the pre-publication full workspace run passed production Rust suites but failed one testkit migration-inventory assertion; the assertion was fixed at `7af0e50` and the complete affected testkit and remaining gates passed afterward. That staged validation was not one uninterrupted passing `scripts/check.sh` run.

## Artifact evidence and limitations

An artifact must identify its source SHA, tag/version, Cargo.lock checksum, compiler/target, actual OS, build command, native linkage, bundled SQLite identity, included file checksums, dependency license/SBOM inventory and installed walkthrough result. Local build provenance must be labeled author-generated, not signed CI provenance. The tested compiler is not a minimum-supported-Rust claim. No signing purchase is required; unsigned downloads may need macOS user approval and clean-account launch has not been established.

The publisher must use the standard release command, with a clean registered default branch, manifest-owned QA, exact-HEAD compliance CI, honest scheduler state and a reachable origin. Verify the remote branch, peeled annotated tag and GitHub Release all identify the candidate SHA. Never move a published tag, replace a failed gate with a lookalike check or edit fixed control inputs merely to obtain green.

Local customer installations, retained evidence databases and internal session logs are excluded from source and artifact packaging. Frozen synthetic journals under `fixtures/` are intentional test data.
