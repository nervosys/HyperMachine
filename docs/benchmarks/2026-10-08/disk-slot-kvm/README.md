# The disk slot on real KVM: a sandbox with a disk, restored from a template

**What this checks:** [`--disk-slot`](../../../DISKS.md#the-disk-slot-restoring-a-sandbox-with-a-disk).
Without it a sandbox that asks for a disk cold-boots, and its pause and fork
write the whole of its memory. With it the sandbox is restored from the
template and given the disk afterwards. A computer with a persistent disk that
is created, suspended and forked quickly is what boxd offers; see the
[comparison](../../../PLATFORM_PARITY.md).

## Method

`tools/check-disk-slot-kvm.py` runs two `hv2-sandboxd` daemons from one binary.
One has `--disk-slot --warm-pool 2`; the other has neither. Sandboxes have one
vCPU and 512 MiB, and disks are 64 MiB. On the daemon with the slot:

1. **Restored, and given the disk.** A first sandbox writes a file to the disk
   and ends. A second sandbox is created with the same disk. Its kernel log has
   the block driver's line for the disk changing size, which only a guest that
   was already running sees; a sandbox with no disk has no such line. The disk
   is mounted at the asked path, reports 64 MiB, and holds the first sandbox's
   file.
2. **The placeholder is private.** A sandbox with no disk has a 1 MiB `/dev/vda`
   that reads as all zeros. It writes to it. Another sandbox with no disk still
   reads all zeros from its own.
3. **Pause and resume.** The sandbox writes 4 MiB with no `sync`, is paused and
   resumed, and reads the same SHA-256.
4. **Fork.** It writes 2 MiB more with no `sync` and forks two ways. Each fork
   reads both files with the source's SHA-256. A fork and the source then write
   different values to one path and each reads its own; the other fork has no
   such file.
5. **From the warm pool.** A create with a disk raises the pool's hand-out
   count by one, and the disk is usable.
6. **Reboot.** `POST /sandboxes/{id}/reboot` brings the sandbox back with the
   disk mounted and both earlier files intact.
7. **Timings.** On each daemon in turn (without, with, with, without), ten
   rounds a block: create with a disk, write 1 MiB, pause, resume, fork one way.
   Each call is timed end to end over HTTP.

## Result: all seven pass

The full record is in [`report.json`](report.json). Neither daemon's log has a
warning, error or panic.

- The restored guest's kernel logged `virtio_blk virtio1: [vda] new size: 131072
  512-byte logical blocks (67.1 MB/64.0 MiB)`.
- The guest had `Dirty: 4088 kB` when the pause in case 3 began.
- Timings in milliseconds, 20 calls each:

  | | Without the slot (min / median / max) | With the slot (min / median / max) |
  |---|---:|---:|
  | Create with a disk | 228 / 262 / 714 | 53 / 61 / 166 |
  | Pause | 155 / 179 / 299 | 5.1 / 5.6 / 60 |
  | Resume | 41 / 53 / 96 | 15.5 / 17.6 / 34 |
  | Fork, one way, with its disk | 204 / 233 / 327 | 24 / 29 / 43 |

On the same binary, `check-block-disk-kvm.py` (8 checks), `check-reboot-kvm.py`
(7), `check-disk-pause-kvm.py` (7) and `check-disk-fork-kvm.py` (7) were run
without the flag and passed, so the default behaviour is unchanged.

## What these numbers are, and are not

- The host is a 24-thread Windows machine running WSL2, where a guest is a
  nested KVM guest. WSL's load average was about 3 during the timings, so the
  host was not quiet. Twenty calls a side is a small sample.
- The create with the slot came from a full warm pool each time. Without the
  pool it would add a template restore, about 19 ms in
  [the pool's own record](../warm-pool-kvm/README.md).
- The fork includes copying the disk. The disk directory was on ext4, which has
  no reflinks, and the disk held about 1 MiB, so the copy was small. A fuller
  disk takes proportionally longer, on both daemons.
- The guest is the base template, a small initramfs.
- **No other product was measured.** boxd publishes "about 160 ms" for a fork
  of memory and disk, with no stated method, guest or disk size. This record
  shows HyperMachine's fork of a sandbox with a small disk at 29 ms on this
  host. It does not show it is faster than boxd's.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release build, branch `feat/disk-template-restore`) | `8a5e9cef801b26c7b3d202ead25ecbce12e9889662b98b11fc36b5b83308c18f` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-disk-slot-kvm.py` | `520881fbf9cf92c40f587f8474d783342a60249f5e16edacb02beef08692f6d9` |

## Not shown here

- **A large or full disk,** and a filesystem with reflinks.
- **What the slot costs sandboxes that do not use it:** the extra device in
  every template restore was not measured against a node without it.
- **A networked node.**
- **A named snapshot taken before the flag was set,** which has no slot.
- **Guests other than the base template.** The swap relies on the guest's
  virtio-blk driver handling a capacity change and on dropping its page cache,
  which was only run on this kernel.
