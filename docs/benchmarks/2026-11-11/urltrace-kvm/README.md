# URL tracing, one hop per microVM, on KVM

`tools/check-urltrace-kvm.py` drives `tools/urltrace.py` (the host driver, described in
`docs/URL_TRACING.md`) against one `hv2-sandboxd --network --egress-default deny` node. Each hop
is a fresh microVM whose only job is `hv2-urltrace URL`: one request, one JSON report, no
redirect followed. Its egress is pinned to that hop's host name. After the hop the driver reads
the node's own record of what the sandbox tried to do on the network
(`GET /sandboxes/{id}/network/decisions`), then deletes the sandbox. Three runs, `run1` to `run3`,
each pass every case. An earlier set (`first-set/run1` to `run3`) had one failure, explained below.

## Setup

- WSL2 Debian, kernel `6.18.33.2-microsoft-standard-WSL2`, 24 CPUs, `/dev/kvm` from `kvm_amd`,
  KVM under Hyper-V (nested). Every daemon log says `Detected hypervisor platform: Kvm` (17 times
  per run: 16 sandboxes and the node's template; no `Tcg`).
- Repo commit `6bcf8536` (HEAD) plus uncommitted work: `tools/urltrace.py`,
  `tools/check-urltrace-kvm.py`, `tools/guest-image/init-urltrace`, `crates/hv2-urltrace/`.
- Daemon: Linux release build `hv2-sandboxd`, sha256
  `049b0357ff0080e5dd18783ab5e1bbdea410e06092fcf4123f55c169bd708406` (the same binary as the
  standalone-network runs). Flags: `--network --egress-default deny --memory-mb 512 --cpu-cores 1
  --capacity 4`.
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Guest image `urltrace.cpio.gz` (2.8 MB), sha256
  `cfcd6041bb1611781f585989da8beb7a2a9013d8cc0e816f396eab894d89c10f`, built with
  `tools/guest-image/build.sh --init tools/guest-image/init-urltrace --extra hv2-urltrace`.
  Tracer `hv2-urltrace` 1.2.0 (static musl), sha256
  `2ad9bb1538874f4ceb8deac8dff6cf1c7628578686d96bad24d4daafc3906eee`. The full file list is
  `image-files.txt`.
- Each `report.json` carries the sha256 of the daemon, kernel, image, tracer, the driver and the
  check. The Python on the host is 3.13.5.

## What is in the image, and what is not

`bin/busybox` (the host's static build, 2 MB), `bin/sh` (a symlink to it), `bin/bash`
(`tools/guest-image/bash-shim`, a three-line `sh` script that `build.sh` always installs),
`bin/hv2-guest-agentd`, `bin/hv2-urltrace`, `init`, and empty directories. **There is a shell.**
Two reasons, both stated plainly: `init` (a busybox script) needs busybox to mount `/proc` and
set up `resolv.conf`, and the node's exec API runs every command as `/bin/sh -c ...`
(`crates/hv2-sandboxd/src/main.rs`, `exec`), so the agent cannot start the tracer without a
`sh`. What `init-urltrace` removes is the console shell, the other ~400 busybox applets (it links
five: `mount mkdir ln ip sleep`), the `PATH` entries, and `/usr`. The tracer is at `/bin/hv2-urltrace`
because `build.sh` installs extras into `/bin`. A compromised tracer can still run
`/bin/busybox sh`; the control that matters is the node's egress policy, not the absence of a
shell. Removing busybox needs a static init program and an exec path that does not use `sh`.

## Result

| Case | run1 | run2 | run3 |
|---|---|---|---|
| 1 `https://example.com/`: one hop, 200, verified, ended `final`; decisions name only example.com (lookup and connection allowed) | pass | pass | pass |
| 2 `http://github.com/`: 301 to `https://github.com/`, second hop in a different sandbox, 200; each hop's decisions name only its host | pass | pass | pass |
| 3 `https://expired.badssl.com/`: verified false with a reason, response still fetched (200) | pass | pass | pass |
| 4 pinned to example.com, tracer asked for `https://www.wikipedia.org/`: guest fails to resolve, node records the refused lookups, nothing allowed, driver flags it | pass | pass | pass |
| 5a `--resolve` to another public site's address (198.35.26.224, www.wikipedia.org), https and http: refused and recorded | pass | pass | pass |
| 5b `--resolve 169.254.169.254` (port 80 and 443): refused, recorded as `reserved address` | pass | pass | pass |
| 5c `--resolve 10.0.2.2` (gateway) and `10.0.2.3` (its DNS address): refused and recorded | pass | pass | pass |
| 5d `--resolve` to this host's own address (172.28.97.192): refused, `reserved address` | pass | pass | pass |
| 6 no hop's sandbox exists afterwards (GET answers 404 for all 16, the list is empty, the driver confirmed each deletion) | pass | pass | pass |
| 7 with no allow rule, example.com cannot be fetched (no network object; and every address denied) | pass | pass | pass |
| 8 `http://example.com/?q=$(id);echo$IFS$HOME&x=*&y=%27` reaches the tracer byte for byte | pass | pass | pass |
| 9 the driver's URL validation rejects 26 hostile inputs and accepts 3 good ones | pass | pass | pass |
| 10 characterisation: `allowOut` alone does not pin (see below) | pass | pass | pass |

What the node recorded, for example case 1 (`run1/traces/case1.json`): two `dns` allows for
`example.com` (two lookups, presumably A and AAAA; the log does not say), one `tcp` allow to `example.com` port 443 with
reason `allowOut name, by this gateway's DNS answer`, and nothing else. Case 5a: the guest
completes the TCP handshake, sends its TLS ClientHello with SNI `example.com`, and the gateway
refuses it (`tcp deny ... the name does not resolve to the address connected to`); the guest sees
`Connection reset by peer`. Case 5b to 5d: refused at the SYN, the guest sees `Connection refused`.
Case 4 and 7: the lookup is answered `REFUSED`; the guest's resolver reports `Try again`.

## Finding: `allowOut` alone is not a pin

The first version of the driver sent `{"network": {"allowOut": ["example.com"]}}` and the node
allowed a connection to github.com recorded as reason `default allow`, not `allowOut name`. It
was the node's decisions record that showed it, not the guest's report. Cause, from the code:
`NetworkPolicy::from_e2b` (`crates/hv2-net/src/network_policy.rs`) starts from the operator's
`--egress-default` only when no `allowOut`, `denyOut` or `allow_internet_access` is present; with
any of them present the default becomes **allow** (E2B's semantics). So with the node run with
`--egress-default deny`, a sandbox given only an allow list can reach everything. The driver now
sends `denyOut: ["0.0.0.0/0", "::/0"]` as well, and the name rule overrides the address deny; the
driver also flags any allowed connection whose reason is not `allowOut name...`. Case 10
characterises the node's behaviour (a sandbox with `allowOut: ["example.com"]` fetched
`www.wikipedia.org`, 200, reason `default allow`, all three runs). This is documented in the
daemon's source but not obvious from `--egress-default deny`; it is not a bug in the check or the
tracer, and the daemon was not changed. If the node ever makes an allow list alone a pin, case 10
fails and the driver's extra `denyOut` becomes redundant, not wrong.

## The failure in the first set (`first-set/run2`, case 2)

```
AssertionError: ended error exec
```

Hop 2 (`https://github.com/`) ran 28 s and was killed at the program's time limit (`exec_s`
28.03, `timed_out: true`, no report). The node's record for that hop is normal: two lookups and
one connection to github.com port 443, allowed. The other two runs of the first set passed, with
hop 2 taking 9.9 s and 14.1 s. The report's own stage timings in the passing runs show resolve,
connect, TLS handshake and first byte at about 0.1 s each, and the rest of the time spent reading
the 262144-byte body cap. Running the same tracer natively in WSL, with no VM and no gateway,
six times: total 7.2, 7.8, 8.5, 11.6, 11.9 and 15.0 s. So github.com serves that page slowly to
this host, and the 30 s hop limit was too tight for the 256 KiB read. It is a property of the
target and the network, not a defect of the node, the tracer or the check. The check now reads at
most 64 KiB per hop and allows 60 s (`--max-body`, `--hop-timeout`); `run1` to `run3` use that.
The first set is kept because it is a run.

Other timing, for scale (not a benchmark): the sandbox is created in 0.04 to 0.1 s (restored from
the node's template snapshot) and a hop to example.com takes 0.3 to 1.5 s including the sandbox;
case 3 took 46 s once (expired.badssl.com answered slowly).

## Not verified

- Any page that needs a browser or JavaScript: the tracer fetches and does not render; script
  redirects are reported as hints only.
- HTTP/2-only sites (the tracer offers only `http/1.1`) and IPv6-only sites (IPv4 only throughout).
- A real malicious site. The targets were example.com, github.com and badssl.com.
- Whether a hostile guest can flood the node's decision log past its 512 entries
  (`DECISION_LOG` in `crates/hv2-net/src/gateway/mod.rs`) to push earlier entries out. The driver
  compares the entry count with the node's cumulative `stats` counters and flags
  `decision_log_incomplete` if they disagree or the log is full; that detection was not exercised.
- UDP and DNS tunnelling through the one allowed name: a guest may put data in lookups of
  subdomains only if the policy allows them; here only the exact name is allowed, and the gateway
  refused lookups of other names (case 4), but a tunnel through the allowed name itself, or
  through HTTP requests to it, was not tried. The gateway drops non-DNS UDP per its source
  comments; not tested.
- That requests leave from the host's own IP address. They do (the gateway dials from the host),
  which is a property of the design; no isolation of the source address was attempted or tested.
- Loopback: a guest's `127.0.0.1` never leaves the guest, so it cannot be expressed through the
  gateway and is not a case.
- Decision timestamps: the API's decisions have none (the daemon's log has them).
- A tracer run with a different guest kernel, `--memory-mb` below 512, more than one hop at the
  same time, or a node that was not started by the driver (`--node`).
- A build of exactly one commit: the runs are of a working tree, as noted above.
