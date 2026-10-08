# Machine networking on real KVM

**What this checks:** a [machine](../../../MACHINES.md#networking) created with a
`network` has one NIC behind the node's egress gateway, and the gateway applies
the machine's rules at every boot. This is a Phase 1 item of the [VMware
replacement plan](../../../VMWARE_REPLACEMENT.md).

## Method

`tools/check-machine-network-kvm.py` runs an owned `hv2-sandboxd` with `--network`
and a `--machine-dir`. It also runs an HTTP server on the host's own address,
standing in for a service a machine may reach. That address is private, so the
daemon is started with `--tenant-reserved-cidr <address>/32` to let a machine's
`allowOut` name it. No Internet access is needed. Through the API it checks:

1. **Reaches what it may.** A machine created with `allowOut` naming the
   service's address has `eth0` configured, and fetches the marker file from the
   service. A request to the cloud metadata address (`169.254.169.254`) fails.
   The gateway's decision log has an allow for the service and a deny for the
   metadata address.
2. **No network, no NIC.** A machine created without `network` has no `eth0`,
   and its decisions route answers 400.
3. **Refused what it may not.** A machine created with
   `allowInternetAccess: false` and no `allowOut` cannot fetch the marker, and
   the decision log has the deny.
4. **The network returns.** After a stop and start, and again after the guest
   runs `reboot -f`, the first machine fetches the marker again. Its trust
   bundle holds one certificate after those three boots, and `/etc/resolv.conf`
   points at `/proc/net/pnp`.
5. **A bad network is refused.** A create whose `allowOut` has an entry that is
   neither an address nor a usable name answers 400, and no machine is left.

## Result: all five pass

The full record is in [`report.json`](report.json).

- The guest's address was `10.0.2.15/24`, from the kernel command line.
- The first machine's log was `allow <service> (allowOut address)` and
  `deny 169.254.169.254:80 (reserved address)`.
- The third machine's deny was also `reserved address`: the operator's grant
  only applies to an address the machine's own `allowOut` names.
- The daemon's log has no warning, error or panic.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (debug build, branch `feat/machine-network`) | `38d7cc5f58e7a4508340bb8d52f1bc09c8ed679357eb716fabad8886c4fea792` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` (the base template) | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |
| `tools/check-machine-network-kvm.py` | `5649adeacab52edc84c6c5c3a528cd6abb9229ee99529d0db81f3c4e51d9222e` |

The host was WSL2 kernel 6.18.33.2 with real KVM.

## Not shown here

- **The Internet, DNS and HTTPS.** The one allowed destination was a plain-HTTP
  server on the host, by address. Name-based `allowOut`, the gateway's resolver
  and TLS interception were not exercised for a machine.
- **Inbound connections.** A machine has none; nothing was tested.
- **A release build, and throughput.** The daemon was a debug build, and no
  speed was measured.
- **The daemon restarting.** Autostart with a network was not re-run here; the
  boot path is the same one stop/start uses.
