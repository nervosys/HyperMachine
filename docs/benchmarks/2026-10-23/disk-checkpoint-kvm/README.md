# Checkpointing a sandbox together with its disk, on KVM (2026-10-23)

`tools/check-disk-checkpoint-kvm.py` on WSL2's KVM (24 host CPUs), running as
root, a template guest on the 6.6.52 kernel and busybox initrd, a 64 MiB disk
named `data`. All 8 cases pass (`report.json`, which has the request times and
digests).

1. A sandbox holding the disk, running a counter, with a synced file, one
   written just before with no sync, and a file on its RAM root, was
   checkpointed by name (201; it was 409). The node kept one `cp1.disk.img`.
2. It ran on and wrote more to its disk.
3. Restore returned it as it was: the same boot ID, the RAM file, the disk
   mounted from `/dev/vda`, the synced and unsynced files equal by SHA-256,
   none of the later writes, and the counter running from an earlier value.
   It then held a **new** disk, `data-restored-<digits>`; `data` was free.
4. `data` attached to a new sandbox by name still held everything written
   after the checkpoint: restore had not touched it.
5. Restoring the same checkpoint a second time returned the checkpointed
   disk again, not the second life's writes; the first restored disk was let
   go.
6. With the checkpoint's memory file removed, restore failed (500), left the
   sandbox on the disk it had, and left no new disk behind.
7. A template (snapshot) of a sandbox with a disk is still refused (409).
8. Deleting a checkpoint removed its copy of the disk, and so did the sandbox
   ending.

NOT VERIFIED: a disk larger than 64 MiB, a filesystem with reflinks (here a
full sparse copy was taken), a node with `--disk-slot`, a sandbox that was
itself restored from a checkpoint and then checkpointed, the route through a
control plane (disks are node-local and the control plane does not forward
`/disks`), and the cost of the pause for a large disk.
