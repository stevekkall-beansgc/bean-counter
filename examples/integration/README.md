# Local product integration example

This is a language-neutral CLI example for the supported native profile. The scripts target only the published **v0.2.1** Apple-silicon macOS artifact, tested on macOS 26.6.2. The archive is unsigned. Other operating systems and clean-account launch are unverified.

## Install and verify the pinned artifact

On the tested host, download and verify the exact release artifact, then install it into a new directory:

```sh
./examples/integration/install-v0.2.1-macos.sh "$HOME/bean-counter-v0.2.1"
```

The script pins the v0.2.1 release URL and archive SHA-256 (`04703f40014abe594af9b54d8dc8477eddc20c762dff1980f434fe9d7462ee0f`). It checks that the published `SHA256SUMS` contains exactly that archive digest and checks the downloaded bytes against the same pinned value. It refuses unsupported OS/architecture/version or an existing destination, extracts into a temporary directory, checks the executable version, and prints the installed binary SHA-256. The artifact is not signed or notarized.

## Machine-readable product calls

The bundled setup file and event/evidence are **synthetic only**. They are not customer assent, real operator authority or evidence of a real model outcome. Run the end-to-end example with a fresh billing directory:

```sh
./examples/integration/run-synthetic-macos.sh "$HOME/bean-counter-v0.2.1/ledger" "$HOME/bean-counter-demo-store" "$HOME/bean-counter-demo-results"
```

This end-to-end example uses the published v0.2.1 binary's noninteractive JSON initialization command. The new guided setup command is not present in that released binary. Product and agent callers can use:

```sh
"$LEDGER" billing init "$BILLING_DIR" --setup "$SETUP_JSON" --json
```

The example product submits `work-success.json` and `work-unsuccessful.json` as completed work, captures each machine-readable `receipt.body.target`, retries the identical success event file, then submits explicit success and unsuccessful-by-cutoff outcomes against their respective targets. On macOS, `/usr/bin/plutil` reads the JSON target and fills the outcome template. The complete statement saved under the new private results directory must reconcile to `100` USD atoms (`+2 +98 +2 -2` = USD 1.00). No outcome alone creates an adjustment or credit. The product must supply evidence and an outcome; the ledger does not infer or verify a model result. The script preserves setup, receipts, request/response JSON and the reconciled statement there.

## Candidate-only guided setup

The new `ledger billing setup DIR --setup FILE` command belongs to this Stage 2 source candidate only; it is not available in v0.2.1. After building this checkout, try it with a fresh private path and your explicit terms file:

```sh
cargo build --release --locked -p ledgerlab-cli
./target/release/ledger billing setup "$HOME/new-private-billing" --setup "$HOME/private-setup.json"
```

It validates and summarizes terms, asks for explicit confirmation, and refuses existing paths or hosts outside the tested macOS 26.6.2 Apple-silicon environment. Never use `setup-synthetic.json` as actual customer configuration.

The adapter contract is the CLI itself: send strict JSON on stdin (`accept -`, `outcome -`, `correct -`) or by regular file; request `--json`; parse the returned JSON in the product's own language. Keep event `id` and `operation_id` stable and retain the receipt target. Do not hide exit codes or convert an unknown commit outcome into a new identity. The setup input requires the operator's real terms, assent evidence and truthful authority/finality attestations; copy and edit a private setup JSON for actual use. Never pass the synthetic setup file as real customer configuration.

## Storage and recovery

Use a new private local directory on durable storage. One process at a time may own an installation; no shared/network directory, online backup or cross-host recovery guarantee is provided. The limits are 1,000 accepted decisions, 1,000 aliases, 1,000 permission changes, 32 MiB economic payloads plus separate 32 MiB alias ingress, 64 KiB setup/permission input, 256 KiB event input and 8 MiB per accepted bundle. These are ceilings, not resource guarantees.

For a real installation, stop every writer before copying the complete installation, preserve `.ledger` sidecars, and reopen a separate verification copy with the matching binary. Follow [billing recovery](../../docs/billing-recovery.md). For an uncertain commit, reopen the same store and retry identical input with its original IDs. Keep refusal and integrity errors visible; do not delete a lock file or try a different identity to guess whether a write committed.
