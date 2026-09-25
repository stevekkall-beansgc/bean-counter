# Synthetic two-charge candidate pilot

**Unreviewed candidate; not frozen, released, or suitable for real billing.**
Contract: `contracts/candidates/admission-v1/AMENDMENT.md`.

The pilot reserves one logical host slot durably, then charges one admission
atom. A separate deterministic worker extracts five product names. The host
independently checks its exact JSON artifact and applies the preauthorized
acceptance rule before charging 499 outcome atoms. It does not invoke OpenCode
or claim Zen availability. Customer fees are independent of provider cost.

With the existing pinned Rust 1.98.1 toolchain and cached dependencies:

```sh
sh scripts/check-zen-charge-pilot-e2e.sh
```

This builds default, ordinary candidate, and private E2E-hooks binaries in three
separate artifact directories, then runs a Python process E2E driver.
It never invokes `cargo test`, installs a toolchain or downloads dependencies.
Exit 77 means toolchain preflight blocked; no E2E scenarios executed.

After that build, from this isolated worktree:

```sh
python3 examples/zen-charge-pilot/run.py \
  --ledger "$PWD/work/zen-charge-pilot-target/debug/ledger" \
  --directory "$PWD/work/zen-charge-pilot-demo"
```

The run directory must be new and inside this worktree. Inputs/requests remain
there if a process fails. Never rerun initialization over an existing directory.
For unknown/lost responses, use the saved request and original store:

```sh
work/zen-charge-pilot-target/debug/ledger zen-charge-candidate submit \
  "$PWD/work/zen-charge-pilot-demo/store" \
  "$PWD/work/zen-charge-pilot-demo/admit.json" --json
work/zen-charge-pilot-target/debug/ledger zen-charge-candidate statement \
  "$PWD/work/zen-charge-pilot-demo/store" --json
```

`reserve.json`, `admit.json`, and `outcome.json` are separately saved before
submission. If the worker fails, `fail.json` releases the slot and retains only
any already charged admission fee. If the process stops after reserve/admission,
the slot stays held until explicit reconciliation/failure; the demo never
silently launches a replacement worker. To submit an outcome later, use the same
binding and the retained, verified artifact. An invalid artifact is refused;
it does not automatically terminate the order.

E2E coverage added: no-reservation refusal; zero-price reservation; A before
worker launch; independent exact artifact verification; A-only pending/failure;
repeat and fully renamed subordinate evidence; mutation of every bound field;
invented order and reused authorization; separate accepted purchases; slot
contention; four competing admission processes; abrupt exits before/after
reserve/admit/outcome commits; reopened statements and original receipt replay;
expired admission windows; cross-order reservation and outcome/failure races;
concurrent retries with renamed evidence; exact window/clock boundaries and late
receipt lookup; malformed/duplicate-key/null input; duplicate authorizations;
price bounds; corrupted journal replay; complete fixed receipt and canonical
response-byte comparison against the saved expected fixture; default-feature
billing smoke and normal-candidate hook isolation; oversized input rejection
through both public APIs; and caller cancellation during an uncommitted
transaction with owner-lock retention and receipt recovery. The cancellation
guarantee assumes the Tokio runtime remains live and driven; runtime shutdown
leaves the operation result unknown until replay/reconciliation.
The dedicated gate passed with 278 counted CLI process checks using the pinned
Rust 1.98.1 toolchain; see the candidate `REVIEW-STATUS.md` and `E2E-RESULT.json`
for the exact command and scope. The hardening path has had a separate static
review; full candidate review and freeze remain pending.
The worker/SQLite/CLI are separate real processes;
there is no live Zen call or simulated assertion that one occurred.

## Repeatable live OpenCode provider E2E

The separate live gate calls the configured Zen Space Bunny route and local
Qwen route, then submits each actual model artifact to a new synthetic
candidate store:

```sh
sh scripts/check-opencode-zen-charge-e2e.sh
```

