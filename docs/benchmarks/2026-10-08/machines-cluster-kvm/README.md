# Machines through the control plane, on real KVM with two teams

**What this checks:** the control plane serves
[`/machines`](../../../MACHINES.md#through-the-control-plane): it places a machine
on a node, finds it again, gates it by API key scope, and keeps each
[team](../../../TEAMS.md)'s machines to that team. This is a Phase 1 item of the
[VMware replacement plan](../../../VMWARE_REPLACEMENT.md).

## Method

`tools/check-machines-cluster-kvm.py` runs an owned Redis, one `hv2-sandboxd` node
with a `--machine-dir`, and `hv2-control-plane`, over verified TLS and node mTLS.
The API key policy has two red keys, one blue key, a red key without the
`machines` scope, and the legacy administrator. Every request goes through the
control plane:

1. **Placed and shared within the team.** Red's first key creates `web-01`. The
   reply says it is running and belongs to `red`. Red's second key lists it with
   the node that holds it, and runs a command in it. Its kernel command line has
   `root=/dev/vda`.
2. **Isolated between teams.** Blue's list is empty. By name and by ID, blue
   gets 404 on get, delete, exec, stop, console and network decisions. Blue then
   creates its own `web-01`, which has a different ID. A file red writes is not
   in blue's machine. The administrator lists both and reads red's file by the
   machine's ID; the administrator's `web-01` (no team) does not exist.
3. **Refused.** The key without the scope gets 403. A second `web-01` in red
   gets 409. A create that names a node that does not exist gets 503.
4. **Keeps its disk.** After a stop (exec answers 409) and a start by red's
   second key, red's file is still there.
5. **Deleted, and the other remains.** Deleting the running machine answers
   409. After a stop, the delete answers 204 and red's list is empty. Blue's
   `web-01` is still listed and still runs commands.

## Result: all five pass

The full record is in [`report.json`](report.json). Neither the node's log nor
the control plane's has a warning, error or panic.

In the same session, on the same daemon build, the two node-level checks were
re-run because this change touches every machine handler: `check-machines-kvm.py`
and `check-machine-network-kvm.py` each passed five of five.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (debug build, branch `feat/machines-control-plane`) | `3a4bc4fde357ac33c17dee3fed2cc084d3cf6c50b5c8695e00dffcadb3ab4d26` |
| `hv2-control-plane` (debug build, same branch) | `ec01fa56e7a3111217c6c5a0e601ef82b4350575f5f09baacd4747d5c231caa7` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-machines-cluster-kvm.py` | `7ac564e346d9e4e805805a46f32323d5b0769d27e5fe6168650455b57b77390f` |

The host was WSL2 kernel 6.18.33.2 with real KVM.

## Not shown here

- **More than one real node.** There was one node, so placement had one choice,
  and the 503 for a node that is not answering was not exercised here. Both are
  covered since by `crates/hv2-cluster/tests/machine_routing.rs`, against
  stand-in nodes that boot nothing.
- **A control plane restart.** Finding machines by asking the nodes should
  survive one; it was not tried.
- **Single sign-on sessions.** Only API keys were used.
- **A release build, and latency.** Both binaries were debug builds, and nothing
  was timed. Each request asks every node for its machines first.
