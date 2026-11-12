#!/usr/bin/env python3
"""Trace a possibly malicious URL, one hop per microVM. See docs/URL_TRACING.md.

For each hop the host creates a fresh sandbox on an hv2-sandboxd node whose egress is
pinned to that hop's host name, runs `hv2-urltrace URL` in it (one request, one JSON
report, no redirect followed), fetches the NODE's own record of what the sandbox tried
to do on the network, deletes the sandbox, and only then reads `next_url` from the
report to start the next hop in a new sandbox.

The guest is assumed compromised. Its report is a claim; the node's decisions
(GET /sandboxes/{id}/network/decisions) are the record. Both are kept verbatim in
trace.json, and any disagreement is flagged.

    urltrace.py URL --daemon PATH --kernel PATH --initrd PATH --output DIR
                    [--max-hops N] [--hop-timeout SECS] [--user-agent STR] [--node URL]

With --node http://127.0.0.1:PORT an already running node is used (it must have been
started with --network and the default egress policy deny; the driver cannot check
that). Otherwise the driver starts its own on free ports and stops it at the end.

Exit status: 0 if the trace completed (whatever it found), 2 for a bad URL or arguments,
1 for a driver error (a stage failed: ended "error").

Python 3 standard library only. Needs Linux and /dev/kvm only to start its own node.
"""
import argparse
import hashlib
import ipaddress
import json
import os
import re
import socket
import subprocess
import sys
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

GUEST_TRACER = "/bin/hv2-urltrace"
# The exec request is {"argv": [GUEST_TRACER, URL, ...]}: the guest agent runs argv[0]
# directly with the rest as its arguments. There is no shell in the image or in the path,
# so nothing in the URL is ever interpreted by anything but the tracer's own parser.

MAX_URL_LEN = 2048
MAX_REPORT_BYTES = 4 * 1024 * 1024       # the node's exec answer, in total
MAX_DECISIONS_BYTES = 4 * 1024 * 1024
DECISION_LOG_CAPACITY = 512              # only for nodes that report no "dropped": their whole log
DEFAULT_UA = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
              "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36")
LABEL = re.compile(r"^(?!-)[a-z0-9-]{1,63}(?<!-)$")
SANDBOX_ID = re.compile(r"^[A-Za-z0-9_-]{1,64}$")


class DriverError(Exception):
    def __init__(self, stage, message):
        super().__init__(f"{stage}: {message}")
        self.stage = stage
        self.message = message


# ---------------------------------------------------------------- hostile text

def clean(value, limit=200):
    """Printable, capped, no terminal control or format characters."""
    text = value if isinstance(value, str) else repr(value)
    total = len(text)
    text = "".join("?" if unicodedata.category(ch)[0] == "C" else ch for ch in text[:limit])
    return text if total <= limit else text + "...[+%d]" % (total - limit)


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# ------------------------------------------------------------------ URL checks

def validate_url(url, what="URL"):
    """Return (host, normalised_url) or raise ValueError saying why not.

    http or https only; a DNS host name (not an address literal, not an IDN in
    Unicode form); no userinfo; no whitespace, control characters, quotes or
    backslashes; ASCII only; length capped."""
    if not isinstance(url, str):
        raise ValueError(f"{what} is not a string")
    if not url or len(url) > MAX_URL_LEN:
        raise ValueError(f"{what} is empty or longer than {MAX_URL_LEN} characters")
    for ch in url:
        o = ord(ch)
        if o <= 0x20 or o >= 0x7F:
            raise ValueError(f"{what} contains a space, control or non-ASCII character (U+{o:04X})")
        if ch in "'\"`\\<>{}|^":
            raise ValueError(f"{what} contains the character {ch!r}")
    try:
        parts = urllib.parse.urlsplit(url)
        port = parts.port
    except ValueError as error:
        raise ValueError(f"{what} does not parse: {error}")
    if parts.scheme.lower() not in ("http", "https"):
        raise ValueError(f"{what} scheme is not http or https")
    if not url.lower().startswith(parts.scheme.lower() + "://"):
        raise ValueError(f"{what} has no // authority")
    if parts.username is not None or parts.password is not None or "@" in parts.netloc:
        raise ValueError(f"{what} has userinfo")
    host = parts.hostname
    if not host:
        raise ValueError(f"{what} has no host")
    if port is not None and not (1 <= port <= 65535):
        raise ValueError(f"{what} port is out of range")
    if parts.netloc.endswith(":"):
        raise ValueError(f"{what} has an empty port")
    host = host.lower()
    if host.endswith(".") or "[" in parts.netloc:
        raise ValueError(f"{what} host has a trailing dot or is an IPv6 literal")
    is_address = True
    try:
        ipaddress.ip_address(host)
    except ValueError:
        is_address = False
    if is_address:
        raise ValueError(f"{what} host is an address literal, not a DNS name")
    labels = host.split(".")
    if len(host) > 253 or len(labels) < 2 or not all(LABEL.match(l) for l in labels):
        raise ValueError(f"{what} host {clean(host, 80)!r} is not a DNS name")
    if labels[-1].isdigit() or labels[-1].startswith("0x"):
        raise ValueError(f"{what} host looks like a numeric address")
    return host, url


