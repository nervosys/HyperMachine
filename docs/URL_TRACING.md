# Tracing a URL that may be malicious

`tools/urltrace.py` follows a URL and its redirects without trusting anything that comes back.
Every hop runs in its own throwaway microVM; the host decides what the VM may reach, and keeps its
own record of what the VM actually did. The guest tracer is `crates/hv2-urltrace`.

Status: run on KVM under WSL2 only, in two sets of three runs. The earlier busybox image passed
all its cases (`docs/benchmarks/2026-11-11/urltrace-kvm/`). The current image, with no shell and
the tracer started by the `argv` exec form, passed all 16 cases in each of three runs
(`docs/benchmarks/2026-11-11/urltrace-noshell-kvm/`). Not run on any other platform, and not run
against a real malicious site.

## The design, and why

1. **One hop per VM.** For each hop the host creates a fresh sandbox, runs one program,
   `hv2-urltrace <URL>`, which makes exactly one request and prints one JSON report, then deletes
   the sandbox. The guest never follows a redirect. The host reads `next_url` from the report and
   starts a new sandbox for the next hop. A hop that compromises its guest cannot see or influence
   the hops after it, and each hop starts from a clean image.
2. **Egress pinned per hop.** The sandbox may reach that hop's host name and nothing else. The
   node is started with networking on and the default egress policy deny; each sandbox is created
   with `allowOut: [host]` and `denyOut: [0.0.0.0/0, ::/0]`. A node with the default deny now treats an
   `allowOut` list sent alone as exclusive, but the driver still sends the deny-all as well, so a
   sandbox stays pinned on a node configured differently: with `--egress-default allow`, `allowOut`
   alone restricts nothing (E2B's reading). Earlier nodes allowed everything once any rule existed
   (`urltrace-kvm` case 10 recorded that). On a live guest of the current image, with no `denyOut`
   sent, a sandbox given only `allowOut: [example.com]` could not resolve `www.wikipedia.org` (four refused
   lookups, reason `the policy would refuse where this name leads`) and reached example.com with a 200,
   in all three runs (`urltrace-noshell-kvm` case 10). The gateway answers DNS only for the allowed name, refuses TCP to
   reserved addresses (loopback, link-local including `169.254.169.254`, RFC 1918, the gateway
   itself), and only accepts an allowed name on an address that name really resolves to.
3. **Two records per hop.** The guest's JSON report is a claim by a machine that has just parsed
   hostile bytes. The node's record of what the sandbox tried to do on the network
   (`GET /sandboxes/{id}/network/decisions`: DNS lookups and TCP attempts, each with a verdict) is
   fetched before the sandbox is deleted and kept next to the report. The driver compares them:
   any decision whose name or destination is not the hop's host is flagged
   `unexpected_network_activity` with the entries, every denied attempt is listed, and a
   connection the report claims but the node did not allow (or the reverse) is flagged.
4. **The guest is not trusted, the host is.** The driver treats the report as hostile data: size
   limits, type checks, nothing evaluated or executed, no report text in a shell or a path, every
   string capped and stripped of terminal control characters before it is printed. The URL goes to
   the guest as one element of an argv list, never interpolated into a shell string: the exec
   request is `argv = ["/bin/hv2-urltrace", URL, ...]`, run directly by the guest agent with no shell, so
   nothing parses its elements. Before anything runs, the URL is validated (http or https, a
   DNS name rather than an address, no userinfo, ASCII only, no whitespace, control characters,
   quotes, backticks or backslashes, at most 2048 characters). A `next_url` from the guest goes
   through the same validation before it is followed.
5. **Bounded.** At most `--max-hops` hops (default 10); a URL already visited ends the trace with
   `loop`; a per-hop time limit (`--hop-timeout`, default 30 s, which becomes the tracer's own
   timeout, with a few seconds more for the exec call); and every sandbox is created with a
   lifetime (hop timeout plus 120 s) that the node enforces, so nothing should outlive a dead driver (the expiry itself was not tested).

## Running it

```
python3 tools/urltrace.py 'http://github.com/' \
  --daemon /path/to/hv2-sandboxd --kernel /path/to/bzImage --initrd /path/to/urltrace.cpio.gz \
  --output out/ [--max-hops 10] [--hop-timeout 30] [--max-body 262144] [--user-agent STR] \
  [--tracer /path/to/hv2-urltrace]
```

Needs root and `/dev/kvm` (the driver starts its own `hv2-sandboxd --network --egress-default
deny --memory-mb 512 --cpu-cores 1` on free ports and stops it at the end). To use a node that is
already running, pass `--node http://127.0.0.1:PORT`; it must have been started with `--network`
and the default egress policy deny, which the driver cannot check. `--tracer` only records the
tracer's sha256.

The image is a Linux kernel (passed separately) plus an initramfs of two programs and empty
directories: `/init` (the guest agent itself as PID 1, which mounts `/proc`, `/sys`, `/dev`, `/tmp`
and links `resolv.conf`; `docs/GUEST_AGENT.md`), the same agent again as `/bin/hv2-guest-agentd`,
and `/bin/hv2-urltrace`. No busybox, no `sh`, no `bash`; the file list is in
`docs/benchmarks/2026-11-11/urltrace-noshell-kvm/image-files.txt`. The driver runs the tracer with
the exec API's `argv` form (`{"argv": ["/bin/hv2-urltrace", URL, ...]}`), which the guest agent
executes directly (`argv[0]` an absolute path). The node creates sandboxes from such an image
because, when the guest has no `/bin/sh`, it no longer fails: the agent as PID 1 links
`resolv.conf`, and the node installs its egress CA through the agent's file read and write
operations. Build the image with (`--no-busybox` requires `--init`, a static PID 1):

```
tools/guest-image/build.sh -o urltrace.cpio.gz --no-busybox --init /path/to/static/hv2-guest-agentd \
  --extra /path/to/static/hv2-urltrace
```

(The tracer must be static, for example `x86_64-unknown-linux-musl`; it carries its own root
certificates, so no CA bundle is needed.) The repeatable check is
`tools/check-urltrace-kvm.py` (same arguments plus `--tracer`).

## Reading the output

One line per hop: the URL, the status, whether the certificate verified (and why not), and where
it redirects. Under it, in this order: hints the tracer found in the body (a meta refresh, a
script that assigns `location`), labelled as hints and never followed; errors the guest reported;
driver errors; **UNEXPECTED NETWORK ACTIVITY** in capitals with the decisions that were not about
the hop's host; the attempts the node denied; and any **DISAGREEMENT** between report and record.
The last line is `ended:` `final` (a hop returned no `next_url`), `loop`, `max-hops`, or `error`
with the stage.

`out/trace.json` holds the input, the versions and sha256 of the driver, daemon, kernel, initrd
and tracer, one entry per hop (URL, sandbox id, the guest report verbatim in `guest_report`, the
node's decisions verbatim in `node_decisions`, the comparison in `flags`, timing, the create
request), the verdict in `ended`, and a short `chain` (URL, status, verified, next). Exit status is
0 if the trace completed, 2 for a bad URL or arguments, 1 if a stage failed (`ended: error`).

## What it does not do

- It fetches; it does not render. A page that redirects with JavaScript, or needs a browser to
  show what it shows, is only reported as a hint.
- HTTP/1.1 only (no HTTP/2), IPv4 only, no cookies, no compression, no proxy.
- It does not stop a hostile server from fingerprinting the request (the tracer's user agent is a
  fixed browser string unless `--user-agent` says otherwise), or from serving different content to
  an analyst.
- Requests leave from the host's own IP address.
- It does not prove the report is true. A compromised guest can print any report; the node's
  record is the check on what it did on the network, not on what the server sent.
- The node's decision log keeps 512 entries. The driver compares the count with the node's
  cumulative counters and flags `decision_log_incomplete` if they disagree, but whether a hostile
  guest can flood the log to hide earlier entries was not tested.
- Data can still leave through the one allowed name: in DNS lookups of it, in the request it
  makes, and (per hop) nothing limits what the guest sends to that host. UDP other than DNS is
  dropped by the gateway according to its source; that was not tested.
- Only the host name is pinned, not the port or the address beyond "where that name resolves".

## Known gaps

- With no shell in the image, the node features that run a shell in the guest (process listing,
  environment variables, checkpoint files, builds, disks, the machines path) do not work, and the
  agent as PID 1 does not reap orphans. Only the process listing was run, and its failure recorded.
  The egress CA written through the agent's file operations was not checked by a client in the guest
  trusting it (the tracer carries its own roots).
- Decisions carry no timestamps in the API (the daemon's log has them), so the order of entries is
  known and their timing is not.
- The guest today is a Linux kernel plus two programs (the agent as PID 1, and the tracer). The intention is to replace it with the
  owner's own OS, Botnix, once it has TCP and TLS. The host protocol does not change: one URL in,
  one JSON report out.
- `allowOut` alone pins only on a node whose egress default is deny (see above); under
  `--egress-default allow` it restricts nothing. The driver always sends the deny-all as well; other
  clients of the same API should do the same to be portable.

## Evidence

`docs/benchmarks/2026-11-11/urltrace-kvm/` (busybox image): three passing runs of the check, an earlier set with
one failure (a slow target, explained there), the guest image's file list, the node's logs, and
every hop's report and decisions, with the list of what was not verified.

`docs/benchmarks/2026-11-11/urltrace-noshell-kvm/` (no shell, `argv` exec, agent as PID 1): three
passing runs of 16 cases, the image's file list, each run's daemon log and report, every hop's
trace, what it took to get there (two intermediate runs are described there but not kept), and its
own list of what was not verified.
