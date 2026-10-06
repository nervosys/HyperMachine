# Version-pinned S3 backup recovery

Backup receipts now retain S3's returned `version_id`, for both PutObject and
multipart completion. Restore accepts `--version-id`, requests that exact
version, and refuses a mismatched response before reading its ciphertext.
The independent whole-object `--sha256` verification remains available and
recommended. Without a version ID, restore requests the current object.

The owned Moto S3 HTTP fixture passes eleven check groups: the existing nine
multipart recovery/failure groups and version-pinned recovery for single and
multipart uploads. Each versioned case creates a backup, places a delete marker
at the current key, restores the old version, uploads changed source bytes under
the same key, and restores both old and new versions with their independent
receipts. Reading the latest object with the old checksum is refused. A changed
response version is refused and its HTTP stream closed. Explicit deletion of
the historical version then causes pinned restore to fail without publication.

A real KVM cohort passes all fifteen recovery/refusal checks. It recovers a
five-part 36,606,988-byte encrypted backup by version ID after deleting its current
key and making the original store path unavailable. Guest memory/process state,
filesystem/boot identity, mounted 9P volume data/identity and a named snapshot
survive at the new path. All owned processes exit cleanly and guest/volume
inventories are empty. Daemon and guest inputs match the preceding
[multipart evidence](../multipart-backup/README.md); provisional workspace boot
changes were not compiled or executed. No daemon changes were required.

Two failed synthetic cohorts remain with their exact coordinators. They incorrectly
observed botocore's inherited `closed` property, then instrumented `StreamingBody.close`;
its context manager instead closes the underlying HTTP stream directly. The final
fixture observes that actual close call. The production helper remained identical
through these runs, and the KVM cohort passed before this fixture correction.

This verifies local protocol behavior through real HTTP operations. It does not
establish AWS durability, IAM, retention, service availability or a performance
win. Bucket versioning and retention remain operator responsibilities. A `null`
version is mutable; versions can be removed by explicit deletion or lifecycle
policy. Lost upload acknowledgements may leave no version ID in an attempt
receipt, requiring operator inspection and byte-checksum verification.

The archive contains reports, S3 logs and frozen tools, without raw guest data,
keys, credentials, ciphertext bundles or executables. Dependency versions are
recorded in reports. Verify the archive on Windows or Linux with:

```sh
python -O tools/verify-version-backup.py docs/benchmarks/2026-10-02/version-backup
```

Reproduce in Linux with pinned backup dependencies and `moto[s3,server]==5.2.3`,
using new private output directories:

```sh
python -O tools/check-multipart-backup.py --versioned --output /private/new-version-run
python -O tools/check-object-backup.py --versioned --multipart \
  --daemon /owned/hv2-sandboxd --kernel /owned/bzImage \
  --initrd /owned/guest-ssh.cpio.gz --output /private/new-version-kvm
```
