# Supported local workflow candidate

These inputs are synthetic. They create no live customer charge, provider payment or invoice. The supported scope and known diagnostic limitation are in [the recipe checklist](../../docs/local-workflow-supported-recipe.md). This is verification of the unpublished 0.10.0 candidate, not release approval. Start with [the tester handoff](../../docs/workflow-candidate-testing.md) for a new private output directory, archive identity checks and expected results.

Use the repository's pinned Rust toolchain. From this checkout, capture a normal operator-resource build, then build the two input-only helpers from that same captured source:

```sh
python3 scripts/build_workflow.py --local-execution --operator-resources --output "$BUILD"
LEDGER_WORKFLOW_BUILD_INPUTS_JSON="$BUILD/build-inputs.json" CARGO_TARGET_DIR="$BUILD/target" cargo build --manifest-path "$BUILD/source/Cargo.toml" \
  -p ledgerlab --example prepare_local_publication --example prepare_local_terms \
  --release --target aarch64-apple-darwin --locked --offline
```

For an assembled native workflow-candidate package, no local Rust build is needed. The existing package format stores every companion file privately without an executable bit. Copy the two input-only helper binaries into your private working directory before running them; their bytes remain unchanged:

```sh
mkdir -m 700 -p "$OUT/helper-bin"
install -m 700 helpers/prepare_local_publication "$OUT/helper-bin/prepare_local_publication"
install -m 700 helpers/prepare_local_terms "$OUT/helper-bin/prepare_local_terms"
LEDGER="$PWD/ledger"
PUB="$OUT/helper-bin/prepare_local_publication"
TERMS="$OUT/helper-bin/prepare_local_terms"
```

The commands below run from the package root as well as the source checkout.

For a source build, set `LEDGER` to `$BUILD/target/aarch64-apple-darwin/release/ledger`, `PUB` and `TERMS` to the corresponding `release/examples/prepare_local_publication` and `prepare_local_terms` executables. Set `OUT` to a new private output directory. Other native targets require their corresponding built executable paths.

```sh
python3 scripts/e2e/workflow_supported.py --ledger "$LEDGER" \
  --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/main"
```

For an already committed main sequence that stopped during reconciliation, verify only its saved seven original operations in a **new** evidence directory:

```sh
python3 scripts/e2e/workflow_supported.py --case verify-main --resume \
  --verify-from "$OUT/main" --output "$OUT/main-verification" \
  --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS"
```

This mode reads canonical same-Work decision/action associations and runs only original status and identical-request retries against the existing installation. It does not initialize, create facts, reconstruct current-revision requests or submit new financial work. It preserves the original `RUN.json`, records a separate runner identity, and checks every logical table before and after the tail. Reconciliation sums retained postings; it does not price or balance them.

The main case uses the existing public CLI for initialization, explicit all-null operator settings, source/customer/party evidence, publication, full assent, acceptance and activation. It then checks the fixed service, attributable cost, premium, None/discount/cost corrections and complete causal reversal. It saves a quiescent `main/pre-reverse-fixture` for finite negative checks. Prices come from the source policy and facts, not an output oracle supplied to the evaluator.

Run the three separate cases with the same executable/helper arguments and new output directories:

```sh
python3 scripts/e2e/workflow_supported.py --case pending --pending-finalize-check \
  --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/pending"
python3 scripts/e2e/workflow_supported.py --case no-success \
  --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/no-success"
python3 scripts/e2e/workflow_supported.py --case deadline \
  --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/deadline"
```

The pending check retains genuine usage/final-cost evidence without financially finalizing it, reverses the root, then verifies that the unconsumed proof cannot finalize the closed work. The deadline case publishes and accepts its separate short window before Work and waits for actual expiry. No clock or existing terms are changed. Every positive case performs public separate-process original status and identical retry with no append.

```sh
python3 scripts/e2e/workflow_supported_negatives.py --ledger "$LEDGER" \
  --fixture "$OUT/main/pre-reverse-fixture" --output "$OUT/negatives" \
  --groups N1 N2 N3 N4 N6 N8 N9
python3 scripts/e2e/workflow_supported_negatives.py --ledger "$LEDGER" \
  --fixture "$OUT/main" --output "$OUT/terminal-outcome" --groups N7
python3 scripts/e2e/workflow_supported.py --case restore \
  --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/restored" \
  --restore-from "$OUT/no-success" --restore-operations no-success-work no-success-close
```

Negative cases use private whole-installation copies and check no append. N4 retains a genuine unconsumed proof before copying twin states: the exact correction commits in the unrestricted control and refuses after public current-rights restriction in the other copy. Its generic integrity diagnostic is recorded as a limitation. N5 exact timing endpoints are additionally covered by deterministic core timing tests; no public timestamps are fabricated.

`RUN.json` retains executable/request/output hashes, command times, receipts and assertions. A failure stops that invocation. `--resume` is an explicit recovery action; requests already saved are retained, and committed operations are not newly billed. Original identity status/retry must reconcile an unknown commit. The runner has no SQL write/seed path, hidden limit change, remote service or private proof import.

For an issue report, include the exact candidate commit, executable SHA256, command/error code and synthetic request/receipt IDs, plus the relevant `RUN.json` entry. Review logs before sharing; never include real customer data or credentials. Keep the full installation private. A restore copies the whole quiescent installation including retained WAL state, then checks the original receipts; copying a live database file alone is not a backup procedure.

Do not use this candidate as a migration of a live billing installation. The example starts fresh private synthetic installations and explicitly enables the additive workflow schema. Old writer refusal and legacy compatibility are separate qualification gates. No remote CI, other native platform or release pass is implied by a successful example run.
