# Snapshots of a machine's disk: taken, restored, downloaded

**What this checks:** [snapshots](../../../MACHINES.md#snapshots) of a
machine's disk. boat snapshots a sandbox's files and lets them be listed and
downloaded; see the [comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-snapshots-kvm.py` runs an owned `hv2-sandboxd` and one
machine from the base template with a 1 GiB disk. The guest writes a note
saying `first version` and 300 KiB of random data, whose digest it keeps.

1. **Running.** A snapshot of the running machine is refused with 409, and
   none is listed.
2. **Taken.** Stopped, a snapshot named `first version` is taken. It must be
   listed, read alone, and hold far less than the disk's gibibyte on the
   node.
3. **Restored.** Started, the guest changes the note to `second version`,
   adds a file and deletes the data; stopped, a second snapshot is taken.
   The first is restored: started again, the note says `first version`, the
   added file is gone and the data has its digest.
4. **And back.** The second is restored: the change and the added file are
   back and the data is gone. The first again: as it was. Each snapshot is
   its own.
5. **Downloaded.** The second snapshot's disk is downloaded. It must be
   1 GiB as its `content-length` says, `e2fsck -f -n` must find it clean,
   and `debugfs` must read `second version` and the added file out of it,
   without booting anything.
6. **Across a resize.** The disk is grown to 2 GiB, and the first snapshot
   restored: the machine's record says 1 GiB again, and the guest's root is
   under 1 GiB.
7. **Refusals and deletes.** A restore onto a running machine, a snapshot
   that is not there, a path that is not a snapshot ID
   (`../../machine.json`), a restore of one that is not there, a blank name,
   a field that is not one. Deleting the second leaves the first listed;
   deleting the machine leaves nothing of it on the node.

## Result

All seven passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-12. From [`report.json`](report.json):

- The first snapshot of the 1 GiB disk held 5,226,496 bytes on the node,
  which keeps its disks on ext4, where a copy is sparse and not shared.
- The downloaded disk was 1,073,741,824 bytes, clean, and `debugfs` read
  `second version` from it.

## What this does not show

- **A filesystem that shares blocks.** On XFS or Btrfs, `cp --reflink=auto`
  shares them; this node's is ext4, so that path was not run.
- **A machine from a disk image**, whose disk has its own partitions. The
  copy and the restore do not look inside a disk, so nothing here depends on
  it being ext4, but only a template machine's was taken.
- **Large disks.** A 1 GiB disk, mostly empty.
- **The control plane**, which forwards the routes; and `hm ... snapshot`
  against a node, which is tested against a fixture, the download included.
- **A restore or a snapshot interrupted part way.**
