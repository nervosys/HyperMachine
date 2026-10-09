# Standby

A sandbox in standby has its vCPUs stopped and everything else kept: its memory
stays resident, its devices stay attached and its connections stay open. The
next thing sent to it resumes it, so nothing has to resume it by name.

It sits between running and [paused](PLATFORM_PARITY.md):

| | Running | Standby | Paused |
|---|---|---|---|
| Guest CPU | used | none: its vCPUs take no exits | none |
| Guest memory | on the node | on the node | on disk; the node's memory is free |
| Its slot on the node | held | held | released |
| Comes back | — | on the next request, in well under a millisecond on the node | on resume or, with `autoResume`, on a request; a restore from disk |
| Survives a node restart | no | no | yes, and any node resumes it |

## Using it

```sh
curl -s -X POST localhost:3980/sandboxes/$ID/standby     # into standby
curl -s localhost:3980/sandboxes/$ID/standby             # {"standby":true,"wakes":0,...}
curl -s -X POST localhost:3980/sandboxes/$ID/exec -d '{"cmd":"true"}'   # wakes it
hm sandbox vm standby $ID
```

- **What wakes it.** Anything the node sends the guest: a command, a file
  operation, a connection to one of its ports through the proxy, a TCP or UDP
  tunnel. The wake happens where the node hands the guest a packet, so every
  route gets it and none needs to know.
- **What does not.** Listing or inspecting the sandbox, the standby status
  route, and the node's own metric sampling, which skips a sandbox in standby.
  Its metrics therefore have a gap for the time it was stopped.
- **By itself.** `--idle-standby-after SECS` puts a sandbox in standby once it
  has been idle that long, with the same meaning of idle as an
  [idle pause](PLATFORM_PARITY.md): no request in flight or begun, and a quiet
  guest CPU in every sample of the window. At least 30 seconds.
- **With a pause.** A sandbox in standby can still be paused to disk and resumed
  from there, running. Forking, snapshotting or checkpointing one has not been
  tested. It keeps counting towards its lifetime, and is ended at its timeout
  like any other.

`GET /sandboxes/{id}/standby` reports `standby`, how many times traffic has
woken the sandbox (`wakes`), how long the last wake took on the node
(`lastWakeMicros`: finding the guest stopped to its vCPUs being told to run),
and `vcpuExits`, which does not move while the guest is stopped.

## What to know

- **The guest's clock does not stop.** When the guest runs again it finds time
  has passed, as after any long preemption. Timers that came due fire at once.
- **Nothing inside the guest wakes it.** A guest in standby does not run, so a
  cron job or a timer in it does not fire until something outside wakes it.
- **Its outbound connections are not serviced** while it is stopped, so a peer
  may time them out.
- **Memory is not given back.** Use a pause for that.
- **Sandboxes only.** [Machines](MACHINES.md) do not have standby yet.

Evidence: [real KVM: a stopped guest, woken by a command and by a port request, with timings](benchmarks/2026-10-08/standby-kvm/README.md).
