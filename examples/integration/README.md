# Local product integration example

This directory contains integration material for the published **v0.2.1** Apple-silicon macOS artifact and a source-built Stage 2 Linux candidate. The published native behavior was exercised on macOS 26.6.2. The artifact metadata declares macOS 11.0 as its deployment minimum, but that does not certify runtime behavior across macOS versions; other versions are untested. The archive is unsigned and clean-account launch remains unverified. The Linux source candidate is not a published release and must pass the approved Ubuntu 24.04 x86-64 package/install/full-path CI before Linux support is stated.

## Install and verify the pinned artifact

On the tested host, download and verify the exact release artifact, then install it into a new directory:

```sh
./examples/integration/install-v0.2.1-macos.sh "$HOME/bean-counter-v0.2.1"
```

The script pins the v0.2.1 release URL and archive SHA-256 (`04703f40014abe594af9b54d8dc8477eddc20c762dff1980f434fe9d7462ee0f`). It checks that the published `SHA256SUMS` contains exactly that archive digest and checks the downloaded bytes against the same pinned value. It requires macOS and arm64, rejects versions below the artifact-declared macOS 11.0 minimum, and warns when the version differs from tested 26.6.2. This lower bound comes from Mach-O metadata; the OS loader decides binary loadability and the minimum does not certify behavior on all later versions. The installer refuses an existing destination, extracts into a temporary directory, checks the executable version, and prints the installed binary SHA-256. The artifact is not signed or notarized.

## Machine-readable product calls

The bundled setup file and event/evidence are **synthetic only**. They are not customer assent, real operator authority or evidence of a real model outcome. Run the end-to-end example with a fresh billing directory:

```sh
./examples/integration/run-synthetic.sh "$HOME/bean-counter-v0.2.1/ledger" "$HOME/bean-counter-demo-store" "$HOME/bean-counter-demo-results"
```

This end-to-end example uses the published v0.2.1 binary's noninteractive JSON initialization command. The new guided setup command is not present in that released binary. Product and agent callers can use:

```sh
"$LEDGER" billing init "$BILLING_DIR" --setup "$SETUP_JSON" --json
```

The example product submits `work-success.json` and `work-unsuccessful.json` as completed work, captures each machine-readable `receipt.body.target`, retries the identical success event and outcome files, then submits explicit success and unsuccessful-by-cutoff outcomes. It corrects the success outcome to unsuccessful using the current revision, retries that correction unchanged, and reconciles the complete statement to `0` atoms. The script uses Python 3's standard JSON library rather than platform-specific JSON tools. No outcome alone creates an adjustment or credit. All terms, evidence and results are synthetic; the ledger does not infer or verify a model result. It preserves setup, receipts, requests/responses, correction, statement, permissions and quiescent-copy/reopen results under the new private results directory.

## Candidate-only guided setup

The new `ledger billing setup DIR` command belongs to this Stage 2 source candidate only; it is not available in v0.2.1. It guides an operator through the actual terms without requiring a prepared JSON file. For existing prepared terms, `ledger billing setup DIR --setup FILE` remains available. For unattended callers use `billing init DIR --setup FILE --json`.

```sh
cargo build --release --locked -p ledgerlab-cli
./target/release/ledger billing setup "$HOME/new-private-billing"
```

It asks the operator for actual terms, retained evidence, and whether correction permission is authorized; validates them through the existing typed parser; displays a confirmation summary; and requires explicit `CREATE`. `cancel`, invalid input, and existing paths do not initialize a store. The generated `setup.json` is retained inside the new private installation with mode 0600. If that final copy fails after initialization, the JSON result reports success and separately identifies the failed target; a partial file is removed when possible. Consult the actual agreement and retained assent evidence to prepare a private repeatable configuration. Guided setup accepts a native macOS Apple-silicon or Linux x86-64 executable. macOS 26.6.2 was previously exercised, but this wizard requires the focused test. Linux x86-64 remains untested until the full approved Ubuntu 24.04 package/install/economic/recovery path passes CI. The published v0.2.1 artifact declares minimum macOS 11.0; the Stage 2 candidate source build has not been separately inspected for a deployment target. Windows remains deferred. Never use `setup-synthetic.json` as actual customer configuration.

The Linux package script is `sh scripts/package-linux-x86_64.sh ARTIFACT_DIRECTORY`; it prints a deterministic candidate archive named `bean-counter-setup-candidate-<12-character-commit>-x86_64-unknown-linux-gnu.tar.gz`. The installer accepts that archive, its adjacent `SHA256SUMS`, and a new destination. It checks Linux x86-64, archive digest, expected package contents and executable startup, then installs privately. To run the installed example, use `examples/integration/run-synthetic.sh` included in the archive. This package is not a released artifact or signed provenance claim.

The adapter contract is the CLI itself: send strict JSON on stdin (`accept -`, `outcome -`, `correct -`) or by regular file; request `--json`; parse the returned JSON in the product's own language. Keep event `id` and `operation_id` stable and retain the receipt target. Do not hide exit codes or convert an unknown commit outcome into a new identity. The setup input requires the operator's real terms, assent evidence and truthful authority/finality attestations. Never pass the synthetic setup file as real customer configuration.

## Storage and recovery

Use a new private local directory on durable storage. One process at a time may own an installation; no shared/network directory, online backup or cross-host recovery guarantee is provided. The limits are 1,000 accepted decisions, 1,000 aliases, 1,000 permission changes, 32 MiB economic payloads plus separate 32 MiB alias ingress, 64 KiB setup/permission input, 256 KiB event input and 8 MiB per accepted bundle. These are ceilings, not resource guarantees.

For a real installation, stop every writer before copying the complete installation, preserve `.ledger` sidecars, and reopen a separate verification copy with the matching binary. Follow [billing recovery](../../docs/billing-recovery.md). For an uncertain commit, reopen the same store and retry identical input with its original IDs. Keep refusal and integrity errors visible; do not delete a lock file or try a different identity to guess whether a write committed.
