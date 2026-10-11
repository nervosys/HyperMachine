# A machine from a disk image, with ACPI: processors, interrupts, power

**What this checks:** that a guest started by firmware from a disk image, on
a distribution's own kernel, finds what the VMM's
[ACPI tables](../../../FIRMWARE_BOOT.md#acpi-tables) describe: more than one
processor, interrupt routing for its PCI devices, and a way to turn itself
off. Before these tables such a machine had one processor whatever it was
created with, and `poweroff` in it did not stop the machine.

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-firmware-kvm.py` runs an owned `hv2-sandboxd` with
`--firmware` and an image directory, and drives it through `/machines` and
the machine's serial console. The firmware is Rust Hypervisor Firmware 0.5.0
and the image is CirrOS 0.6.2, whose kernel is Ubuntu's 5.15.0-71. Neither is
in this repository; their hashes are in [`report.json`](report.json).

The first four cases are the ones [the earlier run](../../2026-10-08/machine-firmware-kvm/README.md)
recorded, and they run here on a guest that now has the tables: boot to the
login prompt, console input, a file kept across stop and start, and across
the guest's own `reboot`. Two are new:

5. **Two processors.** A second machine is created from the same image with
   `"cpuCount": 2`. Logged in on its console, the guest must report two from
   `nproc` and two `processor` lines in `/proc/cpuinfo`; its kernel log must
   name the FACP, APIC and DSDT tables and hold no ACPI error; and at least
   one virtio device must be listed under `IO-APIC` in `/proc/interrupts`.
   Each answer is carried beside a value the guest's shell computes, so that
   it is the guest's answer and not the command as the terminal echoed it.
6. **Power off.** `sudo poweroff` is typed at that machine. It must become
   `stopped`, and still be `stopped` eight seconds later, with its restart
   policy at the default, which the reboot case shows restarting a guest
   that stops by itself. Started again through the API, it must boot and
   again report two processors.

## Result

All seven cases passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-10.

- The guest counted 2 processors both ways, before and after being turned off
  and started again.
- Its kernel log named the three tables and held 0 ACPI errors.
- One virtio device, the disk, interrupts through the I/O APIC.
- The node's log has `Guest turned itself off through ACPI` and then
  `its guest turned itself off; leaving it off` for the second machine, and
  for the first machine's reboot `Guest pulsed the reset line through the
  i8042` and `restarting it`. So a restart and a power-off take different
  paths and are told apart.

The first run of the new cases failed at its own last step: the driver
deleted the second machine while it was running, and was refused. Everything
before that step had passed. The driver now stops the machine first, and the
run recorded here is the next one.

## What this does not show

- **A full distribution.** CirrOS is small and has no systemd. That is
  [a check of its own](../machine-ubuntu-kvm/README.md), with Ubuntu 24.04.
- **Another firmware.** That the RSDP reaches the guest depends on the
  firmware passing it on. Rust Hypervisor Firmware does. edk2's `CloudHv`
  has not been run.
- **More than two processors by the guest's count.** With the same firmware
  and image, a VM with four booted to the login prompt, but nothing was read
  from inside it.
- **A network device's interrupt.** The machines here have a disk and no NIC.
  A NIC's route is written the same way and was not exercised.
- **Being turned off from outside.** Nothing presses a power button: an API
  stop still stops the VM without asking the guest.
