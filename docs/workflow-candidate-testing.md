# Test the v0.11.0 bounded workflow checkpoint

Use the matching release assets and QUALIFICATION.json for exact publication/source/target/native results. This handoff is synthetic testing, not a live-business migration recommendation or a full Wave2b claim. Published v0.10.0 and older assets remain immutable and version-matched. The [supported recipe](local-workflow-supported-recipe.md) defines the exact scope.

You need Python 3.11+, compatible native storage/OS, a private physical directory and your own backup process. Packages are unsigned/unnotarized; other hosts, clean-account launch and unfamiliar-human acceptance are not inferred from a source build. Record actual OS/architecture, archive/binary hashes and the version-bound qualification. No resource completion guarantee is claimed.

## Verify and install before live work

Save the archive, SHA256SUMS, verifier, installer and platform installer from the SAME release. Set SOURCE_COMMIT to the exact 40-character source in QUALIFICATION.json after checking version0.11.0 and your target. Never substitute an older archive/helper.

```sh
VERSION=0.11.0
TARGET=aarch64-apple-darwin
# Qualified native Ubuntu x86-64 instead: TARGET=x86_64-unknown-linux-gnu
SOURCE_COMMIT=REPLACE_WITH_EXACT_40_CHARACTER_RELEASE_SHA
python3 verify-native-package.py "bean-counter-v$VERSION-$TARGET.tar.gz" SHA256SUMS "$SOURCE_COMMIT" --version "$VERSION" --target "$TARGET"
mkdir -m 700 "$HOME/bean-counter-v0.11.0-test"
TEST_PARENT=$(cd "$HOME/bean-counter-v0.11.0-test" && pwd -P)
python3 install-native-package.py "bean-counter-v$VERSION-$TARGET.tar.gz" SHA256SUMS "$SOURCE_COMMIT" "$TARGET" "$TEST_PARENT/package"
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

The version must be `ledger 0.11.0 (local development)`. That established binary text does not substitute for archive/source verification. The helpers prepare inputs and confer no authority; their exact bytes come from the package and are copied because companion files have no executable bit. Choose a new directory when one exists. On macOS use pwd-P physical paths instead of /tmp or /var aliases. Finish installation and helper preparation before accepting live windows; never backdate or extend accepted terms.

## Execute the bounded synthetic journeys

Follow [the conditional walkthrough](../examples/workflow-conditional/README.md). Use three separate new outputs: matching, nonmatching-finalized and nonmatching-pending. Matching totals are 3→5→5.30→2→0. Quantity correction3→2 appends−3,+2,−2,−0.30 and closes app/family permanently; final reverse appends only−2. A nonmatching service retains its genuine root and independent app dependency, with no service action/target/family. Finalized totals0→2→0→0 include only app+2/−2; pending totals0→0→0 retire the actual dependency and later finalization refuses TARGET_CLOSED. Original/replacement quantity proofs remain required even when the condition is false.

Every maintained journey retains exact requests, executable identity, canonical same-Work results and original receipt/status/retry checks. The finite context and proof/current-rights runners consume genuine isolated pre-acceptance/pre-work/pre-quantity copies. Missing context refuses INVALID_SHAPE; wrong enum type/value refuses INVALID_SCALAR; same-key changed context refuses IDENTITY_CONFLICT. Existing authority denials can report INTEGRITY_FAILURE; that generic code is not itself proof of an authority cause.

For a completed matching sequence, whole-installation restore uses saved original operations in a NEW output directory:

```sh
python3 scripts/e2e/workflow_supported.py --case restore --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/restored" --restore-from "$OUT/matching" --restore-operations main-quantity-proof main-proof-source-narrow main-work main-finalize main-success main-quantity-replacement-proof main-quantity-correct main-reverse
```

The source process must have ended; retain the whole installation including WAL. The runner checks unchanged original receipts and no logical append after reopening. Raw DB/WAL bytes can legitimately change through checkpointing. This is not a partial-chain continuation command. On unexpected failure preserve the original RUN.json, request bytes/key and installation; reconcile the exact original status/approved identical retry against that same store, never restart with a replacement identity or silently replay a whole financial chain. STATUS is ledger-operation-status/1; SUBMIT is ledger-operation-result/1. Stop and report an unresolved outcome.

Developer qualification separately records native installed legacy-schema upgrade/older-writer checks, old saved-history compatibility, actual interruption/reply-loss tests and mandatory QA. Ignored discovery is not execution. Fault workers retain explicit qualification stack conditions; these do not establish production/default-stack resource guarantees. Historical source proofs and final archive tests remain separately identified.

## Report safely

Use the workflow-candidate issue form with version, exact source/target/archive/binary hashes, actual host, command, expected/actual result and exit/error code. Include only reviewed redacted synthetic request/receipt IDs. Never upload a complete installation, private database/RUN/capture directory, credentials, business evidence or private paths. The matching public qualification and release notes carry sanitized final results; packaged source documentation alone is not publication or unfamiliar-human acceptance.
