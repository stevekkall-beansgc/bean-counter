#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: package-linux-x86_64.sh NEW_ARTIFACT_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
    echo "package must be built on Linux x86-64; cross-builds are not certified" >&2
    exit 2
fi

artifact_dir=$1
if [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ]; then
    echo "refusing existing artifact directory: $artifact_dir" >&2
    exit 2
fi
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=normal)" ]; then
    echo "refusing to package a dirty source checkout; commit the release source first" >&2
    exit 1
fi
cargo build --locked --release --manifest-path "$repo/Cargo.toml" -p ledgerlab-cli
target_dir=${CARGO_TARGET_DIR:-"$repo/target"}
version=$("$target_dir/release/ledger" --version)
case "$version" in
    "ledger 0.3.0 (local development)") ;;
    *) echo "unexpected release binary version: $version" >&2; exit 1 ;;
esac

commit=$(git -C "$repo" rev-parse HEAD)
archive="bean-counter-v0.3.0-x86_64-unknown-linux-gnu.tar.gz"
package="bean-counter-v0.3.0-x86_64-unknown-linux-gnu"
umask 077
mkdir -m 700 "$artifact_dir"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-linux-package.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -m 700 "$tmp/$package"
install -m 755 "$target_dir/release/ledger" "$tmp/$package/ledger"
cp "$repo/LICENSE" "$tmp/$package/LICENSE"
cp "$repo/NOTICE" "$tmp/$package/NOTICE"
cat > "$tmp/$package/START-HERE.md" <<'EOF'
# Bean Counter v0.3.0 — Linux x86-64

This unsigned native archive was built from the exact source commit in `BUILD-INFO.txt`. It includes the `ledger billing setup DIR` guided wizard, plus `billing init DIR --setup FILE --json` for unattended callers. `BUILD-INFO.txt` records the build OS, Rust toolchain, target and dynamic library linkage. The published release notes identify the tested Ubuntu environment.

The package requires Linux x86-64 and compatible GNU C library/runtime dependencies. `ledger --version` exercises loader/linkage compatibility, but the build metadata does not establish support for other distributions or older library versions. Use a private directory on durable local storage with working file locks and sync operations; shared/network storage, multiple writers and rollback detection are not guaranteed.

The synthetic integration helper and fixtures are under `examples/integration/`. They are demonstration data only and must never be used as real customer terms, assent, authority or outcomes. Python 3 is needed only by the shell helper to parse JSON. Follow `docs/billing-recovery.md` for quiescent backup and recovery.
EOF
mkdir -m 700 "$tmp/$package/examples"
mkdir -m 700 "$tmp/$package/examples/integration"
mkdir -m 700 "$tmp/$package/examples/finance"
for item in README.md setup-synthetic.json work-success.json work-unsuccessful.json outcome-success-template.json outcome-unsuccessful-template.json run-synthetic.sh; do
    cp "$repo/examples/integration/$item" "$tmp/$package/examples/integration/"
done
cp "$repo/examples/finance/"* "$tmp/$package/examples/finance/"
mkdir -m 700 "$tmp/$package/docs"
cp "$repo/docs/billing-recovery.md" "$tmp/$package/docs/"
cp "$repo/docs/finance-e2e.md" "$repo/docs/finance-csv.md" "$tmp/$package/docs/"
mkdir -m 700 "$tmp/$package/scripts"
cp "$repo/scripts/demo-finance.py" "$tmp/$package/scripts/"
cp "$repo/WORKFLOW.md" "$tmp/$package/"
{
    printf 'source_commit=%s\n' "$commit"
    printf 'version=0.3.0\n'
    printf 'target=x86_64-unknown-linux-gnu\n'
    rustc --version --verbose
    cargo --version
    cc --version 2>&1 | head -n 1 || true
    uname -a
    cat /etc/os-release 2>/dev/null || true
    ldd --version 2>&1 | head -n 1 || true
    ldd "$tmp/$package/ledger" 2>&1 || true
} > "$tmp/$package/BUILD-INFO.txt"
cp "$repo/CURRENT-REQUIREMENTS.md" "$tmp/$package/"
cp "$repo/docs/billing-quickstart.md" "$tmp/$package/docs/"
cp "$repo/examples/integration/README.md" "$tmp/$package/examples/integration/"
python3 "$repo/scripts/release-inventory.py" "$repo" "$tmp/$package" x86_64-unknown-linux-gnu
tar -czf "$artifact_dir/$archive" -C "$tmp" "$package"
(cd "$artifact_dir" && sha256sum "$archive" > SHA256SUMS)
printf '%s\n' "$artifact_dir/$archive"
