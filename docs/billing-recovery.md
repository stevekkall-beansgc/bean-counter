# Quiescent backup and recovery

Supported procedure for the local SQLite billing profile. A JSON statement is not a backup. Back up the complete installation: the private setup configuration, `.ledger/local.db`, `.ledger/owner.lock`, and any `.ledger/local.db-wal`, `.ledger/local.db-shm` or other retained state present. Keep private directory/file permissions. Do not cherry-pick database files, omit sidecars, edit SQL, or delete the lock file. The exact v0.9.0 installed-package journey exercised this procedure on macOS 26.6.2 arm64 and Ubuntu 24.04.5 x86-64/glibc 2.39, including identical file hashes before reopening and retained statements, receipts and retry identities after restore. See [M8 qualification](m8-native-package-qualification.md).

1. Stop the invoking application and all CLI work; wait for every process to exit and its store to close. Prevent new invocations for the whole copy. The copy command below does not acquire the Ledger owner lock and is supported **only under this administrator-enforced quiescence**. If any process is still running or uncertain, do not copy.
2. Save a complete statement for each customer and permission status for each customer/source pair. Those commands must finish before copying. Record source commit/binary hash, time and every statement cutoff/hash separately.
3. Copy to a new private destination outside the active installation. On the tested native environment, from the parent directory:

   ```sh
   umask 077
   cp -Rp billing billing-backup
   ```

   Do not overwrite an existing backup. Keep a known-good earlier generation, protect backups as sensitive billing data, and allow enough disk for the database and sidecars plus multiple copies. Choose retention and recovery-point frequency based on acceptable data loss. Check the backup by copying it to a separate verification directory and reopening that copy with the matching binary, while original writers stay stopped. A failed copy or failed verification is not a successful backup.
   Before reopening any copy, compare every retained file byte and private mode, including hidden configuration, database and sidecars. With writers excluded, set `ORIGINAL` and `COPY` to the absolute source and newly copied directory, then run:

   ```sh
   python3 - "$ORIGINAL" "$COPY" <<'PY'
   import hashlib, os, pathlib, stat, sys
   def inventory(name):
       root = pathlib.Path(name)
       if root.is_symlink() or not root.is_dir():
           raise SystemExit("expected a regular installation directory")
       entries = {}
       for path in [root, *sorted(root.rglob("*"))]:
           mode = path.lstat().st_mode
           key = path.relative_to(root).as_posix()
           if stat.S_ISDIR(mode):
               entries[key] = ["directory", stat.S_IMODE(mode)]
           elif stat.S_ISREG(mode):
               digest = hashlib.sha256()
               with path.open("rb") as stream:
                   for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                       digest.update(chunk)
               entries[key] = ["file", stat.S_IMODE(mode), path.stat().st_size, digest.hexdigest()]
           else:
               raise SystemExit("refusing a symlink or special file in installation")
       return entries
   original, copy = inventory(sys.argv[1]), inventory(sys.argv[2])
   if original != copy:
       raise SystemExit("copy differs: preserve both and investigate before reopening")
   print("Whole-installation paths, modes, sizes and SHA-256 hashes match before reopening.")
   PY
   ```

   Record the result privately. A matching inventory establishes equality at this quiescent checkpoint; it does not establish valid database contents or replace the statement/receipt checks after reopen. Do not hash while writers can change either directory.
4. To restore, stop and exclude all original writers first. Preserve the damaged/old installation for investigation; copy the complete known-good backup to a **new** private directory. Compare the backup and restored copy with the same inventory command before opening either one. For every customer, run `ledger billing --directory RESTORED statement --customer CUSTOMER --json`; for every source, run `ledger billing --directory RESTORED permissions --customer CUSTOMER --source SOURCE --json`. Verify net atoms, cutoff, snapshot hash, full receipt links and permission history against the captured checkpoint. Retry a previously accepted delivery with the same customer, source and ID; it must return the original receipt without increasing the balance.
5. Select the restored directory as the only active writer. Never resume both copies. Keep the old directory inaccessible to automated writers. Reconcile every input and acknowledgement after the backup cutoff before resuming billing. Restoration to an older snapshot loses later records; the program cannot detect that rollback or deduplicate deliveries absent from the restored backup. External side effects cannot be undone by restoration. This profile dispatches no payments.

Validated scenarios include whole-installation copy after CLI exit, exact statement/receipt preservation after restore, identical retry, exclusive-owner refusal, injected write refusal, actual SQLite page-limit exhaustion (`SQLITE_FULL`), dropped transaction, lost acknowledgement before/after commit, and subprocess exit immediately before/after commit. Process-exit tests do not simulate power loss or certify a storage device. The OS/filesystem must preserve durability and working locks. There is no multi-host writer replacement, online backup API, portable archive import, automatic disaster recovery, rollback detection or guarantee for network storage.

For unavailable storage, free/provide resources without editing ledger records and retry identical input. For `OUTCOME_UNKNOWN`, reopen and resolve by identical delivery identity. For integrity failure, retain evidence and restore a verified checkpoint; never mark an uncertain charge accepted or rolled back from the error alone. Permission-control uncertainty is resolved by its revision and immutable change history using the administrator status command.
