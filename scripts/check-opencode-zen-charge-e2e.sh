#!/bin/sh
# Live provider-backed E2E only. No Rust unit/contract tests, downloads or installs.
set -eu
cd "$(dirname "$0")/.."

if ! command -v opencode >/dev/null 2>&1; then
  echo 'BLOCKED: OpenCode CLI is absent on PATH. E2E not run.' >&2
  exit 77
fi

build_target="$PWD/work/opencode-zen-charge-target"
if ! command -v rustc >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
  echo 'BLOCKED: pinned Rust 1.98.1 and cargo are required to build the ordinary candidate CLI. E2E not run.' >&2
  exit 77
fi
case "$(rustc --version)" in
  'rustc 1.98.1 '*) ;;
  *) echo 'BLOCKED: pinned Rust 1.98.1 required. E2E not run.' >&2; exit 77 ;;
esac
CARGO_TARGET_DIR="$build_target" cargo build -p ledgerlab-cli --bin ledger \
  --no-default-features --features zen-charge-candidate --locked --offline
candidate_binary="$build_target/debug/ledger"
build_manifest="$PWD/work/opencode-zen-charge-build.json"
PYTHONDONTWRITEBYTECODE=1 python3 - "$candidate_binary" "$build_manifest" <<'PY'
import hashlib, json, subprocess, sys
from pathlib import Path
binary, manifest = map(Path, sys.argv[1:])
root = Path.cwd()
status = subprocess.run(["git", "status", "--porcelain"], cwd=root,
                        capture_output=True, text=True, check=True).stdout
if status:
    raise SystemExit("BLOCKED: provider E2E requires a clean source worktree")
data = binary.read_bytes()
result = {
    "source_commit": subprocess.run(["git", "rev-parse", "HEAD"], cwd=root,
                                    capture_output=True, text=True, check=True).stdout.strip(),
    "source_tree": subprocess.run(["git", "rev-parse", "HEAD^{tree}"], cwd=root,
                                  capture_output=True, text=True, check=True).stdout.strip(),
    "rustc_version": subprocess.run(["rustc", "--version"], capture_output=True,
                                    text=True, check=True).stdout.strip(),
    "feature_profile": "--no-default-features --features zen-charge-candidate",
    "candidate_binary_sha256": hashlib.sha256(data).hexdigest(),
    "candidate_binary_bytes": len(data),
}
manifest.write_text(json.dumps(result, indent=2) + "\n")
PY

PYTHONDONTWRITEBYTECODE=1 python3 scripts/e2e/opencode_zen_charge.py \
  --ledger "$candidate_binary" --build-manifest "$build_manifest"
