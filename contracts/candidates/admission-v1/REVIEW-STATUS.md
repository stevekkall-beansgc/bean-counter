# Candidate handoff — review pending

Worktree: `/Users/stephenkall/Documents/Codex/2026-09-25/bean-counter-zen-charge-pilot`

Branch: `codex/zen-charge-pilot`

Base: `87892ac011902b286776e26925d8a3fb2c2aaa88`

## Implemented and author-validated E2E; independent review pending

- Feature-gated pure Rust candidate state machine and deterministic artifact
  verifier; fixed A/B atom amounts are selected only in the core.
- Candidate facade and separate SQLite journal using the existing owner lock,
  immediate transactions and full synchronous WAL; reopen replays and compares
  retained responses before submitting or reporting.
- Candidate CLI `zen-charge-candidate init|submit|statement`, absent unless built
  with `--features zen-charge-candidate`.
- Two separately preauthorized synthetic orders, one logical host execution
  slot, sequential reservation/admission/outcome transitions, and terminal failure.
- Synthetic process worker and host demo; no live OpenCode/Zen or Agency changes.
- Independent E2E process driver with real CLI processes, SQLite reopen, an
  actual worker child process, concurrent submitters and abrupt process exits.
- Reviewer follow-up: separate `zen-charge-e2e-hooks` feature for private clock
  and crash injection; neither hook is compiled into the default or ordinary
  candidate runtime. Three independently built artifacts exercise that boundary.

## Candidate artifacts

- `contracts/candidates/admission-v1/AMENDMENT.md`: versioned candidate rules,
  identity, host authorization, accepted prices, time, records and review decisions.
- `input.schema.json`: structural input shapes; semantic/lexical rules remain
  explicit in the amendment.
- `setup.json`, `bindings.json`, `outcome.json`, `expected.json`: new synthetic
  authorizations, target/hash fixtures and author-generated receipt/math examples.
- `inventory.json`: SHA-256/byte inventory of all candidate changes, deliberately
  not a reviewed freeze manifest. Does not hash itself.
- `E2E-RESULT.json`: successful dedicated gate result, exact command and hashes
  of the three built binaries; author evidence, not independent review.
- `crates/ledgerlab-core/src/zen_candidate.rs`: pure candidate semantics.
- `crates/ledgerlab/src/zen_candidate.rs`: host coordinator/recovery.
- `crates/ledgerlab/src/store/sqlite/zen_candidate.rs`: candidate journal IO.
- `crates/ledgerlab-cli/src/zen_candidate.rs`: CLI input/output.
- The three crate manifests, module declarations, and CLI dispatch have small
  opt-in feature wiring changes; no new dependency or lockfile change.
- `examples/zen-charge-pilot/{README.md,run.py,fixture_worker.py}`: runnable
  synthetic demonstration and explicit limitations.
- `scripts/check-zen-charge-pilot-e2e.sh`, `scripts/e2e/zen_charge_pilot.py`:
  dedicated build-and-process-E2E entrypoint and scenarios.

## Exact validation result

Command, executed from the worktree:

```sh
CARGO_HOME=/private/tmp/bean-counter-cargo RUSTUP_HOME=/private/tmp/bean-counter-rustup PATH=/private/tmp/bean-counter-cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin sh scripts/check-zen-charge-pilot-e2e.sh
```

Exit **0**, pinned Rust **1.98.1** with the user-provided locked dependency cache:

```text
{"status": "passed", "candidate": true, "cli_process_checks": 270, "live_zen": false, "power_loss_tested": false, "reviewed_freeze": false, "clock_hooks": "separate-e2e-build", "fixed_receipt_fixture_compared": true, "feature_off_runtime_checked": true}
```

All three builds and the complete dedicated process E2E driver passed. Its
counter reports 270 CLI process checks, not 270 distinct scenarios; helper and
worker processes are additional. No unit tests or broad test suites ran, and
no dependency or toolchain installation was attempted. The prior exit-77
toolchain blocker was resolved by the user's isolated `/private/tmp` toolchain.

