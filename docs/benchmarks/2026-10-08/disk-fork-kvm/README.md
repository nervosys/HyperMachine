# Forking a sandbox with its disk, on real KVM

**What this checks:** [forking a sandbox that holds a block disk](../../../DISKS.md#forking-a-sandbox-with-a-disk).
Each fork gets the source's memory and a copy of its disk. boxd describes its
fork as copying a running machine "disk and all"; see the
[comparison](../../../PLATFORM_PARITY.md).

## Method

`tools/check-disk-fork-kvm.py` runs an owned `hv2-sandboxd` with a template and a
`--disk-dir`, a 64 MiB disk, and a source sandbox with one vCPU and 512 MiB that
mounts the disk at `/data`. The source holds a 4 MiB random file that was
synced, a file on its RAM root, and a process incrementing a counter.

1. **Forks, each with its own disk.** Just before the fork the source writes
   another 4 MiB random file with no `sync`. `POST /sandboxes/{id}/fork` with
   `count: 2` answers 201 with two sandboxes. The disk list then shows the
   source's disk held by the source and two disks named `data-fork-…`, one held
   by each fork.
2. **Each fork is the source as it was.** Each has the source's boot ID, the
   RAM-root file, a running counter, `/dev/vda` on `/data`, and both files with
   the SHA-256 the source computed, including the one that was never synced.
3. **The source ran on.** Same boot ID, counter still advancing, the unsynced
   file intact.
4. **Independent afterwards.** The source and each fork write a different value
   to the same file on their disk, and one fork writes a file of its own. Each
   reads back its own value, and the other two do not have the extra file. The
   synced file is still whole on all three.
5. **A fork pauses** to disk and resumes, with its own value still on its disk.
6. **A fork's disk outlives it.** The fork is ended. Its disk reports no
   holder. A new sandbox attaches it by name at a different path and reads the
   fork's value, the fork's own file, and the unsynced file's SHA-256.
7. **Too many is refused.** A fork with `count: 9` answers 400, the disk list is
   unchanged, and the source still runs.

## Result: all seven pass

The full record is in [`report.json`](report.json). The daemon's log has no
warning, error or panic.

- The source had `Dirty: 4088 kB` when the fork began, so the second file was
  still in its page cache. Both forks read it with the same SHA-256, and so did
  a later sandbox reading the fork's disk directly.
- The two-way fork request took 1.2 s. The disk directory was on ext4, which
  has no reflinks, so that includes writing the source's whole memory once and
  copying the disk's allocated blocks twice, all while the source was paused.

On the same daemon build, `check-block-disk-kvm.py` (8 checks),
`check-reboot-kvm.py` (7) and `check-disk-pause-kvm.py` (7) were re-run and
passed. The first and last were changed in this commit: both used to assert that
forking a sandbox with a disk is refused.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (debug build, branch `feat/disk-fork`) | `488a0366b9d559db6d49435a73cc0441695c124fb6a2cf4e13473df7597cce55` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-disk-fork-kvm.py` | `d5846ba0ba1e628336b77cd5ae8bf96928ff0a155577ea9bbf0906553215d2de` |

The host was WSL2 kernel 6.18.33.2 with real KVM.

## What the timing is, and is not

- 1.2 s is one fork request, on a debug build, for a 64 MiB disk with about
  8 MiB written, a 512 MiB guest, and a filesystem with no reflinks. It is not
  a benchmark.
- It is far slower than boxd's published "about 160 ms" for a fork with disk,
  and slower than HyperMachine's own fork of a sandbox without a disk (127 ms
  median in an earlier measurement). A sandbox with a disk writes its whole
  memory, where a template sandbox writes only what changed.
- **No other product was measured.**

## Not shown here

- **A filesystem with reflinks,** where the disk copy should take no time. Not
  tried; the claim in the docs that it does is from how `cp --reflink` works,
  not from a run.
- **A large or full disk,** and how long the source is then paused.
- **More than two forks,** up to the limit of eight.
- **A networked node.**
- **Forking a fork.**
