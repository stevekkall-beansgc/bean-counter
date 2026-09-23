#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: run-synthetic-macos.sh LEDGER_BINARY NEW_BILLING_DIRECTORY NEW_RESULTS_DIRECTORY" >&2
    exit 2
fi

ledger=$1
installation=$2
results=$3
examples=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ] || [ "$(sw_vers -productVersion)" != "26.6.2" ]; then
    echo "unsupported host: this example targets tested macOS 26.6.2 with Apple silicon" >&2
    exit 2
fi
case "$("$ledger" --version)" in
    "ledger 0.2.1 (local development)") ;;
    *) echo "expected the pinned v0.2.1 ledger binary" >&2; exit 2 ;;
esac
if [ -e "$installation" ] || [ -L "$installation" ]; then
    echo "refusing existing installation path: $installation" >&2
    exit 2
fi
if [ -e "$results" ] || [ -L "$results" ] || [ "$results" = "$installation" ]; then
    echo "refusing existing or reused results path: $results" >&2
    exit 2
fi

umask 077
mkdir -m 700 "$results"

run_json() {
    result_file=$1
    shift
    if "$@" --json > "$result_file"; then
        return 0
    else
        result=$?
        echo "billing command failed with exit $result:" >&2
        cat "$result_file" >&2
        return "$result"
    fi
}

# This v0.2.1 binary predates the candidate-only guided command. The explicit
# setup JSON is synthetic; real operators must supply their own terms and evidence.
cp "$examples/setup-synthetic.json" "$results/setup-synthetic.json"
run_json "$results/setup-result.json" "$ledger" billing init "$installation" --setup "$examples/setup-synthetic.json"

run_json "$results/accept-success.json" "$ledger" billing --directory "$installation" accept "$examples/work-success.json"
success_target=$(plutil -extract receipt.body.target raw -o - "$results/accept-success.json")
run_json "$results/retry-success.json" "$ledger" billing --directory "$installation" accept "$examples/work-success.json"
plutil -replace target -string "$success_target" -o "$results/outcome-success.json" "$examples/outcome-success-template.json"
run_json "$results/outcome-success-result.json" "$ledger" billing --directory "$installation" outcome "$results/outcome-success.json"

run_json "$results/accept-unsuccessful.json" "$ledger" billing --directory "$installation" accept "$examples/work-unsuccessful.json"
unsuccessful_target=$(plutil -extract receipt.body.target raw -o - "$results/accept-unsuccessful.json")
plutil -replace target -string "$unsuccessful_target" -o "$results/outcome-unsuccessful.json" "$examples/outcome-unsuccessful-template.json"
run_json "$results/outcome-unsuccessful-result.json" "$ledger" billing --directory "$installation" outcome "$results/outcome-unsuccessful.json"

run_json "$results/statement.json" "$ledger" billing --directory "$installation" statement --customer synthetic-customer
net=$(plutil -extract net_atoms raw -o - "$results/statement.json")
complete=$(plutil -extract complete raw -o - "$results/statement.json")
if [ "$net" != "100" ] || [ "$complete" != "true" ]; then
    echo "statement did not reconcile: expected complete net of 100 atoms" >&2
    cat "$results/statement.json" >&2
    exit 1
fi
snapshot=$(plutil -extract snapshot_hash raw -o - "$results/statement.json")
echo "Complete statement saved to $results/statement.json (snapshot $snapshot)."
echo "Synthetic example reconciled: +2 +98 +2 -2 = 100 USD atoms (USD 1.00)."
echo "The retry used the identical event file. Outcome evidence was synthetic operator attestation, not model verification."
