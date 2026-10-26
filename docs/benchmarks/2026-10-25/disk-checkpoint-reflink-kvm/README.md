# Checkpointing a sandbox's disk on a filesystem with reflinks (2026-10-25)

`tools/check-disk-checkpoint-kvm.py --work-dir DIR` on WSL2's KVM, as root, the
same 10 cases as [2026-10-23](../../2026-10-23/disk-checkpoint-kvm/README.md).
The node's disk directory was on a loop-mounted XFS (`mkfs.xfs -m reflink=1`,
`xfs_info` printed `reflink=1`, 8 GiB image file in WSL's ext4) in `xfs/`, and
on WSL's ext4 (no reflinks) in `ext4/` as the control. One run each.

- Both: 10 of 10 cases pass (`report.json`).
- The 1 GiB case (400 MiB of random data) now records `disk_copy_shares_extents`,
  from `filefrag -v` on the node's `*.disk.img` files after the checkpoint:
  **true on XFS, false on ext4**. So `cp --reflink=auto` cloned the disk on XFS
  and fully copied it on ext4.
- Request times for that checkpoint / restore: XFS 0.595 s / 0.45 s, ext4
  1.187 s / 1.31 s. One run each on a loop device inside a file, so it is a
  direction and not a benchmark.

NOT VERIFIED: Btrfs; XFS on a real block device; a repeat run (one run each);
`--disk-slot` nodes; disks much beyond 1 GiB.
