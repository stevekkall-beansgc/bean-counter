# Completed work, failed attempts and later assessment

This offline example uses the **verified matching v0.9.6 executable**, Python 3.11+ and local synthetic fixtures. The example, complete fixture directory and checker are bundled in the matching native package. Download/install using [START-HERE](../../../START-HERE.md); its release page and qualification record determine actual publication and qualified identities. Retain the package source and executable/example hashes. No account, provider, credentials, paid service or network call is involved. Copying `bounded-cohort.py` alone omits its dependencies: keep it beside the entire `bounded-cohort/` directory containing `setup-synthetic.json`, `artifacts.json`, `failed-work.json` and `observations.json`. The program validates all four JSON files and physical paths before creating a trial or invoking the ledger. For an intentionally separate copy, pass `--fixtures-dir "/absolute/physical/path/to/bounded-cohort"`; it never searches other directories.

The illustrative agreement charges **+2 scale-2 USD atoms per completed report**. Its outcome is a **new quality assessment after the ordinary start**: a valid report whose `generated_total` equals `sum(values)` receives code `success` (+2 atoms); a valid report failing that criterion receives `criteria-not-met` (0 atoms). These are synthetic operator-selected labels and amounts, not new engine vocabulary or recommended commercial terms. Completion means the report file was generated and durably saved; quality is assessed later. A negative quality result does not imply generation failed or no work occurred. Invalid/unparseable reports leave the outcome unacknowledged.

The **five-completed-artifacts synthetic fixture** in [artifacts.json](artifacts.json) defines two correct reports and three incorrect reports. The program creates five actual report files, timestamps each completed save independently and records its SHA-256 and stable generation operation ID **before** submitting that base. It sets ordinary start 12 seconds after trial setup, correction start 2 seconds later, with separately ordered occurrence/receipt/acceptance ends. It displays all synthetic terms before initialization. These short durations illustrate the ordering; they are not business contract advice. If local work misses either start, the run stops and preserves evidence; it never copies a start onto a completion, clamps time or resets its store.

After actually reaching the ordinary start, it reads each unchanged artifact and performs a new sum comparison, then captures the assessment time. It does not merely delay submitting an earlier known outcome. This later assessment is eligible because it is the explicitly agreed outcome event in this fixture. An application that cannot separate completed work from a later outcome fact needs a separate contract decision; a new setup per batch does not remove ordering requirements.

For a provider adapter, use the [live-app integration recipe](../../../docs/live-app-integration.md). Do not construct terms after fetching, use attempt start as a completed-work timestamp, or classify a known response before the window and merely sleep before submitting it. Transport errors and unexpected response shapes remain unknown. The reference already demonstrates the separate work-failure and later-assessment-failure paths below.

## Run a fresh trial

