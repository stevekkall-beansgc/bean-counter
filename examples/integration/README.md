# Local product integration — Bean Counter

The current M5 local SQLite source profile includes customers and sources, fixed-price and scale-18 usage, explicit billing periods, recurrence and finance exports. Read the [billing quickstart](../../docs/billing-quickstart.md), [M2 caller contract](../../docs/billing-m2-cli-contract.md) and [M5 qualification](../../docs/m5-qualification.md). The [Python](billing_outbox.py) and [Node.js](billing_outbox.mjs) examples persist original requests and reconcile uncertain results. Their inputs are synthetic. The patched helpers in this checkout recognize complete history explanations `/2` and `/3`, while rejecting period statement `/4` and unknown versions. The helpers bundled in v0.9.0 still require `/2` and cannot acknowledge usage `/3`; the v0.9.1 distribution packages this correction. A pending acknowledgement does not mean the charge failed: preserve the original request and retry it after updating the helper.

## Native release and installation

[v0.9.1](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.1) is the patch distribution with corrected helpers and complete integration/operations guides. Obtain its exact source and native qualification/checksum evidence from that release. The [M8 qualification](../../docs/m8-native-package-qualification.md) preserves the v0.9.0 predecessor at source `2487dcfa9333b54c8622fd826e289fcaccb7ee22`. The bounded qualification targets are macOS 26.6.2 on Apple silicon and Ubuntu 24.04.5 x86-64/glibc 2.39; other versions and distributions remain unverified.

Use the archive, `SHA256SUMS`, expected source commit, verifier and installer from the same trusted release/candidate distribution. The installer and verifier must be obtained separately before extracting an unverified archive. Python 3.11+ is required for verification and installation; Rust and Node are unnecessary for ordinary installed billing. Do not treat the checksum or unsigned provenance as a signature.

```sh
# SOURCE_COMMIT is the full source SHA recorded in the trusted release evidence.
export PACKAGE_ROOT="$HOME/bean-counter-v0.9.1"
sh install-macos-arm64.sh bean-counter-v0.9.1-aarch64-apple-darwin.tar.gz SHA256SUMS "$SOURCE_COMMIT" "$PACKAGE_ROOT"
# Or on the named Ubuntu host:
sh install-linux-x86_64.sh bean-counter-v0.9.1-x86_64-unknown-linux-gnu.tar.gz SHA256SUMS "$SOURCE_COMMIT" "$PACKAGE_ROOT"
export LEDGER="$PACKAGE_ROOT/ledger"
"$LEDGER" --version
```

The installers strictly verify the archive and its manifest before extraction or executing its binary, refuse an existing destination, and create a private installation. Keep `install-native-package.py` and `verify-native-package.py` beside the shell installer. Inspect `BUILD-INFO.txt`, `PROVENANCE.json`, `MANIFEST.json` and `SBOM.spdx.json` for the exact version, source, target, toolchain, binary hash and runtime linkage. Native packages are unsigned; macOS signing/notarization and clean-account downloaded launch remain unverified.

## Configure real terms

`ledger billing setup DIR` asks an operator for actual parties, agreement, exact USD price, fixed outcome codes, UTC windows, retained assent evidence and truthful authority/finality attestations. Read and submit permissions are required; correction permission is optional and separately confirmed. The command validates and summarizes the terms, then requires an explicit `CREATE`. Cancelled or invalid input and an existing destination do not initialize a store. The generated `setup.json` is private (mode 0600). If its final copy fails after the store is initialized, the result reports success and separately identifies the retention failure; do not mistake that for rollback.

The program retains assertions but does not obtain customer consent, verify authority, or verify a model outcome. Consult the actual agreement and retain real assent evidence before using a private installation. Never use the bundled `setup-synthetic.json` as real customer configuration.

For a product or agent with a prepared setup file, use the noninteractive interface:

```sh
"$LEDGER" billing init "$BILLING_DIR" --setup "$SETUP_JSON" --json
```

Send strict JSON by regular file or stdin (`accept --customer C --source S -`, `outcome --customer C --source S -`, `correct --customer C --source S -`), request `--json`, and parse returned JSON in the product's own language. Keep event `id` and `operation_id` stable, pass explicit customer/source scope, and retain the receipt target. Preserve exit codes and reconcile an unknown result by reopening the same installation and retrying the identical input and IDs.

## Synthetic product journey

**Unreleased first-use correction:** the published v0.9.1 archive still bundles a helper that requires v0.9.0 and refuses its own binary. The corrected helper in this checkout accepts only the exact v0.9.1 identity; older distributions retain their own helpers, and future versions require fresh qualification. Delivering this correction requires a separately qualified patch release. Preserve published archives and checksums.

The setup, events and evidence are **synthetic only**. They are not customer assent, real operator authority or proof of a real model outcome. For a corrected candidate, keep `PACKAGE_ROOT` set to its absolute installed directory and `LEDGER` to its executable as above. To test this unreleased source correction with the verified installed v0.9.1 binary, keep `LEDGER` pointing to that binary and set `PACKAGE_ROOT` to the absolute corrected checkout directory. This is a source-correction test, not a successful run of the unchanged published helper.

Open a new private working directory outside the installation or checkout, then run this command there. The helper finds its fixtures beside itself, independent of your current directory. Python 3.11+ is required for this synthetic check; Node and Rust are unnecessary. The two paths under the working directory must not exist; choose another fresh directory when repeating:

```sh
sh "$PACKAGE_ROOT/examples/integration/run-synthetic.sh" \
  "$LEDGER" \
  "$PWD/bean-counter-demo-store" \
  "$PWD/bean-counter-demo-results"
```

The helper checks completed-work charges, explicit success and unsuccessful outcomes, stable identical retries, an authorized correction with exact inverse/replacement postings, a complete statement, permission history, restart and a quiescent copy/reopen whose statement is unchanged. The correction reconciles to zero atoms. It saves receipts and results in the new private results directory.

Use one process at a time on durable local storage. Stop every writer before copying the complete installation, preserve `.ledger` sidecars and reopen a separate verification copy with the matching binary. See [billing recovery](../../docs/billing-recovery.md). Shared/network storage, online backup, cross-host recovery and rollback detection are not guaranteed. The admission ceilings are 100,000 accepted decisions, 100,000 aliases, 1,000 permission changes, 256 MiB economic payloads and a separate 64 MiB alias ingress. These are limits, not capacity guarantees; M3 measured one synthetic workload of 3,499 retained decisions.
