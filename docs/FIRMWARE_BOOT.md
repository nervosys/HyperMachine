# Firmware boot

A VM can start from firmware instead of from a kernel the host supplies. The
firmware finds a disk, reads its EFI system partition and starts the bootloader
there, as a physical machine does. That is what lets a stock cloud image boot
with its own bootloader and kernel, unchanged.

This is in the VMM (`hv2-core`) today, on KVM. It is not yet wired into
[machines](MACHINES.md), which still boot the node's kernel.

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

## Not yet

- **Machines.** No machine can ask for firmware boot. That needs an image import
  path, and a machine that is ready without the guest agent a stock image does
  not have.
- **ACPI.** The firmware is given no RSDP, so the guest has no ACPI tables: one
  vCPU, no power button, and interrupts through the legacy PIC.
- **Other firmware.** Only Rust Hypervisor Firmware has been run. edk2's
  `CloudHv` build, which ISO installers and Windows need, has not.
- **Other images.** Only CirrOS 0.6.2 has been booted. Ubuntu, Debian and others
  have not been tried.
- **A network device on the PCI bus.** The guest in the check had no NIC.
- **Other hosts.** WHPX and HVF refuse a PVH boot source.
- **QCOW2.** The disk must be a raw image.

Evidence: [real KVM: a stock CirrOS cloud image from firmware to its login prompt](benchmarks/2026-10-08/firmware-boot-kvm/README.md).
