# Firmware boot

A VM can start from firmware instead of from a kernel the host supplies. The
firmware finds a disk, reads its EFI system partition and starts the bootloader
there, as a physical machine does. That is what lets a stock cloud image boot
with its own bootloader and kernel, unchanged.

This is in the VMM (`hv2-core`), on KVM, and a [machine](MACHINES.md#from-a-disk-image)
created from a disk image uses it.

## How it works

- **PVH entry.** `BootSource::Pvh { firmware }` loads an ELF firmware image and
  enters it by the PVH boot protocol: 32-bit protected mode, at the address the
  image's PVH note names, with `EBX` pointing at a `hvm_start_info` structure
  that carries the memory map. This is how Rust Hypervisor Firmware and edk2's
  `CloudHv` build expect to be started. There is no fw_cfg device, no flash and
  no real-mode reset vector.
- **A disk on the PCI bus.** `VM::attach_block_pci` puts the file-backed
  virtio-blk device behind a PCI function. Firmware and a stock kernel find it
  by enumerating bus 0; nothing on a command line names it.

```sh
cargo run -p hv2-core --example pvh_firmware_probe -- hypervisor-fw   # no disk
HV2_DISK=cirros.raw HV2_UNTIL=login: HV2_SETTLE_SECS=180 \
    cargo run -p hv2-core --example pvh_firmware_probe -- hypervisor-fw
```

The firmware is not shipped in this repository. The check below used Rust
Hypervisor Firmware 0.5.0 (`hypervisor-fw`, Apache-2.0), from its release page.

## ACPI tables

A kernel started by firmware is a distribution's, and takes no command line
from here. What it knows of the machine it reads from ACPI tables. The VMM
writes a set at `0xE0000`, in the part of low memory the map it gives the
firmware already keeps out of RAM, and puts the RSDP's address in the
`hvm_start_info` it hands the firmware. The firmware passes that address to
the system it starts as an EFI configuration table.

The tables (`acpi_tables::build_pc`) describe:

- **Each processor**, and the I/O APIC. This is how the guest finds more than
  one: with no tables it found one, however many the VM had.
- **The PCI host bridge**, the window its devices' BARs are in, and which I/O
  APIC input each device's interrupt pin raises. A kernel that has moved from
  the legacy PIC to the I/O APIC has no interrupt for a PCI device without it.
- **Fixed power hardware**: PM1 event and control registers at ports `0x600`
  and `0x604`, and `\_S5`. The guest turns itself off by writing the control
  register, which the VMM sees as it sees the keyboard controller's reset
  pulse. The tables are deliberately not the hardware-reduced kind this
  project gives its own kernel: a guest booted through EFI on a
  hardware-reduced platform turns off and restarts by asking the firmware,
  and what a firmware then does is not the VMM's to know.

A guest that turns itself off stops the VM and is marked as having done so
(`VM::powered_off`), which is what lets a caller tell it from a guest that
restarted.

[Checked on real KVM](benchmarks/2026-10-10/machine-acpi-kvm/README.md) with
Rust Hypervisor Firmware and CirrOS: a machine asked for two processors has
two by its kernel's count, the kernel reports the three tables and no ACPI
error, its disk interrupts through the I/O APIC, and `poweroff` in the guest
stops the machine. [With Ubuntu 24.04](benchmarks/2026-10-10/machine-ubuntu-kvm/README.md),
whose kernel is 6.8: four processors, systemd as the first process with every
unit started, and `systemctl poweroff` stopping the machine.

## A user at creation: the cloud-init seed

A cloud image has no account and no name until cloud-init gives it them.
`userData` on `POST /machines` (`hm sandbox vm machine create --user-data
FILE`) is a `#cloud-config` document or a script for it. The node writes a
1 MiB FAT volume labelled `cidata` holding that as `user-data`, and a
`meta-data` naming the machine's ID as the instance and its name as the
hostname, and gives it to the machine as a second disk, read-only, after the
root disk on the PCI bus. cloud-init's NoCloud datasource finds it by the
label.

The volume is written by the node itself (`hv2-sandboxd`'s `seed` module),
not by `mkfs.vfat` or `genisoimage`, so creating a machine needs no tool
installed. The instance ID is the machine's own for its life, so cloud-init
does its first-boot steps once and not at each start.

The user data is not kept in the machine's record, which says only that
some was given: it commonly holds a password or a key, and the record is
returned to whoever asks about the machine. It is in `seed.img` in the
machine's directory on the node, readable there by whoever can read the
root disk.

[Checked on real KVM](benchmarks/2026-10-10/machine-cloud-init-kvm/README.md)
with Ubuntu 24.04's image as published and user data that sets a password.
[Given a network as well](benchmarks/2026-10-11/machine-docker-kvm/README.md),
that machine installs Docker with `apt` and runs containers.

## Not yet

- **Importing images.** A machine is made from a raw image an operator has put
  in the node's `--image-dir`. There is no upload API and no format conversion.
- **A power button.** The guest has ACPI tables now (see below), and turns
  itself off through them. Nothing outside presses a button: stopping a
  machine from the API still stops the VM without asking the guest.
- **Other firmware.** Only Rust Hypervisor Firmware has been run. edk2's
  `CloudHv` build, which ISO installers and Windows need, has not.
- **Other images.** CirrOS 0.6.2 and
  [Ubuntu 24.04](benchmarks/2026-10-10/machine-ubuntu-kvm/README.md) have been
  booted, the second to a fully started systemd on four processors, and
  [as published](benchmarks/2026-10-10/machine-cloud-init-kvm/README.md) with
  its account given by cloud-init. Debian and others have not been tried.
- **More than a NIC and a disk on the PCI bus.** A [machine made from an image](MACHINES.md#from-a-disk-image)
  can have a network device there, configured by DHCP. There is no second disk
  or NIC.
- **Other hosts.** WHPX and HVF refuse a PVH boot source.
- **QCOW2.** The disk must be a raw image.

Evidence:
- [real KVM: a stock CirrOS cloud image from firmware to its login prompt](benchmarks/2026-10-08/firmware-boot-kvm/README.md)
- [real KVM: such a machine with a network, its stock guest configured by DHCP](benchmarks/2026-10-09/machine-image-network-kvm/README.md)
- [real KVM: a machine made from that image, logged into and restarted through the API](benchmarks/2026-10-08/machine-firmware-kvm/README.md)
