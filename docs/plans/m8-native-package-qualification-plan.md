# M8 native package qualification plan

**Status:** execution plan for the existing M8 distribution milestone. This plan does not claim package qualification or authorize a release.

## Goal

Ship native macOS Apple-silicon and Linux x86-64 packages that contain the M5 product journey from one exact released source commit, and qualify the installed binaries and recovery path on the advertised operating systems. The canonical [billing roadmap](../billing-roadmap.md) remains the source of truth: M8 is partial until this evidence exists.

## Current baseline

- The latest source release is v0.8.1 at commit a0616630ef86826778e4a1d9ee20c4bcc28c40dc. M5 billing behavior was qualified in the source-built v0.8.0 release at 12df6a7e690edd9c8f1a4659bd29caf16508c57.
- The latest native packages are v0.3.0 and are not M5-qualified.
- The two bounded targets are macOS Apple silicon (aarch64-apple-darwin, tested host evidence: macOS 26.6.2) and Ubuntu 24.04 x86-64 (x86_64-unknown-linux-gnu, tested host evidence: Ubuntu 24.04.5/glibc 2.39).
- Current package scripts, the Linux candidate workflow, ledger --version, and package inventory contain v0.3.0 assumptions. Audit and reconcile those identities before producing a new archive. Preserve frozen CLI, storage, and billing contract identifiers; do not “fix” version drift by changing a contract ID.
- The M5 source qualification explicitly excludes native binaries. The schema-10 to schema-11 upgrade must be exercised using the installed package. Earlier schema upgrades may require more than one explicit invocation; do not advertise a one-step v0.3.0-to-current upgrade unless it is separately proven.

## Scope

1. Qualify only the two named targets above. Do not expand to Intel macOS, Linux ARM64, Windows, musl/Alpine, OCI images, other distributions, or a general platform matrix.
2. Choose the next artifact version through the repository's release process. Do not reuse an existing tag or label a new package v0.3.0.
3. Make release identity consistent across Cargo metadata, ledger --version, archive names, installers, package manifests, provenance, workflow names, and release notes. Record the exact source commit, target, toolchain, build host, binary hash, archive checksum, dependency/license inventory, and included file hashes. Do not claim signed provenance, notarization, reproducibility, or broader OS support without evidence.
4. Include the M5 quickstart, recovery guide, workflow examples, and synthetic fixtures in both archives. Verify from a clean directory using only the downloaded/extracted archive; the user must not need a source checkout.
5. Qualify the installed binary's M5 journey and recovery behavior on each target, then publish the exact verified archive bytes and evidence under the normal Bean release gates.

## Work sequence, persona, model, and manager

| Step | Work | Persona / owner | Model | Manager session |
| --- | --- | --- | --- | --- |
| 1. Baseline audit | Inventory version strings, archive contents, release metadata, current workflows, and exact platform prerequisites. Record discrepancies and proposed release identity. | Navy Bean, release operator; BEANA owns delivery | OpenAI Luna for the bounded inventory | OpenAI Sol |
| 2. Package and provenance repair | Update package scripts, verifier, installer, manifests, and Linux/macOS build paths to use one chosen release identity and accurate evidence. Preserve contract IDs and existing safety checks. | BEANA, implementation owner | OpenAI Luna, local repository session | OpenAI Sol |
| 3. Installed-path qualification | Build on the named hosts; install from extracted packages; run the acceptance journey below, including backup/restore and upgrade refusal/recovery. Capture exact evidence and hashes. | BEANA, qualification owner | OpenAI Luna, local repository session | OpenAI Sol |
| 4. Independent review and release | Review the exact source, archive bytes, platform claims, and evidence; fix findings; run the required exact-commit QA/CI/release gates; publish only the verified artifacts. | Astra, independent technical reviewer; Navy Bean, release operator | OpenAI Astra review, then OpenAI Luna for release validation | OpenAI Sol |

Project-specific repository content stays in local OpenAI sessions. Do not send source, private roadmap text, or repository evidence to the remote Zen free lane; the repository's governed route accepts only synthetic or published-public envelopes.

## Acceptance criteria

For **each** target:

- A fresh user can verify the archive checksum, install it, run ledger --version, and complete the documented workflow without a checkout or undocumented dependency.
- The binary completes fixed-price and scale-18 usage work, customer terms, period close, cumulative quantity conversion, a pre-close quantity delta, a post-close adjustment exactly once, recurrence, fiscal reporting, statement/finance export, and exact retry/conflict behavior.
- A supported schema-10 store upgrades through the documented explicit command to schema 11, preserving accepted history and retry identities. Unsupported older writers refuse before mutation. Backup/restore of the complete installation is demonstrated. Document any earlier-version multi-step upgrade path precisely.
- Archive contents, installer, dependency/license inventory, provenance, file manifest, binary SHA-256, archive SHA-256, source commit, target, and tested OS/runtime agree. Release notes state the exact tested environment and remaining limitations.
- The normal local billing, CLI/e2e, contract/schema, exact-commit CI, and Bean release gates pass on the intended source commit. No release tag or publication is made while a gate or either platform walkthrough is incomplete.

## Out of scope and stop conditions

No new billing behavior, database backend, hosted runtime, signing service, notarization, automatic update system, broad OS support, resource guarantee, or release infrastructure redesign. Stop if a target cannot be tested, the version identity conflicts with a frozen interface, the package cannot preserve M5 behavior, or the upgrade/recovery evidence is incomplete. Narrow the advertised claim or return with the specific blocker; do not infer qualification from source-built tests.

## Effort

Medium: approximately 4–8 engineering days, plus platform runner availability, independent review, and release-gate time. This assumes the existing package scripts are repairable and there is no M5 defect requiring product changes.

## Source references

- [Canonical billing roadmap](../billing-roadmap.md), especially M8 and storage-format activation limits.
- [Current local SQLite requirements](../../CURRENT-REQUIREMENTS.md), including native evidence and release acceptance.
- [M5 qualification](../m5-qualification.md), which excludes native packages and defines the source-built baseline.
