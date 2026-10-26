# Checkpointing a sandbox's disk on a `--disk-slot` node (2026-10-25)

`tools/check-disk-checkpoint-kvm.py --daemon-arg=--disk-slot` on WSL2's KVM, as
root, the template guest on the 6.6.52 kernel and busybox initrd. All 10 cases
pass, the same as [2026-10-23](../../2026-10-23/disk-checkpoint-kvm/README.md)
without the flag. One run.

The flag took effect: `daemon.log` (colour codes stripped) has the template VM
attaching a 1 MiB block device, the slot's placeholder disk; the same check
without the flag (the ext4 control in
[disk-checkpoint-reflink-kvm](../disk-checkpoint-reflink-kvm/README.md)) has no
such line. The daemon's argument list is not in the report, so that log line
is the evidence.

NOT VERIFIED: a repeat run; creating from a template (rather than a checkpoint)
with a disk on this node, which `check-disk-slot-kvm.py` covers separately;
disks much beyond 1 GiB.
