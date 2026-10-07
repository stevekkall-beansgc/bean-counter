# Download Bean Counter and get your first receipt

Bean Counter is an Apache-2.0 **local billing engine** for applications that produce completed generated work. Record an agreed fixed fee or quantity-based charge, keep a durable receipt, reconcile retries and make explained corrections. You run it on your computer and manage its storage and backups.

These instructions match **v0.9.5 Open Beta** with unchanged billing contract v0.3 and schema 11. The [matching release page](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.5) and [version-bound qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.5/QUALIFICATION.json) determine actual publication, exact source and qualified archive identities. A source checkout or native candidate alone does not establish qualification or publication. Use these instructions with matching published assets; older releases retain their own instructions and immutable bytes. This page installs those version-matched bytes and runs a synthetic example; no account, paid API, Rust compiler or Node installation is needed. The package includes the [bounded-cohort example](examples/integration/bounded-cohort/README.md), its full fixtures and qualification checker.

Historical **[v0.9.4 Open Beta](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.4)** was released from `e897ef108753447371282cb5253fc88634e5977d`. Exact-source QA/CI, both native installed journeys and matching public-download checks passed for that version; see its [qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.4/QUALIFICATION.json). Its evidence applies only to those immutable assets.

## 1. Check your computer

The qualification targets are **macOS 26.6.2 on Apple silicon** and **Ubuntu 24.04.5 x86-64 with glibc 2.39**. Other systems are unverified; Windows is unsupported. You need a terminal, `curl` and **Python 3.11 or newer** to verify/install and run the example. The installed billing executable itself does not need Python.

```sh
uname -sm
python3 --version
```

If Python is missing or older than 3.11, install a supported Python before continuing. Packages are unsigned/unnotarized. Clean-account downloaded launch is not yet qualified: if your OS blocks launch, record the exact message rather than disabling protection to force a pass.

## 2. Choose your download

Run **one** matching block in your terminal, then continue below in that same terminal.

For Apple-silicon Mac (`Darwin arm64`):

```sh
ARCHIVE=bean-counter-v0.9.5-aarch64-apple-darwin.tar.gz
INSTALLER=install-macos-arm64.sh
```

For the named Ubuntu x86-64 host (`Linux x86_64`):

```sh
ARCHIVE=bean-counter-v0.9.5-x86_64-unknown-linux-gnu.tar.gz
INSTALLER=install-linux-x86_64.sh
```

Download the archive and its matching verification/install files into a new private directory:

```sh
umask 077
DOWNLOAD_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-download.XXXXXX")
cd -P "$DOWNLOAD_DIR"
RELEASE_URL=https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.5
(
  for asset in "$ARCHIVE" "$INSTALLER" install-native-package.py verify-native-package.py SHA256SUMS QUALIFICATION.json; do
    curl --fail --location --remote-name "$RELEASE_URL/$asset" || exit 1
  done
)
```

All six downloads must succeed. Stop if a download fails; rerun the block in a fresh directory after resolving the error. You can instead save those same six files together from the [release page](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.5). Obtain the tools from that trusted release before extracting the archive. Checksums and unsigned provenance are not signatures.

## Use private physical paths

The engine rejects symlink components in installation, store and JSON input paths. On macOS, familiar `/tmp` and `/var` paths are aliases; obtain the physical directory before passing any child path to the CLI. The fresh synthetic trial recipe appears in step 4, after installation.

Pass new children under the physical parent to the engine. A temporary trial directory is not a business backup plan. For durable installation, choose an operator-owned private durable parent, obtain its physical path with `cd -P`/`pwd -P`, then select a new child. Resolve only paths deliberately chosen and trusted by the operator. Do not follow arbitrary input symlinks or relax the guard: JSON inputs must be regular files under physical paths, or use supported stdin. Keep `LEDGER` bound to the verified executable and preserve its SHA-256. A trial parent must have no group/other access, normally mode 0700. For a deliberately chosen trial directory, use the quoted commands `mkdir -p "$BC_TRIAL_ROOT"` and `chmod 700 "$BC_TRIAL_ROOT"`, obtain its physical path again, then choose a fresh child. The bounded example reports the actual parent path and mode before writing; it never chmods an existing business directory.

## 3. Verify and install

The source commit comes from the matching qualification record downloaded from the trusted release. It identifies the executable independently of later documentation commits. Compare it with the full source SHA in the trusted release notes; stop if they differ. The archive manifest alone is not a trusted source for this expected identity. Choose an installation path that does not already exist; the installer refuses to overwrite one.

