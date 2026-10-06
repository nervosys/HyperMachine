# Scheduler-integrated locked retention

The scheduler retain command plans offline from its authoritative journal and holds the same exclusive state lock used by capture and pin changes throughout preflight and deletion. Explicit --apply enables deletion; optional --plan-sha256 rejects an altered plan before storage access. Policies require a positive keep count and age; empty catalogs are refused.

The owned Linux Moto S3 emulator passed 24 CLI integration checks. New checks prove offline planning using a client that rejects storage access, reject an incorrect plan digest before client construction, and refuse application under a contended state lock without changing the object inventory. Reviewed application deletes three expired versions, repetition verifies absence, a pinned encrypted recovery point restores identical plaintext, and subsequent capture and retention delete one additional expired version. Unregistered and skipped uncertain versions remain untouched. Final inventory contains four versions and six historical confirmed receipts.

Linux scheduler tests passed 19/19; Windows passed 17 portable tests with two Linux-only skips. Run on Linux with backup dependencies and Moto:

```sh
python -O check-scheduled-backups.py --output fresh-report.json
python -O test-schedule-object-backups.py
```

This verifies a local owned emulator workflow, not managed durability, IAM, distributed locking, KVM recovery, or performance leadership. External S3 clients and lifecycle rules do not participate in the scheduler lock. Previous evidence archives remain immutable.
