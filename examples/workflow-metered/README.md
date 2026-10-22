# Metered-work development journey

This synthetic candidate extends the released fixed-service example with one
integer Unit-priced service. It is not a new release or a full Wave 2b claim.
Use a reviewed captured local/operator-resource CLI and its two matching
`prepare_local_*` helpers; finish both executable builds before live work.

```sh
python3 scripts/e2e/workflow_metered.py --ledger "$LEDGER" \
  --publication-helper "$PUB" --terms-helper "$TERMS" --output "$NEW_PRIVATE_OUTPUT"
```

The frozen inputs declare three requests at USD1 each, attributable app cost
USD2, and a 10% premium on the original service only. Expected totals are
**3 → 5 → 5.30 → 2 → 0**. Quantity correction replaces three units with two and
appends −3,+2,−2,−0.30, permanently closing the app/family. It does not create a
new 0.20 premium. Original Work quantity, rates, terms and receipts stay immutable.

`--pending` checks pending app retirement/unclaimed-family closure and subsequent
finalize/outcome refusal. `--variant fractional` uses a renamed policy and rate
0.125 (0.375 → 2.375 → 2.4125 → 0.25 → 0); `--variant rounded-zero` uses a
one-atom unit rate, a permanent zero-rounded outcome claim without a zero Action,
then exact closure. Every case needs a separate new private output directory.

The runner saves a quiescent `pre-quantity-fixture` before its quantity correction.
`workflow_metered_negatives.py` consumes only private copies of that fixture.
The two existing opt-in financial failure tests can consume the same fixture
with `LEDGER_WORKFLOW_TEST_OPERATION=main-quantity-correct`; retain their documented
captured-build and observed CLI-matching worker-stack prerequisites.

The development driver never automatically resubmits. On an unexpected failure,
it preserves the original request/output, attempts original-key status and retains
the private installation. Do not replace identities or modify accepted clocks,
terms, rights or bounds. Keep these private records out of Git and public reports.

Status/retry, whole-installation restore and old fixed-history compatibility use
the existing maintained workflow driver's exact saved-request paths. A verification
tail is not a continuation command for a partially committed financial sequence.
No new statement endpoint, supplier AP, predicates, stages or platform claim is
added by this example. Existing signed scale-18 arithmetic and all financial,
authority, capacity, snapshot, cancellation and atomicity checks remain mandatory.
