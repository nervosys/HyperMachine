# Checkpointing a sandbox together with its disk, on KVM (2026-10-23)

`tools/check-disk-checkpoint-kvm.py` on WSL2's KVM (24 host CPUs), running as
root, a template guest on the 6.6.52 kernel and busybox initrd, a 64 MiB disk
named `data`. All 10 cases pass (`report.json`, which has the request times and
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

9. A sandbox that had been restored (so on a disk made by a restore) was
   checkpointed again and restored from the newer checkpoint and then the
   older one, each with the right contents.
10. A 1 GiB disk holding 400 MiB of random data was checkpointed
   (request 1.18 s) and, after 50 MiB of it was overwritten, restored
   (request 0.82 s) with every byte back, by SHA-256. The files were on WSL's ext4,
   which has no reflinks, so these are full sparse copies: the guest waits
   for the copy during a checkpoint. A one-off measurement on a busy
   development host, not a claim.

NOT VERIFIED: a filesystem with reflinks, a node with `--disk-slot`, the
route through a control plane (disks are node-local and the control plane
does not forward `/disks`), and disks much beyond 1 GiB.
