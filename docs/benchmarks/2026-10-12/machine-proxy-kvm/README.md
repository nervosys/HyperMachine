# A machine's port by name, through the node's TLS proxy

**What this checks:** a machine's [published port](../../../MACHINES.md#published-ports)
with `proxy`, reached at `{guestPort}-{machineID}.{domain}` through the
node's sandbox proxy. boat gives each port an HTTPS address; see the
[comparison](../../../BOAT_PARITY.md).

This is a functional check. It records no timings and compares with nothing.

## Method

`tools/check-machine-proxy-kvm.py` makes a self-signed certificate for
`*.hm.test` and runs an owned `hv2-sandboxd` with it, so its proxy speaks
TLS. One machine from the base template serves a directory with BusyBox
`httpd` on its port 8080. Every request through the proxy is HTTPS to
`127.0.0.1`, with the name given as SNI and as `Host`, and the certificate
checked against it.

1. **By name.** Port 8080 published with `proxy`: the answer must give
   `8080-{machineID}` as its label, and `https://8080-{machineID}.hm.test/small.txt`
   must answer with the file. So must a request whose name routes nowhere
   and which carries `e2b-sandbox-id` and `e2b-sandbox-port`.
2. **Only what was asked.** Port 9000 published without `proxy`; port 22 not
   published; a machine ID that is not one; the machine's name in place of
   its ID. None may be answered through the proxy.
3. **Stopped.** With the machine stopped the name answers nothing; started
   again, with its server started again, it answers.
4. **Taken back.** With the port taken back, the name answers nothing.

## Result

All four passed on the first run, on Linux 6.18.33 under WSL2 with nested
KVM, on 2026-10-12. From [`report.json`](report.json): what was not routed
was answered 404 by the proxy, and the stopped machine's port 502.

## What this does not show

- **A certificate from a real authority**, or a domain in DNS. The name
  reached `127.0.0.1` because the check connected there.
- **A login in front of it.** None is offered for a machine's port.
- **A machine from a disk image.**
- **The control plane**, and a proxy on another node than the machine.
- **The node's API over TLS.** Only the proxy took the certificate; the API
  answered plain HTTP.
