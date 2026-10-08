# Persistent machines on real KVM

**What this checks:** [machines](../../../MACHINES.md), long-lived VMs that boot from
their own persistent root disk. This is the first item of Phase 1 in the [VMware
replacement plan](../../../VMWARE_REPLACEMENT.md).

## Method

`tools/check-machines-kvm.py` runs an owned `hv2-sandboxd` with a `--machine-dir`
and checks the following through its API:

1. **Boots from its own disk.** A machine created from the base template, with
   a 1 GiB disk, is running when the create answers. Its root filesystem is ext4
   on the disk: `/proc/mounts` shows `/dev/root / ext4`, and the kernel command
   line has `root=/dev/vda` and no `rdinit`.
2. **Survives stop and start.** It writes 1 MiB from `/dev/urandom` to
   `/root/persist`. Stopping it makes exec answer 409. After a start, the file's
   SHA-256 is unchanged.
3. **Survives a guest reboot.** The guest runs `reboot -f`. The daemon sees the
   VM stop and boots the machine again: a marker left in `/tmp` (tmpfs) is gone,
   and `/root/persist` is unchanged.
4. **Survives the daemon being killed.** A second machine is created with
   `autostart: false`. The daemon gets `SIGKILL`, taking every VM with it. A new
   daemon on the same machine directory starts `web-01` by itself, with
   `/root/persist` unchanged, and leaves the second machine stopped.
5. **Delete only when stopped.** Deleting the running machine answers 409. After
   a stop the delete answers 204, the machine is gone, and so is its directory.

## Result: all five pass

The full record is in [`report.json`](report.json). The 1 GiB sparse root image had
38 MB allocated after the base template's tree and the 1 MiB file. The new
daemon's log shows `machine web-01 started with the daemon`. Neither daemon's
log has an error or a panic.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release, branch `feat/persistent-machines`) | `efad6c493d7ffa7b58b2768bd17670c8ae75d9e70a4709fac1d9ea7dc63422c8` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-machines-kvm.py` | `a43aec5e6af4b70378bad9fe3388700df0e7c7f44e0349e0fdddda309a1b7712` |

The host was WSL2 kernel 6.18.33.2 with real KVM and mke2fs 1.47.2.

## Not shown here

- **Power loss.** The daemon was killed, not the host; there was no power-loss or
  host-crash test.
- **Network, firmware boot, several disks, the control plane.** None of these
  exist for machines yet.
- **Start latency.** Not measured.
