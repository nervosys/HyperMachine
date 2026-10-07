# Rebooting a sandbox

A sandbox survives its guest rebooting. If the guest runs `reboot`, its kernel panics, or
it triple-faults, the VM stops, and within about a second the node brings the sandbox back
up. You can also reboot a sandbox on request:

```sh
curl -s -X POST localhost:3980/sandboxes/$SBX/reboot
```

## What a reboot keeps

| Kept | Not kept |
|---|---|
| Sandbox ID, access token, URL (`{port}-{sandboxID}`) and lifetime | Processes, and anything written to the guest's root filesystem (it is RAM) |
| Network policy and secret scope | Open connections: clients reconnect, as after any reboot |
| [Volumes](VOLUMES.md), remounted where they were | |
| A [disk](DISKS.md), still held and remounted where it was | |
| `envVars` given at creation | |

The guest that comes back is the sandbox's template, booted fresh: restored from the
template snapshot, or cold-booted if the sandbox holds a disk. Keep anything that must
survive a reboot on a volume or a disk.

`envVars` are kept in the node's memory to put back after a reboot. They are never
written to disk. So a sandbox resumed on another node, or after its daemon restarted,
reboots without them.

## Crash loops

A sandbox that reboots five times within 60 seconds is ended, with the `sandbox-lost`
lifecycle event (`kill_reason: lost`), instead of being rebooted forever. Requested reboots
count toward the same limit. A reboot is reported as `sandbox-updated`, and counted in
`hv2_node_transitions_total{kind="reboot"}`.

## How a guest's reboot reaches the node

MMIO guests boot on a hardware-reduced ACPI platform. Linux's default reboot method there
is EFI. With no EFI, that path falls through to a jump into a BIOS that is not there, and
the guest hangs. hv2-core therefore boots these guests with `reboot=k`. The guest pulses
the reset line through the i8042 keyboard controller, and the VM stops on that pulse, as
on Firecracker. Sandbox guests also boot with `panic=1`, so a panic resets after a second
rather than hanging.

Evidence: [real KVM reboot, panic, disk and limit checks](benchmarks/2026-10-06/reboot-in-place-kvm/README.md).
