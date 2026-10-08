# Connect live work to the bounded-cohort model

Use this recipe when your application can separate **completed work** from a **genuinely later assessment**. For example, agree to produce and durably save a report, then evaluate its accuracy after the ordinary window starts. Work completion and the assessment are different facts under terms established before either happens.

If the promised outcome happens during the work itself, delaying its submission does not turn it into a later assessment. One static agreement does not support indefinitely arriving work. These limitations need a separate contract decision; do not backdate facts or rebuild a business store to fit them.

## Run the reference before adapting it

The [bounded-cohort example](../examples/integration/bounded-cohort/README.md) is the runnable reference. It uses actual local file completion times, a new later assessment and synthetic report inputs. It makes no provider requests. Its existing generation step is where your adapter would perform the agreed work; keep the timing, evidence and reconciliation around it.

After [verified installation](../START-HERE.md), use the deliberately chosen installed directory as `PACKAGE_ROOT`. For that trusted existing installation, derive its physical path and bind the executable again:

```sh
export PACKAGE_ROOT="$(cd -P "$PACKAGE_ROOT" && pwd -P)"
export LEDGER="$PACKAGE_ROOT/ledger"
"$LEDGER" --version
umask 077
BC_TRIAL_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bean-counter-live-recipe.XXXXXX")
cd -P "$BC_TRIAL_DIR"
export BC_TRIAL_ROOT="$(pwd -P)"
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/completed" \
  --scenario five-completed-artifacts
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/failed-work" \
  --scenario failed-work
python3 "$PACKAGE_ROOT/examples/integration/bounded-cohort.py" \
  --ledger "$LEDGER" --work-dir "$BC_TRIAL_ROOT/assessment-error" \
  --scenario completed-work-assessment-error
```

`PACKAGE_ROOT` must already be the installation you deliberately selected. These commands are not a way to accept arbitrary symlinked input files. Create only the private trial parent; each work child must be new. Keep the example with its complete fixture directory or pass `--fixtures-dir` explicitly.

## Preserve this order in your adapter

1. **Agree and initialize terms before starting work.** Define exactly what counts as completion, the later assessment criterion, outcome codes and amounts, authority and assent evidence. Choose future ordinary/correction starts with enough time for the planned bounded work. A short offset such as three seconds is not a general live-provider timing recipe. The example's twelve seconds is illustrative too; network duration is not guaranteed.
2. **Perform the work and retain its actual evidence.** Check transport success, HTTP status, expected response schema and parsing. Durably save the agreed artifact and its digest. Capture its real completion time after the save; do not use attempt start, a window start or a fabricated earlier time as completion. An unexpected response shape is unknown evidence, not an empty result set.
3. **Submit and reconcile the completed-work base.** Its factual `occurred_at` must be no later than both configured starts. Retain the full original request and operation identity before sending; save and reconcile the receipt and original base target. If completion misses either start, preserve the work and refusal/eligibility evidence and stop this billing path. Do not clamp times, shift frozen windows or recreate the store to force acceptance.
4. **Perform the agreed assessment after the ordinary start.** Read the retained artifact and perform the explicitly agreed new evaluation, then capture the actual assessment time. An earlier known result followed by `sleep` is delayed reporting, not a new assessment. Merely relabeling that earlier fact does not change its occurrence time or eligibility.
5. **Submit an eligible known outcome and reconcile it.** Its occurrence is inside the ordinary window; receipt and acceptance must meet their own deadlines. Retain the original base target—outcome receipts do not provide `body.target`. Retry only the original bytes and identities after an unknown write result. Keep base, adjustment and net amounts separate.

Follow the [operation-specific reconciliation recipe](integration-agent-guide.md#reconcile-each-operation) and [exact timing rules](billing-quickstart.md#set-up-explicit-terms). Terms and evidence are application attestations; a valid ledger receipt does not independently establish the truth of provider results, assent or work completion.

## Keep the failure states separate

| Observed fact under this completed-report agreement | Application evidence and billing action |
| --- | --- |
| Work fails before a valid artifact is completed | Save work-error evidence. No completion record, completed-work base or outcome. |
| Work completes, then assessment has a transport, blocked-provider or parse error | Preserve and reconcile the existing base. Save unknown assessment evidence with its original target; submit no outcome. |
| A new assessment establishes that completed work fails the agreed quality criterion | Save the actual assessment evidence and submit the explicitly agreed negative-quality code if eligible. |
| A valid response proves zero results | Retain successful transport/status/schema/parse evidence and verified count zero. Whether this completes work or qualifies for an outcome depends on the terms; it does not by itself establish an elapsed cutoff. |
| An agreed cutoff is actually reached inside the ordinary occurrence window | Newly evaluate the stipulated condition at that cutoff, retain evidence and submit only the outcome the terms justify, within receipt/acceptance deadlines. |

Do not implement `except Exception: code = "unsuccessful-by-cutoff"`. A provider block, timeout or parse failure proves neither zero results nor that an agreed cutoff was reached. A rule such as “no verified success by K, including errors” is a different condition requiring its own explicit terms and assent; it is not this reference example's rule.

`complete: true` describes the retained economic history at the snapshot. Report attempted work, completed work, work errors, pending assessments and ledger refusals/errors separately. It does not mean every attempted application operation succeeded.

## Check your adaptation

Retain the actual setup/init, attempt, durable completion and assessment timestamps. Confirm initialization preceded work, completion met both starts, and the assessment really occurred after ordinary start. Exercise a valid quality-negative artifact, a failure before completion and a failure after completion. Reconcile actual totals and exact retries; the reference's 14/16/2-atom scenario totals describe its own synthetic fixtures, not arbitrary provider data.

Preserve original trials and evidence. Run each new synthetic test in a fresh private child; no real customer or provider call is required to test these failure branches. Provider selection and API costs remain your application's concern.