Set `PACKAGE_ROOT` to the absolute physical installed v0.9.6 directory, and `LEDGER` to its verified executable, as in START-HERE. Preserve the source identity and executable/example hashes from that matching package. Use the [physical-path guidance](../../../START-HERE.md#use-private-physical-paths); macOS `/tmp` and `/var` aliases must not reach the engine.

```sh
umask 077
BC_TRIAL_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-trial.XXXXXX")
cd -P "$BC_TRIAL_DIR"
export BC_TRIAL_ROOT="$(pwd -P)"
printf '%s\n' "$BC_TRIAL_ROOT"
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/cohort" \
  --scenario five-completed-artifacts
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/classification" \
  --scenario evidence-classification
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/failed-work" \
  --scenario failed-work
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/assessment-error" \
  --scenario completed-work-assessment-error
PYTHONOPTIMIZE=1 python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/cohort-optimized" \
  --scenario five-completed-artifacts
```

A deliberately chosen existing trial parent must have no group/other access, normally mode 0700. On refusal, the diagnostic shows its actual path/mode and a quoted remedy. Set the chosen parent, then run:

```sh
mkdir -p "$BC_TRIAL_ROOT"
chmod 700 "$BC_TRIAL_ROOT"
cd -P "$BC_TRIAL_ROOT"
export BC_TRIAL_ROOT="$(pwd -P)"
```

Review that path before changing permissions; the example never changes an existing directory's mode. Choose a new child after the remedy. Missing parents and symlink components have separate diagnostics.

The example refuses existing work directories and symlink components. Checks use explicit exceptions and remain active under optimized Python. Allow roughly 12 seconds of deliberate waiting per cohort plus local CLI/file work. Each CLI invocation reopens the same store; exact retries preserve the complete history. This fresh-trial demonstration is not a resumable production outbox. On a failure, retain that trial directory and pending original requests; do not rerun against it or replace request identities. A later fresh synthetic trial may use a new private directory and its newly displayed schedule. A temporary trial is not a backup strategy for business records.

## Read the evidence and arithmetic

The new directory retains `identity.json`, the exact `setup.json`, generated artifacts and completion/assessment records, every exact base/outcome request, and **full** per-command stdout, stderr, argv and exit status. Stdout includes each full returned receipt and complete target explanation; there is no log truncation. `summary.json` is separate from those original records. Engine acceptance/receipt clocks remain engine-generated. The executable hash and example hash identify this run; prior release qualification does not qualify a new package.

Before acknowledging, the example dispatches by operation, compares exact receipt objects and their retained membership, and verifies complete original-base-target history, customer/scope/source, agreement/version, original delivery ID, occurrence and canonical request facts. Base receipts are `base-acceptance` with `body.target`; outcomes are `receipt` without that field, so the saved base target is retained. Outcome event IDs and referenced evidence must match family, code, target, source, occurrence and evidence text. It then compares integer postings. It retries each saved request identically, verifies the original receipt again and requires unchanged complete history. See the fuller [operation-specific reconciliation recipe](../../../docs/integration-agent-guide.md#reconcile-each-operation). Existing Python/Node outboxes remain base-only helpers; this example does not add correction or alias handling to them.

Expected **observed five-completed-artifacts fixture totals** are 10 base atoms (`5 × 2`), 4 adjustment atoms (`2 × 2 + 3 × 0`), net **14 atoms = USD 0.14**, **10 retained entries**, `complete: true`, 10 exact duplicate retries and zero pending outcomes, work failures, ledger refusals or ledger errors. A `success` premium adds to the base; it does not replace it. Zero adjustments retain entries without monetary postings. Entry count is not posting count. The engine verifies retained billing assertions, not independent real-world truth or payment collection. If facts differ, preserve the actual history and failure instead of forcing this arithmetic.

These exact expectations are synthetic fixture checks in `fixture_expectations`, separate from `Trial.verify`'s generic integer posting/entry reconciliation and full receipt/history checks. When adapting, review the agreed work completion criterion, later quality criterion, allowed outcome meanings, immutable terms and resulting counts/totals together. There is no arbitrary expected-total override. Do not change expected totals merely to hide changed pricing or incorrect billing.

## Failure before work completion

The **failed-work synthetic fixture** in [failed-work.json](failed-work.json) attempts five jobs. Four durably generate qualifying reports before both starts. The fifth deliberately raises an offline `TimeoutError` before generating or saving a report. It retains `failed-report-5.work-error.json` with actual attempt/failure times, `transport-error`, unknown result count and the exception. Saving that error is not work completion. It creates no failed artifact, completion record, base request, base receipt, target or outcome. The complete retained economic history must exclude that failed identity.

After ordinary start, the four reports receive new successful quality assessments. Expected totals: **8 base + 8 adjustment = 16 scale-2 USD atoms (USD 0.16), 8 retained entries and 8 identical duplicate retries**. Counts are `attempted_work: 5`, `completed_work: 4`, `billable_work: 4`, `unacknowledged_work_failures: 1`, `pending_outcomes: 0`, `ledger_refusals: 0`, `ledger_errors: 0`. This application failure is separate from a failed or refused ledger invocation. The original five-report quality-negative scenario remains unchanged at 14 atoms/10 entries.

## Unknown assessment after completed work

The **completed-work-assessment-error synthetic fixture** completes one real report and reconciles its base, then observes a local timeout fixture after ordinary start. It retains `report-1.assessment-error.json` with an unknown count and the original base target. It submits no outcome or correction. The complete history remains identical to the history saved before assessment: **2 base + 0 adjustment = 2 atoms, 1 entry and 1 identical retry**. Counts are one attempted/completed/billable work item, zero work failures, one assessment error and one pending outcome. A later provider/assessment failure cannot erase genuinely completed work or select a fabricated quality-negative adjustment. Work completion and outcome eligibility always depend on explicit agreed semantics.

Every billing summary includes `complete_history_note`: `complete:true` describes authorized retained billing history at that snapshot, not successful completion of every attempted job. It separately reports attempted/completed/billable work, work/assessment errors, pending outcomes, ledger refusals/errors, base/adjustment/net amounts and retained entries. Full original records remain the evidence.

To qualify all four bundled scenarios normally and with `-O`, including private-parent remedies, fixture-copy support and symlink refusals, run:

```sh
python3 "$PACKAGE_ROOT/scripts/check-bounded-cohort.py" "$LEDGER" \
  --evidence "$BC_TRIAL_ROOT/qualification"
```

The new evidence directory must not exist. This check deliberately executes the public-parent and missing-fixture failures, records their actual commands/exits/diagnostics, then verifies the documented remedies. Allow roughly 12 seconds per cohort run plus classification checks. The same entrypoint is part of local end-to-end qualification.

## Classify uncertainty separately

The second scenario interprets [observations.json](observations.json), a local fixture pretending to be provider observations. No provider is contacted and no billing write occurs. Only a valid HTTP-200 response matching the explicit `results` array schema establishes a result count. A missing parser match is not proof of zero.

| Observation | Classification | Verified count / disposition |
| --- | --- | --- |
| Valid response with qualifying results | `verified-success` | Known positive; outcome eligibility still needs explicit agreed criterion and timing. |
| Valid successful response with empty results | `verified-zero-results` | Known zero; no automatic adjustment code is selected. |
| HTTP refusal, challenge or block | `provider-blocked` | Unknown; pending and unacknowledged. |
| Timeout/DNS/transport failure | `transport-error` | Unknown; pending and unacknowledged. |
| Malformed or unrecognized response | `parse-error` | Unknown; pending and unacknowledged. |
| Actual time before a stated assessment cutoff K | `pending-before-cutoff` | No cutoff-negative claim. |
| K actually reached and stipulated condition evaluated | `cutoff-assessed` | Preserve underlying classification/count/uncertainty. |

This probe states K as two seconds in the future, records the actual pre-K observation, waits until K and newly checks the valid zero-results fixture. Its cutoff assessment is **not submitted to the ledger** and does not demonstrate a billable cutoff policy. To bill `unsuccessful-by-cutoff`, explicitly agree K inside the ordinary occurrence window (`starts_at <= K < occurs_before`), actually reach it, evaluate the stipulated condition, retain truthful evidence and meet receipt/acceptance deadlines. The exclusive occurrence end itself is not an eligible outcome time. A provider block establishes neither zero results nor elapsed K. An explicit business rule for “no verified success by K, including errors” would be a different condition requiring its own terms and assent; this example makes no such choice.

Application-owned evidence conventions (timestamps, transport/HTTP/parse status, verified count or unknown, cutoff/reached state, classification and digests) live outside frozen ledger schemas. Do not add these fields to strict request contracts. The outcome `evidence` string carries this example's synthetic assessment record; the engine does not fetch or authenticate its underlying artifact/provider data. The three unknown classification fixtures stay separate from the five-report/10-entry arithmetic case.
