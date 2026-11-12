# URL tracing with no shell in the image, on KVM

`tools/check-urltrace-kvm.py` drives `tools/urltrace.py` (the host driver, described in
`docs/URL_TRACING.md`) against one `hv2-sandboxd --network --egress-default deny` node, as in
`../urltrace-kvm/README.md`. The difference is the guest image: besides the kernel it holds two
programs, the guest agent (installed twice, as `init` so it is PID 1, and as `bin/hv2-guest-agentd`)
and the tracer, and **no shell and no busybox**. The tracer is started by an `argv` exec, with nothing to parse its
arguments. Three consecutive runs, `run1` to `run3`, pass all 16 cases each. Each run directory
holds `report.json`, `daemon-log.txt`, `image-files.txt` and `traces/`.

The runs that came before these (steps 1 and 3 under "What it took") were overwritten by the final
runs and are **not kept as files**. What is recorded of them is the description below, written from
the run output as it was seen; it cannot be checked against files.

## Setup

- WSL2 Debian under Windows, 24 CPUs (`host_cpus` in each `report.json`), `/dev/kvm`. Every daemon
  log has 19 lines `Detected hypervisor platform: Kvm` (18 sandboxes and the node's template) and
  none says `Tcg` (counted with `grep -c` on all three logs). The kernel string of the WSL host,
  `6.18.33.2-microsoft-standard-WSL2` on `kvm_amd` under Hyper-V (nested), is taken from
  `../urltrace-kvm/README.md`, the same host the same day; these `report.json` files do not record it.
- Repo commit `27c52971` (HEAD) plus uncommitted changes on top of it. The binaries were built
  from that working tree, not from a commit: `crates/hv2-sandboxd/src/main.rs` (the node changes
  below, and the `argv` exec form), `crates/hv2-guest-agent/src/bin/agentd.rs` (the agent as PID 1),
  `tools/urltrace.py`, `tools/check-urltrace-kvm.py`, `tools/guest-image/build.sh` and the docs.
- Daemon: Linux release build `hv2-sandboxd`, sha256
  `deaac8c5d7f03358412fd8b992b7a2855c316fc35819f18af582a8b091e9f883` (as given, and as in all three
  `report.json`). Flags: `--network --egress-default deny --memory-mb 512 --cpu-cores 1 --capacity 4`
  (ports chosen per run).
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Guest image `urltrace.cpio.gz` (2.5 MB), sha256
  `e1e285cfdbddb5a0f318ebdbf3ef4540da51ead79a2bc327fe9042afd6860ec7`, built with
  `tools/guest-image/build.sh -o urltrace.cpio.gz --no-busybox --init <static hv2-guest-agentd>
  --extra <static hv2-urltrace>` (`--agent` was also given, the same file, in the build that is
  recorded in the earlier version of this README).
- Tracer `hv2-urltrace` (static musl), sha256
  `2ad9bb1538874f4ceb8deac8dff6cf1c7628578686d96bad24d4daafc3906eee`, unchanged from
  `../urltrace-kvm`. The static guest agent in the image, sha256
  `51fd18a60cb30070c90b43aba77d45f84812c6d31170f345afce6dfad49a8393`, is from the earlier version
  of this README; the image hash is identical to the one recorded there, but `report.json` does
  not hash the agent separately.
- Driver sha256 `0f837f7bc6cccee19e1e64dbc30530a40bf3e3473a59f31215153c7f01a9c688`, check sha256
  `8cc356ae2af2d53df52f678341885b1b0b54f21772ee3441fcb3be11e2718772` (the corrected check, same in
  all three runs). Hop limit 60 s, body cap 65536 bytes.

## What is in the image

`image-files.txt` (identical in `run1` to `run3`, and checked by case 13 from the cpio itself):
`init` and `bin/hv2-guest-agentd` (both 1754984 bytes, the same agent), `bin/hv2-urltrace`, and the
empty directories `bin dev etc proc root sys tmp`. There is no `busybox`, `sh` or `bash` in it,
and case 13 fails the run if any file named like a shell appears or if anything else is present.
Note what "no shell" means here: the image holds none. Case 11 shows that executing `/bin/sh`
fails in a live guest; it does not show that a compromised tracer could not bring its own code.

## Result

| Case | run1 | run2 | run3 |
|---|---|---|---|
| 1 `https://example.com/`: one hop, 200, verified; decisions name only example.com | pass | pass | pass |
| 2 `http://github.com/`: 301 then a second hop in a different sandbox, 200 | pass | pass | pass |
| 3 `https://expired.badssl.com/`: verified false with a reason, response still fetched | pass | pass | pass |
| 4 pinned to example.com, tracer asked for www.wikipedia.org: refused by the node | pass | pass | pass |
| 5a `--resolve` to another public site's address: refused and recorded | pass | pass | pass |
| 5b `--resolve 169.254.169.254` (cloud metadata): refused and recorded | pass | pass | pass |
| 5c `--resolve` to the gateway and its DNS address: refused and recorded | pass | pass | pass |
| 5d `--resolve` to this host's own address: refused and recorded | pass | pass | pass |
| 7 with no allow rule, example.com cannot be fetched | pass | pass | pass |
| 8 a URL with shell metacharacters reaches the tracer byte for byte | pass | pass | pass |
| 9 URL validation rejects hostile input (26 rejected, 3 accepted) | pass | pass | pass |
| 10 `allowOut: [example.com]` alone on a deny-default node: another site refused, example.com reached | pass | pass | pass |
| 11 no shell in the image: `/bin/sh`, `/bin/busybox` and the `cmd` form fail because the file does not exist | pass | pass | pass |
| 12 PID 1 is the guest agent (`/proc/1/exe`, run once with no shell, names itself) | pass | pass | pass |
| 13 the image's file list holds no busybox, sh or bash, only init, the agent and the tracer | pass | pass | pass |
| 6 no hop's sandbox exists afterwards (18 sandboxes made in each run, all answer 404, the list is empty) | pass | pass | pass |

Recorded values, identical in the three runs unless stated:

- Case 10: the other site (`www.wikipedia.org`) got no response; the guest saw
  `failed to lookup address information: Try again`; the node recorded four refused lookups, and
  the set of refusal reasons is exactly `["the policy would refuse where this name leads"]`.
  example.com returned 200 with one allowed `tcp` decision, reason
  `allowOut name, by this gateway's DNS answer`. The create request was
  `{"templateID": "base", "timeout": 180, "network": {"allowOut": ["example.com"]}}`, with no
  `denyOut`.
- Case 11, each through the exec API (recorded in `report.json`):
  `argv ["/bin/sh","-c","echo x"]` returned HTTP 500 with `exec_in_guest: Script error: the guest
  agent could not run /bin/sh: could not start /bin/sh: No such file or directory (os error 2)`;
  `argv ["/bin/busybox","sh"]` returned HTTP 500 with the same text for `/bin/busybox`; the `cmd`
  form `{"cmd":"echo x"}` returned HTTP 500 with the `/bin/sh` text. As a control,
  `argv ["/bin/hv2-urltrace","--help"]` returned HTTP 200, exit code 2, and the tracer's usage text.
- Case 12: `GET .../processes` returned HTTP 502, `listing the guest's processes: Script error:
  the guest agent could not run /bin/sh: ...`. `argv ["/proc/1/exe"]` returned HTTP 200 with exit
  code 1 and stderr `hv2-guest-agentd: Address already in use (os error 98)`: a second copy of PID
  1's own binary, which failed to take the agent's vsock port and named itself. The check also
  recorded `pid1_line_in_daemon_log: false` in all three runs: the line the agent logs when it is
  PID 1 (`hv2-guest-agentd: running as PID 1 (init)`) does not appear in the daemon's log, which
  does not carry the guest's console. The check records this and does not require it.

## What it took

1. **First attempt, blocked.** The shell-less image could not create any sandbox, because the node
   itself ran `/bin/sh` in every guest: to link `resolv.conf` and install its egress CA certificate
   (`configure_guest_network` in `crates/hv2-sandboxd/src/main.rs`) and to warm a template. The
   create call returned: `HTTP 500: configuring the guest's network: Script error: the guest agent
   could not run /bin/sh: could not start /bin/sh: No such file or directory (os error 2)`. Only
   cases 9 and 13 passed in that run.
2. **The node's fix.** `guest_has_no_shell` recognises that error (the text `could not start
   /bin/sh` together with `No such file or directory`). `configure_guest_network` still tries the
   shell first. If, and only if, that error comes back: with no CA to install it returns success
   (the agent as PID 1 has already linked `resolv.conf`), and with a CA it reads
   `/etc/ssl/certs/ca-certificates.crt` through the agent's file read (an absent bundle counts as
   empty), appends the CA, and writes the file back through the agent's file write. Template warm-up
   (`/bin/sh -c true`) ignores that same error. Any other error still fails as before, and an image
   with a shell takes the old path unchanged.
3. **Second attempt: 14 of 16 cases in each of three runs.** Two cases failed identically in all
   three, and both were faults in the check's assertions, not in the node or the guest.
   Case 10 required the refusal reason to contain the word "default"; a lookup refused at the DNS
   stage carries the reason `the policy would refuse where this name leads`. Case 12 required the
   agent's "running as PID 1" console line in the daemon's log, which does not carry the guest's
   console. The substantive assertions of both had already passed. The assertions were corrected:
   case 10 now asserts that no refusal reason speaks of allowing, and records the reasons; case 12
   records whether the line is in the log and does not require it.
4. **Final: these three runs, 16 of 16 each.**

Why correcting the assertions did not weaken the check. Case 10 still asserts that the other site
returns no response, that nothing at all was allowed, that a refused lookup of `www.wikipedia.org`
is in the node's record, and that example.com is reached with 200 and an allowed connection; the
word "default" was a guess at wording, and the new assertion is stricter on the point that
matters (a default allow would put "allow" in a reason). Case 12 still asserts that `/proc/1/exe`
runs and that its output names `hv2-guest-agentd`, which is the evidence for PID 1's identity; the
console line was never evidence the daemon could see.

## Newly shown, compared with `../urltrace-kvm`

- **No shell in the image, and exec by argv.** The earlier image carried busybox and a `sh`
  because init and the node's exec both needed one. Here the image has neither (case 13), the
  node creates sandboxes from it and runs the tracer through `argv` (cases 1 to 8, 10), a request
  for `/bin/sh`, `/bin/busybox` or the `cmd` form fails with a missing-file error (case 11), and a
  URL with `$(id);echo$IFS$HOME` reaches the tracer byte for byte (case 8).
- **The guest agent as PID 1** booted and served the node on KVM in all three runs; names resolved
  in the guest through the `resolv.conf` the agent links (every allowed lookup in cases 1, 2, 3, 8
  and 10 was answered), and `/proc/1/exe` is the agent (case 12).
- **An allow-list alone pins a sandbox on a deny-default node, on a live guest.** In
  `../urltrace-kvm` case 10 recorded the opposite: `allowOut` alone let a sandbox fetch another
  site, reason `default allow`. That was the node's behaviour then. Here the same request shape,
  with no `denyOut`, refused every lookup of the other site and reached only example.com (case 10).
  The node changed between the two directories: the commit `143ccca7` ("an allowOut list alone is
  exclusive on a node whose default is deny") is in the history before these runs. What is shown
  here is the behaviour above, not that code change.

## Not verified

- The egress CA certificate written through the agent's file operations was not checked by a
  client inside the guest trusting it: the tracer carries its own roots, so no case needed the
  bundle, and case 3 shows its own root handling, not the written bundle.
- The other node features that run a shell in the guest were not exercised: process listing,
  environment variables, checkpoint files, builds, disks, and the machines path. They do not work
  with a shell-less image. The only one run is the process listing, whose failure (HTTP 502) is
  recorded in case 12.
- Orphaned processes are not reaped by the agent as PID 1 (documented in `docs/GUEST_AGENT.md`;
  no case created orphans).
- Any page that needs a browser or JavaScript: the tracer fetches and does not render; script
  redirects are reported as hints only.
- HTTP/2-only sites (the tracer offers only `http/1.1`) and IPv6-only sites (IPv4 only throughout).
- A real malicious site. The targets were example.com, github.com and badssl.com.
- Whether a hostile guest can flood the node's decision log past its 512 entries
  (`DECISION_LOG` in `crates/hv2-net/src/gateway/mod.rs`) to push earlier entries out. The driver
  flags `decision_log_incomplete` on a count mismatch; that detection was not exercised.
- UDP and DNS tunnelling through the one allowed name, or through HTTP requests to it, was not
  tried. The gateway drops non-DNS UDP per its source comments; not tested.
- That requests leave from the host's own IP address. They do (the gateway dials from the host);
  no isolation of the source address was attempted or tested.
- Decision timestamps: the API's decisions have none (the daemon's log has them).
- A different guest kernel, `--memory-mb` below 512, more than one hop at the same time, or a node
  that was not started by the driver (`--node`).
- A build of exactly one commit: the runs are of a working tree with uncommitted changes, as
  noted above.
- The two intermediate runs (steps 1 and 3) as files: they were overwritten.
- Any platform other than KVM under WSL2.
