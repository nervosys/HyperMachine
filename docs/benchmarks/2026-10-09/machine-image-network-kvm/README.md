# A networked machine from a stock cloud image, on real KVM

**What this checks:** a [machine made from a disk image](../../../MACHINES.md#from-a-disk-image)
with a network. The guest is a stock operating system with no HyperMachine agent,
so it has to find its NIC and configure it by itself. A full distribution as a
machine with a network is part of what boxd offers; see the
[comparison](../../../PLATFORM_PARITY.md).

## Method

`tools/check-machine-image-network-kvm.py` runs an owned `hv2-sandboxd` with
`--firmware`, an `--image-dir` holding the image and a `--machine-dir`. It also
runs an HTTP server on the host's own address, standing in for a service a
machine may reach; that address is private, so the daemon gets
`--tenant-reserved-cidr <address>/32`. The machine is created with `allowOut`
naming that address and `denyOut` naming `192.0.2.0/24`. The check logs in at
the serial console and runs commands there.

1. **Configured by DHCP.** `ip` in the guest reports `eth0` with the address the
   gateway gives, the gateway as default route, and `/etc/resolv.conf` names the
   gateway's resolver.
2. **Reaches what it may.** `curl` in the guest fetches the marker file from the
   service. A `curl` to `192.0.2.7` does not connect. The gateway's decision log
   has an allow for the service and a deny for `192.0.2.7:80`.
3. **Again after a restart.** After a stop and a start the guest logs in,
   reports the same configuration, and fetches the marker again.

The image is CirrOS 0.6.2 as downloaded, converted to raw. Its own kernel finds
the NIC, and its own DHCP client (`dhcpcd`) configures it.

## Result: all three pass

The full record is in [`report.json`](report.json). The daemon's log has no
warning, error or panic.

- The guest reported `10.0.2.15/24`, default route `10.0.2.2` and resolver
  `10.0.2.3`, both times.
- The decision log had `allow <service> (allowOut address)` and
  `deny 192.0.2.7:80 (denyOut address)`.

## What this found on the way

The first run failed: the guest had no address. Its DHCP client had been offered
one, and was probing it by ARP before using it, as RFC 5227 asks. The gateway
answers ARP for every address, since it routes all of them, and so it answered
for the guest's own. The client read that as another host holding the address
and never took it. The gateway now does not answer an ARP request for the
guest's own address. Sandboxes never showed this, because their kernel is given
its address on the command line and does not probe.

## Other checks on the same build

`check-machines-kvm.py` and `check-machine-network-kvm.py` each passed five of
five. `check-machine-firmware-kvm.py` failed once, before its first case, when
run straight after them in one script; run again three times on the same build
it passed five of five each time. I do not know what that one failure was, and
it did not reproduce.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (debug build, branch `feat/machine-image-network`) | `13da67d03bc19f8daceb931b6c27456653aad72bd3cf5a6990b4a47c7311f015` |
| Rust Hypervisor Firmware 0.5.0, `hypervisor-fw` | `4a0a1e977368f6b15d2198a216bdedf9a350bf5e5ae07e29e695373ec16ad958` |
| CirrOS 0.6.2 converted to raw | `800f530322907ababfe948271255107959ea780ac118a1d47109e3adaf484e96` |
| `tools/check-machine-image-network-kvm.py` | `7c6bc2b6f2ccfa07efcb1ac3b87ac4dc397481cfa7f57837a75116c1233c1be4` |

The host was WSL2 kernel 6.18.33.2 with real KVM.

## Not shown here

- **The Internet, DNS names and HTTPS.** The one allowed destination was a
  plain-HTTP server on the host, by address. The resolver was configured in the
  guest but nothing was resolved through it.
- **A DHCP renewal.** The lease is a day; nothing ran that long.
- **Other DHCP clients and other images.** `dhcpcd` on CirrOS only.
- **Throughput.** Not measured.
- **Sandboxes on a networked node.** The ARP change is in every gateway. The
  template-machine network check passed, but the sandbox egress checks were not
  re-run.
