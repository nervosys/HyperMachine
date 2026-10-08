# Firmware boot on real KVM: a stock cloud image to its login prompt

**What this checks:** [firmware boot](../../../FIRMWARE_BOOT.md) in the VMM. A
firmware image is entered by the PVH boot protocol, finds a disk on the PCI bus
and boots the operating system on it. This is the UEFI item of Phase 1 in the
[VMware replacement plan](../../../VMWARE_REPLACEMENT.md).

## Method

`tools/check-firmware-boot-kvm.py` runs `hv2-core`'s `pvh_firmware_probe` example
and reads the guest's serial console. The guest has one vCPU, 512 MiB, the
legacy PC devices and, in the second run, one virtio-blk disk over PCI.

1. **Entered by PVH.** With no disk, the firmware prints `Booting with PVH Boot
   Protocol` and then reports it has no virtio-blk device to boot from.
2. **Finds the disk and the bootloader.** With the image attached, the firmware
   reports a `1af4:1042` device on the PCI bus and a capacity that matches the
   image file's size. It finds the EFI system partition and loads
   `\EFI\BOOT\BOOTX64.EFI`.
3. **Reaches a login prompt.** After the bootloader is loaded, the console shows
   the guest's login prompt, with no firmware or kernel panic before it.

The image is CirrOS 0.6.2 as downloaded, converted from QCOW2 to raw with
`qemu-img convert`. Nothing in it was changed: its own GRUB, its own Ubuntu
5.15 kernel and its own kernel command line. The run was on a copy, attached
read-write.

## Result: all three pass

The full record is in [`report.json`](report.json), and the console of the second
run is in [`image.console.txt`](image.console.txt). It ends:

```
login as 'cirros' user. default password: 'gocubsgo'. use 'sudo' for root.
cirros login:
```

The image's kernel command line has no serial console, so the console shows
nothing between GRUB and the login prompt. In a separate run that is not in this
record, a copy of the image with `console=ttyS0` added to its GRUB entry showed
the kernel log: `efi: EFI v2.80 by RHF`, the root filesystem mounted from
`/dev/vda1`, and init starting.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `pvh_firmware_probe` (debug build, branch `feat/pvh-firmware-boot`) | `e8c0b5bc858c7b7f74c88918ee8a7185a84ffebedf82e4c22f11cd2c78ab273b` |
| Rust Hypervisor Firmware 0.5.0, `hypervisor-fw` | `4a0a1e977368f6b15d2198a216bdedf9a350bf5e5ae07e29e695373ec16ad958` |
| CirrOS 0.6.2 `x86_64-disk.img`, as downloaded (QCOW2) | `07e44a73e54c94d988028515403c1ed762055e01b83a767edf3c2b387f78ce00` |
| The same image converted to raw, as attached | `800f530322907ababfe948271255107959ea780ac118a1d47109e3adaf484e96` |
| `tools/check-firmware-boot-kvm.py` | `3e333382e066420d05b347595f2e1f4903cc2fd107bf2ff4ab206bbfb86f1769` |

The host was WSL2 kernel 6.18.33.2 with real KVM. Neither the firmware nor the
image is in this repository.

## Not shown here

- **Logging in, or anything after the prompt.** Nothing was typed at the guest.
- **Other images and other firmware.** One image and one firmware were run.
- **ACPI, more than one vCPU, a NIC.** The guest had none of these.
- **Machines.** This is the VMM's example program, not a machine created through
  the API.
- **Boot time.** Not measured. CirrOS waits about forty seconds for a metadata
  service before its prompt, which says nothing about the VMM.
