# Machine webhooks through a control plane, on KVM (2026-10-23)

`tools/check-machine-webhooks-kvm.py` on WSL2's KVM, running as root: an owned
Redis, one `hv2-sandboxd` node (`--machine-dir`, `--allow-private-webhooks`) and
the `hv2-control-plane` over verified TLS and node mTLS, a red key and a blue key
(each its own team), and an HTTP receiver of the script's own on loopback. All 5
cases pass (`report.json`), and 3 runs in a row pass 5 of 5 (`runs/run1..3.json`).

1. Red registers a webhook for the four machine types with a secret, then creates
   a machine, stops it, starts it, stops it and deletes it. The receiver gets
   `machine.lifecycle.created`, `started`, `stopped`, `started`, `stopped`,
   `deleted`, in that order, each with `machine_name` and no `sandbox_id`. Every
   `hv2-signature` verifies as HMAC-SHA256 over `<timestamp>.<body>`, and so does
   the E2B-style `e2b-signature`.
2. Blue's webhook for the same types, and red's webhook with no list of types,
   heard none of it. Red's delivery records show the six events, each delivered
   once and successfully.
3. Blue's machine of the same name reaches blue's webhook (created, started) and
   not red's.
4. Red's webhook with no list of types still receives a sandbox's created and
   killed events, and `GET /events/sandboxes` lists no machine event.
5. A webhook naming `machine.other` or `volume.lifecycle.created` is refused (400).

Unit tests in `hv2-cluster` (`events::tests`, 11 pass) cover the payload and who
hears a machine event; clippy with `-D warnings` is clean on `hv2-cluster` and
`hv2-sandboxd`, and Windows `cargo check` of both is clean.

## Not verified

- A machine whose guest stops or crashes by itself, or one restarted by the node
  after the daemon restarted, sends no event: only changes made through the API do.
- No event for a snapshot taken, restored or deleted.
- Receivers other than this script's; no retry case (a failing receiver) for a
  machine event, though the delivery path is the sandbox events' own.
- More than one node; the event is sent by the node the machine is on.
- Events are not listed anywhere but at the webhook (its delivery records);
  there is no machine event list.
