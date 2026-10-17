# A machine's reverse port through the control plane, on KVM (2026-10-16)

**What this checks:** `GET /machines/{name}/ports/{hostPort}/reverse` through
the control plane. Until now only a node served it; the control plane had no
route, so a cluster caller got 404
([the node's own check](../../2026-10-13/machine-reverse-port-kvm/README.md)).

## Method

[`tools/check-machine-reverse-cluster-kvm.py`](../../../tools/check-machine-reverse-cluster-kvm.py)
starts an owned Redis, one `hv2-sandboxd` node with a `--machine-dir`, and
the `hv2-control-plane` over verified TLS and node mTLS, with keys for two
teams. Everything below goes through the control plane's API.

1. Red creates a machine `web-01`. The caller opens its reverse port 7000 as
   an upgrade; in the guest `nc 127.0.0.1 7000` sends a line. The caller
   receives `+` and then the line, and its reply reaches `nc`.
2. Refusals: a wrong key (401), a key without the `machines` scope (403),
   port 0 (400), a plain GET that is not an upgrade (400), the machine of
   another team by name and by ID (404), and a machine that is not there
   (404).
3. The machine stopped: the node's 409 is the answer.

The control plane finds the machine as its other `/machines/{name}` routes
do, sends the node the machine's team and the cluster token (never the
caller's own team header), and carries the node's upgraded stream.

## Result: 4 cases, 3 runs in a row, all passed

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| A machine program's first bytes at the caller | 0.051 s | 0.007 s | 0.008 s |

Every refusal answered as listed, in all three runs
([`report.json`](report.json), [`runs/`](runs/)). The control plane has a
unit test as well, against a fake node
(`a_machines_reverse_port_is_carried_to_its_node_as_its_team` in
`crates/hv2-cluster/tests/machine_routing.rs`): the node saw the machine's
team once and the token, a client's own team header was not forwarded, and
the 409 came through.

## Not verified

- **The check failing without the change.** The old control plane was not
  run against it; that it answered 404 is from the earlier README and the
  router's source.
- **More than one node**, **a node that does not answer** (the route answers
  503 as the other machine routes do, covered by neither test here), and
  **an administrator's key** (which reaches a machine by its ID).
- **`hm sandbox vm machine reverse` through a control plane**: the check
  speaks the upgrade directly.
- **Waiting connections after a machine stops.** Their entries stay in the
  node's table until it restarts; nothing here changes that.
- **One host, one kernel**, and a guest agent from this release (an older
  agent cannot listen).
