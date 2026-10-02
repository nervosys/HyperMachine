# Encrypted offline S3 backup and KVM recovery

This adds a Linux operator workflow for encrypted snapshot-store capture, a
conditional S3 upload, and verified restore into a new directory. Participating
nodes hold a shared lifetime lock; backup requires an exclusive lock. Captured
VM memory bases must remain inside the store and are rewritten on restore.
Authentication and file-catalog validation precede atomic no-replace publication.
Default daemon operation without a snapshot store is unchanged. See the
[operator guide](../../../OBJECT_STORAGE_BACKUPS.md) for commands and limits.

The fixture uses an owned Moto S3 HTTP emulator and WSL nested KVM, one vCPU
and 1024 MiB per guest. This is functional evidence, not a latency comparison,
managed S3 durability/IAM test, cross-region recovery test or availability SLA.

## Retained cohorts

| Cohort | Outcome | Scope / failure |
|---|---|---|
| `synthetic` | pass | Initial encrypted upload, relocation, conditional-write refusal and invalid-input checks |
| `kvm` | fail | Coordinator polled the control-plane `/health` path instead of this node's `/templates` readiness path; no recovery attempt reached |
| `kvm-corrected` | fail | A paused guest was prepared, but the fixture tried to write a host volume marker before creating its parent volume directory |
| `kvm-final` | pass | Real guest memory/process/filesystem and mounted-volume recovery at a new root; original path unavailable; receipt verified |
| `kvm-coverage` | pass | Also verifies recovered named-snapshot creation and ordinary `.snap` files in volumes |
| `kvm-verified` | pass | Final helper; repeats full recovery and injects client acknowledgement loss after a real emulator upload commit |

Both failed reports and logs remain present. Every cohort stopped its owned
processes and verified its tool/input artifacts unchanged. Frozen tool and
coordinator versions bind each report to the code that actually ran; these are
functional iterations with changing coverage, not matched performance repeats.

## Final verification

The clean daemon passed **41 Linux and 36 Windows tests**, including shared locks
across multiple node handles, exclusive backup exclusion and restart refusal;
Linux also rejects a symlink lock file. Strict Linux and Windows Clippy passed with the
existing `too_many_arguments` exception. The release daemon SHA-256 is
`2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f`.
The three provisional boot files in the worktree were excluded from both native
and Windows source trees. Compiled overlays and clean boot hashes are recorded.

The final coordinator passed **15 check groups** under Python `-O`. It verifies:

- Encrypted conditional upload, stable existing-object bytes after refusal,
  relocated file/snapshot payloads, empty directories and ordinary permissions.
- Refusal of an active store, wrong key, modified/truncated ciphertext, excessive
  expanded data, unsafe paths, symlink entries, missing/extra/duplicate files,
  incorrect hashes, source symlinks, unfinished claims and wrong receipt hashes.
- An injected lost client acknowledgement after the emulator commits the object:
  CLI failure includes a verifiable attempt receipt, and a subsequent restore
  using that receipt succeeds. This is not a genuine network-drop or managed S3
  reliability test; the injection point is explicit in the frozen coordinator.
- Real KVM recovery of a paused guest with its original ID/access token, a live
  process with a unique environment-memory marker, boot identity and tmpfs state.
  A 9P volume written inside the guest retains its bytes, volume ID and token.
  A recovered named snapshot creates another guest with the preserved state.
  An ordinary application file named `user.snap` remains an ordinary file.

The final backup contains nine files representing 1,088,614,409 expanded bytes;
its encrypted object is 36,666,615 bytes. These sizes describe this fixture;
they are not compression, storage-performance or density rankings. The helper
rewrites only recognized VM snapshot locations, after review caught an initial
implementation treating every `.snap` file as a VM. Snapshot payload hashes in
`relocation-evidence.json` independently record unchanged named-snapshot payload
bytes across header relocation in the preceding coverage cohort.

An additional TLS/mTLS, observer-role, OpenSSH/TCP and lifecycle regression passed
**20/20 cases** with this daemon. All **160 audit records** verify with zero
unfinished admissions. Its 22 owned processes stopped and sandbox inventory was
empty. This checks existing interfaces after introducing the lifetime store lock;
it does not measure backup transfer through those interfaces.

The operator helper uses AES-256-GCM through `cryptography==50.0.0` and
`boto3==1.43.107`; the emulator is `moto==5.2.3`. Full dependency versions are
frozen for reproduction. Production credentials, backup encryption keys,
plaintext snapshot/volume data and executables are not included in this archive.
The TLS audit key is deliberately synthetic: byte 0x42 repeated 32 times.

```sh
python3 -O tools/verify-object-backup.py docs/benchmarks/2026-10-02/object-backup
python3 -O tools/check-object-backup.py --output /new/owned/check \
  --daemon /path/to/hv2-sandboxd --kernel /path/to/kernel --initrd /path/to/compatible/guest
```

The archive verifier checks SHA-256 identities, compiled overlays, all retained
cohort outcomes, final check coverage, S3 wire-log evidence, snapshot payload
relocation and the TLS HMAC log. Reproduction requires the compatible guest input
hashes in the reports; generic readiness images do not necessarily supply every
guest feature used by these fixtures.

Offline capture, one-object size limits, file-lock semantics, external cluster
metadata, backend compatibility, ownership/xattr limits, automation and retention
remain bounded as described in the operator guide. The competitive matrix records
this as partial operations coverage, with no across-the-board or performance win.
