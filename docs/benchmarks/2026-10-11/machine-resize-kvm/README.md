# Resizing: a stopped machine given more, and its guest finding it

**What this checks:** [resizing](../../../MACHINES.md#resizing) a machine:
`PATCH /machines/{name}` with `cpuCount`, `memoryMB` or `diskGiB`. boat
changes a sandbox's size on resume and on fork; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-resize-kvm.py` runs an owned `hv2-sandboxd` and one
machine from the base template, created with one processor, 512 MiB and a
1 GiB disk. What the guest has is asked of the guest: the processors in
`/proc/cpuinfo`, `MemTotal`, and the size `df` gives for `/`. 700 KiB of
random data is written to the disk first, and its digest kept.

1. **Running.** A resize of the running machine must be refused with 409,
   and its record and the length of its disk on the host left as they were.
2. **Stopped.** Two processors, 1024 MiB and 3 GiB in one request. The
   answer and the record must say so, and the disk on the host be 3 GiB.
3. **Started.** The guest must count two processors, report between 900 and
   1024 MiB, and have a root filesystem of more than 2.8 GiB. The random
   data must have the digest it had. 1500 MiB must then be writable, which
   is more than the disk held before.
4. **One alone.** Memory changed to 768 MiB by itself: the processors and
   the disk must be as they were, in the record and in the guest. The size
   a disk already has, asked for again, is accepted and changes nothing.
5. **Refusals.** A smaller disk, alone and beside sizes that could be; no
   processors; 64 MiB; a disk of 4096 GiB; nothing; a field that is not a
   size; a machine that is not there. The record and the disk must be
   unchanged, and the machine start with its data.

## Result

All five passed on the first run, on Linux 6.18.33 under WSL2 with nested
KVM, on 2026-10-11. From [`report.json`](report.json), as the guest saw it:

| | Processors | `MemTotal` | Root filesystem |
|---|---|---|---|
| Created: 1, 512 MiB, 1 GiB | 1 | 479,396 kB | 996,780 kB |
| Resized: 2, 1024 MiB, 3 GiB | 2 | 994,508 kB | 3,060,520 kB |
| Memory alone to 768 MiB | 2 | 736,460 kB | 3,060,520 kB |

## What this does not show

- **A machine from a disk image.** For one, the node lengthens the disk and
  leaves the partitions to the guest. That path is unit-tested for the
  file's length and was not booted: whether a given image grows its root
  when it starts is the image's, and was not checked for any.
- **A filesystem that fails its check.** The node then refuses to grow it;
  that is unit-tested with a file that is not a filesystem, and not with a
  damaged one.
- **A resize interrupted part way**, or a node that stops during one.
- **A node without room.** Nothing here asks whether the node has the
  processors, the memory or the disk it is giving. The node in this check
  was started for one processor and 512 MiB and gave twice that.
- **The control plane**, which forwards the route; and
  `hm sandbox vm machine resize` against a node, which is tested against a
  fixture.
- **A sandbox.** This is for machines.
