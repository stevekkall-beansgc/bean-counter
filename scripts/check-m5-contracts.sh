#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
PYTHONPATH="work/check-deps${PYTHONPATH:+:$PYTHONPATH}" \
  python3 scripts/contract_checks/check_m5_contract.py