```sh
SOURCE_COMMIT=$(python3 -c 'import json,re,sys; q=json.load(open("QUALIFICATION.json")); s=q["source_commit"]; sys.exit("wrong qualification identity") if q["version"]!="0.9.5" or not re.fullmatch("[0-9a-f]{40}",s) else None; print(s)')
INSTALL_PARENT=$(cd -P "$HOME" && pwd -P)
export PACKAGE_ROOT="$INSTALL_PARENT/bean-counter-v0.9.5"
sh "$INSTALLER" "$ARCHIVE" SHA256SUMS "$SOURCE_COMMIT" "$PACKAGE_ROOT"
export LEDGER="$PACKAGE_ROOT/ledger"
"$LEDGER" --version
```

Continue only after installation succeeds and the version reports `ledger 0.9.5 (local development)`. That is the matching binary's exact version string. The installer verifies archive identity, file hashes, modes and inventory before running the binary, and creates a private installation.

## 4. Run the synthetic receipt journey

Use a fresh working directory. This example records completed work, success/unsuccessful outcomes, an authorized correction, identical retries and a quiescent backup/reopen. Its terms and evidence are synthetic, not real customer consent or authority.

```sh
umask 077
BC_TRIAL_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-trial.XXXXXX")
cd -P "$BC_TRIAL_DIR"
export BC_TRIAL_ROOT="$(pwd -P)"
printf '%s\n' "$BC_TRIAL_ROOT"
demo_working_dir="$BC_TRIAL_ROOT"
sh "$PACKAGE_ROOT/examples/integration/run-synthetic.sh" \
  "$LEDGER" \
  "$demo_working_dir/bean-counter-demo-store" \
  "$demo_working_dir/bean-counter-demo-results"
```

Success prints **`Synthetic full-path check passed`** and identifies the results directory. The final statement has **five retained entries and zero net atoms** after correction; original retry receipts and the recovered statement agree. Inspect the saved receipts and statement:

```sh
python3 -m json.tool "$demo_working_dir/bean-counter-demo-results/accept-success.json"
python3 -m json.tool "$demo_working_dir/bean-counter-demo-results/statement.json"
```

This is a correctness example, not measured unfamiliar-user timing. Keep its synthetic records separate from any business installation.

## 5. Connect your application

Start with the [integration guide](docs/integration-agent-guide.md) and [Python/Node examples](examples/integration/README.md#configure-real-terms). Configure your actual parties, agreed price/unit rate, assent evidence and permissions using [guided setup](docs/billing-quickstart.md#set-up-explicit-terms). Preserve exact request bytes and original identities when retrying an unknown result. Read the [operator and recovery checklist](docs/billing-operations.md) before business use.

The current fixed outcome policy supports **bounded cohorts**, not indefinite continuously arriving work under one static agreement. Complete work no later than both ordinary/correction window starts, then report each outcome within its window. The [offline bounded-cohort example](examples/integration/bounded-cohort/README.md) records real local completion times and genuinely later quality assessments. Its distinct synthetic trials preserve five completed reports with quality negatives (14 atoms/10 entries), four qualifying completions plus one failure before completion (16 atoms/8 entries), and a completed report with an unknown later assessment (unchanged 2-atom base). Keep the Python file with its entire fixture directory, or pass explicit `--fixtures-dir`; missing dependencies refuse before writing. `complete:true` describes authorized billing history, not success of every attempted job. Setup requires a family and freezes its timing bounds on every base acceptance even if you submit no outcome. Read [the exact timing rules](docs/billing-quickstart.md#set-up-explicit-terms) before choosing real terms. Retain the original base target: outcome/correction receipts have a different shape. Follow [operation-specific reconciliation](docs/integration-agent-guide.md#reconcile-each-operation).

One operator-controlled business owns each private local SQLite installation. Supported operations include multiple customers/sources, fixed and scale-18 usage, effective agreements, immutable close statements, linked corrections, recurrence records, fiscal reporting and finance CSV. There is no hosted API, payment collection, automatic recurrence charging, tax/legal invoice certification, PostgreSQL billing or multi-host guarantee. See [scope and limitations](CURRENT-REQUIREMENTS.md).

## If something fails

- Download or verifier error: keep the message; check the platform, Python version and matching release files. Do not extract or execute an unverified archive.
- Existing installation path: choose a new `PACKAGE_ROOT`; do not overwrite or delete a business installation.
- Synthetic failure: preserve stdout/stderr and the reported results path. Retry the journey in a new private working directory.
- Unknown business write: retry its original bytes and IDs against the same installation. Integrity failure: stop writes and follow [recovery](docs/billing-recovery.md); do not erase locks or edit the database.

For beta feedback, record your OS/Python versions, the step and exact error, whether you needed help, and redacted synthetic results in a [GitHub issue](https://github.com/stevekkall-beansgc/bean-counter/issues). Never upload business records or credentials. No nominated participant list is required. [Release evidence](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.5/QUALIFICATION.json) records the released archives' actual qualification; unfamiliar-user adoption remains unmeasured.
