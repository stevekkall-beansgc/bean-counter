#!/bin/sh
# The unpublished executable proof stays separate from production Cargo files.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../../../../.." && pwd)
cd "$root"
proof=crates/ledgerlab/src/store/postgres/proof
harness=work/postgres-driver-proof
mkdir -p "$harness"
cp "$proof/driver-proof.toml" "$harness/Cargo.toml"
cp "$proof/driver-proof.lock" "$harness/Cargo.lock"
cargo fmt --manifest-path "$harness/Cargo.toml" -- --check
cargo test --manifest-path "$harness/Cargo.toml" --locked --offline
cargo clippy --manifest-path "$harness/Cargo.toml" --all-targets --locked --offline -- -D warnings
cargo check --manifest-path "$harness/Cargo.toml" --no-default-features --locked --offline
python3 "$proof/source_audit.py" "$harness/Cargo.toml"
