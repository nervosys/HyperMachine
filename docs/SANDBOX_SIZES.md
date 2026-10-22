# Sandbox sizes

A sandbox is its template's size unless it asks for another: `cpuCount`
and `memoryMB` on a create (`hm sandbox vm create --cpus --memory-mb`),
from 1 vCPU to this host's (at most 32), and 256 MiB to 64 GiB in even
numbers. A sandbox can change size later, running, on resume, or on a
fork; see [changing its size](REBOOT.md#changing-its-size).

## A fast start at any size

A template's snapshot is of a guest its size, and a guest's memory cannot
be restored as another size. So a sandbox of another size boots, which
takes hundreds of milliseconds where a restore takes tens.

The node therefore builds the template again at each size it is asked for:

- **The first sandbox at a size boots.** While it does, the node boots one
  guest of that template at that size in the background and snapshots it.
- **Every later sandbox at that size is restored from that snapshot**,
  whether it is created, resized, forked or resumed at the size. Like any
  restore, each gets its own clock and random seed.
- **`--template-size CPUSxMB`**, which can be given more than once, builds
  the base template at that size when the node starts, so not even the
  first sandbox at that size boots.
- **`--sized-templates N`** (default 4, at most 64) is how many such
  snapshots a node keeps. Once it holds that many, other sizes boot every
  time. `0` turns this off.
- **`--sized-template-disk-gb N`** (default 16) bounds the disk they take
  together. Each takes as much as the guest's memory, in the node's
  temporary directory, so a size whose memory would take those held and
  being built past N GiB is not built, and its sandboxes boot. Four 64 GiB
  sizes would otherwise be 256 GiB. The node does not look at the disk's
  free space itself: set N to what its temporary directory can spare.
- One is built at a time, beside the node's sandboxes and outside its
  `--capacity`.
- A template rebuilt under the same name forgets the sizes built from its
  old guest; they are built again as they are asked for.

### What it does not change

- **Its own size.** A sandbox not its template's size is still a size of
  its own, however it started. Its pause and its
  [checkpoints](CHECKPOINTS.md) write its memory whole, and it cannot be
  made into a template.
- **A sandbox with a disk** boots, as it does at its template's size.
- **The [warm pool](WARM_POOL.md)** holds sandboxes of the base template's
  size only.
- **Other nodes.** The sized snapshots are this node's, not shared through
  a snapshot store.

Evidence: [sizes at creation](benchmarks/2026-10-12/sandbox-sizes-kvm/README.md),
and [restored at another size](benchmarks/2026-10-13/sized-templates-kvm/README.md).
