#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
python3 scripts/contract_checks/check_design.py \
  --design docs/design/sources/LEDGER-LAB-V0-DETAILED-DESIGN.md \
  --output work/validation/document-checks.json
python3 scripts/contract_checks/check_phase0.py
