# A published port reached from another network namespace

`tools/check-docker-sandbox-kvm.py --network --netns` (19 cases), 3 runs in a row on
the WSL2 Debian host (KVM, Hyper-V-nested), commit `d647ea50` plus the `--netns` case.
All 3 runs passed 19 of 19 (`summary.txt`).

The new case starts a container publishing 8082 in the guest, creates a network
namespace `hmout` joined to the host by a veth pair (10.99.0.1 / 10.99.0.2), and sends
`GET /` with `Host: 8082-<sandbox>.localhost` from inside it:

- control, to `127.0.0.1:<proxy>`: `ConnectionRefusedError` (the namespace has its own loopback);
- to `10.99.0.1:<proxy>` (the node's address): `200 hi-publish`.

This shows the node proxy, which binds `0.0.0.0`, answers a client arriving over a
different network path with its own stack.

## Not verified

- A second **machine**, a real NIC, or a firewall between client and node: this is one
  kernel with a veth pair.
- A client on a LAN address other than the veth one; IPv6.
- Authentication on the proxy port (the case sends none).
