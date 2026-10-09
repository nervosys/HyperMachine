# A machine booted by firmware from a stock cloud image, on real KVM

**What this checks:** a [machine made from a disk image](../../../MACHINES.md#from-a-disk-image).
It boots by [firmware](../../../FIRMWARE_BOOT.md), is driven over its serial
console because it has no guest agent, and keeps its disk. This is the UEFI item
of Phase 1 in the [VMware replacement plan](../../../VMWARE_REPLACEMENT.md), now
reachable through the machine API.

## Method

`tools/check-machine-firmware-kvm.py` runs an owned `hv2-sandboxd` with
`--firmware`, an `--image-dir` holding the image and a `--machine-dir`.
Everything goes through the node's API:

1. **Boots by firmware.** `POST /machines` with `"image": "cirros.raw"` answers
   with a running machine whose `boot` is `firmware` and which has one vCPU.
   Its console shows the firmware's PVH banner, the EFI partition found and the
   bootloader loaded, and then the image's login prompt.
2. **The console takes input.** The check types the image's documented default
   login at the console, then a command whose output only the guest's shell can
   produce (`echo kept-$((6*7))` written to a file, synced and read back). It
   reads the guest's kernel version the same way. `exec` answers 409 and says
   there is no guest agent.
3. **Survives stop and start.** After a stop (the console route answers 409)
   and a start, the check logs in again and reads the file back.
4. **Survives a guest reboot.** `sudo reboot` in the guest stops the VM. The
   daemon boots the machine again by its restart policy, and after a new login
   the file is still there.
5. **Refusals.** A create with a network, an image that is not in the
   directory, a path in place of a file name, an image together with a template,
   and a zero-size disk each answer 400 and leave no machine.

The image is CirrOS 0.6.2 as downloaded, converted from QCOW2 to raw. Nothing
in it was changed.

## Result: all five pass

The full record is in [`report.json`](report.json).

- The guest's kernel, read over the console, was `5.15.0-71-generic`: the
  image's own, not the node's.
- The file read back `kept-42` after the stop and start and after the reboot.
- The daemon logged `its guest stopped by itself; restarting it` once, and no
  warning, error or panic.

On the same daemon build, `check-machines-kvm.py` and
`check-machine-network-kvm.py` were re-run and each passed five of five, since
this change touches the machine create and boot paths.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (debug build, branch `feat/machine-firmware-boot`) | `c086164cb61e46e0177c4aaddc62bbccc10643c5fdb9855fe77dc48987f34673` |
| Rust Hypervisor Firmware 0.5.0, `hypervisor-fw` | `4a0a1e977368f6b15d2198a216bdedf9a350bf5e5ae07e29e695373ec16ad958` |
| CirrOS 0.6.2 converted to raw, as placed in the image directory | `800f530322907ababfe948271255107959ea780ac118a1d47109e3adaf484e96` |
| `tools/check-machine-firmware-kvm.py` | `a29b06c2c6010dcd6ee2861b8f2adb56aba7cc01eb1387e7f978bdcace227f54` |

The host was WSL2 kernel 6.18.33.2 with real KVM. Neither the firmware nor the
image is in this repository.

## Not shown here

- **A stop without a sync.** The guest ran `sync` before each stop. Stopping an
  image machine does not ask the guest, so unsynced writes can be lost; that was
  not measured.
- **The control plane and `hm`.** The check used the node API. The control plane
  forwards the same routes, and `hm` has a protocol test, but neither was run
  against this guest.
- **Other images, more than one vCPU, a network.** One image; one vCPU; no NIC.
- **Boot time.** Not measured. CirrOS waits about forty seconds for a metadata
  service before its prompt.