The first compile exposed sha2 0.11's digest-array `LowerHex` incompatibility.
The fix uses the repository's existing `canonical::hex(&h.finalize())`, without
new dependencies or version changes. The dedicated command then passed on its
first rerun, with no further build or scenario defects observed. Fixed expected
fixtures and existing frozen contract artifacts remain unchanged.

Non-test static checks: Python source parsed with `ast.parse`, JSON files parsed,
embedded subprocess snippets also parsed with `ast.parse`, shell syntax checked
with `sh -n`, and `git diff --check` passed. A read-only
comparison found no changed paths pinned by existing freeze inventories and no
modified existing contract artifacts. These checks are not semantic validation.

## Threat cases executed by the passing dedicated gate

Exact retries and mutation of all subordinate labels; each binding field
changed independently; an entirely renamed invented order/binding/evidence;
caller-injected operation ID or verified flag; wrong/reordered artifact; borrowed
purchase authorization; separately accepted second purchase; overlapping slot
requests; competing admissions; replay after terminal failure; process death
before/after reservation, admission and outcome commits; original receipt
recovery in new processes; zero pre-admission charge and A-only failure/pending;
expired admission window. Reviewer-requested additions:

- Cross-order reservation race; outcome versus failure race; synchronized
  independent CLI processes with owner-lock refusals explicitly reconciled.
- Concurrent initial admissions and retries with all subordinate evidence labels
  mutated; duplicate receipts and unchanged statements checked.
- Lower/inclusive and upper/exclusive admission/outcome boundaries; outcome
  expiry; receipt lookup after expiry and backward time; backward-clock refusal
  for new decisions, including a different authorized order.
- Malformed JSON, duplicate keys, top-level/nested null and invalid UTF-8;
  duplicate authorization/order IDs; zero/out-of-range/noncanonical prices;
  valid inclusive minimum/maximum prices through complete process lifecycles.
- Corrupted receipt bytes, bound target, and sequence gaps in quiescent
  disposable journals: reopened reads and new submissions must refuse without
  appending records. Corruption uses a separate SQLite process.
- Full fixed receipts from `expected.json` compared to CLI results and exact
  retained canonical response bytes at fixture timestamps, without rewriting
  the golden fixture or importing a production evaluator.
- Feature-off build/runtime checks: version/help, candidate-route refusal, and
  existing billing init/accept/retry/reopened statement. Ordinary candidate
  build must ignore private clock/crash environment variables and reject a
  caller-supplied time field.

All listed scenarios completed in the passing author-run E2E gate. This is
bounded local synthetic evidence, not a general robustness or freeze claim.

## Separate-review decisions and limits

Before freeze, a separate reviewer must check the exact candidate bytes,
independent money/hash oracles, domain/identity choices, acceptance delegation,
authorization root, A retention/cancellation policy, slot/failure cleanup and
fixed-window rules and the final manifest alongside the passing process evidence.
The existing frozen profiles stay unchanged throughout.

The candidate uses preauthorized deterministic acceptance, not a new human
approval at completion. The logical slot is cooperative host ownership, not a
physical resource or Zen quota guarantee. No live model call, corrections/refunds,
authority revocation, automatic recovery/reaper, cross-installation deduplication,
power-loss proof, or actual failure during COMMIT is implemented/tested. Raw
private-directory control remains the administrator trust boundary. A stopped
worker/host can leave a slot held until explicit reconciliation/failure.

Exact-time cases passed in the private hooks build; normal-candidate hook
isolation also passed. Concurrent start barriers do not force both possible
terminal winners or measure physical SQL overlap; serialized terminal cases
cover both state orderings. The default-feature check is a bounded compatibility
smoke, not a full legacy regression suite. Actual uncertain-COMMIT/power-loss
behavior and coherently rewritten administrator-owned histories remain outside
the evidence. Private hooks must never enter a released candidate artifact.

Source changes are confined to this worktree; build caches/toolchain state use
the user-authorized `/private/tmp` locations. No commits, pushes, releases or
deployments were performed. The candidate is not frozen.
