# Download Bean Counter and get your first receipt

Bean Counter is an Apache-2.0 **local billing engine** for applications that produce completed generated work. Record an agreed fixed fee or quantity-based charge, keep a durable receipt, reconcile retries and make explained corrections. You run it on your computer and manage its storage and backups.

The current release is [v0.9.3 Open Beta](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.3). This page installs those released bytes and runs a synthetic example; no account, paid API, Rust compiler or Node installation is needed.

## 1. Check your computer

Native qualification covers **macOS 26.6.2 on Apple silicon** and **Ubuntu 24.04.5 x86-64 with glibc 2.39**. Other systems are unverified; Windows is unsupported. You need a terminal, `curl` and **Python 3.11 or newer** to verify/install and run the example. The installed billing executable itself does not need Python.

```sh
uname -sm
python3 --version
```

If Python is missing or older than 3.11, install a supported Python before continuing. Packages are unsigned/unnotarized. Clean-account downloaded launch is not yet qualified: if your OS blocks launch, record the exact message rather than disabling protection to force a pass.

## 2. Choose your download

Run **one** matching block in your terminal, then continue below in that same terminal.

For Apple-silicon Mac (`Darwin arm64`):

```sh
ARCHIVE=bean-counter-v0.9.3-aarch64-apple-darwin.tar.gz
INSTALLER=install-macos-arm64.sh
```

For the named Ubuntu x86-64 host (`Linux x86_64`):

```sh
ARCHIVE=bean-counter-v0.9.3-x86_64-unknown-linux-gnu.tar.gz
INSTALLER=install-linux-x86_64.sh
```

Download the archive and its matching verification/install files into a new private directory:

```sh
umask 077
DOWNLOAD_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-download.XXXXXX")
cd "$DOWNLOAD_DIR"
RELEASE_URL=https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.3
(
  for asset in "$ARCHIVE" "$INSTALLER" install-native-package.py verify-native-package.py SHA256SUMS QUALIFICATION.json; do
    curl --fail --location --remote-name "$RELEASE_URL/$asset" || exit 1
  done
)
```

All six downloads must succeed. Stop if a download fails; rerun the block in a fresh directory after resolving the error. You can instead save those same six files together from the [release page](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.3). Obtain the tools from that trusted release before extracting the archive. Checksums and unsigned provenance are not signatures.

## 3. Verify and install

The full source commit below identifies the **released executable**, even if the repository's documentation has since advanced. Choose an installation path that does not already exist; the installer refuses to overwrite one.

```sh
SOURCE_COMMIT=d9de6e2e50e846b2210c8c889f8d7493e127d5f7
export PACKAGE_ROOT="$HOME/bean-counter-v0.9.3"
sh "$INSTALLER" "$ARCHIVE" SHA256SUMS "$SOURCE_COMMIT" "$PACKAGE_ROOT"
export LEDGER="$PACKAGE_ROOT/ledger"
"$LEDGER" --version
```

Continue only after installation succeeds and the version reports `ledger 0.9.3 (local development)`. That is the released binary's exact version string. The installer verifies archive identity, file hashes, modes and inventory before running the binary, and creates a private installation.

## 4. Run the synthetic receipt journey

Use a fresh working directory. This example records completed work, success/unsuccessful outcomes, an authorized correction, identical retries and a quiescent backup/reopen. Its terms and evidence are synthetic, not real customer consent or authority.

```sh
DEMO_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-demo.XXXXXX")
cd "$DEMO_DIR"
demo_working_dir=$(pwd -P)
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

The current fixed outcome policy supports **bounded cohorts**, not indefinite continuously arriving work under one static agreement. Complete work no later than both ordinary/correction window starts, then report each outcome within its window. Setup requires a family and freezes its timing bounds on every base acceptance even if you submit no outcome. Read [the exact timing rules](docs/billing-quickstart.md#set-up-explicit-terms) before choosing real terms. Retain the original base target: outcome/correction receipts have a different shape. Follow [operation-specific reconciliation](docs/integration-agent-guide.md#reconcile-each-operation).

One operator-controlled business owns each private local SQLite installation. Supported operations include multiple customers/sources, fixed and scale-18 usage, effective agreements, immutable close statements, linked corrections, recurrence records, fiscal reporting and finance CSV. There is no hosted API, payment collection, automatic recurrence charging, tax/legal invoice certification, PostgreSQL billing or multi-host guarantee. See [scope and limitations](CURRENT-REQUIREMENTS.md).

## If something fails

- Download or verifier error: keep the message; check the platform, Python version and matching release files. Do not extract or execute an unverified archive.
- Existing installation path: choose a new `PACKAGE_ROOT`; do not overwrite or delete a business installation.
- Synthetic failure: preserve stdout/stderr and the reported results path. Retry the journey in a new private working directory.
- Unknown business write: retry its original bytes and IDs against the same installation. Integrity failure: stop writes and follow [recovery](docs/billing-recovery.md); do not erase locks or edit the database.

For beta feedback, record your OS/Python versions, the step and exact error, whether you needed help, and redacted synthetic results in a [GitHub issue](https://github.com/stevekkall-beansgc/bean-counter/issues). Never upload business records or credentials. No nominated participant list is required. [Release evidence](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.3/QUALIFICATION.json) records actual qualification; unfamiliar-user adoption remains unmeasured.
