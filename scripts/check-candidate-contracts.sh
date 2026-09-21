#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
python3 scripts/contract_checks/v2_candidate/audit.py
