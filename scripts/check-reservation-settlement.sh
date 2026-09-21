#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
python3 scripts/reservation_settlement/audit.py
node scripts/reservation_settlement/verify.mjs
# A file avoids pipeline failure masking if reconstruction itself fails.
probe_file=$(mktemp "${TMPDIR:-/tmp}/ledgerlab-settlement-attacks.XXXXXX")
trap 'rm -f "$probe_file"' EXIT HUP INT TERM
python3 scripts/reservation_settlement/audit.py --emit-attacks > "$probe_file"
node scripts/reservation_settlement/verify.mjs --attacks < "$probe_file"
