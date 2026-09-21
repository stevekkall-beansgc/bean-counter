#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
python3 scripts/check_boundaries.py
