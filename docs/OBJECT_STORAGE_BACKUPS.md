# Encrypted offline snapshot-store backups

`tools/backup-snapshot-store.py` backs up an offline HyperMachine snapshot store
through S3 and restores it into a new local directory. This is an operator tool
for Linux, Python 3.11 or newer, and nodes built with the snapshot-store backup
lock protocol. Install its pinned dependencies into a dedicated environment:

```sh
python3 -m venv .backup-venv
.backup-venv/bin/pip install -r tools/requirements-backup.txt
```

The workflow covers paused sandbox state, shared templates, named snapshots,
store-contained volume files and store-contained signing/egress material. Pause
every running sandbox you intend to recover, then stop **every node** using the
store. Stopping a running guest does not automatically make it a recoverable
paused guest. Also stop other programs that write the store. This is an offline
backup, not a live fleet snapshot or an application-consistent database backup.

Upgraded nodes hold a shared OS lock on `.backup.lock` for their entire store
lifetime, before constructing templates or serving requests. Backup requires an
exclusive nonblocking lock and refuses while a participating node is running;
nodes refuse startup while the offline lock is held. Multiple nodes can retain
shared locks simultaneously. The lock file is not copied into the backup and is
recreated for a restored store. Do not replace or remove an active lock file.
All nodes must use this protocol; old binaries do not participate. The helper
refuses a store without its existing lock file. Shared filesystems must provide
working cross-node file-lock semantics; the fixture verifies local Linux locking,
not NFS or a distributed-filesystem guarantee.

## Backup

Use a separate random 32-byte encryption key outside the store and protect it as
an operator secret. Keep an independent copy: S3 never receives this key and
cannot recover the backup without it. Do not put keys in arguments or source
control. Configure S3 credentials through boto3's standard provider chain (for
example an operator profile or workload role); the helper has no credential
arguments. A minimal operator example, after pausing guests and stopping nodes:

```sh
umask 077
openssl rand -hex 32 > /secure/operator/backup.key
python3 tools/backup-snapshot-store.py backup \
  --store /srv/hypermachine/snapshots \
  --bucket operator-backups --object cluster-a/2026-10-02-unique.hmb \
  --key-file /secure/operator/backup.key > /secure/operator/backup-receipt.json
```

Use a **new** object key for each backup. Upload sends `If-None-Match: *`, so an
S3 service supporting conditional writes refuses an existing object. The helper
also sends a SHA-256 checksum and outputs the encrypted object length and digest.
Retain this receipt independently of the bucket. Conditional-write behavior is
described in [AWS documentation](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html).
The helper does not retry requests automatically. If upload fails after sending
bytes, the object may already exist. The failure JSON includes an `attempt_receipt`
with the encrypted object's checksum and length and `upload_confirmed: false`;
retain it, inspect the object and compare that checksum before deciding how to
proceed. An attempt receipt establishes which bytes the client tried to send,
not proof that the server committed them. The helper never
deletes or overwrites an existing backup as recovery from an upload error.

`--compression-level 1` is the default; levels 1–9 select gzip's capture-speed
and storage-size tradeoff. The receipt records the selected level. Higher levels
can reduce uploaded bytes at the cost of additional capture CPU and elapsed time;
the restore format and encryption are unchanged.
The [paired KVM-store comparison](benchmarks/2026-10-02/backup-compression/README.md)
produced 13.3% fewer bytes at level 6, with slower capture in both cohorts.
Level 6 also passed real KVM recovery through multipart S3 upload.

`--endpoint https://s3.example` selects an S3-compatible service; ordinary AWS
S3 uses the SDK default endpoint. TLS verification stays enabled. HTTP is allowed
only for loopback fixtures. `--region` defaults to `us-east-1`. Bucket policy,
versioning, object lock, retention and independent replication remain operator
responsibilities. Ciphertext at or above 64 MiB uses multipart upload with
64 MiB parts; smaller backups use `PutObject`. `--multipart-threshold-mib`
(1–4096) and `--multipart-part-mib` (8–128) configure these values. Objects above
5,000,000,000 bytes always use multipart upload. The encrypted format accepts
at most 64 GiB of ciphertext, including its 36-byte framing, and refuses larger
compressed plaintext before encryption.

