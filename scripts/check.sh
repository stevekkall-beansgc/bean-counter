#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
case "$(rustc --version)" in
  'rustc 1.98.1 '*) ;;
  *) echo 'Use the pinned Rust 1.98.1 development compiler.' >&2; exit 1 ;;
esac
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --offline
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo check --workspace --all-targets --no-default-features --locked --offline
sh scripts/check-boundaries.sh
sh scripts/check-contracts.sh
# Separately frozen reservation amendment; legacy contract gates stay unchanged.
python3 scripts/contract_checks/check_reservation_freeze.py
sh scripts/check-reservation-settlement.sh
