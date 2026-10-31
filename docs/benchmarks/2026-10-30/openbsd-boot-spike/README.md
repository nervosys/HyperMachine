# OpenBSD 8.0 as a machine on KVM: boots to the installer shell (pf not yet tested)

Approved by the owner as a trial, for SentryWall's `pf` path. An owned `hv2-sandboxd` (WSL2, nested KVM,
the Rust Hypervisor Firmware the Ubuntu machines use), `miniroot80.img` from cdn.openbsd.org
(5,832,704 bytes, sha256 `fe268f37…99a`, matching the mirror's `SHA256` file; the `.sig` was **not**
verified, there is no `signify`). `install80.img` (839 MB) was not used: the download was cut off at
278 MB by the mirror, so its hash did not match and it was never booted.

## Result

**With the queue-size fix below, OpenBSD 8.0 (RAMDISK_CD #79) boots on this stack to its installer
prompt, and `S` gives a root shell.** One run (attempt 10, repeated as attempt 11). Seen on the
serial console: ACPI 6.5, `com0` as console, `pvbus0: KVM`, `virtio0 … "Qumranet Virtio 1.x Storage"`,
`vioblk0`, `sd0: 8MB`, `softraid0`, `root on rd0a`, the installer's `(I)nstall, (U)pgrade,
(A)utoinstall or (S)hell?`. `sysctl` and `dmesg` run in the shell. **`pfctl` is not in the ramdisk
(`sh: pfctl: not found`)**, so whether `pf` works is still unknown.

| attempt | what | result |
|---|---|---|
| 1 | miniroot as published | firmware: `Failed to find EFI partition: HeaderNotFound`. The image has an MBR with an `ef` partition and no GPT. |
| 2, 3 | miniroot wrapped in a GPT (`sgdisk -g`; `make-gpt.sh`) | OpenBSD's EFI loader loads the kernel (`bsd.rd`), prints `entry point at 0x1001000`; the console then shows only a byte ramp (the kernel's console is the EFI framebuffer, which this machine lacks). |
| 4 | root filesystem rebuilt with `makefs` (`make-com0-root.sh`): `bsd.rd` (sha256 `40cddcb7…e45ce`, matches) plus `/etc/boot.conf` = `stty com0 115200`, `set tty com0`; boot sector and disklabel kept | kernel boots on `com0`, finds `vioblk0`/`sd0`, prints `softraid0 … scsibus1` and **stops**. |
| 5, 6 | same, 240 s and 600 s | the same line; exits fell from ~1900/s to ~740/s: stuck, not slow. |
| 7, 8, 9 | a daemon with temporary trace lines (not committed) | The loader's ~37,500 polled reads all completed (kernel read OK). The kernel driver's first request was kicked once and the device answered "nothing available"; no interrupt, no ISR read. The configuration writes showed **OpenBSD never writes `queue_size`** (offset 0x18), where the firmware writes `0x10`. |
| 10, 11 | with the fix | installer prompt, shell. |

## The defect, and the fix (`crates/hv2-core/src/devices/virtio_queue.rs`)

virtio 1.x defines `queue_size` as the device's maximum until the driver writes a smaller one, and a
driver may use it as it is. OpenBSD's does. Our `GuestQueue::is_ready()` required a size the driver had
written, so its queue never became ready and its first request was never served. Linux and the firmware
write the size, which is why they worked. The register read already returned the maximum; readiness and
the ring arithmetic did not. Now `ring_size()` (the driver's size, else the maximum) is used for both.
`size()` still reports the driver's own value (0 until written). Test:
`a_driver_that_never_writes_the_queue_size_gets_the_maximum`; the 179 `virtio` tests in `hv2-core`
pass on Windows, clippy clean.

My first hypothesis (a level/edge interrupt polarity mismatch) was wrong: the traces showed no
interrupt was ever lost, the request was simply never seen.

## Not verified

- `pf` on OpenBSD: the ramdisk has no `pfctl`; it needs a full install to disk (base sets, the GENERIC
  kernel), which needs a network install over the machine's NIC (`vio` driver) or the 839 MB image.
- That the `vio` (virtio-net) and `vioblk` drivers work for real I/O beyond the installer's first reads.
- That Ubuntu and the other virtio devices are unchanged by the fix beyond the unit tests and **one**
  KVM re-run: `tools/check-machine-script-kvm.py` with the fixed daemon booted Ubuntu 24.04.5, answered
  `exec` as root (vsock), took a file bundle, got DHCP and DNS on the virtio NIC, installed `nftables`
  with `apt`, and printed its script's marker; all 5 cases passed. Not run: the full Ubuntu distro,
  Docker and snapshot checks.
- The `hv2-sandboxd` used for attempts 10 and 11 was built in WSL from the working tree with this fix.
- Repeatability: the installer shell was reached twice; no more.
