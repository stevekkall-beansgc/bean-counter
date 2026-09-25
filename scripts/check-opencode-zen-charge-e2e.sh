#!/bin/sh
# Live provider-backed E2E only. No Rust unit/contract tests, downloads or installs.
set -eu
cd "$(dirname "$0")/.."

if ! command -v opencode >/dev/null 2>&1; then
  echo 'BLOCKED: OpenCode CLI is absent on PATH. E2E not run.' >&2
  exit 77
fi

build_target="$PWD/work/opencode-zen-charge-target"
candidate_binary="$PWD/work/zen-charge-pilot-target/debug/ledger"
if command -v rustc >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
  case "$(rustc --version)" in
    'rustc 1.98.1 '*) ;;
    *) echo 'BLOCKED: pinned Rust 1.98.1 required. E2E not run.' >&2; exit 77 ;;
  esac
  CARGO_TARGET_DIR="$build_target" cargo build -p ledgerlab-cli --bin ledger \
    --no-default-features --features zen-charge-candidate --locked --offline
  candidate_binary="$build_target/debug/ledger"
elif [ -x "$candidate_binary" ]; then
  for input in Cargo.lock Cargo.toml crates/ledgerlab-cli/Cargo.toml \
    crates/ledgerlab-core/Cargo.toml crates/ledgerlab/Cargo.toml .cargo/config.toml; do
    if [ -f "$input" ] && [ "$input" -nt "$candidate_binary" ]; then
      echo "BLOCKED: $input is newer than the prebuilt candidate CLI; use pinned Rust 1.98.1 to rebuild." >&2
      exit 77
    fi
  done
  newer_source="$(find crates/ledgerlab-cli crates/ledgerlab-core crates/ledgerlab \
    -type f \( -name '*.rs' -o -name Cargo.toml \) -newer "$candidate_binary" -print -quit)"
  if [ -n "$newer_source" ]; then
    echo "BLOCKED: $newer_source is newer than the prebuilt candidate CLI; use pinned Rust 1.98.1 to rebuild." >&2
    exit 77
  fi
  echo 'Using the fresh ordinary candidate CLI; the pinned compiler is unavailable.' >&2
else
  echo 'BLOCKED: pinned Rust 1.98.1 and a fresh ordinary candidate CLI are unavailable. E2E not run.' >&2
  exit 77
fi

PYTHONDONTWRITEBYTECODE=1 python3 scripts/e2e/opencode_zen_charge.py \
  --ledger "$candidate_binary"
