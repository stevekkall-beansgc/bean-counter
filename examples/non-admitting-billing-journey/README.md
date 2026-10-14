# Saved-policy billing journey

This runnable demonstration uses the existing core pricing and outcome engines. It retains the original policy and a synthetic local acknowledgement, computes a bill, then starts another process to reload the saved inputs and recompute every amount and explanation.

The acknowledged terms set a USD 0.41 base charge and USD 0.37 per provider call. Three calls are priced by the engine. Provider cost observation stays separate from the customer's payable. One frozen outcome family has two alternative branches: a 20% success premium or a 10% failure discount. Failure here means a downstream assessment failed after the billable work completed successfully. The two outcomes do not stack into one invoice.

This is synthetic, non-admitting data. No customer is charged. The acknowledgement and provider evidence are demonstration assertions, not authenticated commercial consent or a financial receipt. The R2 constructor, budgets, support profiles and financial admission remain unchanged.

## Build and run

The reviewed acceptance run uses two separate steps, with source review and root dispatch between them. Use the existing pinned Rust/Cargo 1.98.1 environment and offline dependency cache. The infrastructure directory points to the independently accepted external watchdog/capture artifacts; neither file is modified by this demo.

```sh
python3 -B examples/non-admitting-billing-journey/run.py build \
  --worktree EXACT_CLEAN_WORKTREE --source-commit EXACT_REVIEWED_COMMIT \
  --output NEW_EXTERNAL_CAPSULE --infrastructure REVIEWED_RUNNER_DIRECTORY

python3 -B examples/non-admitting-billing-journey/run.py run \
  --capsule EXACT_CAPSULE --ready-sha256 ROOT_PINNED_READY_SHA256 \
  --expected INDEPENDENT_MONEY_EXPECTED_JSON --expected-sha256 REVIEWER_PINNED_EXPECTED_SHA256 \
  --infrastructure REVIEWED_RUNNER_DIRECTORY
```

The independent money file is used only by the external checker after calculation. It is never sent to or read by the pricing executable. The executable's first process receives only the original input file and a new archive path; the second receives only the archive path. Fresh and replay computational results must match completely.

The runner makes a clean, immutable source/input capture and records toolchain and binary hashes. Build has a separate 240-second watchdog; each demo process has a 60-second watchdog, 768 MiB sampled group-RSS threshold, a 50 ms sampling target with a maximum 100 ms observed gap, and capped output. These are external operational checks, not a hard instantaneous memory theorem. It supports the current macOS host and creates/kills only its own process groups. No automatic retry occurs.

The new capsule contains the retained archive, complete fresh/replay reports, process outcomes and `REPORT.md`. The readable report displays amounts and original explanations produced by the core. The renderer formats decimal atom strings; it computes no prices or adjustments.

Source or run failure preserves the capsule and stops. A demo PASS demonstrates only this original-terms calculation and restart reproduction. Financial journal admission, authenticated customer/provider evidence, complete R2/Wave 2 support, all histories, full resource bounds and future correction capacity remain open.
