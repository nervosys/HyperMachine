# A port of the caller's reached from inside a sandbox

**What this checks:** `GET /sandboxes/{id}/ports/{port}/reverse` and
`hm sandbox vm reverse`. Before this, ports went only into a sandbox, and
nothing made a port of the caller's reachable from the guest. boat has
`boat forward --reverse`; see the [comparison](../../../BOAT_PARITY.md) and
[how it works](../../../REVERSE_PORTS.md).

## Method

`tools/check-reverse-port-kvm.py` runs an owned `hv2-sandboxd` and one
sandbox, booted from a busybox initrd carrying the guest agent from this
change. The sandbox has no network card: its interfaces are `lo` and the
kernel's `sit0`, neither backed by a device.

1. **The API:** the check opens the reverse upgrade for guest port 7000.
   - In the guest, `nc 127.0.0.1 7000` sends a line.
   - The check receives `+`, then that line.
   - Its reply reaches `nc`'s output.
2. **`hm`:** the check serves a random 512 KiB file over HTTP on its own
   loopback. `hm sandbox vm reverse --port 7001 --to 127.0.0.1:<that>
   --waiting 4` runs beside it. In the guest, three `wget`s of the file run
   at once, through `127.0.0.1:7001`, and each copy's SHA-256 is the file's.
3. **Caller gone:** `hm` is interrupted and the server stopped. Seven
   seconds later, past the guest's 5 s grace, `nc 127.0.0.1 7001` in the
   guest is refused.
4. **Refusals:** port 0 (400), a sandbox that is not there (404), a plain
   GET that is not an upgrade (400).

## Result

All four cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM. From [`report.json`](report.json):

| | |
|---|---|
| A guest program's first bytes at the caller | 0.065 s after the guest command was sent |
| Three 512 KiB fetches at once, through `hm` | 0.274 s, every copy exact |
| A program connecting after the caller left | refused (`nc` exit 1) |

The first run of the check failed its own setup: it expected only `lo` in
the guest, and found `sit0` as well. The check now asserts that no
interface is backed by a device.

## What this does not show

- **A machine.** Only sandboxes have it.
- **The control plane** forwarding it; that was built and is not run here.
- **Many connections.** Three at once, with four waiting.
- **An old guest agent** was run afterwards, with the image from before
  reverse ports. The request is refused with 502, the sandbox's `exec` and
  process list go on working, and the message says to rebuild the image
  (`docs/benchmarks/2026-10-13/old-agent-probe.txt`).
