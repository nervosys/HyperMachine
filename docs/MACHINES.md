# Machines

A machine is a long-lived VM, as a vSphere host runs them. It is the other
kind from a sandbox:

| | Sandbox | Machine |
|---|---|---|
| Root filesystem | RAM (an initramfs), gone when it stops | its own ext4 disk, kept |
| Lifetime | capped (24 h) | none |
| A guest reboot or crash | rebooted fresh from its template | booted again from its disk |
| The daemon restarting | the sandbox is lost | the machine starts again by itself (`autostart`) |
| Starts in | milliseconds (snapshot restore) | seconds (a cold boot from disk) |

Machines are served by Linux nodes (`hv2-sandboxd`) with KVM. Each machine lives
in a directory under `--machine-dir`, which must be durable storage on that
node. The default is `<temp>/hv2-sandboxd-machines`. The directory holds
`machine.json` (what the machine is and whether it should be running) and
`root.img`.

## Using one

```sh
# A machine from the base template: 2 vCPUs, 2 GiB, a 20 GiB root disk, running.
curl -s -X POST localhost:3980/machines \
  -d '{"name":"web-01","templateID":"base","cpuCount":2,"memoryMB":2048,"diskGiB":20}'

curl -s -X POST localhost:3980/machines/web-01/exec -d '{"cmd":"df -h /"}'
curl -s -X POST localhost:3980/machines/web-01/stop
curl -s -X POST localhost:3980/machines/web-01/start
curl -s localhost:3980/machines/web-01/console      # the end of its serial console
curl -s -X DELETE localhost:3980/machines/web-01     # stopped machines only
```

| Field on create | Default | |
|---|---|---|
| `name` | required | Letters, digits, `-` and `_`, up to 63 characters. |
| `templateID` | `base` | Any template this node has, including one built from an OCI image. Its file tree becomes the root disk. |
| `cpuCount`, `memoryMB` | the node's defaults | |
| `diskGiB` | 8 | 1–2048. The image is sparse, so it costs only what is written. |
| `autostart` | true | Start it again when the daemon starts, if it was running. |
| `restartPolicy` | `always` | `always` boots it again when its guest reboots or crashes, at most 5 times in 5 minutes; `never` leaves it stopped. |
| `start` | true | Start it once created. |

## How it boots

At creation, the template's initramfs is unpacked into a fresh sparse ext4 image
with `mkfs.ext4 -d`. Device nodes are skipped, since the guest's init mounts
devtmpfs. The guest boots the node's kernel with no initramfs and
`root=/dev/vda rw rootfstype=ext4 init=/init`, from that image attached as a
virtio-blk disk. Every write goes to the image before the guest sees it complete,
and a guest flush is an `fsync`.

Stopping a machine syncs its filesystems through the guest agent, then stops the
VM. There is no ACPI power button yet, so a guest's own `poweroff` halts the guest
without the VM exiting. Stop a machine through the API.

## Not yet

- **Firmware boot (UEFI) and stock cloud images or ISOs.** Machines boot the
  node's kernel today.
- **Networking.** A machine has no NIC yet.
- **Formats.** QCOW2, and several disks per machine.
- **Control plane.** It does not route `/machines` yet; use the node API.
- **Migration.** Moving a machine between nodes, live migration, and HA restart
  on another host.

These are tracked in the [VMware replacement plan](VMWARE_REPLACEMENT.md).

Evidence: [real KVM: boot from disk, stop/start, guest reboot, daemon kill, delete](benchmarks/2026-10-07/machines-kvm/README.md).
