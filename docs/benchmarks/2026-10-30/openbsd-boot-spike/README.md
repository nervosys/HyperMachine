# OpenBSD 8.0 as a machine on KVM: a spike (kernel boots, stalls at the first disk read)

Approved by the owner as a trial. An owned `hv2-sandboxd` (WSL2, nested KVM, the same Rust Hypervisor
Firmware the Ubuntu machines use), `miniroot80.img` from cdn.openbsd.org (5,832,704 bytes, sha256
`fe268f37…99a` matching the mirror's `SHA256` file; the `.sig` was **not** verified, no `signify`).
`install80.img` (839 MB) was not used: the download was cut off at 278 MB by the mirror (hash
mismatch, so it was never booted).

| attempt | image | result |
|---|---|---|
| 1 | miniroot as published | firmware: `Failed to find EFI partition: HeaderNotFound`, `Unable to boot from any virtio-blk device`. The image has an MBR with an `ef` partition and no GPT. |
| 2 | miniroot wrapped in a GPT (`sgdisk -g`, ESP type set; `make-gpt.sh`) | OpenBSD's EFI loader ran, loaded the kernel (`bsd.rd`, ends `]=0xb2e010`) and printed `entry point at 0x1001000`. |
| 3 | same, keystrokes sent to the console during the boot delay (`set tty com0`, `boot`) | the same. |

| 4 | miniroot with its root filesystem rebuilt (`make-com0-root.sh`): `bsd.rd` from the mirror (sha256 `40cddcb7…e45ce`, matches) plus `/etc/boot.conf` = `stty com0 115200` / `set tty com0`; the boot sector and disklabel kept | **The OpenBSD 8.0 kernel boots on the serial console** (RAMDISK_CD #79): ACPI 6.5, the IOAPIC, `com0` as console, `pvbus0: KVM`, `virtio0 … "Qumranet Virtio 1.x Storage"`, `vioblk0`, `sd0: 8MB`. It then prints `softraid0 at root` / `scsibus1 at softraid0: 256 targets` and **nothing more**. |
| 5, 6 | the same, with keys sent when the installer prompt should appear; attempt 6 waited 600 s | no installer prompt, no `root on rd0a` line. Exits fell from ~1900/s (240 s run, 460,510 exits) to ~740/s (600 s run, 445,010): the guest is stuck, not slow. |

Attempt 1-3 (no serial console) printed only a byte ramp: that was the kernel running with its console on
the EFI framebuffer, which this machine has none of.

## Where it stops, and a hypothesis (not tested)

The stall is right after the first thing OpenBSD does with the disk (softraid probes `sd0`). Read from
`crates/hv2-core` by a subagent and not yet checked by an experiment: the virtio-blk device holds its
interrupt as a **level** (`signal_used` asserts GSI 10 and only the ISR read releases it), has no MSI-X,
and the MADT declares GSI 10 **level, active-high** (flags 0x000D); real PCI INTx is active-low. Ubuntu
works with this; OpenBSD may program the pin differently, so the first completion interrupt could be
lost. That is a guess: no interrupt was traced.

## Next steps, if continued

- Trace the interrupt: count `KVM_IRQ_LINE` assertions and ISR reads against the guest's reads of `sd0`.
- Try the MADT flag for the disk's GSI as level/active-low for this one machine (not for Ubuntu) and re-boot.
- If it reaches the installer prompt: `S` for a shell, `pfctl -s info`; then a full install needs the 839 MB
  `install80.img`, whose download from the mirror failed at 278 MB.

## Not verified
- That OpenBSD boots to the installer prompt, a shell or `login:` on this stack (it does not, so far).
- That its virtio-blk and virtio-net drivers (`vioblk`, `vio`) work against these devices.
- That `pf` runs; the point of the trial for SentryWall.
- Everything about `install80.img` (including sets) beyond its partition table.
- Whether another firmware (OVMF, a BIOS boot) would get further; not tried.
