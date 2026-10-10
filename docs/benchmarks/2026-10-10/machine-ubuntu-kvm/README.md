# Ubuntu 24.04 as a machine: systemd, four processors, power off

**What this checks:** that a machine made from a full distribution's cloud
image is that distribution, running: its own kernel finds the processors it
was given, its own init starts every unit, and it turns itself off. The
[earlier checks](../machine-acpi-kvm/README.md) used CirrOS, which is small
and has no systemd. boat's sandbox is Ubuntu 24.04 on Linux 6.8; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. The one time it records is not a claim, and it
compares with nothing.

## The image

`ubuntu-24.04-minimal-cloudimg-amd64.img` from
`cloud-images.ubuntu.com/minimal/releases/noble/release/`, fetched on
2026-10-10 (sha256 `2f119f50…a777df` as downloaded), converted from QCOW2 to
raw with `qemu-img`. Two things were then changed in its root filesystem,
mounted on the host:

- **A password for `root`**, so that there is an account to use on the serial
  console. A cloud image has none until cloud-init is given one.
- **`/etc/cloud/cloud-init.disabled`**, since this node offers no datasource
  and cloud-init would wait for one.

Nothing else: its kernel (6.8.0-146-generic), its bootloader and its units
are Ubuntu's. The hash in [`report.json`](report.json) is of the raw image
after those two changes. Neither the image nor the firmware, Rust Hypervisor
Firmware 0.5.0, is in this repository.

## Method

`tools/check-machine-distro-kvm.py` runs an owned `hv2-sandboxd` with
`--firmware` and the image, creates one machine with `"cpuCount": 4` and
`"memoryMB": 2048`, and drives it through its serial console. Each answer the
guest gives is carried between two markers its own shell computes, numbered
per question, so it is the guest's answer and not the command as the terminal
echoed it.

1. **Boots** by firmware to `ubuntu login:`.
2. **What it runs on.** The guest must count four processors from `nproc` and
   from `/proc/cpuinfo`; `/proc/1/comm` must be `systemd`; its kernel log must
   name the FACP, APIC and DSDT tables and hold no ACPI error; a virtio device
   must be listed under `IO-APIC` in `/proc/interrupts`.
3. **Finished starting.** `systemctl is-system-running` must settle on
   `running` or `degraded`, with no job left queued; units that failed are
   written into the report.
4. **Power off.** `systemctl poweroff` must leave the machine `stopped`, and
   still `stopped` eight seconds later, with its restart policy at `always`.
5. **Comes back.** Started through the API, a file written before must still
   be there, and the guest must count four processors again.

## Result

All five cases passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-10.

| | |
|---|---|
| The guest says it is | Ubuntu 24.04.5 LTS, kernel 6.8.0-146-generic |
| Processors, by `nproc` and by `/proc/cpuinfo` | 4 and 4 |
| First process | `systemd` (255.4-1ubuntu8.17) |
| ACPI tables named in its kernel log, and ACPI errors | 3 and 0 |
| virtio devices on the I/O APIC | 1 |
| Memory the guest sees | 1,966 MB of the 2,048 given |
| `systemctl is-system-running` | `running`, no job queued, no unit failed |
| After `systemctl poweroff` | `stopped`, and left so; the node logged `Guest turned itself off through ACPI` |
| After start | the file is there, and four processors |
| From create to the login prompt | 39.7 s here, 48.1 s on the run before |

The first run of this check failed in its second case, in the driver: it
took the answer to the first question for the answer to the second. The
machine had booted and been logged in to. The questions are numbered now, and
the run recorded here is the next one.

## What this does not show

- **A network.** The machine has a disk and no NIC, so nothing was installed
  in it: no `apt`, and no Docker, which this image does not ship.
- **cloud-init.** It was switched off. A machine cannot yet be given a user,
  a key or a hostname at creation; that needs a datasource, which is not
  built.
- **A guest agent.** `exec` still answers 409 for a machine from an image;
  the console is the way in.
- **How fast it boots.** The time above is one guest on a busy development
  host, under nested virtualization.
- **Another distribution, or another Ubuntu image.** One image was booted.
- **Another firmware.** Rust Hypervisor Firmware only.
