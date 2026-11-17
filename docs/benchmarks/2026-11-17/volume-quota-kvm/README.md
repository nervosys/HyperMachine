# Volume quotas and the host-disk guard (O7), checked on KVM

The O7 fix in [SECURITY_AUDIT_2026-11-16.md](../../../security/SECURITY_AUDIT_2026-11-16.md), described in
[SANDBOXES.md](../../../SANDBOXES.md) ("Volumes: how much a guest may write"):

- **Quotas:** `--volume-max-bytes` / `--volume-max-inodes` refuse growth in a volume with `EDQUOT`.
- **Reserve guard:** growth that would leave the volume's filesystem under the reserve
  (`--volume-reserve-percent`, default 5%) is refused with `ENOSPC`.

Linux under WSL2 with nested KVM, toolchain 1.100.0, release build of `47527234`, plus uncommitted
control-plane edits for O14 then in progress (template-build scoping in `hv2-cluster`, which the daemon
links but which touch none of the volume code). The daemon's sha256 is `a9ca8fd805a6975947c01f11621a534c02e45cd1e8f4c0a40b2a5b8e277dd0b5`.
Guest: kernel `/var/tmp/hm-docker/bzImage`, initrd `/var/tmp/hm-batch/busybox.cpio.gz`.

[`check-volume-quota-kvm.py`](check-volume-quota-kvm.py) runs three owned `hv2-sandboxd` daemons in turn,
mounts a volume at `/vol` in a sandbox over 9P, and drives the guest with `dd`/`touch`/`rm` through
`POST /sandboxes/{id}/exec`. Results: [report.json](report.json); daemon logs: `daemon-*.log.txt`.

## Results (all 9 cases passed)

| Case | Node | What the guest did | Result |
|---|---|---|---|
| a | quota (8 MiB, 24 inodes) | wrote 1 MiB, read it back; a second sandbox read the same SHA-256 | mount is `9p`; hashes equal; host holds 1 MiB |
| b | quota | `dd` of 16 MiB on top | `dd: error writing '/vol/big': Disk quota exceeded`; host total 8388608 bytes = limit exactly |
| b2 | quota | one more write at the limit | `Disk quota exceeded`, host total unchanged |
| c | quota | `rm` the big file, write 4 MiB | succeeds, host total 5 MiB (deleting freed quota) |
| d | quota | `touch` files in a loop | `touch: /vol/f23: Disk quota exceeded`; host has exactly 24 entries |
| d2 | quota | `mkdir`, `ln -s` at the limit | both `Disk quota exceeded` |
| e | quota | `rm` 5 entries, create 6 | 5 succeed, the 6th is refused; host back at 24 entries |
| f | guard (64 MiB tmpfs, default reserve) | host filler leaves 9 MiB free; guest `dd` of 32 MiB | `No space left on device` after 5.75 MiB; host free 3407872 B >= reserve 3355443 B (5% of 64 MiB) |
| g | guard | `rm` the guest file, write 2 MiB | succeeds |
| h | no guard (`--volume-reserve-percent 0`) | same fill | `No space left on device` after 9 MiB; host free 0 |

Case h is the control: with the guard off the guest takes the tmpfs to 0 free, with it on the host keeps
its reserve. Writes in the guest used `conv=fsync` so the refusal surfaces at `dd`, not at a later flush.

**Platform:** the daemon logs hold 7 `Detected hypervisor platform: Kvm` lines (quota 3, guard 2, noguard
2) and 0 `Tcg`.

## NOT VERIFIED here

- **Race between guests:** two sandboxes writing the same volume at once against the quota was not run.
- **Content API:** the 507 path for uploads (`/volumecontent/...`) was not exercised; only the 9P path was.
- **Inode reserve:** the guard's 1%-of-inodes branch was not reached (tmpfs inodes are not scarce here).
- **Deleting an open file:** the documented late return of quota for a file a guest still holds open was
  not tested.
- **Real disks:** the guard was exercised on tmpfs and the quotas on a plain directory (`/var/tmp`), not
  on ext4 or XFS.
- **Repeats:** one run; no flakiness measured.
- **Timing:** none claimed.
