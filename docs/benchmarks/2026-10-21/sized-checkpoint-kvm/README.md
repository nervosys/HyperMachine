# A sandbox of its own size, checkpointed on KVM (2026-10-21)

`tools/check-sized-checkpoint-kvm.py` on WSL2's KVM (24 host CPUs), a node
whose template is 1 vCPU and 256 MiB, the 6.6.52 kernel and the busybox
initrd with the guest agent. All 5 cases pass; `report.json` has the digests
of the daemon, kernel, initrd and driver.

1. A sandbox created at 2 vCPUs and 1 GiB is checkpointed (0.71 s, memory
   written whole), changed, and restored (0.16 s): 2 vCPUs, 994508 kB seen
   in the guest, the record `(2, 1024)`, and the file as it was.
2. A sandbox at the template's size is checkpointed, resized to 2 vCPUs and
   1 GiB, and restored: 1 vCPU, 221732 kB, the record `(1, 256)`.
3. The sized checkpoint, after the sandbox is resized to 1 vCPU and 512 MiB:
   restored at 2 vCPUs and 1 GiB again.
4. A file of the sized checkpoint is read through the files API (0.13 s).

5. A sandbox at 2 vCPUs and 1 GiB is snapshotted into a template; a
   sandbox started from it has 2 vCPUs, the 1 GiB, the record `(2, 1024)` and
   the file written before the snapshot; asking for 1 vCPU and 256 MiB on
   that create is a 400.

The times are the check's own stopwatch, not a benchmark.

NOT VERIFIED: a sandbox restored from a sized template and then
checkpointed; sizes above 1 GiB; GNU userland (the initrd is busybox).
