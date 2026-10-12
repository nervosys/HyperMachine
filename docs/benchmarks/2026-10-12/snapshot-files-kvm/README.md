# Files out of a machine's disk snapshot, without booting it

**What this checks:** listing and reading the files in a machine's
[disk snapshot](../../../MACHINES.md#snapshots). boat lists a snapshot's
files and downloads part of them; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-snapshot-files-kvm.py` runs an owned `hv2-sandboxd` with a
firmware and an image directory holding Ubuntu 24.04's cloud image as
published (`noble-stock.raw`: GPT, root on partition 1, `/boot` on its own
ext4 partition).

1. **A template machine.** Its guest writes a 10,240-byte file named
   `a file "q" $(touch pwned) 'x'`, a short note owned by 1000:1000, a
   directory and a symbolic link. Stopped, it is snapshotted. Listing
   `/srv/data` must give exactly those four with their kinds, the note's size
   and owner, and no `.` or `..`; each file must read back byte for byte, and
   nothing named in the odd name may have been run.
2. **A cloud image, never booted.** A machine is created from the image,
   stopped, and snapshotted. Without a partition named, the root must be
   found on partition 1 and `/usr/lib/os-release` read from it.
   `/etc/os-release`, a symbolic link, must be listed as one and refused when
   read. Some numbered partition must hold the kernels at its root: `/boot`.
3. **Refusals.** A path that is not there; a directory read as a file; a
   file listed as a directory; a path with `..`; a relative path; a partition
   that is not ext (the image's BIOS boot partition, 14); a partition that is
   not there; a snapshot that is not there.

## Result

All three passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-12, with e2fsprogs 1.47.2. From [`report.json`](report.json): the
image's root was partition 1, `/boot` partition 16, and its
`PRETTY_NAME` `Ubuntu 24.04.5 LTS`.

Two of the refusals' words were put right after the first passing run: one
said `no no` for a path that is not there, and every refusal to read a
non-file said symbolic links are not followed, where only a link's should.
The check was run again after.

## What it found before it was written

`debugfs` takes a request as one string and, with this version, a quoted
path in it was not read as the path: `cat "/usr/lib/os-release"` printed
nothing. So no path is put in a request: it is walked from the root a name
at a time, and only inode numbers are given to `debugfs`.

## What this does not show

- **Filesystems that are not ext.** XFS and Btrfs images are refused, and
  none was tried.
- **Large files and directories.** A file is read whole, up to 64 MiB, and
  a directory listed whole.
- **A disk with logical partitions in an extended one.** MBR's four primary
  entries and GPT are read; nothing inside an extended partition.
- **The control plane**, which forwards the routes; and `hm ... snapshot ls`
  and `cat` against a node, which are tested against a fixture.
