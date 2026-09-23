#!/bin/sh
# Populate locked development dependencies before offline acceptance checks.
set -eu
cd "$(dirname "$0")/.."
command -v rustup >/dev/null
command -v node >/dev/null
python3 --version
case "$(rustc --version 2>/dev/null || true)" in
  'rustc 1.98.1 '*) ;;
  *) rustup toolchain install 1.98.1 --profile minimal --component rustfmt --component clippy ;;
esac
rustc --version
python3 -m pip install --disable-pip-version-check --target work/check-deps -r scripts/requirements-contracts.txt
cargo fetch --locked
