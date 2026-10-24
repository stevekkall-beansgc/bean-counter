#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: package-linux-x86_64.sh NEW_ARTIFACT_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
    echo "package must be built on native Linux x86-64; cross-builds are not certified" >&2
    exit 2
fi
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target=x86_64-unknown-linux-gnu
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=all)" ]; then
    echo "refusing to package a dirty source checkout" >&2
    exit 1
fi
cd "$repo"
# The 0.11.0 checkpoint advertises the opt-in local workflow. Preserve the
# immutable capture beside the requested fresh artifact directory.
cargo fetch --locked
capture="$1.workflow-build"
python3 "$repo/scripts/build_workflow.py" --local-execution --operator-resources --conditional-work --output "$capture"
capture=$(CDPATH= cd -- "$capture" && pwd -P)
export LEDGER_WORKFLOW_BUILD_INPUTS_JSON="$capture/build-inputs.json"
export CARGO_TARGET_DIR="$capture/target"
cargo build --manifest-path "$capture/source/Cargo.toml" --package ledgerlab \
  --example prepare_local_publication --example prepare_local_terms \
  --target "$target" --release --locked --offline
python3 "$repo/scripts/package-native.py" "$repo" "$1" "$target" --workflow-candidate
