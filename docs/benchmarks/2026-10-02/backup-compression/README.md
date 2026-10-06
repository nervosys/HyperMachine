# Backup compression size and capture tradeoff

The operator can now select gzip levels 1–9 with `--compression-level`.
Level 1 remains the default. Receipts record the selected level; encryption,
multipart checksums and restore format remain unchanged.

Two AB/BA cohorts each capture the same offline, recovered KVM store four
times per level, verifying every decompressed file hash against the source
catalog. The store contains four files and 1,077,309,947 logical bytes, including
layered VM memory. Sixteen captures pass, with unchanged source catalogs and
tools. Timing covers scan, dependency validation, gzip tar capture and final
rescan. Verification reads occur afterward and are excluded from capture timing.

| Cohort | Level | Median capture seconds | Encrypted object bytes |
|---|---:|---:|---:|
| Initial | 1 | 4.379 | 36,364,250 |
| Initial | 6 | 6.375 | 31,512,382 |
| Repeat | 1 | 3.821 | 36,364,250 |
| Repeat | 6 | 6.091 | 31,512,382 |

Level 6 produces 13.3% fewer bytes in both cohorts. In the repeat, capture is
59.4% slower by median, and slower in four of four pairs. The initial cohort
overlapped the separately owned KVM recovery check during its later pairs;
it remains recorded rather than replaced. The repeat runs after that fixture
stopped. Warm filesystem caches and uncontrolled shared WSL host work limit
generalization. No end-to-end upload/restore timing, managed competitor ranking,
or generally faster backup follows from these measurements. Operators with
bandwidth or storage constraints can choose the smaller representation.

A separate real KVM/Moto S3 cohort at level 6 passes all 15 recovery and
refusal checks. Its four-part 31,739,145-byte encrypted object restores paused
memory/process state, filesystem/boot identity, mounted 9P volume data and
identity, and a named snapshot into a new root while the old path is unavailable.
All owned processes exit cleanly and inventories are empty. Moto does not
establish AWS durability or IAM enforcement. The accepted daemon and guest
inputs match the preceding [multipart evidence](../multipart-backup/README.md);
the provisional workspace boot edits are excluded.

The archive freezes the tools, reports and S3 operation log; no raw memory,
store data, key, credentials or executable is included. Verify with:

```sh
python -O tools/verify-backup-compression.py docs/benchmarks/2026-10-02/backup-compression
```

Reproduce the paired comparison on an offline store with a new private directory:

```sh
python -O tools/bench-backup-compression.py --store /owned/offline-store \
  --pairs 4 --output /private/new-compression-run
```

The pinned backup dependencies and `moto[s3,server]==5.2.3` support the separate
KVM fixture; add `--compression-level 6 --multipart` to its recorded invocation.
CLI checks also refused levels 0 and 10 before reading the store or using S3.
The unchanged default compression path also passes all nine multipart
round-trip and failure-injection groups, with clean emulator shutdown.
