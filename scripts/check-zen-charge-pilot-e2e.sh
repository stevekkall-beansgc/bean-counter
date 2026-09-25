#!/bin/sh
# Genuine subprocess E2E only. No cargo test, unit suite, downloads or installs.
set -eu
cd "$(dirname "$0")/.."
if ! command -v rustc >/dev/null 2>&1; then
  echo 'BLOCKED: rustc is absent on PATH; requires pinned Rust 1.98.1. E2E not run.' >&2
  exit 77
fi
case "$(rustc --version)" in
  'rustc 1.98.1 '*) ;;
  *) echo 'BLOCKED: pinned Rust 1.98.1 required. E2E not run.' >&2; exit 77 ;;
esac
if ! command -v cargo >/dev/null 2>&1; then
  echo 'BLOCKED: cargo is absent on PATH. E2E not run.' >&2
  exit 77
fi
# Separate artifact directories prevent feature-unification or stale-binary confusion.
export CARGO_TARGET_DIR="$PWD/work/zen-charge-default-target"
cargo build -p ledgerlab-cli --bin ledger --no-default-features --locked --offline
export CARGO_TARGET_DIR="$PWD/work/zen-charge-pilot-target"
cargo build -p ledgerlab-cli --bin ledger --no-default-features --features zen-charge-candidate --locked --offline
export CARGO_TARGET_DIR="$PWD/work/zen-charge-e2e-target"
cargo build -p ledgerlab-cli --bin ledger --no-default-features --features zen-charge-e2e-hooks --locked --offline
PYTHONDONTWRITEBYTECODE=1 python3 scripts/e2e/zen_charge_pilot.py \
  --ledger "$CARGO_TARGET_DIR/debug/ledger" \
  --candidate-ledger "$PWD/work/zen-charge-pilot-target/debug/ledger" \
  --default-ledger "$PWD/work/zen-charge-default-target/debug/ledger"
