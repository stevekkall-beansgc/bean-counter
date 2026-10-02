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
cargo build --locked --release --target "$target" --manifest-path Cargo.toml -p ledgerlab-cli
python3 "$repo/scripts/package-native.py" "$repo" "$1" "$target"
