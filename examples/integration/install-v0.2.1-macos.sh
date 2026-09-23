#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: install-v0.2.1-macos.sh NEW_INSTALL_DIRECTORY" >&2
    exit 2
fi

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "incompatible host: the pinned artifact is macOS arm64" >&2
    exit 2
fi
host_version=$(sw_vers -productVersion)
if ! printf '%s\n' "$host_version" | awk -F. 'NF >= 2 && $1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ { ok=($1 >= 11) } END { exit !ok }'; then
    echo "incompatible or unknown macOS version '$host_version': the published binary declares macOS 11.0 as its minimum" >&2
    exit 2
fi
if [ "$host_version" != "26.6.2" ]; then
    echo "warning: macOS $host_version is not the tested 26.6.2 host; the binary declares minos 11.0, which does not certify behavior on this version" >&2
fi

destination=$1
if [ -e "$destination" ] || [ -L "$destination" ]; then
    echo "refusing existing destination: $destination" >&2
    exit 2
fi

umask 077
tmp_base=${TMPDIR:-/tmp}
tmp_base=$(CDPATH= cd -P "$tmp_base" && pwd)
tmp=$(mktemp -d "$tmp_base/bean-counter-v021-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

version=v0.2.1
archive="bean-counter-${version}-aarch64-apple-darwin.tar.gz"
base="https://github.com/stevekkall-beansgc/bean-counter/releases/download/${version}"
curl -fL "$base/$archive" -o "$tmp/$archive"
curl -fL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"
expected_archive_sha=04703f40014abe594af9b54d8dc8477eddc20c762dff1980f434fe9d7462ee0f
listed_archive_sha=$(awk -v name="$archive" '$2 == name || $2 == ("*" name) { count++; hash=$1 } END { if (count != 1) exit 1; print hash }' "$tmp/SHA256SUMS")
if [ "$listed_archive_sha" != "$expected_archive_sha" ]; then
    echo "published checksum list does not match the pinned v0.2.1 archive digest" >&2
    exit 1
fi
actual_archive_sha=$(shasum -a 256 "$tmp/$archive")
actual_archive_sha=${actual_archive_sha%% *}
if [ "$actual_archive_sha" != "$expected_archive_sha" ]; then
    echo "downloaded v0.2.1 archive checksum mismatch" >&2
    exit 1
fi
echo "$actual_archive_sha  $archive"
mkdir "$tmp/extracted"
tar -xzf "$tmp/$archive" -C "$tmp/extracted"

package="$tmp/extracted/bean-counter-${version}-aarch64-apple-darwin"
binary="$package/ledger"
if [ ! -x "$binary" ]; then
    echo "release archive did not contain the expected executable" >&2
    exit 1
fi
case "$("$binary" --version)" in
    "ledger 0.2.1 (local development)") ;;
    *) echo "unexpected binary version in pinned archive" >&2; exit 1 ;;
esac
mkdir -m 700 "$destination"
cp -R "$package/." "$destination/"
shasum -a 256 "$destination/ledger"
echo "Verified v0.2.1 binary installed at $destination/ledger"
echo "The published artifact is unsigned; macOS may show first-launch approval."
