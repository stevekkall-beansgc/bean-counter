#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: package-macos-arm64.sh NEW_ARTIFACT_DIRECTORY" >&2
    exit 2
fi
if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "package must be built on macOS arm64" >&2
    exit 2
fi
artifact_dir=$1
if [ -e "$artifact_dir" ] || [ -L "$artifact_dir" ]; then
    echo "refusing existing artifact directory: $artifact_dir" >&2
    exit 2
fi
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "$(git -C "$repo" status --porcelain --untracked-files=normal)" ]; then
    echo "refusing to package a dirty source checkout" >&2
    exit 1
fi
cargo build --locked --release --manifest-path "$repo/Cargo.toml" -p ledgerlab-cli
target_dir=${CARGO_TARGET_DIR:-"$repo/target"}
case "$("$target_dir/release/ledger" --version)" in
    "ledger 0.3.0 (local development)") ;;
    *) echo "unexpected release binary version" >&2; exit 1 ;;
esac

package=bean-counter-v0.3.0-aarch64-apple-darwin
umask 077
mkdir -m 700 "$artifact_dir"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-macos-package.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -m 700 "$tmp/$package"
install -m 755 "$target_dir/release/ledger" "$tmp/$package/ledger"
cp "$repo/LICENSE" "$repo/NOTICE" "$repo/CURRENT-REQUIREMENTS.md" "$tmp/$package/"
cat > "$tmp/$package/START-HERE.md" <<'EOF'
# Bean Counter v0.3.0 — Apple-silicon macOS

This unsigned and unnotarized native archive includes `ledger billing setup DIR`, the guided terminal wizard. For products and unattended callers use `ledger billing init DIR --setup FILE --json` with real operator-provided terms and evidence. The included integration fixtures are synthetic demonstrations only; they are not customer assent or verified model outcomes. Follow `docs/billing-recovery.md` for quiescent backup and recovery. The release notes identify the tested macOS version; other versions remain unverified.
EOF
mkdir -m 700 "$tmp/$package/docs" "$tmp/$package/examples" "$tmp/$package/scripts"
mkdir -m 700 "$tmp/$package/examples/integration" "$tmp/$package/examples/finance"
cp "$repo/docs/billing-quickstart.md" "$repo/docs/billing-recovery.md" "$repo/docs/finance-e2e.md" "$repo/docs/finance-csv.md" "$tmp/$package/docs/"
for item in README.md setup-synthetic.json work-success.json work-unsuccessful.json outcome-success-template.json outcome-unsuccessful-template.json run-synthetic.sh; do
    cp "$repo/examples/integration/$item" "$tmp/$package/examples/integration/"
done
cp "$repo/examples/finance/"* "$tmp/$package/examples/finance/"
cp "$repo/scripts/demo-finance.py" "$tmp/$package/scripts/"
cp "$repo/WORKFLOW.md" "$tmp/$package/"
{
    printf 'source_commit=%s\n' "$(git -C "$repo" rev-parse HEAD)"
    printf 'version=0.3.0\n'
    printf 'target=aarch64-apple-darwin\n'
    rustc --version --verbose
    cargo --version
    uname -a
    sw_vers
    xcrun --show-sdk-version
    vtool -show-build "$tmp/$package/ledger" 2>&1 || true
    otool -L "$tmp/$package/ledger" 2>&1 || true
} > "$tmp/$package/BUILD-INFO.txt"
python3 "$repo/scripts/release-inventory.py" "$repo" "$tmp/$package" aarch64-apple-darwin
COPYFILE_DISABLE=1 tar -czf "$artifact_dir/$package.tar.gz" -C "$tmp" "$package"
(cd "$artifact_dir" && shasum -a 256 "$package.tar.gz" > SHA256SUMS)
printf '%s\n' "$artifact_dir/$package.tar.gz"
