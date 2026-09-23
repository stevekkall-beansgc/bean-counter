# Quiescent backup and recovery

Supported procedure for the local SQLite billing profile. A JSON statement is not a backup. Back up the complete installation: the private setup configuration, `.ledger/local.db`, `.ledger/owner.lock`, and any `.ledger/local.db-wal`, `.ledger/local.db-shm` or other retained state present. Keep private directory/file permissions. Do not cherry-pick database files, omit sidecars, edit SQL, or delete the lock file. This procedure was previously exercised on macOS; its Ubuntu 24.04 x86-64 candidate path remains untested until the authorized package/recovery CI journey passes.

1. Stop the invoking application and all CLI work; wait for every process to exit and its store to close. Prevent new invocations for the whole copy. The copy command below does not acquire the Ledger owner lock and is supported **only under this administrator-enforced quiescence**. If any process is still running or uncertain, do not copy.
2. Save a complete statement and permission status for reconciliation. Those commands must finish before copying. Record source commit/binary hash, time and the backup's cutoff/hash separately.
3. Copy to a new private destination outside the active installation. On the tested native environment, from the parent directory:

   ```sh
   umask 077
   cp -Rp billing billing-backup
   ```

   Do not overwrite an existing backup. Keep a known-good earlier generation, protect backups as sensitive billing data, and allow enough disk for the database and sidecars plus multiple copies. Choose retention and recovery-point frequency based on acceptable data loss. Check the backup by copying it to a separate verification directory and reopening that copy with the matching binary, while original writers stay stopped. A failed copy or failed verification is not a successful backup.
4. To restore, stop and exclude all original writers first. Preserve the damaged/old installation for investigation; copy the complete known-good backup to a **new** private directory. Run `ledger billing --directory RESTORED statement --customer CUSTOMER --json` and `permissions --json`. Verify net atoms, cutoff, snapshot hash, full receipt links and permission history against the captured checkpoint. Retry a previously accepted delivery with its unchanged ID; it must return the original receipt without increasing the balance.
5. Select the restored directory as the only active writer. Never resume both copies. Keep the old directory inaccessible to automated writers. Reconcile every input and acknowledgement after the backup cutoff before resuming billing. Restoration to an older snapshot loses later records; the program cannot detect that rollback or deduplicate deliveries absent from the restored backup. External side effects cannot be undone by restoration. This profile dispatches no payments.

Validated scenarios include whole-installation copy after CLI exit, exact statement/receipt preservation after restore, identical retry, exclusive-owner refusal, injected write refusal, actual SQLite page-limit exhaustion (`SQLITE_FULL`), dropped transaction, lost acknowledgement before/after commit, and subprocess exit immediately before/after commit. Process-exit tests do not simulate power loss or certify a storage device. The OS/filesystem must preserve durability and working locks. There is no multi-host writer replacement, online backup API, portable archive import, automatic disaster recovery, rollback detection or guarantee for network storage.

For unavailable storage, free/provide resources without editing ledger records and retry identical input. For `OUTCOME_UNKNOWN`, reopen and resolve by identical delivery identity. For integrity failure, retain evidence and restore a verified checkpoint; never mark an uncertain charge accepted or rolled back from the error alone. Permission-control uncertainty is resolved by its revision and immutable change history using the administrator status command.
