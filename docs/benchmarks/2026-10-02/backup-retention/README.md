# Receipt-driven backup retention verification

`tools/retain-object-backups.py` implements newest-count, minimum-age and pinned
recovery-point retention for independently registered, confirmed backup
receipts. Planning is offline. Explicit application verifies all retained and
expired ciphertext identities before deleting only exact non-null versions.
Unregistered versions, delete markers and adjacent prefixes are never targets.
The accepted daemon and encrypted backup/restore format are unchanged.

Fourteen regression tests pass on Linux and Windows with assertions disabled.
They cover policy/receipt validation, default offline CLI planning, reviewed-plan
change refusal before storage access, exact-version requests, full preflight,
missing retained objects, already-absent expired versions, checksum/response
mismatch, uncertain deletion progress, interrupts and mismatched acknowledgements.

Six checks pass against an owned **in-process Moto S3 emulator** using boto3:
newest/pin preservation, expired-version deletion, unregistered versions and
delete markers under reused keys, retained checksum verification, adjacent-prefix
preservation and idempotent reapplication. Its opaque fixture bytes test receipt
identity and S3 version behavior; this is not a fresh KVM or encryption-format
recovery test. Existing encrypted recovery evidence remains separate. No request
was sent to managed S3, and no IAM, durability or performance claim follows.

```sh
python -O test-retain-object-backups.py
# In the dedicated backup test environment with boto3 and Moto installed:
python -O check-backup-retention.py --output /new/path/report.json
```

`manifest.json` binds the source, test logs and emulator report. The report also
binds the implementation and coordinator hashes used in the emulator run.
The [operator workflow](../../../OBJECT_STORAGE_BACKUPS.md#receipt-driven-version-retention)
documents catalog registration, explicit application, result preservation,
download cost and concurrent-writer limitations. Backup capture scheduling,
managed storage verification and external metadata recovery remain incomplete.
