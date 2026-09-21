#!/bin/sh
# Reproduce the stopped feasibility gate without changing workspace dependencies.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../../../../.." && pwd)
cd "$root"
diagnostics=crates/ledgerlab/src/store/postgres/diagnostics
harness=work/postgres-driver-gate
mkdir -p "$harness"
cp "$diagnostics/driver-gate.toml" "$harness/Cargo.toml"
cp "$diagnostics/driver-gate.lock" "$harness/Cargo.lock"

# Avoid incidental local PG settings in positive/negative handshake probes.
# The separate subprocess test intentionally injects synthetic PG settings.
unset PGHOST PGHOSTADDR PGPORT PGUSER PGPASSWORD PGDATABASE PGSSLMODE
unset PGSSLROOTCERT PGSSLCERT PGSSLKEY PGOPTIONS PGAPPNAME PGPASSFILE

cargo fmt --manifest-path "$harness/Cargo.toml" -- --check
cargo test --manifest-path "$harness/Cargo.toml" --locked --offline
cargo clippy --manifest-path "$harness/Cargo.toml" --all-targets --locked --offline -- -D warnings
python3 "$diagnostics/source_audit.py" "$harness/Cargo.toml"
