# Local product integration — Bean Counter

The current M5 local SQLite source profile includes customers and sources, fixed-price and scale-18 usage, explicit billing periods, recurrence and finance exports. Read the [billing quickstart](../../docs/billing-quickstart.md), [M2 caller contract](../../docs/billing-m2-cli-contract.md) and [M5 qualification](../../docs/m5-qualification.md). The [Python](billing_outbox.py) and [Node.js](billing_outbox.mjs) examples persist original requests and reconcile uncertain results. Their inputs are synthetic. The patched helpers in this checkout recognize complete history explanations `/2` and `/3`, while rejecting period statement `/4` and unknown versions. The helpers bundled in v0.9.0 still require `/2` and cannot acknowledge usage `/3`; the v0.9.1 distribution packages this correction. A pending acknowledgement does not mean the charge failed: preserve the original request and retry it after updating the helper.

## Native release and installation

These instructions match **v0.9.6 Open Beta** with unchanged billing contract v0.3 and schema 11. The [matching release page](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.6) and [version-bound qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.6/QUALIFICATION.json) determine actual publication, exact source and qualified archive identities. A source checkout or native candidate alone does not establish qualification or publication. Use these instructions with matching published assets; older releases retain their own instructions and immutable bytes. For the complete public download → verified installation → synthetic receipt sequence, start at [START-HERE.md](../../START-HERE.md). The qualification targets are macOS 26.6.2 Apple silicon and Ubuntu 24.04.5 x86-64/glibc 2.39; other versions and distributions remain unverified.

Historical **[v0.9.5 Open Beta](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.5)** was released from `3406c1782b45a28ef39b974bf23978c72b44764e`. Its [version-bound qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.5/QUALIFICATION.json) identifies that release's actual checks and immutable assets. Use its matching instructions for those bytes; the v0.9.6 fixes require their own qualification.

Historical **[v0.9.4 Open Beta](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.4)** was released from `e897ef108753447371282cb5253fc88634e5977d`. Exact-source QA/CI, both native installed journeys and matching public-download checks passed for that version; see its [qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.4/QUALIFICATION.json). Its evidence applies only to those immutable assets.

Use the archive, `SHA256SUMS`, expected source commit, verifier and installer from the same trusted release. The installer and verifier must be obtained separately before extracting an unverified archive. Python 3.11+ is required for verification and installation; Rust and Node are unnecessary for ordinary installed billing. Do not treat the checksum or unsigned provenance as a signature.

```sh
# Obtain QUALIFICATION.json from the trusted matching release and compare its SHA with the release notes.
SOURCE_COMMIT=$(python3 -c 'import json,re,sys; q=json.load(open("QUALIFICATION.json")); s=q["source_commit"]; sys.exit("wrong qualification identity") if q["version"]!="0.9.6" or not re.fullmatch("[0-9a-f]{40}",s) else None; print(s)')
INSTALL_PARENT=$(cd -P "$HOME" && pwd -P)
export PACKAGE_ROOT="$INSTALL_PARENT/bean-counter-v0.9.6"
sh install-macos-arm64.sh bean-counter-v0.9.6-aarch64-apple-darwin.tar.gz SHA256SUMS "$SOURCE_COMMIT" "$PACKAGE_ROOT"
# Or on the named Ubuntu host:
sh install-linux-x86_64.sh bean-counter-v0.9.6-x86_64-unknown-linux-gnu.tar.gz SHA256SUMS "$SOURCE_COMMIT" "$PACKAGE_ROOT"
export LEDGER="$PACKAGE_ROOT/ledger"
"$LEDGER" --version
```

The installers strictly verify the archive and its manifest before extraction or executing its binary, refuse an existing destination, and create a private installation. Keep `install-native-package.py` and `verify-native-package.py` beside the shell installer. Inspect `BUILD-INFO.txt`, `PROVENANCE.json`, `MANIFEST.json` and `SBOM.spdx.json` for the exact version, source, target, toolchain, binary hash and runtime linkage. Native packages are unsigned; macOS signing/notarization and clean-account downloaded launch remain unverified.

## Use private physical paths

The engine rejects symlink components in installation, store and JSON input paths. On macOS, familiar `/tmp` and `/var` paths are aliases; obtain the physical directory before passing any child path to the CLI. Use this literal recipe for a fresh synthetic trial:

```sh
umask 077
BC_TRIAL_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-trial.XXXXXX")
cd -P "$BC_TRIAL_DIR"
export BC_TRIAL_ROOT="$(pwd -P)"
printf '%s\n' "$BC_TRIAL_ROOT"
```

Pass children such as `"$BC_TRIAL_ROOT/cohort"`. This temporary trial directory is not a business backup plan. For durable installation, choose an operator-owned private durable parent, obtain its physical path with `cd -P`/`pwd -P`, then select a new child. Resolve only paths deliberately chosen and trusted by the operator. Do not follow arbitrary input symlinks or relax the guard: JSON inputs must be regular files under physical paths, or use supported stdin. Keep `LEDGER` bound to the verified executable and preserve its SHA-256. A trial parent must have no group/other access, normally mode 0700. For a deliberately chosen trial directory, use the quoted commands `mkdir -p "$BC_TRIAL_ROOT"` and `chmod 700 "$BC_TRIAL_ROOT"`, obtain its physical path again, then choose a fresh child. The bounded example reports the actual parent path and mode before writing; it never chmods an existing business directory.

## Configure real terms

`ledger billing setup DIR` asks an operator for actual parties, agreement, exact USD price, fixed outcome codes, UTC windows, retained assent evidence and truthful authority/finality attestations. Read and submit permissions are required; correction permission is optional and separately confirmed. The command validates and summarizes the terms, then requires an explicit `CREATE`. Cancelled or invalid input and an existing destination do not initialize a store. The generated `setup.json` is private (mode 0600). If its final copy fails after the store is initialized, the result reports success and separately identifies the retention failure; do not mistake that for rollback.

