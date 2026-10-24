# Conditional-work checkpoint walkthrough

Use only the matching v0.11.0 native archive, verifier, installer, helpers and qualification record. Start with [installation and safety](../../docs/workflow-candidate-testing.md) and [supported semantics](../../docs/local-workflow-supported-recipe.md). Inputs are public synthetic data; outputs/installations are private.

Build/install and prepare both helper executables before live work windows. From the package root, set LEDGER, PUB and TERMS as in the handoff, then use separate new output directories:

```sh
python3 scripts/e2e/workflow_conditional.py --context matching --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/matching"
python3 scripts/e2e/workflow_conditional.py --context nonmatching --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/nonmatching-finalized"
python3 scripts/e2e/workflow_conditional.py --context nonmatching --pending --ledger "$LEDGER" --publication-helper "$PUB" --terms-helper "$TERMS" --output "$OUT/nonmatching-pending"
```

Expected totals are matching **3→5→5.30→2→0**, nonmatching finalized **0→2→0→0**, and nonmatching pending **0→0→0** with later TARGET_CLOSED. Service false does not waive the independent app. Original and replacement quantity assertions are genuinely retained even on the false path. Saved receipts/status/identical retries and canonical same-Work action associations are checked by the maintained runner.

The finite context/proof/current-rights checks use genuine isolated fixture copies:

```sh
python3 scripts/e2e/workflow_conditional_negatives.py --ledger "$LEDGER" --fixture "$OUT/matching" --output "$OUT/context-negatives"
python3 scripts/e2e/workflow_attested_negatives.py --ledger "$LEDGER" --fixture "$OUT/matching" --output "$OUT/proof-rights-negatives"
```

Stop on the first unexpected failure. Preserve the original request bytes, key, RUN.json and installation. Resolve status and any approved identical retry against that same installation; do not rerun a whole chain or create replacement identities. Keep databases, raw evidence and private paths out of Git and public issues.

For source development, capture with `scripts/build_workflow.py --local-execution --operator-resources --conditional-work --output NEW_BUILD`, then build the helpers from that exact captured source. A fresh old Compiler8 publication is not interchangeable with this new descriptor; old saved receipt recovery remains supported.
