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

`--endpoint https://s3.example` selects an S3-compatible service; ordinary AWS
S3 uses the SDK default endpoint. TLS verification stays enabled. HTTP is allowed
only for loopback fixtures. `--region` defaults to `us-east-1`. Bucket policy,
versioning, object lock, retention and independent replication remain operator
responsibilities. This implementation uses one `PutObject` and refuses ciphertext
larger than 5,000,000,000 bytes; multipart backups are not implemented.

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
