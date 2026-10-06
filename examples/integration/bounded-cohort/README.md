# Five completed reports, then a later quality assessment

This offline example uses the **verified released v0.9.4 executable**, Python 3.11+ and local synthetic fixtures. It is a separate documentation/example revision and is not bundled in immutable v0.9.4 archives. Download/install the executable using [START-HERE](../../../START-HERE.md), then obtain this reviewed example revision and retain its commit or file hashes. No account, provider, credentials, paid service or network call is involved.

The illustrative agreement charges **+2 scale-2 USD atoms per completed report**. Its outcome is a **new quality assessment after the ordinary start**: a valid report whose `generated_total` equals `sum(values)` receives code `success` (+2 atoms); a valid report failing that criterion receives `criteria-not-met` (0 atoms). These are synthetic operator-selected labels and amounts, not new engine vocabulary or recommended commercial terms. Completion means the report file was generated and durably saved; quality is assessed later. A negative quality result does not imply generation failed or no work occurred. Invalid/unparseable reports leave the outcome unacknowledged.

[artifacts.json](artifacts.json) defines two correct reports and three incorrect reports. The program creates five actual report files, timestamps each completed save independently and records its SHA-256 and stable generation operation ID **before** submitting that base. It sets ordinary start 12 seconds after trial setup, correction start 2 seconds later, with separately ordered occurrence/receipt/acceptance ends. It displays all synthetic terms before initialization. These short durations illustrate the ordering; they are not business contract advice. If local work misses either start, the run stops and preserves evidence; it never copies a start onto a completion, clamps time or resets its store.

After actually reaching the ordinary start, it reads each unchanged artifact and performs a new sum comparison, then captures the assessment time. It does not merely delay submitting an earlier known outcome. This later assessment is eligible because it is the explicitly agreed outcome event in this fixture. An application that cannot separate completed work from a later outcome fact needs a separate contract decision; a new setup per batch does not remove ordering requirements.

## Run a fresh trial

Set `REPO` to the absolute physical checkout containing this example revision, and `LEDGER` to the absolute verified installed v0.9.4 executable. Preserve the executable's hash and release source identity independently of the example revision. Use the [physical-path guidance](../../../START-HERE.md#use-private-physical-paths); macOS `/tmp` and `/var` aliases must not reach the engine.

```sh
umask 077
BC_TRIAL_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-trial.XXXXXX")
cd -P "$BC_TRIAL_DIR"
export BC_TRIAL_ROOT="$(pwd -P)"
printf '%s\n' "$BC_TRIAL_ROOT"
python3 "$REPO/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/cohort" \
  --scenario five-completed-artifacts
python3 "$REPO/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/classification" \
  --scenario evidence-classification
PYTHONOPTIMIZE=1 python3 "$REPO/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/cohort-optimized" \
  --scenario five-completed-artifacts
```

The example refuses existing work directories and symlink components. Checks use explicit exceptions and remain active under optimized Python. Allow roughly 12 seconds of deliberate waiting per cohort plus local CLI/file work. Each CLI invocation reopens the same store; exact retries preserve the complete history. This fresh-trial demonstration is not a resumable production outbox. On a failure, retain that trial directory and pending original requests; do not rerun against it or replace request identities. A later fresh synthetic trial may use a new private directory and its newly displayed schedule. A temporary trial is not a backup strategy for business records.

## Read the evidence and arithmetic

The new directory retains `identity.json`, the exact `setup.json`, generated artifacts and completion/assessment records, every exact base/outcome request, and **full** per-command stdout, stderr, argv and exit status. Stdout includes each full returned receipt and complete target explanation; there is no log truncation. `summary.json` is separate from those original records. Engine acceptance/receipt clocks remain engine-generated. The executable hash and example hash identify this run; prior release qualification does not qualify a new package.

Before acknowledging, the example dispatches by operation, compares exact receipt objects and their retained membership, and verifies complete original-base-target history, customer/scope/source, agreement/version, original delivery ID, occurrence and canonical request facts. Base receipts are `base-acceptance` with `body.target`; outcomes are `receipt` without that field, so the saved base target is retained. Outcome event IDs and referenced evidence must match family, code, target, source, occurrence and evidence text. It then compares integer postings. It retries each saved request identically, verifies the original receipt again and requires unchanged complete history. See the fuller [operation-specific reconciliation recipe](../../../docs/integration-agent-guide.md#reconcile-each-operation). Existing Python/Node outboxes remain base-only helpers; this example does not add correction or alias handling to them.

Expected **observed** fixture totals are 10 base atoms (`5 × 2`), 4 adjustment atoms (`2 × 2 + 3 × 0`), net **14 atoms = USD 0.14**, **10 retained entries**, `complete: true`, 10 exact duplicate retries and zero pending/refused/failed operations. A `success` premium adds to the base; it does not replace it. Zero adjustments retain entries without monetary postings. Entry count is not posting count. The engine verifies retained billing assertions, not independent real-world truth or payment collection. If facts differ, preserve the actual history and failure instead of forcing this arithmetic.

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
