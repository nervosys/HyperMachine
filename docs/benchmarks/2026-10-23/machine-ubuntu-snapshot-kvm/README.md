# A partitioned Ubuntu machine's snapshot, and /dev/kvm inside it, on KVM (2026-10-23)

`tools/check-machine-snapshot-ubuntu-kvm.py` on WSL2's KVM (an AMD host with
nested virtualization), running as root, with Rust Hypervisor Firmware 0.5.0
and `ubuntu-24.04-minimal-cloudimg-amd64.img` as published (sha256
`b3296484e362ba2e713489a5de6225c719d9500962bde25d15f2f21e512c7658`),
converted to raw. The machine is created with cloud-init user data, left to
finish its first boot, stopped and snapshotted. All 6 cases pass
(`report.json`).

The disk is GPT with four partitions (`p1` ext4 `cloudimg-rootfs`, `p14`
BIOS boot, `p15` vfat `UEFI`, `p16` ext4 `BOOT`).

1. `files?path=/etc` with no partition named lists the **root** filesystem
   (135 entries, `os-release` among them): the largest ext partition is picked.
2. `archive?path=/etc/cloud` is a gzipped tar whose 57 regular files are
   **byte for byte the files on the disk**, which the script mounts read-only
   through a loop device as its own oracle: none missing, none extra.
3. `&partition=1` gives the same archive.
4. Partition 14 (BIOS boot, no filesystem) and partition 99 (absent) are each
   refused with 400. The vfat and the small ext4 BOOT partitions were not
   tried.
5. The snapshot's `file` route reads back `hm-kvm-probe.txt`, which the guest
   wrote on its first boot (see below).

## `/dev/kvm` in the guest

The probe file in this directory is what the Ubuntu guest saw. The machine's
CPU has the `svm` flag, Ubuntu's own 6.8 kernel loads `kvm_amd` ("Nested
Virtualization enabled", "Nested Paging enabled"), and `/dev/kvm` exists. A
program in the guest then opened it, `KVM_GET_API_VERSION` returned 12,
`KVM_CREATE_VM` and `KVM_CREATE_VCPU` succeeded, and it ran a real-mode
program of its own (`mov al,0x42; out 0x10,al; hlt`) in its own virtual
machine: the first `KVM_RUN` exited with an I/O of byte 0x42 to port 0x10,
and the second with HLT. So a guest of a machine can run a guest.

This is the machine from a disk image, which runs the distribution's own
kernel. A **sandbox** boots the template kernel (6.6.52 here), which is built
without KVM: it has the `svm` flag and no `/dev/kvm`. Nothing masks it.

NOT VERIFIED: an Intel host (`kvm_intel` found "VMX not supported by CPU" on
this AMD one, as it should), a nested guest larger than the toy one (Linux,
say), anything about its speed, a node that is not root, and an archive over
64 MiB. The password in the user data is the check's own.
