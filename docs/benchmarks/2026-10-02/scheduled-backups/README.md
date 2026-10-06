# Durable scheduled offline backup verification

The Linux one-shot runner captures the latest due UTC slot, coalesces missed
intervals, serializes invocations and records ciphertext identity before upload.
Confirmed journal records feed a derived retention catalog. Uncertain uploads
block capture across restart until exact-version checksum reconciliation or an
explicit reviewed skip. Skip preserves history and deletes nothing. Exact-version
pin/unpin operations survive catalog repair. The original backup helper changes
only by adding an optional pre-upload receipt callback; manual callers keep the
existing encrypted format, offline protocol and conditional upload behavior.

Nineteen regression tests pass on Linux with assertions disabled. Windows passes
seventeen portable state/configuration tests and explicitly skips two Linux-only
filesystem checks. Coverage includes schedule restart/coalescing, strict schema,
private atomic state, parent-directory sync path, state lock contention,
pre-upload/local failure retry, pending intent and journal publication failures,
preserved acknowledged receipts, explicit skip and durable pins. Windows runtime
scheduling is unsupported.

Sixteen end-to-end checks pass with actual scheduled CLI invocation against an
owned **in-process Moto S3 emulator**. Five encrypted backups are confirmed and
registered; one skipped uncertain upload remains intact. Checks cover live-store
lock refusal without upload, later offline retry, lost upload acknowledgement,
blocked later slots, substitution refusal, version-pinned checksum reconciliation,
retention registration, encrypted restore with identical plaintext, explicit skip,
skipped history preservation, catalog repair, pins and unpins. Fixture clocks are
injected; no operating-system cron/timer is installed. No fresh KVM guest is run.

The existing manual helper additionally passes fourteen regression checks against
an owned **loopback Moto HTTP server**, including encrypted roundtrip, malformed
archive/receipt refusal, active-store exclusion, conditional upload and ambiguous
upload receipt preservation. That owned server exits zero with no cleanup errors.
No request is sent to managed S3 and no durability, IAM or performance win is
claimed. The accepted daemon and provisional core files are untouched.

```sh
python -O test-schedule-object-backups.py
# Dedicated backup test environment with boto3, cryptography and Moto:
python -O check-scheduled-backups.py --output /new/path/report.json
python -O check-object-backup.py --output /new/path/manual-regression
```

The manifest binds source, results and platform logs. Each emulator report also
binds its actual implementation/coordinator hashes. The
[operator workflow](../../../OBJECT_STORAGE_BACKUPS.md#durable-scheduled-offline-capture)
covers configuration, periodic invocation, independent journal protection,
reconciliation, explicit skip, pin management and serializing retention through
the same lock. Coordinated guest maintenance, distributed filesystems,
managed-store enforcement and timer deployment remain unverified.
