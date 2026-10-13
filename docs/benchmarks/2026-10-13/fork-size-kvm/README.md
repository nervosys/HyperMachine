# Forks of a sandbox at another size

**What this checks:** `POST /sandboxes/{id}/fork` with `cpuCount` and
`memoryMB`. Before this a fork was always its source's size. boat changes
a sandbox's size on a fork; see the [comparison](../../../BOAT_PARITY.md)
and [how it works](../../../REBOOT.md#on-a-fork).

The times below are the check's own stopwatch around one request. They
show which forks boot and which restore; they are not a benchmark.

## Method

`tools/check-sandbox-fork-size-kvm.py` runs an owned `hv2-sandboxd`. Its
template is one vCPU and 256 MiB, and it has a disk directory. The check
asks each guest what it has, through `/proc/cpuinfo` and `MemTotal`.

1. **Bigger:** a sandbox with an env var and a file in `/tmp` is forked
   twice at 2 vCPUs and 512 MiB.
   - Each fork sees 2 processors and about 512 MiB, and its record says so.
   - The variable is in each; the file in `/tmp`, which is memory, is not.
   - The source is still 1 vCPU, and its file is still there.
2. **No size:** a fork of the same source is 1 vCPU and has the file, as
   forks did before.
3. **At the template's size, from a source that is not:** a fork of a
   2-vCPU fork at 1 vCPU and 256 MiB.
   - It has the variable and not the source's memory.
   - Paused and resumed, a file in its `/tmp` survives. It was restored from
     the template, so it pauses as such a sandbox does.
4. **Disk:** a sandbox with a disk, a file written to it and synced, is
   forked at 2 vCPUs.
   - The fork has the file.
   - What the fork then writes, the source does not see.
5. **Usage:** a 2-vCPU fork's vCPU-seconds are twice its running seconds,
   to within 5%.
6. **Refusals:** 0 processors, more than the host has, 128 MiB, an odd
   amount. No sandbox is added.

## Result

All six cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM, on a 24-thread host. From [`report.json`](report.json):

| | Guest sees | Took |
|---|---|---|
| Two forks at 2 vCPUs, 512 MiB (booted, together) | 2 processors, 479,180 kB each | 0.955 s |
| A fork with no size (restored from the checkpoint) | the source's memory | 0.045 s |
| A fork at 1 vCPU, 256 MiB (restored from the template) | | 0.049 s |
| A fork of a sandbox with a disk, at 2 vCPUs (booted) | its own copy of the disk | 0.741 s |

Usage for a 2-vCPU fork: 4.365 running seconds, 8.730 vCPU-seconds.

## What this does not show

- **A large size.** Two vCPUs and half a gibibyte.
- **Unflushed writes.** The source synced its disk first. A fork of
  another size has the disk as after a power cut, which is documented and
  not checked here.
- **How long the source is paused** for a disk copy. It was not measured.
- **A cluster**, where the fork goes to the node that runs the sandbox.
- **Speed.** One sample each.
