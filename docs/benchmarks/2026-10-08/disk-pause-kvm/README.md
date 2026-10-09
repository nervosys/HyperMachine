# Pausing a sandbox that holds a disk, on real KVM

**What this checks:** a sandbox with a [block disk](../../../DISKS.md#pausing-a-sandbox-with-a-disk)
can be paused and resumed. Before this it could not: the pause was refused. A
computer with a persistent disk that can also be suspended is the core of what
boxd offers; see the [comparison](../../../PLATFORM_PARITY.md).

## Method

`tools/check-disk-pause-kvm.py` runs an owned `hv2-sandboxd` with a template and
a `--disk-dir`, a 64 MiB disk, and a sandbox with one vCPU and 512 MiB that
mounts the disk at `/data`. A process in the guest increments a counter ten
times a second.

1. **Pauses, and keeps its claim.** The pause answers 204. While the sandbox is
   paused the disk still reports it as its holder, a second sandbox asking for
   the disk gets 409, and deleting the disk gets 409.
2. **The same guest.** After a resume the guest's boot ID is unchanged, a file
   on its RAM root is still there, the counter is still running, a file written
   to the disk before the pause reads back, and `/proc/mounts` still shows
   `/dev/vda` on `/data`.
3. **Unflushed writes.** The guest writes 8 MiB from `/dev/urandom` to the disk
   and takes its SHA-256, with no `sync`. `/proc/meminfo` is read to show the
   data is still dirty in the page cache. The sandbox is paused and resumed, and
   the SHA-256 is taken again.
4. **Again.** A third pause and resume; both files still read back.
5. **Fork is still refused** with 409.
6. **Reached the disk.** After a `sync` the sandbox is ended. A new sandbox
   attaches the disk and reads the same SHA-256 and both files.
7. **Ending a paused one.** A sandbox created with `autoPause` and the disk is
   accepted. It is paused and then deleted; the disk reports no holder, and a
   further sandbox attaches it.

## Result: all seven pass

The full record is in [`report.json`](report.json). The daemon's log has no
warning, error or panic.

- The guest had `Dirty: 8176 kB` when the pause in case 3 began, so the 8 MiB
  file had not been written back. Its SHA-256 was the same after the resume and
  when a different sandbox later read it from the disk.
- The counter read 10 before the first pause and 11 after the resume, then
  moved on.

On the same daemon build, `check-block-disk-kvm.py` (eight checks) and
`check-reboot-kvm.py` (seven checks) were re-run and passed, since this change
touches how every sandbox with a disk is brought up.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (debug build, branch `feat/disk-pause`) | `3d89d1cf996147c3ed7c72a4d708698e1f5cb96500d79cabaf22071f62b3baf4` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-disk-pause-kvm.py` | `da329cdf836607ac4f42e400d491799516221d02aed52e7101a8c531f6aa6078` |

The host was WSL2 kernel 6.18.33.2 with real KVM.

## Not shown here

- **How long the pause and resume take.** Not measured. A sandbox with a disk
  booted, so its whole memory is written, where a sandbox restored from a
  template writes only what it changed. It will be slower and larger.
- **Idle pause and wake on traffic** for a sandbox with a disk. Only pauses and
  resumes asked for by name were run.
- **A network device.** The node ran without `--network`.
- **The daemon restarting** while such a sandbox is paused. It is not expected
  to survive that, and this was not tried.
- **Power loss.** Not tried.
