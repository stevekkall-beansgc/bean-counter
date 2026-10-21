#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: package-macos-arm64.sh NEW_ARTIFACT_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "package must be built on native macOS arm64" >&2
    exit 2
fi
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target=aarch64-apple-darwin
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=all)" ]; then
    echo "refusing to package a dirty source checkout" >&2
    exit 1
fi
cd "$repo"
# The 0.10.0 candidate advertises the opt-in local workflow. Preserve the
# immutable capture beside the requested fresh artifact directory.
cargo fetch --locked
capture="$1.workflow-build"
python3 "$repo/scripts/build_workflow.py" --local-execution --operator-resources --output "$capture"
capture=$(CDPATH= cd -- "$capture" && pwd -P)
export LEDGER_WORKFLOW_BUILD_INPUTS_JSON="$capture/build-inputs.json"
export CARGO_TARGET_DIR="$capture/target"
cargo build --manifest-path "$capture/source/Cargo.toml" --package ledgerlab \
  --example prepare_local_publication --example prepare_local_terms \
  --target "$target" --release --locked --offline
python3 "$repo/scripts/package-native.py" "$repo" "$1" "$target" --workflow-candidate
