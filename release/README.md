# Release support and historical contract

The [current status](../STATUS.md), [local SQLite requirements](../CURRENT-REQUIREMENTS.md) and [release profile](local-sqlite.md) govern the published product. [v0.2.1](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.2.1) passed the local-profile release gates and exact-commit compliance CI, and provides an unsigned Apple-silicon macOS archive with checksums and provenance. The amended local phases are complete with owner acceptance; separate independent review was waived, not passed. Deferred guarantees and platforms have not passed.

## Historical full-platform contract

The remaining text preserves the original broader release plan as historical requirements. It does not expand the supported local profile or negate its published release.

`targets.toml` freezes the exact design §21/24 matrix. That full-platform matrix has not been certified. The separate local-profile v0.2.1 native archive was tested on macOS 26.6.2 with Apple silicon; this does not certify other platforms. Ubuntu 22.04 was an intended Linux build environment in the original plan; inspect finished binaries for actual ABI requirements. No universal Linux/Alpine/kernel promise follows from a Rust target triple.

One foreground process, stable operator credentials, explicit config/secrets, loopback default, one HTTP port, authenticated economic routes, trusted-proxy TLS boundary for non-loopback, bounded resources and 20-second drain with at least 30-second host grace. PostgreSQL runs with read-only root/config and no writable HOME/project. SQLite requires one owning process and the complete durable data directory, WAL/FULL/foreign keys and verified macOS fullfsync. Independent writers require PostgreSQL. One fenced dispatcher; imported/restored state holds dispatch until reconciliation. No company/model/telemetry service dependency.

`artifact-manifest.schema.json` is the release-evidence field contract, not an example claiming tests passed. Release artifacts require exact pinned dependency/backend/image versions, MSRV evidence, SHA-256, SPDX SBOM and provenance identity verification. MAC unsigned download friction is separate from runtime correctness. The primary quickstart is a native archive, npm launcher optional. No paid infrastructure or OS signing purchase is required by Phase 0.

## Runner and namespace inventory

The original Phase 0 [runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) listed ubuntu-22.04, ubuntu-22.04-arm, ubuntu-24.04, ubuntu-24.04-arm, macos-15 ARM64 and windows-2022. That list was documentation inventory, not execution evidence for the full platform. The local-profile exact-commit compliance CI did run for v0.2.1; the broader native platform matrix remains unverified. Private/larger-runner use requires separate budgeting.

Historical crates.io lookups did not establish ownership of the three package names. The Rust crates are not published to crates.io; npm/image publication is not claimed. Namespace questions do not block the existing source and native-archive GitHub releases.

At Phase 0, this repository was local and unpublished, with `scripts/check.sh` as its local scaffold gate. That exception ended when the local SQLite profile was published. The current release path uses manifest-owned local QA and exact-commit hosted compliance CI; v0.2.1 also shipped source/build identities, SPDX inventory, checksums and a verified installed walkthrough. The original Phase 7 multi-platform certification remains deferred.
