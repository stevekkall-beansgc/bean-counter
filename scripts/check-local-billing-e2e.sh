#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
case "$(rustc --version)" in
  'rustc 1.98.1 '*) ;;
  *) echo 'Use the pinned Rust 1.98.1 development compiler.' >&2; exit 1 ;;
esac
cargo test -p ledgerlab-cli --all-targets --all-features --locked --offline
