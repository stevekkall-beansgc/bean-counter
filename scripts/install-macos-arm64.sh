#!/bin/sh
set -eu

if [ "$#" -ne 4 ]; then
    echo "usage: install-macos-arm64.sh RELEASE_ARCHIVE SHA256SUMS EXPECTED_SOURCE_COMMIT NEW_INSTALL_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "incompatible host: this release archive is macOS Apple silicon" >&2
    exit 2
fi
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
python3 "$script_dir/install-native-package.py" "$1" "$2" "$3" aarch64-apple-darwin "$4"
