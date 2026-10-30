# OpenBSD 8.0 as a machine on KVM: a spike (does not boot to a prompt)

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

After the entry point the console shows only a 0x01–0xff byte ramp and then nothing. The vCPU keeps
running (about 1.37 million exits in 120 s, no emulation error in the daemon log), so the kernel is
alive but silent, or stuck. **No kernel message, no `boot>` prompt and no installer has been seen.**

## Why it is stopped here

- The loader's own banner and `boot>` prompt never reached the serial console, so the keys sent to it
  were probably not read: the firmware's EFI console input may not be the serial port.
- OpenBSD wants `set tty com0` (a line in `/etc/boot.conf` on its FFS partition) to use a serial
  console. Writing that needs an FFS write from Linux; not tried.
- The byte ramp has no known source yet (kernel, loader or the UART model).

## Not verified

- That OpenBSD boots to `login:` or the installer on this stack at all.
- That its virtio-blk and virtio-net drivers (`vioblk`, `vio`) work against these devices.
- That `pf` runs; the point of the trial for SentryWall.
- Everything about `install80.img` (including sets) beyond its partition table.
- Whether another firmware (OVMF, a BIOS boot) would get further; not tried.
