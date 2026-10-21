# Test the unpublished 0.10.0 workflow candidate

This is an independent synthetic test handoff, not a release announcement or a migration guide for a live business. No tag or GitHub Release has been created. Published v0.9.7 billing assets remain separate. Retain the candidate's exact source SHA, target, archive checksum, installed executable hash and qualification record.

The [supported recipe](local-workflow-supported-recipe.md) is narrow: one work source, one retail binding, fixed successful service plus attributable app cost, and one direct outcome family. It supports the listed corrections, causal reversal, closure and original retries. Generic Unit pricing, multiple families/sources, supplier AP/payment, hosting and resource guarantees remain outside this claim.

## Prepare the exact package before work

Use only an archive supplied with matching `SHA256SUMS`, verifier, installer and exact-source qualification record. Do not substitute a v0.9.7 download. Check the record's actual host/target results; a source build, emulation or another OS version does not certify your machine. Packages remain unsigned/unnotarized; clean-account downloaded launch is unverified.

You need Python 3.11+, a private physical directory, compatible local storage, CPU/RAM/free disk and your own backup/recovery process. Record OS/version, CPU architecture, RAM, free disk and filesystem for the test report. No benchmark or completion guarantee is claimed.

Run in the directory containing the supplied archive and verification files. Set `SOURCE_COMMIT` from the qualification record after checking its version is `0.10.0`. Use the target matching that record and your native host:

```sh
VERSION=0.10.0
TARGET=aarch64-apple-darwin
# On the qualified native Ubuntu x86-64 host instead:
# TARGET=x86_64-unknown-linux-gnu
SOURCE_COMMIT=REPLACE_WITH_EXACT_40_CHARACTER_CANDIDATE_SHA
python3 verify-native-package.py "bean-counter-v$VERSION-$TARGET.tar.gz" \
  SHA256SUMS "$SOURCE_COMMIT" --version "$VERSION" --target "$TARGET"
mkdir -m 700 "$HOME/bean-counter-candidate-test"
TEST_PARENT=$(cd "$HOME/bean-counter-candidate-test" && pwd -P)
python3 install-native-package.py "bean-counter-v$VERSION-$TARGET.tar.gz" \
  SHA256SUMS "$SOURCE_COMMIT" "$TARGET" "$TEST_PARENT/package"
cd "$TEST_PARENT/package"
OUT="$TEST_PARENT/results"
mkdir -m 700 "$OUT"
mkdir -m 700 "$OUT/helper-bin"
install -m 700 helpers/prepare_local_publication "$OUT/helper-bin/prepare_local_publication"
install -m 700 helpers/prepare_local_terms "$OUT/helper-bin/prepare_local_terms"
LEDGER="$PWD/ledger"
PUB="$OUT/helper-bin/prepare_local_publication"
TERMS="$OUT/helper-bin/prepare_local_terms"
"$LEDGER" --version
```

The last command must report exactly `ledger 0.10.0 (local development)`. The verifier checks contents, modes, hashes, source/target identities, SBOM and provenance before installation executes anything. The helpers prepare inputs; they grant no authority. Their bytes match the package; they are copied because companion files are stored without an executable bit.

Choose a new parent path if it already exists. Never change an unrelated directory's permissions. On macOS, `pwd -P` avoids `/tmp` or `/var` symlink aliases reaching the engine. Build/install everything before the live one-hour main outcome window starts. Never backdate or extend accepted terms.

## Main test and expected history

```sh
python3 scripts/e2e/workflow_supported.py --ledger "$LEDGER" \
  --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/main"
```

This creates a fresh synthetic installation, sets all nine optional resource settings to null, retains evidence/assent/authority, publishes, accepts exact terms and activates before work. Null removes operational quotas; financial/format/integrity checks, cancellation and coordination/IO bounds remain.

| Milestone | Retained retail total |
| --- | ---: |
| Fixed successful service | $1.00 |
| Final attributable app cost | $3.00 |
| Success premium | $3.25 |
| Outcome corrected to None | $3.00 |
| Accepted 20% service discount | $2.80 |
| App cost corrected from $2.00 to $1.60 | $2.40 |
| Root and exact dependent reversals | $0.00 |

Final inverses are −$1.00, −$1.60 and +$0.20. Earlier records stay immutable. Reconciliation follows same-Work/Binding decision/action associations, not a guessed prefix or whole-customer total. No supplier payable or payment is created. Separate-process reopening/status and seven exact original retries must return the same original receipts, with identity duplicates and no logical-table append.

If financial work already committed but verification stopped, do not replay the sequence. Use only the saved-request mode in a new evidence directory:

```sh
python3 scripts/e2e/workflow_supported.py --case verify-main --resume \
  --verify-from "$OUT/main" --output "$OUT/main-verification" \
  --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS"
```

Preserve the original `RUN.json` on failure. Resolve an unknown commit using original status/identical retry against the same installation, never a replacement identity. A refusal does not authorize changing terms, rights, timestamps or guards. Stop at an unexpected failure and report it.

## Other bounded checks

Follow the [example commands](../examples/workflow-supported/README.md) for separate pending, no-success, honestly elapsed deadline, finite negatives, terminal-outcome refusal and whole-installation restore. Pending work reverses $1 to zero; its genuine but unused final proof then cannot finalize closed work. No-success/deadline each retain only $1, no OutcomeClaim, and terminal family `closed_no_success`/`expired`. The deadline case accepts its own short window before Work and waits for real expiry.

N4 compares twin private states: the same correction succeeds unrestricted and refuses after genuine public current-rights restriction. That denial may report generic `INTEGRITY_FAILURE`. This is a diagnostic limitation, not a rule that every integrity error means insufficient authority. Other stale/wrong-subject/identity/maximum/unsupported/quota/terminal cases must refuse without financial append.

Restore copies the whole quiescent installation, including retained WAL, and must return original receipts with no append. Copying an active database file alone is not this procedure. Logical/canonical invariance is checked; raw DB/WAL byte identity across legitimate reopen/checkpoint activity is not claimed.

Developer qualification separately records two actual financial interruption/lost-reply tests, migration and older-writer checks. Ignored discovery is not test execution. The opt-in test stack matches the observed CLI stack and is recorded, not swept; no default-stack/capacity claim follows.

## Report safely

Use the workflow-candidate issue form. Include version, source SHA, target, archive/executable SHA256, host/resources, command, expected/actual outcome, exit/error code and sanitized synthetic request/receipt IDs. Share only reviewed small `RUN.json` entries. Never upload a complete installation, credentials, real customer evidence or private paths.

The distribution qualification record distinguishes exact-source local QA, exact-SHA CI and each native run. Earlier `3b6fdf6`/`353e2ca` proofs retain their own identities. This document alone establishes neither public distribution, unfamiliar-human acceptance, another platform nor release approval.
