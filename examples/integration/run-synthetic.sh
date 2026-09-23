#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: run-synthetic.sh LEDGER_BINARY NEW_BILLING_DIRECTORY NEW_RESULTS_DIRECTORY" >&2
    exit 2
fi
ledger=$1
installation=$2
results=$3
examples=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -e "$installation" ] || [ -L "$installation" ] || [ -e "$results" ] || [ -L "$results" ] || [ "$results" = "$installation" ]; then
    echo "refusing existing or reused installation/results path" >&2
    exit 2
fi
command -v python3 >/dev/null 2>&1 || { echo "python3 is required to read/write JSON in this example" >&2; exit 2; }
case "$($ledger --version)" in
    "ledger 0.2.1 (local development)") ;;
    *) echo "expected a version 0.2.1 candidate or released ledger binary" >&2; exit 2 ;;
esac
umask 077
mkdir -m 700 "$results"

run_json() {
    result_file=$1
    shift
    if "$@" --json > "$result_file"; then return 0; else
        status=$?
        echo "billing command failed with exit $status: $*" >&2
        cat "$result_file" >&2
        return "$status"
    fi
}
json_value() {
    python3 -c 'import json,sys; value=json.load(open(sys.argv[1]));
for part in sys.argv[2].split("."): value=value[int(part)] if isinstance(value,list) else value[part]
print(value if isinstance(value,str) else json.dumps(value,separators=(",",":")))' "$1" "$2"
}
set_target() {
    python3 -c 'import json,sys; value=json.load(open(sys.argv[1])); value["target"]=sys.argv[2]; json.dump(value,open(sys.argv[3],"w"),separators=(",",":")); open(sys.argv[3],"a").write("\n")' "$1" "$2" "$3"
}

# Synthetic fixtures are conspicuously labeled; they are never real agreement terms.
cp "$examples/setup-synthetic.json" "$results/setup-synthetic.json"
run_json "$results/setup-result.json" "$ledger" billing init "$installation" --setup "$results/setup-synthetic.json"
run_json "$results/accept-success.json" "$ledger" billing --directory "$installation" accept "$examples/work-success.json"
success_target=$(json_value "$results/accept-success.json" receipt.body.target)
run_json "$results/retry-success.json" "$ledger" billing --directory "$installation" accept "$examples/work-success.json"
set_target "$examples/outcome-success-template.json" "$success_target" "$results/outcome-success.json"
run_json "$results/outcome-success-result.json" "$ledger" billing --directory "$installation" outcome "$results/outcome-success.json"
run_json "$results/retry-outcome-success.json" "$ledger" billing --directory "$installation" outcome "$results/outcome-success.json"

run_json "$results/accept-unsuccessful.json" "$ledger" billing --directory "$installation" accept "$examples/work-unsuccessful.json"
unsuccessful_target=$(json_value "$results/accept-unsuccessful.json" receipt.body.target)
run_json "$results/retry-unsuccessful.json" "$ledger" billing --directory "$installation" accept "$examples/work-unsuccessful.json"
set_target "$examples/outcome-unsuccessful-template.json" "$unsuccessful_target" "$results/outcome-unsuccessful.json"
run_json "$results/outcome-unsuccessful-result.json" "$ledger" billing --directory "$installation" outcome "$results/outcome-unsuccessful.json"

python3 - "$success_target" "$results/correction.json" <<'PY'
import json, sys
value = {
    "schema": "ledger-billing-correction/1", "id": "product-correction-1",
    "target": sys.argv[1], "family": "delivery",
    "occurred_at": "2026-09-23T13:00:00.000000Z",
    "evidence": "SYNTHETIC ONLY: correction replaces the success outcome for portability testing.",
    "expected_revision": "1", "replacement": {"kind": "code", "code": "unsuccessful-by-cutoff"}
}
with open(sys.argv[2], "w") as f:
    json.dump(value, f, separators=(",", ":")); f.write("\n")
