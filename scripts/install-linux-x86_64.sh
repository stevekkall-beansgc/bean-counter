#!/bin/sh
set -eu

if [ "$#" -ne 4 ] && { [ "$#" -ne 6 ] || [ "$5" != "--version" ]; }; then
    echo "usage: install-linux-x86_64.sh RELEASE_ARCHIVE SHA256SUMS EXPECTED_SOURCE_COMMIT NEW_INSTALL_DIRECTORY [--version VERSION]" >&2
    exit 2
fi
if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
    echo "incompatible host: this release archive is Linux x86-64" >&2
    exit 2
fi
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$#" -eq 6 ]; then
    python3 "$script_dir/install-native-package.py" "$1" "$2" "$3" x86_64-unknown-linux-gnu "$4" --version "$6"
else
    python3 "$script_dir/install-native-package.py" "$1" "$2" "$3" x86_64-unknown-linux-gnu "$4"
fi
