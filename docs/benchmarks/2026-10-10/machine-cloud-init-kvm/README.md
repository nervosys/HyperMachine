# Ubuntu's image as published, given a user at creation by cloud-init

**What this checks:** that a machine can be made from a distribution's cloud
image exactly as the distribution publishes it, and be given an account when
it is created. [The check before this](../machine-ubuntu-kvm/README.md) ran
Ubuntu from an image that had a password written into it by hand and
cloud-init switched off, because a machine could be given nothing at
creation. Now `userData` on `POST /machines` reaches the guest's cloud-init.

This is a functional check. The one time it records is not a claim, and it
compares with nothing.

## The image, and what the machine was given

`ubuntu-24.04-minimal-cloudimg-amd64.img`, sha256 `2f119f50…a777df`, fetched
from `cloud-images.ubuntu.com` on 2026-10-10 and converted from QCOW2 to raw
with `qemu-img`. Nothing in it was changed. The hash in
[`report.json`](report.json) is of the raw file.

The machine was created with this as its `userData`:

```yaml
#cloud-config
password: hmcheck
chpasswd:
  expire: false
```

which sets a password for the image's default user, `ubuntu`. That password
is how the check logs in, so every answer below was got through an account
that exists only because the guest read what the machine was given.

## Method

`tools/check-machine-distro-kvm.py --user ubuntu --sudo --user-data FILE`, on
an owned `hv2-sandboxd` with Rust Hypervisor Firmware 0.5.0. One machine,
four processors, 2 GiB, a disk and no network.

The cases are those of the check before, with one added after login:

- **cloud-init read the machine's seed.** `cloud-init status` must reach
  `done`; `hostname` must be the machine's name; cloud-init's recorded
  instance ID must be the machine's ID; `cloud-id` must say `nocloud`; and
  the guest must see a second disk, read-only, labelled `cidata`.

and one more assertion in the last case: after the machine is turned off and
started again, cloud-init still has the same instance ID and one instance
recorded, so it did not take the second boot for a new machine and run its
first-boot steps again.

The check also asserts that the machine's record, as the API returns it,
says cloud-init data was given and does not contain the data.

## Result

All six cases passed, on Linux 6.18.33 under WSL2 with nested KVM, on
2026-10-10.

| | |
|---|---|
| Logged in as | `ubuntu`, with the password from `userData`, at the first try |
| `cloud-init status` | `status: done` |
| `hostname` | `distro-01`, the machine's name |
| cloud-init's instance ID | `vm-5a9e5d395ce76950`, the machine's ID |
| `cloud-id` | `nocloud` |
| Disks the guest sees | `vda` 3.5G and `vdb` 1M; `vdb` labelled `CIDATA`, read-only |
| Processors, first process, ACPI | 4 of 4, `systemd`, 3 tables and 0 errors, as before |
| virtio devices on the I/O APIC | 2: both disks |
| After power off and start | same instance ID, 1 instance recorded, the file written before is there |

Two things in the result are not what one would want, and both come from the
machine having no network:

- **`systemctl is-system-running` is `degraded`**, with one failed unit,
  `systemd-networkd-wait-online.service`. cloud-init writes a network
  configuration for a NIC that is not there, and systemd waits for it.
- **It took 145.5 seconds to reach the login prompt**, against 40 to 48 with
  cloud-init switched off. Most of that is the same wait.

A machine with a NIC has neither:
[the check after this one](../../2026-10-11/machine-docker-kvm/README.md)
ends `running`, 36 seconds to the prompt.

## What this does not show

- **A machine with a network**, as above; so also nothing installed, and no
  Docker.
- **An SSH key.** The user data here sets a password. A key in user data
  takes the same path into the guest, and there is no network to connect
  over yet.
- **Changing the data after creation.** The seed is written once. cloud-init
  would not read it again for the same instance in any case.
- **Other datasources, or other distributions' cloud-init.**
- **The seed read by another implementation of FAT.** Linux's `vfat` driver
  in this guest mounted it and cloud-init read both files; the unit tests
  read it back with a reader written beside the writer. No other tool was
  pointed at it.
