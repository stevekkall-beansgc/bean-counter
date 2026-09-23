# Run the local finance CSV end to end

This walkthrough uses **synthetic** customer and agreement data. It runs on the tested Apple-silicon macOS environment. Download `bean-counter-v0.2.1-aarch64-apple-darwin.tar.gz` and `SHA256SUMS` from the [v0.2.1 release](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.2.1). In the download directory, verify the archive before extracting it:

```sh
set -e
python3 - <<'PY'
import hashlib
from pathlib import Path
name = 'bean-counter-v0.2.1-aarch64-apple-darwin.tar.gz'
entries = dict(line.split(maxsplit=1) for line in Path('SHA256SUMS').read_text().splitlines())
actual = hashlib.sha256(Path(name).read_bytes()).hexdigest()
assert entries[actual].lstrip('*') == name, 'archive checksum does not match SHA256SUMS'
print('Archive SHA-256 verified:', actual)
PY
tar -xzf bean-counter-v0.2.1-aarch64-apple-darwin.tar.gz
cd bean-counter-v0.2.1-aarch64-apple-darwin
./ledger billing --help
```

Use a new private output directory outside the downloaded package:

```sh
set -e
E2E_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-e2e.XXXXXX")"
python3 scripts/demo-finance.py --ledger ./ledger --output "$E2E_DIR/example"
python3 scripts/demo-finance.py --ledger ./ledger --output "$E2E_DIR/extended" --checks
```

The first run initializes explicit terms, accepts two $2.50 work items, retries the first with its exact and renamed identity, refuses a different customer, applies a 50-cent rebate, replaces that rebate with an authorized correction, reverses the current rebate, explains retained work and exports a complete JSON statement and two finance CSVs. Every command reopens the SQLite installation. The second run adds refusal, output failure, empty-history, revocation, incremental-export and outcome-pricing examples. The script records actual command results in `RESULT.json` under each output directory.

The extended run creates four fresh stores under `extended/outcome-pricing/`. Their synthetic balances demonstrate the supported fixed-amount workflow: completed work alone posts `+2` atoms (USD $0.02); a success outcome posts `+2,+98` ($1.00); an explicitly evidenced `unsuccessful-by-cutoff` outcome posts `+2,-2` ($0.00); and correcting success to unsuccessful retains `+2,+98,-98,-2` ($0.00). It retries the identical success outcome and confirms no additional posting. The outcome is an operator-attested synthetic example, not automatic verification of a real model result. Absence of an outcome alone does not issue a credit. Base billing is for completed work, not reserved capacity.

Keep the timestamps ordered when adapting the example: the base event's `occurred_at` must be no later than both the ordinary and correction `starts_at`; each window must satisfy `starts_at < occurs_before <= received_by <= accepted_by`. Submit a success or unsuccessful outcome only with its observed occurrence time and required retained evidence, before its receipt and acceptance cutoffs. Corrections use their own correction window and the current `expected_revision`. The demo sets the base event to September 22, its windows to start September 22, and the synthetic outcome/correction to September 23.

The hand calculation is `+250 +250 −50 +50 −50 +50 = +500` USD atoms, or **$5.00**. Expect six posting rows, a complete trailer with `posting_count=6`, `control_net_atoms=500`, and cutoff `5`. The two CSVs must have identical bytes and `export_id`. The example consumer should insert six records on first import and identify six duplicates on repeat. It is a demonstration consumer, not a claim that an external finance system will deduplicate automatically.

Verify those checks against the saved files:

```sh
set -e
python3 - "$E2E_DIR/example" <<'PY'
import csv, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
statement = json.loads((root / 'statement.json').read_text())
result = json.loads((root / 'RESULT.json').read_text())
assert statement['complete'] and statement['net_atoms'] == '500' and statement['cutoff'] == '5'
assert result['same_input_repeat_identical'] and result['consumer_repeat_duplicates'] == 6
assert (root / 'finance.csv').read_bytes() == (root / 'finance-repeat.csv').read_bytes()
rows = list(csv.DictReader((root / 'finance.csv').open(newline='')))
assert len(rows) == 7 and rows[-1]['row_type'] == 'complete'
assert sum(int(row['amount_atoms']) for row in rows[:-1]) == 500
assert rows[-1]['posting_count'] == '6' and rows[-1]['control_net_atoms'] == '500'
print('Complete statement and repeated CSV reconcile to $5.00')
PY
```

The first demonstration leaves a usable installation. Once its commands have exited, make a quiescent whole-installation copy and reopen a *separate verification copy*:

```sh
set -e
cp -Rp "$E2E_DIR/example/store" "$E2E_DIR/store-backup"
cp -Rp "$E2E_DIR/store-backup" "$E2E_DIR/store-verify"
./ledger billing --directory "$E2E_DIR/store-verify" statement --customer customer-1 --json > "$E2E_DIR/restored-statement.json"
python3 - "$E2E_DIR/example/statement.json" "$E2E_DIR/restored-statement.json" <<'PY'
import json, sys
original, restored = (json.load(open(path)) for path in sys.argv[1:])
assert original == restored
print('Quiescent verification copy reopens with the exact statement')
PY
```

Read [quiescent backup and recovery](billing-recovery.md) before using a copy as an active installation; do not run two copies as writers. Inspect `WORKFLOW.md` and `docs/finance-csv.md` for editable inputs, mapping, export IDs and file-failure retries. Outputs, customer installations and backups are private operator data. Local file creation is not accounting-system delivery or payment. Independent acceptance remains platform-blocked; this walkthrough is for the owner's planned test and does not pre-report its result.
