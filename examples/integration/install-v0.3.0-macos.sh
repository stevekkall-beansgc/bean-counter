#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: install-v0.3.0-macos.sh NEW_INSTALL_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "incompatible host: this release archive is macOS arm64" >&2
    exit 2
fi
host_version=$(sw_vers -productVersion)
if ! printf '%s\n' "$host_version" | awk -F. 'NF >= 2 && $1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ { ok=($1 >= 11) } END { exit !ok }'; then
    echo "incompatible or unknown macOS version '$host_version': the archive declares macOS 11.0 as its minimum" >&2
    exit 2
fi
if [ "$host_version" != "26.6.2" ]; then
    echo "warning: macOS $host_version is untested; the deployment minimum does not certify this version" >&2
fi
destination=$1
if [ -e "$destination" ] || [ -L "$destination" ]; then
    echo "refusing existing destination: $destination" >&2
    exit 2
fi
umask 077
tmp=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-v030-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
archive=bean-counter-v0.3.0-aarch64-apple-darwin.tar.gz
base=https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.3.0
curl -fsSL "$base/$archive" -o "$tmp/$archive"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"
expected=$(awk -v name="$archive" '$2 == name || $2 == ("*" name) { count++; hash=$1 } END { if (count != 1 || length(hash) != 64) exit 1; print hash }' "$tmp/SHA256SUMS")
actual=$(shasum -a 256 "$tmp/$archive")
actual=${actual%% *}
if [ "$actual" != "$expected" ]; then
    echo "release archive checksum mismatch" >&2
    exit 1
fi
mkdir "$tmp/extracted"
tar -xzf "$tmp/$archive" -C "$tmp/extracted"
package="$tmp/extracted/bean-counter-v0.3.0-aarch64-apple-darwin"
binary="$package/ledger"
if [ ! -x "$binary" ] || [ "$("$binary" --version)" != "ledger 0.3.0 (local development)" ]; then
    echo "release archive does not contain the expected executable" >&2
    exit 1
fi
mkdir -m 700 "$destination"
cp -R "$package/." "$destination/"
printf '%s  %s\n' "$(shasum -a 256 "$destination/ledger" | awk '{print $1}')" "$destination/ledger"
echo "Verified v0.3.0 binary installed at $destination/ledger"
echo "The published artifact is unsigned and unnotarized; macOS may show first-launch approval."