PY
run_json "$results/correction-result.json" "$ledger" billing --directory "$installation" correct "$results/correction.json"
run_json "$results/retry-correction.json" "$ledger" billing --directory "$installation" correct "$results/correction.json"

run_json "$results/statement.json" "$ledger" billing --directory "$installation" statement --customer synthetic-customer
net=$(json_value "$results/statement.json" net_atoms)
complete=$(json_value "$results/statement.json" complete)
if [ "$net" != "0" ] || [ "$complete" != "true" ]; then
    echo "corrected statement did not reconcile: expected complete net of 0 atoms" >&2
    cat "$results/statement.json" >&2
    exit 1
fi
run_json "$results/permissions.json" "$ledger" billing --directory "$installation" permissions

# All CLI processes have exited. This is the documented administrator-enforced
# quiescent copy/reopen flow; it does not certify online or network backups.
backup="$results/billing-backup"
cp -Rp "$installation" "$backup"
run_json "$results/backup-statement.json" "$ledger" billing --directory "$backup" statement --customer synthetic-customer
backup_net=$(json_value "$results/backup-statement.json" net_atoms)
if [ "$backup_net" != "$net" ]; then echo "quiescent backup statement differs from source" >&2; exit 1; fi
run_json "$results/backup-retry.json" "$ledger" billing --directory "$backup" accept "$examples/work-success.json"
run_json "$results/backup-statement-after-retry.json" "$ledger" billing --directory "$backup" statement --customer synthetic-customer
python3 - "$results" <<'PY'
import json, os, sys
root = sys.argv[1]
def load(name):
    with open(os.path.join(root, name), encoding="utf-8") as f:
        return json.load(f)
def assert_same_receipt(first, second):
    a, b = load(first)["receipt"], load(second)["receipt"]
    assert a == b, f"receipt changed across identical retry: {first} / {second}"
    return a
success = assert_same_receipt("accept-success.json", "retry-success.json")
assert_same_receipt("accept-unsuccessful.json", "retry-unsuccessful.json")
assert_same_receipt("outcome-success-result.json", "retry-outcome-success.json")
assert_same_receipt("correction-result.json", "retry-correction.json")
success_target = success["body"]["target"]
assert load("outcome-success.json")["target"] == success_target
assert load("correction.json")["target"] == success_target
statement = load("statement.json")
assert statement["complete"] is True and statement["net_atoms"] == "0"
entries = {entry["external_id"]: entry for entry in statement["entries"]}
expected = {
    "product-work-success-1": [("2", "base-posting")],
    "product-outcome-success-1": [("98", "replacement")],
    "product-work-unsuccessful-1": [("2", "base-posting")],
    "product-outcome-unsuccessful-1": [("-2", "replacement")],
    "product-correction-1": [("-2", "replacement"), ("-98", "inverse")],
}
assert set(entries) == set(expected), sorted(entries)
for external_id, wanted in expected.items():
    postings = entries[external_id]["postings"]
    actual = [(p["body"]["amount"]["atoms"], p["body"].get("slot", "base-posting")) for p in postings]
    assert actual == wanted, f"{external_id}: expected {wanted}, got {actual}"
backup_statement = load("backup-statement.json")
after_retry = load("backup-statement-after-retry.json")
assert backup_statement["complete"] is True and after_retry["complete"] is True
assert backup_statement["net_atoms"] == after_retry["net_atoms"] == "0"
assert backup_statement["snapshot_hash"] == after_retry["snapshot_hash"] == statement["snapshot_hash"]
backup_receipt = load("backup-retry.json")["receipt"]
assert backup_receipt == success, "quiescent copy did not preserve the original acceptance receipt"
print("verified exact signed postings, correction inverse/replacement, stable retry receipts/targets, and complete unchanged backup statement")
PY

printf 'Synthetic full-path check passed: correction reconciled %s atoms; statement, permission history, restart, identical retries, and quiescent copy/reopen are saved under %s.\n' "$net" "$results"
echo "All agreement, assent, authority, work, outcomes, and correction data in this run are synthetic only."
