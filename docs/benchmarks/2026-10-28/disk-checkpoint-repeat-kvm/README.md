# Disk checkpoints, three runs in a row on each filesystem

`tools/check-disk-checkpoint-kvm.py` (10 cases) run three times in a row on each of:
XFS (reflink=1, loop device), Btrfs (loop device), ext4 (control), and a `--disk-slot`
node. WSL2 Debian, KVM, Hyper-V-nested, commit `d647ea50`. Earlier single runs are in
`2026-10-25/disk-checkpoint-reflink-kvm/` and `.../disk-checkpoint-slot-kvm/`.
`summary.txt` has one line per run; each `runN/` holds that run's `report.json` and
daemon log, and `runN.log` its console.

| Variant | Runs passing 10 of 10 |
|---|---|
| XFS | 3 of 3 |
| Btrfs | 3 of 3 |
| ext4 | 3 of 3 |
| `--disk-slot` | **2 of 3** |

On XFS and Btrfs the disk copy shared extents (`disk_copy_shares_extents` true) in all 6 runs.
The console tracebacks have the repo path shown as `<repo>`.

## The failed run (`slot/run1.log`)

Passed 2 cases, then died in "restoring gives the sandbox, and its disk, back as they
were": `int(execute(box, "cat /tmp/count"))` got an empty string, at the second read,
0.6 s after a first read that worked. The traceback is the checker's `ValueError`.
Likely cause, NOT proven: the in-guest counter rewrites `/tmp/count` by truncating it,
and the read landed between the truncate and the write, so this would be a race in the
check, not in the restore. Runs 2 and 3, and the 2026-10-25 single run, did not show it.
Counting all four slot runs: 3 of 4 passed.

## Not verified

- That the empty read is the check's race and not a restore fault (no run was
  repeated enough to see it again; a read that retries would settle it).
- A real block device (all are loop devices) and a disk larger than 1 GiB.

## `--disk-slot` again, with the read retried (`slot-retry/`)

After the failed run, the check's second read of the counter retries up to 10 times
(0.3 s apart) when it gets an empty answer. Three more `--disk-slot` runs: **3 of 3
passed 10 of 10**. So `--disk-slot` is 5 of 7 runs overall across the two sets and 3
of 3 with the retry. That the empty read was a race in the check is consistent with
this and still not proven: a race that happens about 1 run in 4 could also simply not
have fired. The first counter read (just before) does not retry.
