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

## Changing its size

A sandbox's processors and memory can be changed after it is created, by
rebooting it at the new size:

```sh
curl -s -X POST localhost:3980/sandboxes/$SBX/resize \
  -H 'content-type: application/json' -d '{"cpuCount": 4, "memoryMB": 4096}'
# → {"sandboxID":"sbx-…","cpuCount":4,"memoryMB":4096,"resized":true}

# A paused one, brought back at another size.
curl -s -X POST localhost:3980/sandboxes/$SBX/resume \
  -H 'content-type: application/json' -d '{"cpuCount": 2, "memoryMB": 2048}'

hm sandbox vm resize $SBX --cpus 4 --memory-mb 4096
hm sandbox vm resume $SBX --cpus 2 --memory-mb 2048
```

- **It is a reboot.** A guest's memory cannot be moved to a different
  number of processors or a different amount of memory. So a resize keeps
  what a reboot keeps (the table above) and loses what is in the guest's
  memory. A resume at another size does the same to a paused sandbox, which
  would otherwise come back with its memory.
- **Either or both.** Whichever of `cpuCount` and `memoryMB` is not given
  stays as it was. A size the sandbox already has changes nothing, and
  nothing is rebooted.
- **From then on it is that size.** Its later reboots, pauses, resumes and
  [usage](USAGE.md) are at it. Usage counts each size for the time the
  sandbox ran at it.
- **At its template's size it is restored from the template**, quickly, as
  any reboot is. At another size it boots, as a sandbox created at that size
  does. It can then be paused and forked, but not checkpointed or made a
  template.
- **Refused:** a size no guest here can be, which is 1 vCPU up to this
  host's, and 256 MiB to 64 GiB in even numbers. Also refused: a resize of
  a paused sandbox (resume it at a size instead). A resume at a size that
  cannot be leaves the sandbox paused.
- Reported as `sandbox-resized`, which webhooks send as
  `sandbox.lifecycle.updated`, and counted among reboots toward the crash
  limit below.

Not here: a size changed on a fork, and a disk's size.

Evidence: [real KVM resize checks](benchmarks/2026-10-13/sandbox-resize-kvm/README.md).

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
