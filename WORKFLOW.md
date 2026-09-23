# Synthetic local billing → finance CSV

This is the amended Phase 5 workflow, based on the released local SQLite product. No customer data, vendor accounting integration, downstream delivery or payment is involved. Editable inputs are in `examples/finance/`; all assent/evidence there is illustrative. The example account labels are supplied mappings, not accounting or tax rules.

Build/install the local candidate with the pinned toolchain, then run the demonstration in a **new** output directory:

```sh
cargo install --path crates/ledgerlab-cli --locked --root ./work/finance-install
python3 scripts/demo-finance.py --ledger ./work/finance-install/bin/ledger --output ./work/finance-demo
```

The demonstration initializes explicit terms; accepts two independent fixed-price work events; retries exact and renamed deliveries; refuses a wrong-customer submission; records an outcome, replacement correction and reversal; retains an explanation; and exports/repeats a pinned statement. Every CLI invocation reopens the store. It verifies results using Python's CSV reader and integer arithmetic, rather than invoking the Rust evaluator for expected values.

Hand calculation (USD, scale 2):

| Decision | Stored signed atoms | Running balance |
| --- | --- | --- |
| First work | +250 | 250 |
| Second work | +250 | 500 |
| Quality rebate | −50 | 450 |
| Replace that rebate with the permitted rebate | +50 inverse, −50 replacement | 450 |
| Reverse the current rebate | +50 inverse | 500 |

The result is six posting rows totaling **500 cents = $5.00**, followed by a complete trailer. `finance.csv` and `finance-repeat.csv` must be byte-identical. `statement.json` retains the authoritative rows used for reconciliation; `RESULT.json` records actual command evidence and the explicit example consumer's six duplicate recognitions. This consumer is a demonstration, not a promise that another finance system deduplicates automatically.

For an existing installation, obtain `snapshot_hash` from `ledger billing --directory DIR statement --customer CUSTOMER`, then run:

```sh
ledger billing --directory DIR export-csv --customer CUSTOMER --snapshot HASH --mapping examples/finance/mapping.json --output finance.csv --json
```

Use a new output filename for every attempt. Existing paths are never replaced. On failure, no successful complete export is acknowledged; a final directory-sync failure can leave a complete file that requires inspection. Ledger history is unaffected by export failure. An updated ledger requires a new snapshot pin, while existing posting IDs stay unchanged for incremental consumer reconciliation.

Read [the exact CSV mapping and failure protocol](docs/finance-csv.md), [billing setup](docs/billing-quickstart.md), and [quiescent recovery](docs/billing-recovery.md). The examples' immutable outcome windows expire in early 2027; new agreements must use truthful explicit terms for their intended time period. This is not automatic renewal or repricing.

`--checks` adds isolated refusal, formula-text encoding, empty-history, remapping conflict, incremental export and read-revocation checks. The CLI integration suite runs that mode. Candidate QA uses `sh scripts/check-local-billing.sh` and `sh scripts/check-local-billing-e2e.sh`. Phase 6 author validation/review preparation does not replace independent acceptance, which remains platform-blocked. No new release is authorized by this workflow amendment.
