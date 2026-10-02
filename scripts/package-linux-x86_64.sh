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
cargo build --locked --release --target "$target" --manifest-path Cargo.toml -p ledgerlab-cli
python3 "$repo/scripts/package-native.py" "$repo" "$1" "$target"
