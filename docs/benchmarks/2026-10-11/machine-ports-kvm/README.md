# Published ports: a way in to a machine

**What this checks:** [published ports](../../../MACHINES.md#published-ports)
for machines: a port of the node that carries connections to a port of the
guest. boat gives each port an HTTPS address and has `boat forward`; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-ports-kvm.py` runs an owned `hv2-sandboxd` allowed to
give out three ports, and one machine from the base template with **no
NIC**. The guest serves a directory with BusyBox `httpd` on its port 8080,
holding 5,120,000 bytes of random data whose digest the guest computed.

1. **Out.** Port 8080 is published with the node choosing; it must choose
   the first of its three, on its loopback. The file fetched through it must
   have the guest's digest, and the machine's record must name the port.
2. **In.** A second port, chosen by the caller, to a guest `nc` writing what
   it receives to a file. 3 MiB and 17 bytes of random data sent to the
   node's port must be what the guest has, by its own count and digest.
3. **At once.** Eight fetches of the file at the same time must each be
   whole.
4. **No listener.** A port published to a guest port nothing listens on
   must accept a connection and close it with nothing sent.
5. **Refusals.** A fourth port when three are given out, a port already
   published, one below and one above the node's range, port 22, guest port
   0, a request that names an address, taking back a port that is not
   published, a machine that is not there. The three published must be
   unchanged.
6. **Taken back.** The port must then refuse connections, the others still
   work, and the same port be given out again and work.
7. **Stopped.** With the machine stopped the ports must still be listed as
   listened on, and a connection be accepted and closed. Started again, and
   its server started again, the file must come through without the port
   being published again.
8. **Node restart.** The node is stopped, and nothing may listen on the port
   while it is down. Started again on the same directory, the ports must be
   listed and listened on, the machine come back, and the file come through.
9. **Deleted.** With the machine deleted, both of its ports must be free for
   another listener.

## Result

All nine passed, on Linux 6.18.33 under WSL2 with nested KVM, on 2026-10-11.
[`report.json`](report.json) has the ports the node gave out and the words
of each refusal.

The first run of the check failed in case 9, in the check: it tried each
port with a plain `bind`, which a connection closed a moment before also
holds off. The node had stopped listening. The check now takes the port as
a server does.

## What this does not show

- **Any address but loopback.** The node listened on `127.0.0.1`, its
  default. `--machine-port-address` was not given another, so nothing here
  came from a second host.
- **A machine from a disk image**, with the agent installed by cloud-init.
- **The control plane**, which forwards the three routes; and the `hm`
  commands against a node, which are tested against a fixture.
- **A connection open when the machine stops**, or when its port is taken
  back. One is expected to end with the machine and to go on past an
  unpublish; neither was looked at.
- **Many connections, or long ones.** Eight at once, each a few megabytes.
- **UDP.** Published ports are TCP.
- **How fast.** Nothing here is timed.
