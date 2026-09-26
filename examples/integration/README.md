# Local product integration — Bean Counter v0.3.0

For caller-owned request persistence, original receipt verification and recovery after an unknown commit result, see the [billing CLI integration contract](../../docs/billing-cli-contract.md) and its [Python](billing_outbox.py) or [Node.js](billing_outbox.mjs) synthetic example. These examples accept an already initialized installation and an existing strict event JSON file; they do not perform setup.

The v0.3.0 release provides a language-neutral `ledger` CLI with a guided terminal setup command and a strict JSON interface for unattended products. Its native packages are unsigned. The tested hosts are macOS 26.6.2 on Apple silicon and Ubuntu 24.04.5 x86-64 with glibc 2.39. Other OS versions and Linux distributions remain unverified; Windows is deferred. The archive's deployment minimum or target triple is not a broader compatibility guarantee.

## Install a release package

Download the matching archive and `SHA256SUMS` from the [v0.3.0 release](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.3.0). Verify the archive digest against `SHA256SUMS` and the digest in the release notes, then extract it into a new private directory. On Apple-silicon macOS, the checked-in helper performs the download, checksum check, extraction, executable version check and private install:

```sh
sh examples/integration/install-v0.3.0-macos.sh "$HOME/bean-counter-v0.3.0"
```

The Linux archive is `bean-counter-v0.3.0-x86_64-unknown-linux-gnu.tar.gz`. On a tested Linux x86-64 host, after downloading that archive and `SHA256SUMS` into the same directory, run the installer from a v0.3.0 source checkout:

```sh
sh scripts/install-linux-x86_64.sh bean-counter-v0.3.0-x86_64-unknown-linux-gnu.tar.gz SHA256SUMS "$HOME/bean-counter-v0.3.0"
```

The Linux binary is dynamically linked; its build metadata and release notes identify the actual runner and glibc environment. The native binary needs no Rust, Node or hosted account for local billing. Python 3 is needed only for the bundled synthetic integration helper.

## Configure real terms

`ledger billing setup DIR` asks an operator for actual parties, agreement, exact USD price, fixed outcome codes, UTC windows, retained assent evidence and truthful authority/finality attestations. Read and submit permissions are required; correction permission is optional and separately confirmed. The command validates and summarizes the terms, then requires an explicit `CREATE`. Cancelled or invalid input and an existing destination do not initialize a store. The generated `setup.json` is private (mode 0600). If its final copy fails after the store is initialized, the result reports success and separately identifies the retention failure; do not mistake that for rollback.

The program retains assertions but does not obtain customer consent, verify authority, or verify a model outcome. Consult the actual agreement and retain real assent evidence before using a private installation. Never use the bundled `setup-synthetic.json` as real customer configuration.

For a product or agent with a prepared setup file, use the noninteractive interface:

```sh
"$LEDGER" billing init "$BILLING_DIR" --setup "$SETUP_JSON" --json
```

Send strict JSON by regular file or stdin (`accept -`, `outcome -`, `correct -`), request `--json`, and parse returned JSON in the product's own language. Keep event `id` and `operation_id` stable and retain the receipt target. Preserve exit codes and reconcile an unknown commit result by reopening the same installation and retrying the identical input and IDs.

## Synthetic product journey

The bundled setup, events and evidence are **synthetic only**. They are not customer assent, real operator authority or proof of a real model outcome. Run the installed helper with fresh private paths:

```sh
sh "$HOME/bean-counter-v0.3.0/examples/integration/run-synthetic.sh" \
  "$HOME/bean-counter-v0.3.0/ledger" \
  "$HOME/bean-counter-demo-store" \
  "$HOME/bean-counter-demo-results"
```

The helper checks completed-work charges, explicit success and unsuccessful outcomes, stable identical retries, an authorized correction with exact inverse/replacement postings, a complete statement, permission history, restart and a quiescent copy/reopen whose statement is unchanged. The correction reconciles to zero atoms. It saves receipts and results in the new private results directory.

Use one process at a time on durable local storage. Stop every writer before copying the complete installation, preserve `.ledger` sidecars and reopen a separate verification copy with the matching binary. See [billing recovery](../../docs/billing-recovery.md). Shared/network storage, online backup, cross-host recovery and rollback detection are not guaranteed. The admission ceilings are 1,000 accepted decisions, 1,000 aliases, 1,000 permission changes, 32 MiB economic payloads and a separate 32 MiB alias ingress; these are limits, not capacity guarantees.
