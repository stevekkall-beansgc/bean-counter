# M8 native-package qualification candidate

M8 remains **Partial**. v0.9.0 is an unreleased distribution candidate; the latest published native archives remain v0.3.0. A passing source test or a workflow definition does not qualify an archive or complete the release gates.

The bounded targets are `aarch64-apple-darwin` on macOS 26.6.2 and `x86_64-unknown-linux-gnu` on Ubuntu 24.04.5 with glibc 2.39. No other OS, architecture or runtime is certified. Billing contract v0.3, schema 11 and the existing version-output format are preserved.

## Candidate qualification procedure

Build from a clean exact source commit using the pinned Rust 1.98.1 toolchain and locked dependencies. The native wrappers pass an explicit target to Cargo. The archive includes the CLI, requirements, recovery and billing guides, workflow, billing/usage/finance/integration examples, dependency licenses, SPDX inventory, unsigned provenance, build/linkage information and a complete file-hash manifest.

Obtain the verifier, Python installer, target shell installer, archive, checksum file and expected full source SHA from the same trusted candidate/release distribution. Strict verification precedes extraction and binary execution. The scripts reject duplicate or noncanonical paths, links, special files, unsafe modes, duplicate JSON keys, missing files, hash mismatches and inconsistent source/version/target evidence, including under Python optimization. The checksum and author-generated provenance are not signatures.

Run the bundled `scripts/check-native-package-journey.py` against the installed `ledger` from a directory outside the source checkout. Supply `--old-binary` pointing to a separately built v0.7.0 binary from `48d76ae249fbfcb79a5012b265da51a71cb996e3` for schema-10 migration evidence. The script uses only synthetic records and ordinary CLI commands; no clock override or database rewrite creates acceptance evidence. A short daily period uses an explicit local anchor and waits for its actual boundary before close. The journey checks fixed and scale-18 work, customer terms, cumulative conversion, before/after-close corrections, exactly-once adjustment presentation, recurrence, fiscal reports, finance export, retry/conflict, retained upgrade identities, older-writer refusal and whole-installation restore.

Run the retained CLI subprocess regressions with `LEDGER_BINARY` set to the installed executable. The source-built manifest-owned unit/e2e gates and exact-SHA compliance CI remain separate mandatory release checks. The Linux workflow retains the exact archive and its `QUALIFICATION.json`, rather than rebuilding bytes after qualification.

## Evidence and release boundary

`MANIFEST.json` and `PROVENANCE.json` identify the exact source commit/tree, Cargo.lock checksum, package version, target and binary hash. `BUILD-INFO.txt` and `BUILD-LINKAGE.txt` record actual host/runtime/toolchain and the bundled SQLite source identity; SPDX covers the locked dependencies and SQLite license. `SHA256SUMS` binds the archive bytes. The standalone journey emits actual host/runtime, binary identity and per-scenario results. Keep operational stores and receipts out of Git; only synthetic qualification summaries and reviewed release evidence may be published.

No signing/notarization, signed attestation, reproducible-build proof, clean-account downloaded launch, broader runtime support, resource guarantees, hosted operation or payment capability is claimed. Earlier v0.3.0-to-current multi-step upgrade paths are not qualified here.

A release requires both exact-target walkthroughs, independent candidate review, clean registered default-branch source, manifest-owned QA, exact-SHA CI, honest scheduler evidence, reachable origin and the governed release command. Any unresolved gate blocks tags/publication and leaves M8 Partial. Current qualification results and remaining blockers must be recorded in issue #5 and the eventual release notes before marking M8 complete.