The program retains assertions but does not obtain customer consent, verify authority, or verify a model outcome. Consult the actual agreement and retain real assent evidence before using a private installation. Never use the bundled `setup-synthetic.json` as real customer configuration.

For a product or agent with a prepared setup file, use the noninteractive interface:

```sh
"$LEDGER" billing init "$BILLING_DIR" --setup "$SETUP_JSON" --json
```

The required fixed outcome family has immutable absolute ordinary/correction windows. Base work must truthfully occur no later than **both** starts, even when you will submit no outcome. The supported flow is a bounded cohort completed after setup and before the earlier start, with outcomes/corrections reported in their windows. Each adjustment permits `starts_at <= occurred_at < occurs_before`, with ordered occurrence/receipt/acceptance and inclusive receipt/acceptance cutoffs. It does not support indefinitely advancing work under one static agreement. Read [the complete timing rules](../../docs/billing-quickstart.md#set-up-explicit-terms); synthetic fixture dates are illustrative observations, never dates to copy onto real work.

Send strict JSON by regular file or stdin (`accept --customer C --source S -`, `outcome --customer C --source S -`, `correct --customer C --source S -`), request `--json`, and parse returned JSON in the product's own language. For base events keep `id` and `operation_id` stable, pass explicit customer/source scope and retain `receipt.body.target` as the original base target. The bundled Python/Node outboxes verify **base acceptance only**; they do not acknowledge outcomes or corrections. Those inputs use their own `id`, not a base `operation_id`, and return `kind: "receipt"` without `body.target`. Keep their request target bound to the saved base target and use the [operation-specific reconciliation recipe](../../docs/integration-agent-guide.md#reconcile-each-operation). Preserve exit codes and reconcile an unknown result by reopening the same installation and retrying the identical input and IDs.

## Synthetic product journey

The v0.9.6 helper accepts only exact `ledger 0.9.6 (local development)`. Older distributions retain their own helpers; future versions require fresh qualification. The published v0.9.1 archive still bundles a synthetic helper requiring v0.9.0 and cannot run this route. Use the matching published v0.9.6 package for these instructions; preserve prior archives and checksums.

The setup, events and evidence are **synthetic only**. They are not customer assent, real operator authority or proof of a real model outcome. Keep `PACKAGE_ROOT` set to the absolute installed v0.9.6 directory and `LEDGER` to its executable as above. Before running, confirm `"$LEDGER" --version` reports the matching identity.

Open a new private working directory outside the installation or checkout, then run this command there. `pwd -P` resolves the physical directory so symlink aliases such as macOS `/tmp` or `/var` do not reach the engine's path guard. The helper finds its fixtures beside itself, independent of your current directory. Python 3.11+ is required for this synthetic check; Node and Rust are unnecessary. The two paths under the working directory must not exist; choose another fresh directory when repeating:

```sh
demo_working_dir=$(pwd -P)
sh "$PACKAGE_ROOT/examples/integration/run-synthetic.sh" \
  "$LEDGER" \
  "$demo_working_dir/bean-counter-demo-store" \
  "$demo_working_dir/bean-counter-demo-results"
```

The helper checks completed-work charges, explicit success and unsuccessful outcomes, stable identical retries, an authorized correction with exact inverse/replacement postings, a complete statement, permission history, restart and a quiescent copy/reopen whose statement is unchanged. The correction reconciles to zero atoms. It saves receipts and results in the new private results directory.

Use one process at a time on durable local storage. Stop every writer before copying the complete installation, preserve `.ledger` sidecars and reopen a separate verification copy with the matching binary. See [billing recovery](../../docs/billing-recovery.md). Shared/network storage, online backup, cross-host recovery and rollback detection are not guaranteed. The admission ceilings are 100,000 accepted decisions, 100,000 aliases, 1,000 permission changes, 256 MiB economic payloads and a separate 64 MiB alias ingress. These are limits, not capacity guarantees; M3 measured one synthetic workload of 3,499 retained decisions.


## Completed work and later assessment

The [offline bounded-cohort example](bounded-cohort/README.md), its full fixtures and qualification checker are bundled with the matching v0.9.6 executable. It records five actually generated local reports before both starts, a new later quality assessment, complete operation-specific receipts and exact retries. The separate evidence-classification scenario keeps verified zero, blocked, transport and parse errors distinct. No provider, account or network access is needed.

Keep `bounded-cohort.py` beside the entire `bounded-cohort/` fixture directory, including its four JSON files; copying just the Python file is incomplete. Use `--fixtures-dir "/absolute/physical/path/to/bounded-cohort"` for a deliberate separate copy. Missing fixtures are diagnosed before any write. Its fixed synthetic expectations remain separate from generic integer receipt/history reconciliation.

The distinct `failed-work` scenario retains four genuinely completed qualifying reports plus one offline exception before completion: 8 base + 8 adjustment = 16 atoms (USD 0.16), eight entries/retries, with no completion/base/target/outcome for the failed identity. `completed-work-assessment-error` retains the existing 2-atom base and unknown assessment without an adjustment. The original five-completed-artifacts quality-negative case remains 14 atoms/10 entries. A complete authorized history is not a claim that every attempt completed; summaries explicitly distinguish work counts, pending outcomes and ledger errors. Run all scenarios and path/dependency guards with `python3 "$PACKAGE_ROOT/scripts/check-bounded-cohort.py" "$LEDGER" --evidence "$BC_TRIAL_ROOT/qualification"` from the matching installed package, or use the checkout's script with the verified v0.9.6 executable for this candidate.