def visited_key(url):
    """What makes two URLs the same visit: scheme, host, port, path and query."""
    parts = urllib.parse.urlsplit(url)
    return (parts.scheme.lower(), (parts.hostname or "").lower(), parts.port, parts.path or "/", parts.query)


# ------------------------------------------------------------------- the node

def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Node:
    """An hv2-sandboxd node's HTTP API, started by us or already running."""

    def __init__(self, base=None):
        self.base = base
        self.proc = None
        self.log = None
        self.log_path = None
        self.argv = None

    @classmethod
    def start(cls, daemon, kernel, initrd, output, memory_mb=512, cpu_cores=1, capacity=4):
        node = cls()
        api = free_port()
        node.base = f"http://127.0.0.1:{api}"
        node.log_path = Path(output) / "daemon-log.txt"
        node.log = node.log_path.open("wb")
        node.argv = [str(daemon), "--port", str(api), "--proxy-port", str(free_port()), "--network",
                     "--egress-default", "deny", "--memory-mb", str(memory_mb),
                     "--cpu-cores", str(cpu_cores), "--capacity", str(capacity)]
        env = dict(os.environ, HV2_KERNEL=str(kernel), HV2_INITRD=str(initrd), RUST_LOG="info")
        node.proc = subprocess.Popen(node.argv, env=env, stdin=subprocess.DEVNULL,
                                     stdout=node.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 240
        while True:
            if node.proc.poll() is not None:
                raise DriverError("node", "the daemon exited at start (see daemon-log.txt)")
            try:
                if node.request("GET", "/sandboxes", timeout=5)[0] == 200:
                    return node
            except OSError:
                pass
            if time.monotonic() > deadline:
                node.stop()
                raise DriverError("node", "the daemon was not ready in 240 s")
            time.sleep(0.2)

    def stop(self):
        if self.proc is not None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
            self.proc = None
        if self.log is not None:
            self.log.close()
            self.log = None

    def hypervisor_platform(self):
        if self.log_path is None or not self.log_path.exists():
            return None
        text = self.log_path.read_bytes().decode(errors="replace")
        return re.findall(r"Detected hypervisor platform: (\w+)", text)

    def request(self, method, path, body=None, timeout=300, limit=MAX_REPORT_BYTES):
        req = urllib.request.Request(self.base + path, method=method,
                                     headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read(limit + 1)
            if len(raw) > limit:
                raise DriverError("node", f"{method} {path}: answer longer than {limit} bytes")
            try:
                return response.status, json.loads(raw) if raw else None
            except (ValueError, RecursionError):
                return response.status, raw[:2000].decode(errors="replace")


# ------------------------------------------------------------ one hop, one VM

def _resolve_v4(host):
    try:
        return sorted({a[4][0] for a in socket.getaddrinfo(host, None, socket.AF_INET)})
    except OSError:
        return []


def compare(host, report, node_decisions, host_ips):
    """Flags from setting the guest's report against the node's record."""
    flags = {"unexpected_network_activity": [], "denied": [], "notes": []}
    entries = node_decisions.get("decisions") if isinstance(node_decisions, dict) else None
    if not isinstance(entries, list):
        flags["notes"].append("node_decisions_malformed")
        return flags
    dropped, recorded = node_decisions.get("dropped"), node_decisions.get("recorded")
    if isinstance(dropped, int) and not isinstance(dropped, bool):
        # The node's own count: it keeps the first and the most recent entries and says how
        # many between were let go.
        if dropped > 0:
            flags["decision_log_incomplete"] = {
                "dropped": dropped, "recorded": recorded if isinstance(recorded, int) else None,
                "entries": len(entries)}
    else:
        # An older node, which reports no count: infer from its counters.
        stats = node_decisions.get("stats") if isinstance(node_decisions.get("stats"), dict) else {}
        counted = sum(v for v in stats.values() if isinstance(v, int))
        flags["notes"].append("node_reports_no_dropped_count")
        if counted > len(entries) or len(entries) >= DECISION_LOG_CAPACITY:
            flags["decision_log_incomplete"] = {
                "entries": len(entries), "counted_by_stats": counted, "capacity": DECISION_LOG_CAPACITY}
    allowed_connections = set()
    for entry in entries:
        if not isinstance(entry, dict):
            flags["unexpected_network_activity"].append({"entry": entry, "why": "not an object"})
            continue
        kind, name = entry.get("kind"), entry.get("name")
        destination, verdict = entry.get("destination"), entry.get("verdict")
        norm = name.lower().rstrip(".") if isinstance(name, str) else None
        ip = destination.rsplit(":", 1)[0] if isinstance(destination, str) else None
        why = None
        if kind == "dns":
            if norm != host:
                why = "dns lookup of a name other than the hop's host"
        elif kind in ("tcp", "https-intercept", "http-inject"):
            if norm is not None and norm != host:
                why = "connection attempt named a host other than the hop's"
            elif norm is None and ip not in host_ips:
                why = "connection attempt to an address that is not the hop's host's, with no name"
            elif norm == host and verdict == "allow" and ip not in host_ips:
                flags["notes"].append(f"allowed connection to {ip}, not in the host's own lookup of {host} "
                                      "(a CDN may rotate addresses)")
        else:
            why = f"decision of kind {clean(kind, 30)!r}"
        if (why is None and kind == "tcp" and verdict == "allow"
                and not (isinstance(entry.get("reason"), str) and entry["reason"].startswith("allowOut name"))):
            why = "connection allowed for a reason other than the hop's name rule (the pin is not in effect)"
        if why:
            flags["unexpected_network_activity"].append({"entry": entry, "why": why})
        if verdict == "deny":
            flags["denied"].append(entry)
        if verdict == "allow" and kind == "tcp":
            allowed_connections.add(destination)
    # The report against the record.
    if isinstance(report, dict):
        connected = report.get("connected_to")
        if isinstance(connected, str) and connected not in allowed_connections:
            flags["guest_reported_connection_the_node_did_not_allow"] = clean(connected)
        elif connected is None and allowed_connections:
            flags["node_allowed_connection_the_guest_did_not_report"] = sorted(allowed_connections)
        url = report.get("url")
        if isinstance(url, dict) and isinstance(url.get("host"), str) and url["host"].lower() != host:
            flags["guest_reported_a_different_host"] = clean(url["host"], 100)
    return flags


def run_hop(node, url, pin_host=None, hop_timeout=30, user_agent=DEFAULT_UA, extra_args=(),
            allow_out=None, omit_network=False, network=None, max_body=None, boot_timeout=180):
    """One sandbox, one program, one request. The sandbox is gone when this returns.

    pin_host: the one host name the sandbox may reach (default: the URL's). allow_out
    overrides the allow list (use [] for none). omit_network sends no network object
    at all (the operator default applies); network replaces the whole object (only for
    characterising the node, never for a real hop). extra_args go to the tracer after the URL
    (the containment checks use --resolve).

    The policy is allowOut: [host] AND denyOut: every address. On a node whose
    --egress-default is deny, allowOut alone is also exclusive now
    (hv2_net::network_policy::from_e2b), but under --egress-default allow it restricts
    nothing (E2B's reading), so the deny-all is always sent too. The name rule overrides
    the address deny, which is what makes a pin."""
    host, _ = validate_url(url)
    pin = pin_host if pin_host is not None else host
    allow = [pin] if allow_out is None else list(allow_out)
    hop = {"url": url, "host": host, "pinned_to": allow, "sandbox_id": None, "tracer_args": list(extra_args),
           "guest_report": None, "node_decisions": None, "flags": None, "errors": [], "timing": {}}
    began = time.monotonic()
    sandbox = None
    try:
        # create: the node kills the sandbox itself at `timeout` if we die.
        body = {"templateID": "base", "timeout": hop_timeout + 120}
        if network is not None:
            body["network"] = network
        elif not omit_network:
            body["network"] = {"allowOut": allow, "denyOut": ["0.0.0.0/0", "::/0"]}
        hop["create_request"] = body
        t = time.monotonic()
        status, made = node.request("POST", "/sandboxes", body, timeout=boot_timeout)
        if status not in (200, 201) or not isinstance(made, dict) or not SANDBOX_ID.match(str(made.get("sandboxID", ""))):
            raise DriverError("create", f"HTTP {status}: {clean(made, 300)}")
        sandbox = made["sandboxID"]
        hop["sandbox_id"] = sandbox
        hop["timing"]["create_s"] = round(time.monotonic() - t, 3)

        # exec: argv, run directly by the guest agent. No shell anywhere.
        argv = [GUEST_TRACER, url, "--timeout", str(hop_timeout), "--user-agent", user_agent]
        if max_body is not None:
            argv += ["--max-body", str(int(max_body))]
        argv += list(extra_args)
        hop["exec_argv"] = argv
        t = time.monotonic()
        status, ran = node.request("POST", f"/sandboxes/{sandbox}/exec",
                                   {"argv": argv, "timeout_secs": hop_timeout + 5},
                                   timeout=hop_timeout + 30)
        hop["timing"]["exec_s"] = round(time.monotonic() - t, 3)
        if status != 200 or not isinstance(ran, dict):
            hop["errors"].append({"stage": "exec", "message": f"HTTP {status}: {clean(ran, 300)}"})
        else:
            hop["exit_code"] = ran.get("exit_code") if isinstance(ran.get("exit_code"), int) else None
            hop["timed_out"] = bool(ran.get("timed_out"))
            stderr = ran.get("stderr")
            hop["guest_stderr"] = stderr[:4096] if isinstance(stderr, str) else None
            stdout = ran.get("stdout")
            if hop["timed_out"]:
                hop["errors"].append({"stage": "exec", "message": "the program ran past its time limit and was killed"})
            try:
                report = json.loads(stdout) if isinstance(stdout, str) and stdout.strip() else None
            except (ValueError, RecursionError) as error:
                report = None
                hop["guest_report_raw"] = stdout[:65536] if isinstance(stdout, str) else None
                hop["errors"].append({"stage": "report", "message": f"not JSON: {clean(error, 200)}"})
            if report is None and "guest_report_raw" not in hop:
                hop["errors"].append({"stage": "report", "message": "the guest printed no report"})
            elif report is not None and not isinstance(report, dict):
                hop["guest_report_raw"] = stdout[:65536]
                hop["errors"].append({"stage": "report", "message": "the report is not a JSON object"})
                report = None
            hop["guest_report"] = report
    except DriverError as error:
        hop["errors"].append({"stage": error.stage, "message": error.message})
    except OSError as error:
        hop["errors"].append({"stage": "node", "message": clean(error, 300)})
    finally:
        # The node's own record, BEFORE the sandbox is deleted (it goes with it).
        if sandbox:
            try:
                status, decisions = node.request("GET", f"/sandboxes/{sandbox}/network/decisions",
                                                 timeout=30, limit=MAX_DECISIONS_BYTES)
                if status == 200 and isinstance(decisions, dict):
                    hop["node_decisions"] = decisions
                else:
                    hop["errors"].append({"stage": "decisions", "message": f"HTTP {status}: {clean(decisions, 300)}"})
            except (DriverError, OSError) as error:
                hop["errors"].append({"stage": "decisions", "message": clean(error, 300)})
            try:
                status, value = node.request("DELETE", f"/sandboxes/{sandbox}", timeout=60)
                gone, _ = node.request("GET", f"/sandboxes/{sandbox}", timeout=30)
                hop["deleted"] = status in (200, 204) and gone == 404
                if not hop["deleted"]:
                    hop["errors"].append({"stage": "delete", "message": f"DELETE {status}, then GET {gone}"})
            except (DriverError, OSError) as error:
                hop["deleted"] = False
                hop["errors"].append({"stage": "delete", "message": clean(error, 300)})
    hop["timing"]["total_s"] = round(time.monotonic() - began, 3)
    if hop["node_decisions"] is not None:
        hop["flags"] = compare(pin, hop["guest_report"], hop["node_decisions"], _resolve_v4(pin))
    return hop


def report_view(hop):
    """The few fields the driver reads from the (untrusted) report, type-checked."""
    report = hop.get("guest_report")
    out = {"status": None, "verified": None, "verification_error": None, "next_url": None,
           "location": None, "hints": [], "errors": []}
    if not isinstance(report, dict):
        return out
    response = report.get("response")
    if isinstance(response, dict):
        if isinstance(response.get("status"), int) and not isinstance(response["status"], bool):
            out["status"] = response["status"]
        for key in ("next_url", "location"):
            if isinstance(response.get(key), str):
                out[key] = response[key]
    tls = report.get("tls")
    if isinstance(tls, dict):
        if isinstance(tls.get("verified"), bool):
            out["verified"] = tls["verified"]
        if isinstance(tls.get("verification_error"), str):
            out["verification_error"] = tls["verification_error"]
    if isinstance(report.get("hints"), list):
        out["hints"] = [h for h in report["hints"][:10] if isinstance(h, dict)]
    if isinstance(report.get("errors"), list):
        out["errors"] = [e for e in report["errors"][:10] if isinstance(e, dict)]
    return out


# ------------------------------------------------------------------ the trace

def trace(node, url, max_hops=10, hop_timeout=30, user_agent=DEFAULT_UA, on_hop=None, max_body=None):
    host, url = validate_url(url)
    result = {"hops": [], "ended": None}
    seen = set()
    current = url
    while True:
        seen.add(visited_key(current))
        hop = run_hop(node, current, hop_timeout=hop_timeout, user_agent=user_agent, max_body=max_body)
        view = report_view(hop)
        hop["view"] = {k: view[k] for k in ("status", "verified", "verification_error", "next_url")}
        result["hops"].append(hop)
        if on_hop:
            on_hop(len(result["hops"]), hop, view)
        stage = next((e["stage"] for e in hop["errors"] if e["stage"] in ("create", "exec", "node", "decisions", "delete")), None)
        if stage or hop["guest_report"] is None:
            result["ended"] = "error"
            result["error_stage"] = stage or "report"
            return result
        nxt = view["next_url"]
        if nxt is None:
            result["ended"] = "final"
            return result
        try:
            _, nxt = validate_url(nxt, "next_url from the guest")
        except ValueError as error:
            result["ended"] = "error"
            result["error_stage"] = "next_url"
            result["error"] = str(error)
            return result
        if visited_key(nxt) in seen:
            result["ended"] = "loop"
            result["loop_url"] = nxt
            return result
        if len(result["hops"]) >= max_hops:
            result["ended"] = "max-hops"
            result["next_url_not_followed"] = nxt
            return result
        if isinstance(view["location"], str):
            joined = urllib.parse.urljoin(current, view["location"])
            if joined != nxt:
                hop["flags"]["next_url_differs_from_location"] = {"location": clean(view["location"], 300), "next_url": clean(nxt, 300)}
        current = nxt


def chain(result):
    return [{"url": h["url"], "status": h["view"]["status"], "verified": h["view"]["verified"],
             "next": h["view"]["next_url"]} for h in result["hops"]]


def summary_lines(result):
    lines = []
    for i, hop in enumerate(result["hops"], 1):
        v = report_view(hop)
        if v["verified"] is True:
            cert = "certificate verified"
        elif v["verified"] is False:
            cert = "CERTIFICATE NOT VERIFIED: " + clean(v["verification_error"] or "no reason given", 120)
        else:
            cert = "no certificate (plain http or no handshake)"
        nxt = clean(v["next_url"], 200) if v["next_url"] else "no redirect"
        line = f"hop {i}: {clean(hop['url'], 200)}  status {v['status']}  {cert}  -> {nxt}"
        lines.append(line)
        for h in v["hints"]:
            lines.append(f"    hint (found in the body, not followed, unverified): {clean(h.get('kind'), 40)}: {clean(h.get('value'), 160)}")
        for e in v["errors"]:
            lines.append(f"    guest reported error at {clean(e.get('stage'), 30)}: {clean(e.get('message'), 160)}")
        for e in hop["errors"]:
            lines.append(f"    DRIVER ERROR at {clean(e['stage'], 30)}: {clean(e['message'], 200)}")
        flags = hop.get("flags") or {}
        unexpected = flags.get("unexpected_network_activity") or []
        if unexpected:
            lines.append(f"    UNEXPECTED NETWORK ACTIVITY ({len(unexpected)} decision(s) not about {clean(hop['host'], 80)}):")
            for u in unexpected[:10]:
                e = u["entry"] if isinstance(u["entry"], dict) else {}
                lines.append(f"      {clean(e.get('kind'), 20)} {clean(e.get('verdict'), 10)} name={clean(e.get('name'), 80)} "
                             f"dest={clean(e.get('destination'), 60)}  [{clean(u['why'], 100)}]")
        if flags.get("denied"):
            lines.append(f"    denied by the node: {len(flags['denied'])}")
            for e in flags["denied"][:10]:
                lines.append(f"      {clean(e.get('kind'), 20)} name={clean(e.get('name'), 80)} dest={clean(e.get('destination'), 60)} "
                             f"({clean(e.get('reason'), 100)})")
        if "decision_log_incomplete" in flags:
            lines.append(f"    DECISION LOG INCOMPLETE: {clean(json.dumps(flags['decision_log_incomplete']), 300)} "
                         "(the node let go entries between the first and the most recent it kept)")
        for key in ("guest_reported_connection_the_node_did_not_allow", "node_allowed_connection_the_guest_did_not_report",
                    "guest_reported_a_different_host", "next_url_differs_from_location"):
            if key in flags:
                lines.append(f"    DISAGREEMENT {key}: {clean(json.dumps(flags[key]), 300)}")
    ended = result["ended"]
    extra = {"error": f" at stage {result.get('error_stage')}", "loop": f" at {clean(result.get('loop_url'), 200)}",
             "max-hops": f", next would be {clean(result.get('next_url_not_followed'), 200)}"}.get(ended, "")
    lines.append(f"ended: {ended}{extra}")
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("url")
    parser.add_argument("--daemon", type=Path)
    parser.add_argument("--kernel", type=Path)
    parser.add_argument("--initrd", type=Path)
    parser.add_argument("--tracer", type=Path, help="the tracer binary baked into the initrd (only to record its sha256)")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--max-hops", type=int, default=10)
    parser.add_argument("--hop-timeout", type=int, default=30)
    parser.add_argument("--max-body", type=int, default=None, help="bytes of body the tracer reads (default: its own, 262144)")
    parser.add_argument("--user-agent", default=DEFAULT_UA)
    parser.add_argument("--node", help="use this running node instead of starting one")
    parser.add_argument("--memory-mb", type=int, default=512)
    parser.add_argument("--cpu-cores", type=int, default=1)
    args = parser.parse_args()

    try:
        validate_url(args.url)
        if not args.user_agent or any(ord(c) < 0x20 or ord(c) >= 0x7F or c in "'\"`\\" for c in args.user_agent) or len(args.user_agent) > 300:
            raise ValueError("--user-agent must be printable ASCII without quotes or backslashes, at most 300 characters")
        if not (1 <= args.max_hops <= 50) or not (3 <= args.hop_timeout <= 110):
            raise ValueError("--max-hops must be 1..50 and --hop-timeout 3..110")
        if not args.node and not (args.daemon and args.kernel and args.initrd):
            raise ValueError("either --node or all of --daemon, --kernel, --initrd")
    except ValueError as error:
        print(f"urltrace: {error}", file=sys.stderr)
        raise SystemExit(2)

    args.output.mkdir(parents=True, exist_ok=True)
    versions = {"python": sys.version.split()[0], "driver_sha256": sha256_file(__file__)}
    for key in ("daemon", "kernel", "initrd", "tracer"):
        path = getattr(args, key)
        if path:
            versions[key + "_sha256"] = sha256_file(path)
    out = {"input": {"url": args.url, "max_hops": args.max_hops, "hop_timeout": args.hop_timeout,
                     "user_agent": args.user_agent, "node": args.node or "started by the driver"},
           "versions": versions}
    node = Node(args.node) if args.node else Node.start(args.daemon, args.kernel, args.initrd, args.output,
                                                        args.memory_mb, args.cpu_cores)
    try:
        if not args.node:
            out["daemon_flags"] = node.argv[1:]
        result = trace(node, args.url, args.max_hops, args.hop_timeout, args.user_agent, max_body=args.max_body,
                       on_hop=lambda i, hop, view: print(f"[hop {i} done in {hop['timing']['total_s']} s]", flush=True))
    finally:
        node.stop()
        if not args.node:
            out["hypervisor_platform"] = node.hypervisor_platform()
    out.update({"hops": result["hops"], "ended": result["ended"], "chain": chain(result)})
    for key in ("error_stage", "error", "loop_url", "next_url_not_followed"):
        if key in result:
            out[key] = result[key]
    (args.output / "trace.json").write_text(json.dumps(out, indent=2) + "\n")
    print("\n".join(summary_lines(result)))
    raise SystemExit(0 if result["ended"] != "error" else 1)


if __name__ == "__main__":
    main()
