# A machine's disk snapshot, archived on KVM (2026-10-22)

`tools/check-machine-snapshot-archive-kvm.py` on WSL2's KVM (24 host CPUs),
running as root, a template machine on the 6.6.52 kernel and busybox
initrd. All 4 cases pass (`report.json` has the digests).

1. A machine whose guest wrote `/srv/data` (a file with an odd name holding
   quotes and `$(touch pwned)`, a file owned by 1000:1000, a subdirectory
   with a file, a symbolic link) was stopped and snapshotted.
   `archive?path=/srv/data` is a 660-byte gzipped tar of exactly
   those five entries, bytes equal, the link a link to `note`, the owner
   1000:1000, nothing run for the odd name.
2. `path=/` is a 1.87 MB archive of 293 entries holding the tree.
3. Refused with a reason: a file (400), no such path (404), `..` (400), no
   such snapshot (404).
4. No scratch directory is left in the daemon's `TMPDIR`.

Unit tests (`snapshot_files::tests`, Linux) on an ext4 image from
`mkfs.ext4 -d`: the tree below a directory and not above it, links kept,
the whole filesystem, and refusals for too much to dump, too much archive, a
file, a missing path and `..`, with no scratch left.

NOT VERIFIED: a partitioned disk or Ubuntu's cloud image, a filesystem not
cleanly unmounted, an archive over 64 MiB through the route, a node that is
not root, and the route through a control plane.
