#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export PYTHONDONTWRITEBYTECODE=1
python3 scripts/contract_checks/check_phase4_freeze.py