Multipart upload sends SHA-256 checksums for every part and conditions completion
on the object being absent. The receipt's `sha256` remains the whole ciphertext
digest used for recovery. `composite_sha256` is AWS's separate digest of the part
digests, followed by the part count; it cannot replace the recovery digest.
See [AWS multipart checksums](https://docs.aws.amazon.com/AmazonS3/latest/userguide/tutorial-s3-mpu-additional-checksums.html).
On failure or an interrupt, the helper attempts to abort only its own known
upload ID and reports `multipart_cleanup`. Lost creation responses or failed
aborts can leave incomplete uploads requiring operator cleanup. Lost completion
responses can leave a complete object: preserve the attempt receipt and inspect
it before retrying. Configure bucket lifecycle cleanup for incomplete uploads;
the helper never aborts uploads owned by other invocations.

## Restore

Stop nodes that would serve the replacement store. Select a **new, absent**
directory; restore never replaces an existing file, directory or symlink. Give
it the digest from the independently retained backup receipt:

```sh
python3 tools/backup-snapshot-store.py restore \
  --destination /srv/hypermachine/recovered-snapshots \
  --bucket operator-backups --object cluster-a/2026-10-02-unique.hmb \
  --key-file /secure/operator/backup.key --sha256 RECEIPT_SHA256
```

`--sha256` is optional but protects against substitution of a different, otherwise
valid encrypted backup under the same key. Without an independent checksum,
authenticated encryption detects corruption but does not prove that the bucket
returned the particular historical backup the operator intended. A checksum
obtained from that same bucket at restore time does not establish independence.

For a versioned bucket, the upload receipt includes `version_id` when S3 returns
one, for both single and multipart uploads. Retain it alongside the checksum.
Add `--version-id RECEIPT_VERSION_ID` to restore a specific historical object,
including when the current key is a delete marker or refers to a newer backup.
The helper checks the response version before reading the ciphertext and still
checks the independently supplied `--sha256` before decrypting. AWS requires
`s3:GetObjectVersion` for this operation; see [GetObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObject.html).
Without `--version-id`, restore reads the current object and reports
`version_pinned: false`. A lost upload acknowledgement may leave an attempt
receipt without a version ID; inspect the bucket's versions and verify bytes
against its checksum rather than inventing an identifier. Versioning remains
operator-configured. A `null` version in an unversioned or suspended bucket is
mutable, and retained versions can be deleted or expired by policy; a version ID
does not establish retention or durability.
[Version-pinned recovery evidence](benchmarks/2026-10-02/version-backup/README.md)
covers delete markers, key reuse, single/multipart versions, response mismatch,
deleted-version refusal and real KVM guest recovery at a new store path.

Restore downloads into private temporary storage, verifies the supplied receipt,
and authenticates the complete ciphertext before parsing it. It validates the
manifest, file catalog, hashes, sizes and relative names; refuses links, duplicate,
extra or missing files; and enforces an expanded-byte limit. It writes files to a
private staging directory beside the destination, rewrites VM snapshot absolute
memory-base references to the new root, syncs files/directories and publishes
with Linux `renameat2(RENAME_NOREPLACE)`. A concurrent destination creation causes
refusal rather than replacement. An error after atomic publication (for example
a parent-directory sync failure) can leave a complete destination; inspect it
before retrying. Earlier failures clean up temporary data and publish no store.

Configure replacement nodes with the recovered `--snapshot-store` path and,
when specified separately, its matching `--volume-dir`. Supply the original
compatible kernel, initrd, guest resources and backend environment. Resume
paused sandbox IDs through the normal API. Layered VM snapshot headers point at
the recovered base images; ordinary volume/application files ending in `.snap`
are preserved as files rather than interpreted as VM snapshots.

## Durable scheduled offline capture

`tools/schedule-object-backups.py` is a Linux one-shot schedule runner. Invoke
`run` periodically from an operator-owned timer or job worker. It evaluates a
fixed UTC anchor and interval, captures the latest due slot once, and coalesces
missed slots into one current backup. It never pretends to capture historical
guest state. Keep clocks synchronized. It does **not** pause guests or stop nodes:
participating live nodes retain their shared lock, and capture is refused until
every store writer is offline. Arrange maintenance windows separately.

Install the existing pinned backup dependencies. Use a version-enabled bucket
and an operator profile/workload role with backup permissions plus
`s3:GetBucketVersioning`; the runner requires an `Enabled` response from
[GetBucketVersioning](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html).
Reconciliation also reads an exact object version.
Credentials stay in the SDK provider chain. Configure an absolute key-file path
outside the store; never put the key bytes or credentials in configuration:

```json
{
  "start_at": "2026-10-02T00:00:00Z",
  "interval_seconds": 86400,
  "store": "/srv/hypermachine/snapshots",
  "bucket": "operator-backups",
  "prefix": "cluster-a/",
  "key_file": "/secure/operator/backup.key",
  "region": "us-east-1",
  "compression_level": 1
}
```

`endpoint`, `work_dir`, `max_expanded_bytes`, `multipart_threshold_mib` and
`multipart_part_mib` optionally use the same semantics/defaults as manual
backup. Unknown fields and invalid policy values are refused. Changing any
configuration value requires a separate state directory; this prevents silently
mixing buckets, prefixes or schedules. Keep old state and receipts independently.

```sh
python3 tools/schedule-object-backups.py run \
  --config /secure/operator/backup-schedule.json \
  --state /var/lib/hypermachine-backups
python3 tools/schedule-object-backups.py status \
  --config /secure/operator/backup-schedule.json \
  --state /var/lib/hypermachine-backups
```

The state directory's parent must already exist. The runner creates a private,
operator-owned directory outside the snapshot store and refuses symlinks or
unmanaged catalog files. Its persistent `.schedule.lock` prevents overlapping
invocations on the tested local Linux filesystem. Journal writes are private,
atomic and synced along with directory entries. No filesystem-independent or
distributed-lock guarantee follows. `status` reports state and repairs the
derived catalog locally; it does not contact S3.

Run the same `run` command regularly, for example from a once-per-minute cron
entry using absolute interpreter/script paths. Each eligible capture gets a
new UUID object key and the manual helper's conditional upload. A known failure
before upload can be retried at a later poll with a new key. The runner records
the attempted ciphertext checksum and size **before** the first upload request.
A pending or uncertain upload blocks later captures, including after restart.
An upload reply or local publication failure is not treated as permission to
send another backup automatically. Preserve each command's JSON result.

After inspecting the uncertain object and its versions, reconcile a known exact
version against the independently journaled ciphertext identity:

```sh
python3 tools/schedule-object-backups.py confirm \
  --config /secure/operator/backup-schedule.json \
  --state /var/lib/hypermachine-backups --version-id VERIFIED_VERSION_ID
```

Confirmation downloads that version and verifies its response ID, full byte count
and checksum before registering it. It retains the original attempt time. If
the object cannot be reconciled, an operator may explicitly advance the cursor:

```sh
python3 tools/schedule-object-backups.py skip \
  --config /secure/operator/backup-schedule.json \
  --state /var/lib/hypermachine-backups --attempt-sha256 REVIEWED_ATTEMPT_SHA256 \
  --reason backup_unrecoverable
```

`object_absence_verified` is another operator-recorded reason after independent
inspection. Skip requires the exact current attempt digest, retains its uncertain
receipt in history, and deletes nothing. It does not automatically verify absence
or register a recovery point. Only a later slot becomes eligible.

Confirmed receipts produce `catalog.json` for the retention helper. `journal.json`
is authoritative: a later invocation repairs catalog publication interrupted
after journal commit. Do not edit the derived catalog. Manage pins durably:

```sh
python3 tools/schedule-object-backups.py pin \
  --config /secure/operator/backup-schedule.json \
  --state /var/lib/hypermachine-backups --object RECEIPT_OBJECT \
  --version-id RECEIPT_VERSION_ID
# The same exact identity with the unpin command explicitly removes its pin.
```

Use the scheduler's `retain` command for retention of its registered backups.
It acquires the same persistent state lock as capture and pin changes, repairs
the derived catalog from the journal, and holds the lock through verification
and deletion. Planning is offline; deletion requires explicit `--apply`.

```sh
python3 tools/schedule-object-backups.py retain \
  --config /secure/operator/backup-schedule.json \
  --state /var/lib/hypermachine-backups \
  --keep-newest 7 --older-than-days 30 --as-of 2026-10-02T00:00:00Z
# Review the plan, then repeat the options with:
# --apply --plan-sha256 PLAN_SHA256
```

A changed plan digest is refused before storage access. Capture does not run
retention automatically. The standalone retention helper still requires external
serialization when used with a scheduler catalog. Other clients and external S3
lifecycle rules do not participate in this local lock. Protect and archive the
journal; its history stops captures at 10,000 confirmed/skipped records.
[Locked retention evidence](benchmarks/2026-10-02/locked-retention/README.md)
covers offline planning, digest rejection, lock contention, exact-version deletion,
pinned recovery, idempotence and continued capture in the owned emulator.

[Scheduled capture verification](benchmarks/2026-10-02/scheduled-backups/README.md)
covers actual encrypted CLI capture and restore, restart/coalescing, offline-lock
refusal, ambiguous uploads, reconciliation, explicit skip, pins and catalog repair
on owned S3 emulators. [Scheduled real KVM recovery](benchmarks/2026-10-02/scheduled-kvm/README.md) additionally verifies multipart capture, exact-version recovery, guest state, mounted volumes and named snapshots. Managed IAM/durability, coordinated guest maintenance and distributed filesystems remain unverified.

## Receipt-driven version retention

`tools/retain-object-backups.py` plans retention without contacting storage.
`--apply` explicitly enables removal of expired **exact non-null versions**.
This operator tool does not list the bucket or infer ownership from its contents:
only confirmed, independently retained backup receipts enter its catalog.
Unversioned/null IDs, uncertain upload receipts, duplicate versions, future or
timezone-free dates and receipts outside the exact prefix are refused.

Keep this catalog outside the store and bucket, protect it as operator state,
and serialize catalog updates with retention runs. Register each confirmed
backup receipt with its operator-recorded UTC creation time. Do not substitute
an upload attempt receipt for a confirmed receipt. A catalog has this shape:

```json
{
  "version": 1,
  "bucket": "operator-backups",
  "prefix": "cluster-a/",
  "receipts": [
    {
      "created_at": "2026-10-02T00:00:00Z",
      "pinned": true,
      "receipt": {
        "operation": "backup",
        "success": true,
        "object": "cluster-a/2026-10-02-unique.hmb",
        "version_id": "RECEIPT_VERSION_ID",
        "encrypted_bytes": 123456,
        "sha256": "RECEIPT_SHA256"
      }
    }
  ]
}
```

The values in `receipt` come from the confirmed backup CLI output; replace the
placeholders with its actual values. Retention keeps the newest configured
count, all pinned recovery points, and all backups newer than the minimum age.
Only entries outside all three protections expire. The count must be positive;
an empty catalog is refused. Use a fixed `--as-of` when reviewing a plan:

```sh
python3 tools/retain-object-backups.py --catalog /secure/operator/catalog.json \
  --keep-newest 7 --older-than-days 30 --as-of 2026-10-02T00:00:00Z
# After reviewing the exact expired versions, repeat the same options and add:
# --apply --plan-sha256 PLAN_SHA256
```

The optional plan digest rejects changed dates, receipts, pins or policies.
Without `--as-of`, evaluation uses current UTC time; repeating the reviewed
digest requires the same evaluation time. An operator scheduler may invoke
`--apply` with its authorized policy and current catalog, retaining each result
independently. This does not schedule backup capture, pause guests, or stop nodes.

Before any deletion, the tool downloads every retained recovery point and every
expired candidate, verifies the response version, byte count and full ciphertext
SHA-256 against its receipt, then closes the stream. This needs download bandwidth
and S3 read permissions even for large multipart backups; it does not decrypt or
require the encryption key. A missing or damaged retained backup blocks all
deletions. Only a candidate's `NoSuchVersion` response is treated as already absent
for idempotent reruns. Other errors fail closed.

Deletion sends `Bucket`, `Key` and the recorded `VersionId`, never an unversioned
delete, a listing-derived target or a delete-marker removal. AWS documents that
deleting a specific version requires `s3:DeleteObjectVersion` and permanently
removes that version. See [DeleteObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteObject.html).
No object-lock governance bypass is requested. Configure bucket IAM, object lock,
replication and lifecycle policies independently.

Requests are not automatically retried. An exception, interrupt or mismatched
delete acknowledgement stops the run and reports confirmed progress plus the
exact `delete_unconfirmed` identity. The version might already be gone; preserve
the result and inspect it before retrying. An interrupted deletion returns 130.
Concurrent external lifecycle expiry or deletion can invalidate recovery points
after preflight; this tool is not a transaction or durability guarantee.

[Retention verification](benchmarks/2026-10-02/backup-retention/README.md) covers
exact-version deletion, reused keys, delete markers, pins, adjacent-prefix and
unregistered-version preservation, idempotence and failure handling. It uses an
owned in-process Moto S3 emulator, not managed S3 or IAM enforcement. No existing
backup format, guest state or accepted daemon is changed.

## Format and limits

The v1 bundle is a gzip tar containing a bounded JSON manifest and regular file
entries. It is encrypted with AES-256-GCM, a fresh random 96-bit nonce, and a full
128-bit tag. The authenticated 20-byte header is `HMBACK01` plus the nonce;
ciphertext follows and the tag occupies the last 16 bytes. The manifest, original
store path, snapshot metadata and guest contents are encrypted. The helper uses
[cryptography's GCM implementation](https://cryptography.io/en/latest/hazmat/primitives/symmetric-encryption/)
and waits for authentication before using decrypted data.

Source symlinks, special files, unfinished paused claims/template scratch builds,
and VM memory dependencies outside the store are refused. Regular file and
directory permissions are preserved without setuid/setgid/sticky bits; ownership,
ACLs, extended attributes, hard-link identity and sparse allocation are not
preserved. Directories/files are owned by the restore operator. Sparse memory
images can require their full logical size when restored. In-memory checkpoint
indexes are not reconstructed; stored checkpoint bytes are not a promise that
the checkpoint API can rediscover them. Redis/control-plane metadata, external
volumes, running guest state, operator API-key configuration and kernel/initrd
artifacts outside the store require separate recovery procedures. Host/backend
compatibility is not established by copying a snapshot.

The default expanded-data cap is 64 GiB, the catalog cap is 100,000 entries and
the manifest cap is 32 MiB. `--max-expanded-bytes` can change the byte cap. Use
`--work-dir` outside the store to select private scratch space; plan for both
compressed plaintext and ciphertext there, plus the full restored store beside
the destination. Temporary plaintext is owner-only and is removed on ordinary
completion/error; the filesystem can retain data after unlink, and process/host
crashes can leave temporary directories for operator cleanup. Encryption keys
and receipts must be retained outside that temporary space.

[Verification evidence](benchmarks/2026-10-02/object-backup/README.md) uses an owned
Moto S3 HTTP emulator and real nested KVM guests. It verifies wire operations,
local recovery and the tested failure behavior; it does not establish managed
S3 durability, IAM enforcement, cross-region recovery, backup scheduling,
retention automation, service availability or a performance win.

[Multipart evidence](benchmarks/2026-10-02/multipart-backup/README.md) additionally
verifies a 75-part object of 5,002,521,063 ciphertext bytes, identical recovery of
5,001,000,000 plaintext bytes, scoped failure cleanup and ambiguous completion,
and five-part recovery of real KVM paused state, volumes and named snapshots.

The [scheduled retention lifecycle evidence](benchmarks/2026-10-02/scheduled-retention-lifecycle/README.md) verifies pruning through a scheduler-generated catalog, repeat cleanup, pinned restore after pruning, and subsequent capture in the owned S3 emulator. Historical deleted receipts remain in the journal; expired missing versions are idempotent cleanup outcomes.
