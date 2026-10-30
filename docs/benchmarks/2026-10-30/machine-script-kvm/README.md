# A destructive script in a throwaway Ubuntu machine (KVM)

`tools/check-machine-script-kvm.py` boots a machine from the Ubuntu 24.04 cloud image with the guest
agent and a NIC behind the node's egress gateway, writes a bundle and binaries in through the
`/files` route, installs packages with `apt`, runs a script as root through `exec`, then stops and
deletes the machine. Owned `hv2-sandboxd`, no Docker.

First use: SentryWall's `integration_real_units.sh` (installs real systemd units, wipes
`/var/lib/sentrywall`, flushes nftables, restarts services to test the start limit). It must not run
on the host. The script's verdict is its own; this check only reports what it printed.

**2 runs.** Run 2 (SentryWall fixed a stale grep in its own script and rebuilt `sentryd` at 8febeed;
sha256 `sentryd` f6d23ea6…, the other two unchanged): all 5 cases passed, script exit 0 in 31.8 s,
**25 passed, 0 failed** (`script-stdout-run2.txt`, `report-run2.json`). Run 1, below, was the
earlier script and binary. All 5 check cases passed (`report-run1.json`). The script exited 0 in 31.9 s and printed
**24 passed, 1 failed** (`script-stdout-run1.txt`). The failure, `A-66 retention note missing`, is a
string match on an API response; it is SentryWall's to judge. The machine: Ubuntu 24.04.5, kernel
6.8.0-147-generic, systemd as PID 1, 2 vCPUs; `exec` answered 24 s after create as root.
Packages from the distribution archive: nftables 1.0.9, curl 8.5.0.

## Not verified

- Repeatability: two runs, on a changed script and binary; the same input was not run twice.
- The `hv2-sandboxd` used was built on 2026-10-28, before the commit this was recorded at.
- OpenBSD / pf: not attempted.
- Whether the A-66 failure is the product or the binary build (a44bd2d); not investigated here.
