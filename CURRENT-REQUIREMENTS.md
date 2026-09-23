# Bean Counter: current local SQLite requirements

Amended 2026-09-23 by the project owner; current status is summarized in [STATUS.md](STATUS.md). Bean Counter is the public project name for the Ledger Lab implementation; the `ledger` command, `ledgerlab*` crates and existing schema identifiers retain their names for compatibility. The initial local-profile amendment started from `7af0e502aec31078f45260223ce32cf6476b2b47`; the published local profile is v0.2.1.

This is the current acceptance and publication scope for `ledger billing`. It supersedes conflicting full-platform publication prerequisites in the historical phase gates, design sources, roadmap and release matrix. Historical specifications, frozen contracts, failures and review verdicts remain intact. A deferred requirement has not passed. Existing pricing, authority, storage-integrity and unsupported-capability guards remain mandatory.

## Supported and deferred matrix

| Capability | Current scope / acceptance |
| --- | --- |
| Operator-configured billing | One customer and agreement per installation; immutable explicit terms, fixed USD price and one fixed-amount outcome family. Retain operator-supplied assent and authority evidence; software does not acquire consent. |
| Charges and adjustments | Existing pure evaluator and frozen economic records; exact integer atoms, authorized outcomes, expected-revision corrections and exact inverse/replacement postings. Never overwrite accepted economic history. |
| Durable records and retries | Local SQLite with documented lock/fsync/storage assumptions; commit before success; restart-safe delivery identity, permanent semantic aliases and conflicts. Unknown commit results require original-identity retry/reconciliation. |
| Authority and isolation | Local private-directory/OS administrator trust. Revocable read/submit/correct permissions within the original ceiling; wrong customer, source, scope, revision and rights refuse. No remote authentication claim. |
| Explanation and statements | Saved receipts and complete reconciled snapshots with retained records and integer totals. No silent truncation, payment collection or tax/legal invoice claim. |
| Backup and recovery | Quiescent whole-installation copies, verified restore and retained generations; preserve config, DB and present sidecars. No online backup, rollback detection, cross-host transfer guarantee or automatic failover. |
| Native evidence | Tested macOS 26.6.2 on Apple silicon. Other operating systems, clean-account downloaded launch, signing/notarization and production host certification are unverified. A source build is not platform certification. |
| Resource ownership | Users supply and operate CPU, RAM, disk, storage and backups. Document prerequisites, limits, uncertainty and failure behavior. Existing admission limits remain enforced. |
| Resource guarantees | **Deferred:** native/Rust allocation-overlap proof, allocator-to-RSS conversion, aggregate RAM/disk reservations, physical page/WAL/workspace capacity proofs, saturation completion and guaranteed completion of accepted offline work under reserved resources. |
| PostgreSQL | **Deferred product support.** Historical backend tests do not certify this billing profile. Existing unsupported operations continue to refuse. |
| Distributed/offline hosts | **Deferred:** multi-host guarantees and writer replacement. |
| Advanced comparison | **Deferred.** Existing foundations do not establish completed support. |
| Hosted operations | **Deferred:** HTTP service, remote authentication, hosted deployment, infrastructure provisioning and payment execution. The bounded local Phase 5/6 work is complete; broader original Phase 5/6 platform expansion is not. |
| Independent review | **Waived by owner for the amended local Phase 6.** No independent technical review was completed or passed. Author tests and owner acceptance are distinct from independent review; no full original G2/all-16-R3 PASS is claimed. |

## Required behavior under resource failure

Unavailable memory, disk, locks or storage may cause refusal or an uncertain result. Never acknowledge an uncommitted charge, duplicate billing on retry, silently discard records or invent resource evidence. Do not interpret an unknown result as rollback. Reopen and resolve the original identity; preserve integrity failures for investigation. Provisioning and monitoring remain the operator's responsibility.

Admission ceilings are 1,000 accepted decisions, 1,000 aliases, 1,000 permission changes, 32 MiB economic payloads and separately 32 MiB alias ingress. Setup/permission input is at most 64 KiB, event input 256 KiB and an economic bundle 8 MiB. These are bounds, not resource reservations or throughput guarantees. Small walkthrough measurements establish neither a production minimum nor peak demand at the limits. See [resources and costs](docs/resources-and-costs.md) and [recovery](docs/billing-recovery.md).

## Release acceptance

The release must identify one exact source commit, pass manifest-owned local QA and exact-commit compliance CI, preserve frozen contract checks and meet the Bean release standard. The local profile's unit entrypoint is `sh scripts/check-local-billing.sh`; its end-to-end entrypoint is `sh scripts/check-local-billing-e2e.sh`. The broader historical `scripts/check.sh` remains available unchanged. New profile checks do not certify deferred PostgreSQL, multi-host or resource-proof requirements.

Publish source, license, examples, these requirements and truthful release notes. Any native artifact additionally needs a checksum, exact source/build identity, dependency/license inventory, provenance and a successful installed-artifact walkthrough. Do not claim an MSRV, reproducible build, signed provenance, notarization or untested platform support without evidence. A native artifact may be withheld while the source is prepared; missing mandatory release QA/CI still blocks a tag.

Only intended Git history and approved release artifacts are publishable. Retained local databases, customer data, credentials, tool caches and internal session dumps remain private. Historical technical reports may refer to local evidence paths that are unavailable in a public checkout; those references are not bundled evidence or current acceptance proof.

See [release profile](release/local-sqlite.md), [quickstart](docs/billing-quickstart.md) and [contributor rules](AGENTS.md).

## Amended local Phases 5 and 6 (2026-09-23)

The released local SQLite v0.1.0 baseline replaced the original full-Phase-4 prerequisite for these phases. The reduced local Phase 4 is complete. Phase 5 is complete and published: one generic finance CSV projection of authoritative retained postings with explicit validated account mapping, pinned complete snapshot, stable record/export IDs, exact signed amounts and correction references, deterministic retries, reconciliation and honest file-failure behavior. It does not add accounting/tax policy, imports, a vendor connector, delivery acknowledgment or payments. See [WORKFLOW.md](WORKFLOW.md) and [docs/finance-csv.md](docs/finance-csv.md).

The amended local Phase 6 is **complete with owner acceptance** after exact-candidate supported-path QA, author inspection, material fixes and review preparation. The owner explicitly waived the separate independent-review requirement. Independent technical review did not occur or pass; author work is not presented as independent. The original full-platform both-store, remote-destination, multi-host and protected-resource matrices remain outside this completed local scope. Preserve all existing guards and frozen bytes.

The local SQLite and CSV work was published through the Bean release gates; [v0.2.1](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.2.1) is the current native release. Historical branch-only and release-authorization steps are preserved in the original task evidence, not active publication blockers. Future releases still require the normal gates and accurate disclosure of the waived review. Hosted service, shared-tooling change, purchase and production-data use remain outside scope. Other deferred/paused capabilities remain deferred.