This gate requires the pinned Rust toolchain and builds the ordinary
`zen-charge-candidate` CLI; it does not use a cached-binary fallback. The shell
gate emits a source/build manifest and the Python driver validates the binary
digest, source commit/tree, compiler, and feature profile before recording
provider evidence. It makes two real
model requests. It uses a temporary empty Git workspace for each OpenCode
run, `--pure`, a text-only agent with every tool denied, closed stdin, and no
sharing. The only user prompt is the public five-product fixture. The gate
requires both exact model IDs to be active with zero catalog cost; it fails
closed if a route is unavailable or its catalog changes. Both the event stream
and sanitized export must contain numeric zero-cost evidence. Missing, malformed,
nonzero, or conflicting cost observations refuse; one source cannot override
the other. It accepts only the
exact sorted JSON artifact, then verifies the 1-atom admission and 499-atom
outcome receipts, exact replay, replay with renamed subordinate IDs, rejection
of a mutated order ID, and the final 1,000-atom statement. It stores sanitized
model metadata, a hash of each sanitized OpenCode export, requests, receipts,
and the final statement under a unique ignored `work/opencode-provider-charge-e2e/`
run directory. Raw model event output and provider error text are not saved.

Before reserving a slot, the gate saves the original failure request. Model
timeouts and process errors trigger that operation, retaining admission A and
releasing the logical slot without charging B. If cleanup fails or its result is
unknown, the gate reports the exact retained store, statement command, and
original failure retry command; it also saves a recovery record when storage is
available. Reconcile that store instead of initializing a replacement. This
cleanup concerns the host's logical slot, not remote provider capacity.

The admission receipt represents a **logical host slot**; it does not reserve
actual Zen or local hardware capacity. Catalog and run cost are the values
OpenCode reports at E2E time, not an invoice guarantee. Provider cost
accounting and payment collection remain false. The candidate remains
unreviewed and unfrozen; these synthetic charges are not payable invoices.

## Separate PUBLIC model validation (2026-09-25)

The candidate E2E above remains synthetic. In an earlier, separate,
single-request Agency check, the reviewed replacement route
`opencode/muse-spark-1.3-contributor-free` extracted the five expected names
from a PUBLIC synthetic product list. At that check, the original
`opencode/space-bunny-free` route was absent from the refreshed OpenCode 1.18.32
catalog and was not called. Catalog availability is time-dependent; the
repeatable provider E2E below performs a fresh active/zero-cost preflight before
each run and records the refreshed route and actual model response.

Agency's current exact-model evidence was valid, and its typed PUBLIC preflight
returned `ALLOW_HARD_ZERO`. The Muse request used `--pure`, an isolated OpenCode
state, closed stdin and the Agency tool permission configuration. The process
exited 0. Its sanitized export reported provider `opencode`, model
`muse-spark-1.3-contributor-free`, finish `stop`, and complete token counts
(8,289 input, 44 output, 468 reasoning, 9,042 total). Postflight returned
`ZERO_MATCH` with reported cost **$0**. The bounded JSON response contained
exactly the five supplied names in order.

The preflight and postflight receipt SHA-256 hashes are respectively
`d9a6aff9328156e69fa6a75b3e91c19815afdbc606a682710eceb85b02fdc58a`
and `08cacd1dfb571269c4d2cd8f95d6fc2496e34449d5bebfc3dcbf9d0f8552d206`.
This check establishes that one public extraction request succeeded at zero
reported cost at that time. It did not run the candidate charge flow with a
live model, authorize a future free request, or change the candidate's pending
independent-review and freeze status.

Clock and crash hooks exist only behind the separate `zen-charge-e2e-hooks`
build feature, which is absent from both default and ordinary candidate builds.
The dedicated harness alone uses `LEDGER_ZEN_E2E_TIME_US` to set deterministic
host observation times for exact E2E scenarios; no normal CLI time input exists.
`LEDGER_ZEN_CANDIDATE_CRASH=before_commit` exits 91 after journal insertion;
`after_commit` exits 92 after commit before acknowledgment. These test abrupt
process termination/acknowledgment loss, not actual disk power loss or an
uncertain COMMIT transport result. Existing production billing is unchanged.
Never use or distribute the private hooks binary as the candidate runtime.
