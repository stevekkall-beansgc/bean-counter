# Operate the local billing installation

Use the exact qualified binary for the installation's storage version. Keep its source commit, binary SHA-256, customer/source inventory and agreed configuration with your private operational records. [M8 qualification](m8-native-package-qualification.md) covers the v0.9.0 named native targets; [recovery](billing-recovery.md) defines the supported copy/restore procedure.

## Check before admitting work

Confirm `ledger --version`, the private canonical installation path, exclusive writer ownership, working local locks/fsync and enough memory/disk for live growth, sidecars, reports and retained backup generations. Admission ceilings are not reserved capacity. Inspect local free space and installation size with the host's tools; a successful ledger read does not establish resource headroom or storage durability.

For every operator-maintained customer/source pair, run:

```sh
"$LEDGER" billing --directory "$BILLING_DIR" permissions --customer "$CUSTOMER" --source "$SOURCE" --json
"$LEDGER" billing --directory "$BILLING_DIR" statement --customer "$CUSTOMER" --json
```

Preserve exit status and JSON output. Require the complete history result, the intended scope, retained receipt identities and exact integer balance. Compare against the application's original requests, acknowledged receipts and prior checkpoint; do not infer correctness from a plausible net balance alone. Read-only history/permission commands are the current diagnostic surface; there is no separate health daemon or automatic all-customer monitor.

## Reconcile interrupted callers

The application persists exact bytes, customer/source, delivery ID and semantic operation ID before submitting work. Exit 8 or an absent response leaves the result unknown. Reopen the same installation and retry that identical request; an exact duplicate returns the original receipt. A new identity can create another charge and is not recovery. Do not label an unresolved request accepted or rolled back.

The [Python and Node examples](../examples/integration/README.md) acknowledge only after a complete target explanation contains that original receipt and operation. Usage needs explanation `/3`; the older v0.9.0 bundled helpers' `/2` restriction is corrected in v0.9.1. Period statement `/4` has a different purpose and is not an interchangeable target-history acknowledgement.

Permission-control uncertainty is reconciled with the immutable change ID and permission revision/history. A changed request under an old identity is a conflict. A refusal, incomplete explanation or integrity failure is not an acknowledgement.

## Operate periods and reports

Use the [M5 quickstart](billing-quickstart.md#billing-lifecycle-m5) and the versioned M5 request examples/contracts. A customer term must cover retained accepted work. Record the exact close request and returned immutable period statement. Retry an unknown close with its original command identity; never reopen or rewrite a closed statement.

Before close, eligible quantity corrections append linked deltas. After close they create a later linked adjustment. An adjustment appears once through the standard or explicitly issued ad hoc statement. Recurrence requires explicit occurrence acceptance; due-occurrence queries do not charge automatically. Fiscal reports pin their own calendar and ledger snapshot and do not change customer economics.

Reconcile the immutable statement's exact integer totals, adjustment identities and snapshot hash with [finance export](finance-e2e.md). A complete CSV trailer is a projection check, not downstream delivery, payment, tax or legal-invoice proof.

## Backup, restore and refusal handling

Stop and exclude every caller for the entire complete-installation copy, retain all files and present sidecars with private permissions, keep earlier generations, and verify a separate restored copy against saved checkpoints before selecting one active writer. Never resume both copies, delete owner locks, cherry-pick SQL files or edit retained rows. Reconcile requests after the backup cutoff before admitting new work; rollback to an old backup cannot recover identities absent from it.

For exit 7, investigate the specific unavailable lock/storage/resource condition. For exit 9, retain the installation and diagnostic evidence and stop writes pending verified recovery. Unsupported older writers are expected to refuse; use the explicit qualified upgrade path with a whole-installation checkpoint. Restore does not establish rollback detection, power-loss tolerance, online backup or multi-host operation.
