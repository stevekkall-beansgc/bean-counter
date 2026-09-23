#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: install-linux-x86_64.sh CANDIDATE_ARCHIVE SHA256SUMS NEW_INSTALL_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
    echo "incompatible host: this candidate archive is Linux x86-64" >&2
    exit 2
fi
archive=$1
sums=$2
destination=$3
if [ -e "$destination" ] || [ -L "$destination" ]; then
    echo "refusing existing destination: $destination" >&2
    exit 2
fi
if [ ! -f "$archive" ] || [ ! -f "$sums" ]; then
    echo "candidate archive and SHA256SUMS must both be regular files" >&2
    exit 2
fi
archive_name=$(basename -- "$archive")
if ! awk -v name="$archive_name" '$2 == name { count++; digest=$1 } END { if (count != 1 || length(digest) != 64) exit 1 }' "$sums"; then
    echo "SHA256SUMS must contain exactly one entry for the candidate archive" >&2
    exit 1
fi
expected=$(awk -v name="$archive_name" '$2 == name { print $1 }' "$sums")
actual=$(sha256sum "$archive" | awk '{print $1}')
if [ "$actual" != "$expected" ]; then
    echo "candidate archive checksum mismatch" >&2
    exit 1
fi
case "$archive_name" in
    bean-counter-setup-candidate-????????????-x86_64-unknown-linux-gnu.tar.gz) ;;
    *) echo "unexpected candidate archive name: $archive_name" >&2; exit 1 ;;
esac
package=${archive_name%.tar.gz}
tmp=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-linux-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
tar -tzf "$archive" | awk -v p="$package" '$0 == p "/ledger" { found=1 } END { exit !found }' || {
    echo "archive does not contain the expected candidate package" >&2
    exit 1
}
tar -xzf "$archive" -C "$tmp"
binary="$tmp/$package/ledger"
if [ ! -f "$binary" ] || [ ! -x "$binary" ]; then
    echo "candidate archive did not contain an executable ledger" >&2
    exit 1
fi
case "$($binary --version)" in
    "ledger 0.2.1 (local development)") ;;
    *) echo "unexpected executable version in candidate archive" >&2; exit 1 ;;
esac
umask 077
mkdir -m 700 "$destination"
cp -R "$tmp/$package/." "$destination/"
chmod 700 "$destination"
chmod 755 "$destination/ledger"
printf '%s  %s\n' "$(sha256sum "$destination/ledger" | awk '{print $1}')" "$destination/ledger"
echo "Installed verified candidate at $destination/ledger"
echo "This candidate package is not a published release; BUILD-INFO.txt records its build host and dynamic linkage."
