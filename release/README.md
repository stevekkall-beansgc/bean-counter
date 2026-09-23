# Release contract, not certification

**Current scope amendment (2026-09-23):** [Bean Counter local SQLite requirements](../CURRENT-REQUIREMENTS.md) and [release profile](local-sqlite.md) govern the newly authorized publication. The historical full-platform contract below remains preserved; its old no-publication/CI exception no longer governs the current profile. Deferred guarantees and platforms have not passed.

`targets.toml` freezes the exact design §21/24 matrix. No archive, image or platform has been certified. This Mac runs macOS 26 and supplies development evidence only. Ubuntu 22.04 sets the intended Linux build environment; inspect finished binaries for actual ABI requirements. No universal Linux/Alpine/kernel promise follows from a Rust target triple.

One foreground process, stable operator credentials, explicit config/secrets, loopback default, one HTTP port, authenticated economic routes, trusted-proxy TLS boundary for non-loopback, bounded resources and 20-second drain with at least 30-second host grace. PostgreSQL runs with read-only root/config and no writable HOME/project. SQLite requires one owning process and the complete durable data directory, WAL/FULL/foreign keys and verified macOS fullfsync. Independent writers require PostgreSQL. One fenced dispatcher; imported/restored state holds dispatch until reconciliation. No company/model/telemetry service dependency.

`artifact-manifest.schema.json` is the release-evidence field contract, not an example claiming tests passed. Release artifacts require exact pinned dependency/backend/image versions, MSRV evidence, SHA-256, SPDX SBOM and provenance identity verification. MAC unsigned download friction is separate from runtime correctness. The primary quickstart is a native archive, npm launcher optional. No paid infrastructure or OS signing purchase is required by Phase 0.

## Runner and namespace inventory

The [official runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) lists ubuntu-22.04, ubuntu-22.04-arm, ubuntu-24.04, ubuntu-24.04-arm, macos-15 ARM64 and windows-2022. This is documentation inventory only, observed during Phase 0; no remote job ran. Actual runner execution, images, availability and cost controls remain a Phase 1/7 gate. Standard public runners can be used without mandatory compute spend; private/larger-runner use requires separate budgeting.

Public crates.io API lookups for the three package names did not return usable evidence through the web tool. Namespace ownership/availability remains unverified; no name is reserved. Npm/image names likewise must be confirmed before publication. These do not block local Phase 1 work.

No CI workflow is activated in this local, unpublished repository. `scripts/check.sh` is the complete local Phase 0 gate. A Phase 1 pipeline task must select verified immutable action revisions, establish a remote/public-cost policy and run the listed native smoke jobs; inventing action pins or reporting unrun CI green would be misleading. Phase 7 owns release/periodic workflows, SBOM/provenance and exact artifact tests. This is the documented exception to the repo-release day-one CI/remote checklist under the task's no-publication scope.
