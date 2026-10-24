# Sharing a snapshot between teams, on KVM (2026-10-23)

`tools/check-snapshot-share-kvm.py` on WSL2's KVM, running as root: an owned
Redis, one `hv2-sandboxd` node (with a snapshot store) and the
`hv2-control-plane` over verified TLS and node mTLS, a red, a blue and a green
key (one team each) and the administrator. All 6 cases pass (`report.json`), and
3 runs in a row pass 6 of 6 (`runs/run1..3.json`). Everything goes through the
control plane's API.

1. Red writes a file in a sandbox and snapshots it. Before any share, blue and
   green cannot list the snapshot (in `/snapshots` or `/templates`) or start from
   it (404).
2. A share with no body, `{}` or `acknowledgeContents: false` is refused (400,
   naming memory, files and environment variables) and shares nothing.
3. Red shares it with blue. Blue lists it, starts a sandbox from it and reads
   red's file in it; that sandbox is blue's and red cannot reach it. Green still
   cannot list or start from it.
4. Blue gets 404 reading the share list, sharing it on to green, revoking its own
   share and deleting the snapshot. Sharing with the owning team is 409, a
   malformed team label 400, and the operator's `base` template 404.
5. After the node is stopped and started again, the share is still there: blue
   starts a sandbox from the snapshot and finds the file.
6. Red revokes the share (204). Blue can no longer list it or start from it
   (404), and the sandbox blue started keeps running. Red can still start from it.

Unit tests cover the record's share list (it round-trips and an older record has
none), who may use a snapshot (`usable_by`), and the control plane's ownership
check; clippy with `-D warnings` is clean on `hv2-cluster` and `hv2-sandboxd`.

A first run found that a `PUT` with an empty body and a JSON content type was
rejected by the extractor before the handler could say why; the node now reads
the body itself, and a missing or malformed one is "not acknowledged".

## Not verified

- **Removing secrets.** Nothing is scrubbed: the other team gets the sandbox's
  memory, files and environment variables as they were. That is why the request
  must acknowledge it.
- Several nodes: the control plane sends the change to each node and a node
  following the store picks up a change within about two seconds, but only one
  node ran here. A node that was down when a share was revoked is not tested.
- A share to a team in another cluster, or to an account outside the cluster
  (no share code, no export).
- Machine snapshots (not shareable) and checkpoints (not templates).
