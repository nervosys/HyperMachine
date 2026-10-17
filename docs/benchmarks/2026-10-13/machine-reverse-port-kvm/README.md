# A port of the caller's reached from inside a machine

**What this checks:** `GET /machines/{name}/ports/{port}/reverse`, and
`hm sandbox vm machine reverse`. This is the machine half of the
[reverse port](../../../REVERSE_PORTS.md) that sandboxes already had. boat
has `boat forward --reverse`; see the [comparison](../../../BOAT_PARITY.md).

## Method

`tools/check-machine-reverse-port-kvm.py` runs an owned `hv2-sandboxd` with
`--no-template` and a `--machine-dir`, and creates one 512 MiB machine,
booted from the initrd carrying this release's guest agent.

1. **The API:** the reverse upgrade for guest port 7000 is opened. In the
   guest, `nc 127.0.0.1 7000` sends a line, and the caller receives `+` and
   then that line. Its reply reaches `nc`.
2. **`hm`:** the check serves a random 512 KiB file over HTTP on its own
   loopback. `hm sandbox vm machine reverse --port 7001 --to <that>
   --waiting 4` runs beside it. In the guest, three `wget`s of the file run
   at once through `127.0.0.1:7001`, and each copy's SHA-256 matches.
3. **Caller gone:** `hm` is interrupted and the server stopped. Seven
   seconds later, past the 5 s grace, a program connecting to the guest's
   port is refused.
4. **Refusals:** port 0 (400), a machine that is not there (404), a plain
   GET that is not an upgrade (400).

## Result

All four cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM. From [`report.json`](report.json):

| | |
|---|---|
| A machine program's first bytes at the caller | 0.008 s after the guest command was sent |
| Three 512 KiB fetches at once, through `hm` | 0.039 s, every copy exact |
| A program connecting after the caller left | refused (`nc` exit 1) |

The first run failed on the check's own `hm` argument order, and the next
on rustfmt. The Windows build of `hv2-sandboxd` failed on the missing
machine stub, which was added; `cargo check` of it and `hm` is clean now.

## What this does not show

- **Through the control plane.** At the time, a machine's reverse port was not
  forwarded by the control plane and a cluster caller got 404. It is now:
  see [the cluster check](../../2026-10-16/machine-reverse-cluster-kvm/README.md).
- **A machine stopped while a connection waits.** Its waiting connections
  end with the VM, but their entries stay in the node's table until the
  node restarts.
- **Many connections** beyond the three at once.
- **An old guest agent** was run for a sandbox, which shares the code: it
  is refused with 502 and a message saying to rebuild the image. Not run for
  a machine itself.
