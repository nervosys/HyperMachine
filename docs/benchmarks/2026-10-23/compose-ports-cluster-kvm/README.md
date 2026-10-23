# Compose `ports:` through the control plane and gateway, on KVM (2026-10-23)

`tools/check-compose-ports-cluster-kvm.py` on WSL2's KVM, running as root: an
owned Redis, one `hv2-sandboxd` node (with a guest kit, so it pulls
`alpine:3.20` itself; no Docker anywhere), the `hv2-control-plane` over verified
TLS and node mTLS with `--native-port-range`, and `hv2-native-gateway`. A red key
and a blue key carry a `principal_id`; the control plane's administrator key does
not. The real `hm sandbox vm compose` is the client. All 6 cases pass
(`report.json`), and 3 runs in a row pass 6 of 6 (`runs/run1..3.json`).

1. `up` of a service with `ports: ["9000:8080"]` reserves container port 8080 (the
   host side is dropped; the gateway picked public port 41000, the first of the
   window), prints it, and the control plane's API and `hm public-ports list`
   show the same one reservation: `{machinePort: 8080, publicPort: 41000,
   protocol: tcp}`.
2. A TCP client of that public port on the gateway reaches the service's own
   listener in the guest (`nc -ll -p 8080 -e /bin/cat`, started by compose) and
   gets its bytes echoed back through the gateway, the node's mutual TLS and the
   guest's agent.
3. Another principal's key cannot see that sandbox's reservation.
4. `down` removes the sandbox and its reservation (the API answers 404/410), and
   the public port stops accepting connections.
5. `up` with a key that has no `principal_id` fails, says which port it was
   publishing, and leaves no sandbox behind (`GET /sandboxes` is empty).
6. Nothing `hm` printed contains any of the API keys or the cluster token.

## Found on the way

- **A team-scoped key cannot run `compose` with an `image:` service.** The first
  run used keys with a `team_id`; the control plane answered `403 this resource is
  shared by every team and is not available to team keys` to the template build
  that `compose` (like `run IMAGE`) makes first. Templates are shared by every
  team, so this is the control plane's rule, not a bug in compose. The check's
  keys therefore carry a `principal_id` and no `team_id`. A team's users who need
  a service from an image need an operator to have built its template.
- `POST /templates` on the control plane is sent to every registered node, and
  answers 502 with `[]` when none has registered yet. The script waits for the
  node's `base` template to be listed first.

## Not verified

- UDP and `protocol: both` through compose (compose maps `/udp` to a UDP
  reservation; the check exercises TCP only).
- More than one published port in a file, and the 16-per-sandbox cap through compose.
- Several nodes: one node holds every service here.
- Public Internet reachability; the gateway bound loopback.
- A host port in `ports:` is dropped by design, not honoured, and no case checks a
  published range (`8000-8010:80` is refused, covered by `check-hm-compose-kvm.py`).
- No timing claim: `up` took 6 s here, almost all of it pulling the image and
  building the template.
