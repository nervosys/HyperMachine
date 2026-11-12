#!/usr/bin/env python3
"""The URL-tracing sandbox (tools/urltrace.py, docs/URL_TRACING.md) on real KVM.

One `hv2-sandboxd --network --egress-default deny` node, started here; every hop runs
in its own microVM whose guest image holds the guest agent and `hv2-urltrace`. Each
case is recorded pass/fail with its evidence and the check carries on after a failure,
so one broken case does not hide the others:

 1. https://example.com/: one hop, 200, certificate verified, ended "final"; the node's
    decisions show the lookup and the connection to example.com allowed and nothing else.
 2. http://github.com/: hop 1 is a 301 to https://github.com/, hop 2 runs in a different
    sandbox; each hop's decisions name only that hop's host.
 3. https://expired.badssl.com/: verified false with a reason, response still fetched.
 4. containment by name: a sandbox pinned to example.com is asked for www.wikipedia.org;
    the guest must fail and the node must record the refused lookup.
 5. containment by address: the pinned sandbox is told (--resolve) to connect to another
    public site's address, to the cloud metadata address, to the gateway and DNS
    addresses and to this host's own address; each is refused and recorded (loopback cannot be
    expressed: a guest's 127.0.0.1 never leaves the guest).
 6. every hop's sandbox is gone after the hop (GET answers 404, the list is empty).
 7. no network before a policy: a sandbox created with no allow rule cannot fetch
    example.com (once with no network object at all, once with every address denied).
 8. a hostile-looking URL reaches the tracer byte for byte (no shell interpretation).
 9. the driver's own URL validation rejects what it must, without starting a VM.
10. the allow-list rule: on this deny-default node a sandbox created with ONLY
    allowOut: ["example.com"] (no denyOut) is refused another site, with the node's
    decisions showing the default deny, and still reaches example.com. The driver sends
    the deny-all as well on real hops.
11. no shell in the image: exec with argv /bin/sh -c, argv /bin/busybox sh and the old
    `cmd` form all fail because the file does not exist (errors recorded).
12. PID 1 is the guest agent: /proc/1/exe, run once with no shell, names itself. Whether the
    agent's "running as PID 1" line reached the daemon log is recorded, not required: that
    log does not carry the guest's console.
13. the image's file list (from the cpio on the host) holds no busybox, sh or bash:
    only /init, the agent, the tracer and empty directories.

14. a guest cannot push its first entries out of the node's record: after one allowed fetch of
    example.com, many refused lookups (one tracer exec each) push the log past its 512 kept
    entries; the node must report dropped > 0, keep the first entries (lowest seq, the
    allowed lookup and connection), give strictly increasing seq and an `at` inside the run's
    wall-clock window, and the driver's comparison must set decision_log_incomplete.

Writes report.json, the daemon's log (daemon-log.txt) and one trace file per case into
--output. Needs root and /dev/kvm. Exit status is 1 if any case failed.
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import urltrace as ut  # noqa: E402


def compact(hop):
    entries = ((hop.get("node_decisions") or {}).get("decisions")) or []
    return [[e.get("kind"), e.get("name"), e.get("destination"), e.get("verdict"), e.get("reason")] for e in entries]


def view(hop):
    return ut.report_view(hop)


def expect(condition, message):
    if not condition:
        raise AssertionError(message)


def allowed(hop, kind=None):
    return [e for e in ((hop.get("node_decisions") or {}).get("decisions") or [])
            if e.get("verdict") == "allow" and (kind is None or e.get("kind") == kind)]


def denied(hop):
    return [e for e in ((hop.get("node_decisions") or {}).get("decisions") or []) if e.get("verdict") == "deny"]


def only_host(hop, host):
    """The node's record names `host` and nothing else, and has both a lookup and a connection allowed."""
    entries = (hop.get("node_decisions") or {}).get("decisions") or []
    expect(entries, "the node recorded nothing")
    stray = [e for e in entries if e.get("name") != host]
    expect(not stray, f"decisions that do not name {host}: {stray}")
    expect(any(e["kind"] == "dns" and e["verdict"] == "allow" for e in entries), "no allowed lookup recorded")
    expect(any(e["kind"] == "tcp" and e["verdict"] == "allow" for e in entries), "no allowed connection recorded")
    expect(not denied(hop), f"a refusal was recorded: {denied(hop)}")
    flags = hop["flags"]
    expect(not flags["unexpected_network_activity"] and "decision_log_incomplete" not in flags, flags)
    node_record = hop["node_decisions"]
    expect(node_record.get("dropped") == 0, f"the node reports dropped={node_record.get('dropped')!r}")
    expect(isinstance(node_record.get("recorded"), int) and node_record["recorded"] == len(entries), node_record.get("recorded"))
    expect(all(isinstance(e.get("seq"), int) and isinstance(e.get("at"), int) for e in entries), "an entry lacks seq or at")


def public_address_of_another_site(not_in):
    for site in ["www.wikipedia.org", "www.iana.org", "www.python.org", "www.kernel.org"]:
        try:
            for info in socket.getaddrinfo(site, 443, socket.AF_INET):
                ip = info[4][0]
                if ip not in not_in:
                    return site, ip
        except OSError:
            continue
    return None, None


def own_address():
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.connect(("192.0.2.1", 9))  # no packet is sent: this only picks the outgoing interface
        return s.getsockname()[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for name in ["daemon", "kernel", "initrd", "tracer", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--memory-mb", type=int, default=512)
    parser.add_argument("--hop-timeout", type=int, default=60)
    parser.add_argument("--max-body", type=int, default=65536,
                        help="body bytes the tracer reads; github.com's page took 7-15 s to read 256 KiB even without a VM")
    args = parser.parse_args()
    assert os.geteuid() == 0 and Path("/dev/kvm").exists(), "root and /dev/kvm required"
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "cases": [],
              "input_sha256": {k: ut.sha256_file(getattr(args, k)) for k in ["daemon", "kernel", "initrd", "tracer"]},
              "input_names": {k: getattr(args, k).name for k in ["daemon", "kernel", "initrd", "tracer"]},
              "driver_sha256": ut.sha256_file(ut.__file__), "check_sha256": ut.sha256_file(__file__),
              "host_cpus": os.cpu_count(), "memory_mb": args.memory_mb, "hop_timeout": args.hop_timeout, "max_body": args.max_body}
    node = ut.Node.start(args.daemon, args.kernel, args.initrd, args.output, args.memory_mb, 1, 4)
    report["daemon_flags"] = node.argv[1:]
    sandboxes = []   # every sandbox id any case made
    traces = {}      # case name -> hops, written out at the end

    def hop(case_key, url, **kw):
        h = ut.run_hop(node, url, hop_timeout=args.hop_timeout, max_body=args.max_body, **kw)
        if h["sandbox_id"]:
            sandboxes.append(h["sandbox_id"])
        traces.setdefault(case_key, []).append(h)
        return h

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        try:
            detail = action()
            row["success"] = True
            if detail is not None:
                row["detail"] = detail
        except Exception as error:  # a failure is a result; carry on
            row["error"] = f"{type(error).__name__}: {str(error)[:900]}"
        row["seconds"] = round(time.monotonic() - began, 3)
        print(("ok:" if row["success"] else "FAIL:"), name, "" if row["success"] else row["error"], flush=True)

    def run_trace(key, url):
        result = ut.trace(node, url, max_hops=5, hop_timeout=args.hop_timeout, max_body=args.max_body)
        traces[key] = result["hops"]
        for h in result["hops"]:
            if h["sandbox_id"]:
                sandboxes.append(h["sandbox_id"])
        return result

    try:
        def c1():
            result = run_trace("case1", "https://example.com/")
            expect(result["ended"] == "final", result)
            expect(len(result["hops"]) == 1, f"{len(result['hops'])} hops")
            h = result["hops"][0]
            v = view(h)
            expect(v["status"] == 200, v)
            expect(v["verified"] is True, v)
            only_host(h, "example.com")
            return {"chain": ut.chain(result), "decisions": compact(h), "sandbox": h["sandbox_id"], "timing": h["timing"]}
        case("1 https://example.com/ one hop, 200, verified; node saw only example.com", c1)

        def c2():
            result = run_trace("case2", "http://github.com/")
            expect(result["ended"] == "final", f"ended {result['ended']} {result.get('error_stage')}")
            hops = result["hops"]
            expect(len(hops) >= 2, f"{len(hops)} hops")
            v1, v2 = view(hops[0]), view(hops[1])
            expect(v1["status"] == 301 and v1["next_url"] == "https://github.com/", v1)
            expect(v2["status"] in (200, 301, 302, 303, 307, 308), v2)
            expect(hops[0]["sandbox_id"] != hops[1]["sandbox_id"], "both hops ran in one sandbox")
            for h in hops:
                only_host(h, h["host"])
            return {"chain": ut.chain(result), "sandboxes": [h["sandbox_id"] for h in hops],
                    "decisions": [compact(h) for h in hops]}
        case("2 http://github.com/ 301 then a second hop in a different sandbox", c2)

        def c3():
            result = run_trace("case3", "https://expired.badssl.com/")
            h = result["hops"][0]
            v = view(h)
            expect(v["verified"] is False, f"verified={v['verified']}")
            expect(v["verification_error"], "no reason given")
            expect(v["status"] is not None, "the response was not fetched")
            only_host(h, "expired.badssl.com")
            return {"status": v["status"], "verification_error": v["verification_error"], "ended": result["ended"]}
        case("3 expired.badssl.com: verified false with a reason, response still fetched", c3)

        def c4():
            h = hop("case4", "https://www.wikipedia.org/", pin_host="example.com")
            v = view(h)
            expect(v["status"] is None, f"a response came back: status {v['status']}")
            entries = h["node_decisions"]["decisions"]
            refused = [e for e in entries if e["kind"] == "dns" and e["verdict"] == "deny" and e["name"] == "www.wikipedia.org"]
            expect(refused, f"no refused lookup of www.wikipedia.org recorded: {compact(h)}")
            expect(not allowed(h), f"something was allowed: {allowed(h)}")
            expect(h["flags"]["unexpected_network_activity"], "the driver did not flag the activity")
            return {"guest_errors": view(h)["errors"], "decisions": compact(h), "stats": h["node_decisions"]["stats"]}
        case("4 pinned to example.com, asked for www.wikipedia.org: refused by the node", c4)

        def refused_by_address(key, url, ip, expect_kind_reason=None):
            h = hop(key, url, pin_host="example.com", extra_args=["--resolve", ip])
            v = view(h)
            expect(v["status"] is None, f"a response came back from {ip}: status {v['status']}")
            to_ip = [e for e in h["node_decisions"]["decisions"] if e["destination"].rsplit(":", 1)[0] == ip]
            expect(to_ip, f"the node recorded nothing for {ip}: {compact(h)}")
            expect(all(e["verdict"] == "deny" for e in to_ip), f"a connection to {ip} was allowed: {to_ip}")
            if expect_kind_reason:
                expect(any(expect_kind_reason in e["reason"] for e in to_ip), f"reasons: {[e['reason'] for e in to_ip]}")
            return {"guest_errors": v["errors"], "decisions": compact(h)}

        def c5():
            sites_ip = ut._resolve_v4("example.com")
            site, ip = public_address_of_another_site(sites_ip)
            expect(ip, "no other public address found to try")
            out = {"other_site": site, "other_address": ip, "example_com_addresses": sites_ip}
            out["https_to_other_public_address"] = refused_by_address("case5-public-https", "https://example.com/", ip)
            out["http_to_other_public_address"] = refused_by_address("case5-public-http", "http://example.com/", ip)
            return out
        case("5a --resolve to another public site's address: refused and recorded", c5)

        def c5b():
            return {"http": refused_by_address("case5-metadata-http", "http://example.com/", "169.254.169.254", "reserved address"),
                    "https": refused_by_address("case5-metadata-https", "https://example.com/", "169.254.169.254", "reserved address")}
        case("5b --resolve 169.254.169.254 (cloud metadata): refused and recorded", c5b)

        def c5c():
            out = {}
            for label, ip in [("gateway", "10.0.2.2"), ("dns_server", "10.0.2.3")]:
                out[label] = refused_by_address(f"case5-{label}", "http://example.com/", ip)
            return out
        case("5c --resolve to the node's gateway and its DNS address: refused and recorded", c5c)

        def c5d():
            own = own_address()
            return {"host_address": own, "http": refused_by_address("case5-own", "http://example.com/", own)}
        case("5d --resolve to this host's own address: refused and recorded", c5d)

        def c7():
            out = {}
            h = hop("case7-no-network-object", "https://example.com/", omit_network=True)
            expect(view(h)["status"] is None, f"fetched with no network object: {view(h)['status']}")
            out["no_network_object"] = {"guest_errors": view(h)["errors"], "driver_errors": h["errors"],
                                        "exit_code": h.get("exit_code"), "decisions": compact(h)}
            h = hop("case7-deny-all", "https://example.com/", allow_out=[])
            expect(view(h)["status"] is None, f"fetched with every address denied: {view(h)['status']}")
            expect(not allowed(h), f"allowed: {allowed(h)}")
            expect(any(e["kind"] == "dns" and e["verdict"] == "deny" and e["name"] == "example.com" for e in denied(h)),
                   f"no refused lookup: {compact(h)}")
            out["deny_all"] = {"guest_errors": view(h)["errors"], "decisions": compact(h)}
            return out
        case("7 with no allow rule, example.com cannot be fetched", c7)

        def c8():
            url = "http://example.com/?q=$(id);echo$IFS$HOME&x=*&y=%27"
            h = hop("case8", url)
            report_ = h["guest_report"]
            expect(isinstance(report_, dict), "no report")
            expect(report_.get("input_url") == url, f"input_url {report_.get('input_url')!r}")
            line = (report_.get("request") or {}).get("request_line", "")
            expect("$(id);echo$IFS$HOME&x=*&y=%27" in line, f"request line {line!r}")
            return {"request_line": line, "status": view(h)["status"]}
        case("8 a URL with shell metacharacters reaches the tracer byte for byte", c8)

        def c9():
            bad = ["", "file:///etc/passwd", "ftp://example.com/", "http://", "http://example.com/a b", "http://example.com/\n",
                   "http://example.com/'", "http://example.com/\"", "http://example.com/`id`", "http://example.com/\\x",
                   "http://user:pw@example.com/", "http://127.0.0.1/", "http://[::1]/", "http://0x7f.1/", "http://2130706433/",
                   "http://localhost/", "http://exa_mple.com/", "http://example.com:99999/", "http://example.com:/",
                   "http://*.example.com/", "http://example.com․evil.test/", "http://" + "a" * 64 + ".com/",
                   "http://example.com/" + "a" * 3000, "http://example.com/\x1b[2J", "javascript:alert(1)", "//example.com/"]
            accepted = []
            for u in bad:
                try:
                    ut.validate_url(u)
                    accepted.append(u[:60])
                except ValueError:
                    pass
            expect(not accepted, f"accepted: {accepted}")
            good = ["https://example.com/", "http://github.com/a?b=c#d", "https://a.b.example.co.uk:8443/x"]
            for u in good:
                ut.validate_url(u)
            return {"rejected": len(bad), "accepted_ok": len(good)}
        case("9 URL validation rejects hostile input", c9)

        def c10():
            # The allow-list rule: on a node whose default is deny, `allowOut` alone is
            # exclusive (hv2_net::network_policy::from_e2b). No denyOut is sent here, on
            # purpose: the driver still sends one on real hops, but this case shows what
            # the node does without it.
            h = hop("case10-other-site", "https://www.wikipedia.org/", network={"allowOut": ["example.com"]})
            v = view(h)
            expect(v["status"] is None, f"a response came back from another site: status {v['status']}: {compact(h)}")
            expect(not allowed(h), f"something was allowed: {allowed(h)}")
            refused = [e for e in denied(h) if e["kind"] == "dns" and e["name"] == "www.wikipedia.org"]
            expect(refused, f"no refused lookup of www.wikipedia.org: {compact(h)}")
            # A lookup is refused for where the name would lead, so its reason does not say
            # "default deny"; what must never appear is a default *allow*.
            reasons = sorted({str(e.get("reason", "")) for e in denied(h)})
            expect(not any("allow" in r.lower() for r in reasons), f"a reason speaks of allowing: {reasons}")
            h2 = hop("case10-allowed-site", "https://example.com/", network={"allowOut": ["example.com"]})
            v2 = view(h2)
            expect(v2["status"] == 200, f"example.com was not reached: status {v2['status']}: {compact(h2)}")
            expect(allowed(h2, "tcp"), f"no connection allowed: {compact(h2)}")
            return {"other_site": {"status": v["status"], "guest_errors": v["errors"], "decisions": compact(h),
                                   "refusal_reasons": reasons},
                    "allowed_site": {"status": v2["status"], "decisions": compact(h2)},
                    "create_request": h["create_request"]}
        case("10 allowOut: [example.com] alone on a deny-default node: another site refused, example.com reached", c10)

        # One throwaway sandbox for the cases that need the guest and not the tracer.
        probe = {}

        def probe_sandbox():
            if "id" not in probe:
                body = {"templateID": "base", "timeout": 300,
                        "network": {"allowOut": ["example.com"], "denyOut": ["0.0.0.0/0", "::/0"]}}
                status, made = node.request("POST", "/sandboxes", body, timeout=180)
                expect(status in (200, 201) and isinstance(made, dict), f"create: HTTP {status} {made}")
                probe["id"] = made["sandboxID"]
                sandboxes.append(probe["id"])
            return probe["id"]

        def exec_raw(body):
            status, ran = node.request("POST", f"/sandboxes/{probe_sandbox()}/exec", dict(body, timeout_secs=10), timeout=60)
            return {"http_status": status, "answer": ran}

        def text_of(result):
            return json.dumps(result["answer"]) if not isinstance(result["answer"], str) else result["answer"]

        def failed_for_a_missing_file(result):
            ran = result["answer"]
            ok_exit = isinstance(ran, dict) and ran.get("exit_code") == 0
            expect(result["http_status"] != 200 or not ok_exit, f"it ran and succeeded: {result}")
            expect("No such file or directory" in text_of(result), f"not a missing-file error: {result}")
            expect(not (isinstance(ran, dict) and "x" in str(ran.get("stdout", "")).split()), f"it printed x: {result}")

        def c11():
            out = {}
            out["argv_sh_c_echo_x"] = exec_raw({"argv": ["/bin/sh", "-c", "echo x"]})
            out["argv_busybox_sh"] = exec_raw({"argv": ["/bin/busybox", "sh"]})
            out["cmd_form_echo_x"] = exec_raw({"cmd": "echo x"})
            for key, result in out.items():
                try:
                    failed_for_a_missing_file(result)
                except AssertionError as error:
                    raise AssertionError(f"{key}: {error}")
            out["argv_tracer_still_runs"] = exec_raw({"argv": ["/bin/hv2-urltrace", "--help"]})
            return out
        case("11 no shell in the image: /bin/sh, /bin/busybox and the cmd form all fail because the file does not exist", c11)

        def c12():
            out = {}
            # Without a shell, the processes listing (which reads /proc through one) cannot work either.
            status, listed = node.request("GET", f"/sandboxes/{probe_sandbox()}/processes", timeout=60)
            out["processes_listing"] = {"http_status": status, "answer": listed}
            # /proc/1/exe is PID 1's binary. Running it as a second process makes it try to
            # bind the agent's vsock port, which the real one holds: it fails, and what it
            # prints first is its own name. That is PID 1's identity seen from inside the
            # guest with no shell and no other program.
            probe_run = exec_raw({"argv": ["/proc/1/exe"]})
            out["proc_1_exe"] = probe_run
            ran = probe_run["answer"]
            expect(isinstance(ran, dict), f"no answer: {probe_run}")
            expect("hv2-guest-agentd" in str(ran.get("stderr", "")) or "hv2-guest-agentd" in text_of(probe_run),
                   f"/proc/1/exe did not identify itself as the agent: {probe_run}")
            log = node.log_path.read_bytes().decode(errors="replace") if node.log_path.exists() else ""
            line = "hv2-guest-agentd: running as PID 1 (init)"
            # Recorded, not required: the node's log does not carry the guest's console.
            out["pid1_line_in_daemon_log"] = line in log
            return out
        case("12 PID 1 is the guest agent (/proc/1/exe, run once with no shell, names itself)", c12)

        def c13():
            import gzip
            raw = gzip.decompress(args.initrd.read_bytes())
            def cpio(flag):
                done = subprocess.run(["cpio", flag, "--quiet"], input=raw, capture_output=True, check=True)
                return done.stdout.decode()
            names = [n for n in cpio("-t").splitlines() if n]
            (args.output / "image-files.txt").write_text(cpio("-tv"))
            shells = {"busybox", "sh", "bash", "dash", "ash", "zsh", "ksh", "csh", "tcsh", "fish"}
            found = [n for n in names if os.path.basename(n) in shells]
            expect(not found, f"shell-like files in the image: {found}")
            expected = {".", "init", "bin", "bin/hv2-guest-agentd", "bin/hv2-urltrace", "dev", "etc", "proc", "root", "sys", "tmp"}
            expect(set(names) == expected, f"unexpected contents: extra {sorted(set(names) - expected)}, missing {sorted(expected - set(names))}")
            return {"files": sorted(names)}
        case("13 the image's file list (from the cpio) holds no busybox, sh or bash, only init, the agent and the tracer", c13)

        def c14():
            # A compromised guest floods the node's record with refused lookups. The first
            # entries (its real activity, here an allowed fetch of example.com) must survive.
            body = {"templateID": "base", "timeout": 1800,
                    "network": {"allowOut": ["example.com"], "denyOut": ["0.0.0.0/0", "::/0"]}}
            status, made = node.request("POST", "/sandboxes", body, timeout=180)
            expect(status in (200, 201) and isinstance(made, dict), f"create: HTTP {status} {made}")
            sid = made["sandboxID"]
            sandboxes.append(sid)
            decisions_path = f"/sandboxes/{sid}/network/decisions"

            def run_tracer(url):
                argv = [ut.GUEST_TRACER, url, "--timeout", "3", "--user-agent", ut.DEFAULT_UA,
                        "--max-body", str(args.max_body)]
                return node.request("POST", f"/sandboxes/{sid}/exec", {"argv": argv, "timeout_secs": 15}, timeout=60)

            def fetch():
                st, got = node.request("GET", decisions_path, timeout=30, limit=ut.MAX_DECISIONS_BYTES)
                expect(st == 200 and isinstance(got, dict), f"decisions: HTTP {st} {got}")
                return got

            try:
                window_start_ms = int(time.time() * 1000)
                st, first_run = run_tracer("https://example.com/")
                expect(st == 200 and isinstance(first_run, dict), f"first run: HTTP {st} {first_run}")
                after_first = fetch()
                first_entries = after_first["decisions"]
                expect(after_first["dropped"] == 0, after_first)
                expect(any(e["kind"] == "dns" and e["verdict"] == "allow" and e["name"] == "example.com" for e in first_entries)
                       and any(e["kind"] == "tcp" and e["verdict"] == "allow" and e["name"] == "example.com" for e in first_entries),
                       f"the allowed fetch left no allowed lookup and connection: {compact({'node_decisions': after_first})}")
                first_count = after_first["recorded"]
                execs, began = 0, time.monotonic()
                recorded = first_count
                while recorded <= 700:
                    execs += 1
                    expect(execs <= 600 and time.monotonic() - began < 1500,
                           f"gave up after {execs} execs, recorded {recorded}")
                    run_tracer(f"https://flood-{execs}.example.org/")
                    if execs % 10 == 0 or recorded + 10 > 700:
                        recorded = fetch()["recorded"]
                flood_seconds = round(time.monotonic() - began, 1)
                final = fetch()
                window_end_ms = int(time.time() * 1000)
                entries = final["decisions"]
                seqs = [e["seq"] for e in entries]
                expect(final["dropped"] > 0, f"nothing dropped: recorded {final['recorded']}, kept {len(entries)}")
                expect(final["recorded"] == len(entries) + final["dropped"], final["recorded"])
                expect(all(isinstance(s, int) for s in seqs) and all(b > a for a, b in zip(seqs, seqs[1:])),
                       "seq values are not strictly increasing")
                head = entries[:first_count]
                expect([e["seq"] for e in head] == list(range(1, first_count + 1)),
                       f"the lowest sequence numbers are {seqs[:first_count + 2]}, not 1..{first_count}")
                expect(all(e["name"] == "example.com" and e["verdict"] == "allow" for e in head),
                       f"the first entries are not the allowed example.com ones: {compact({'node_decisions': {'decisions': head}})}")
                expect(head == first_entries, "the first entries changed after the flood")
                expect(any(e["kind"] == "dns" for e in head) and any(e["kind"] == "tcp" for e in head), "head lacks dns or tcp")
                slack = 5000
                bad_at = [e for e in entries if not (isinstance(e.get("at"), int) and not isinstance(e["at"], bool)
                                                     and window_start_ms - slack <= e["at"] <= window_end_ms + slack)]
                expect(not bad_at, f"`at` outside the host's window {window_start_ms}..{window_end_ms}: {bad_at[:3]}")
                expect(all(b["at"] >= a["at"] for a, b in zip(entries, entries[1:])), "`at` goes backwards")
                flags = ut.compare("example.com", None, final, ut._resolve_v4("example.com"))
                expect("decision_log_incomplete" in flags, f"the driver did not flag it: {flags.get('notes')}")
                expect(flags["decision_log_incomplete"]["dropped"] == final["dropped"], flags["decision_log_incomplete"])
                traces["case14"] = [{"url": "https://example.com/ then flood-N.example.org", "sandbox_id": sid,
                                     "node_decisions": final, "flags": flags, "execs": execs}]
                return {"execs": execs, "flood_seconds": flood_seconds, "recorded_after_first_run": first_count,
                        "recorded": final["recorded"], "dropped": final["dropped"], "kept": len(entries),
                        "lowest_seqs": seqs[:first_count + 2], "kept_head_entries": compact({"node_decisions": {"decisions": head}}),
                        "first_tail_seq": seqs[len([s for s in seqs if s <= 256])] if len(seqs) > 256 else None,
                        "last_seq": seqs[-1], "at_window_ms": [window_start_ms, window_end_ms],
                        "at_min_max_ms": [min(e["at"] for e in entries), max(e["at"] for e in entries)],
                        "flag": flags["decision_log_incomplete"]}
            finally:
                st_del, _ = node.request("DELETE", f"/sandboxes/{sid}", timeout=60)
                st_gone, _ = node.request("GET", f"/sandboxes/{sid}", timeout=30)
                for h in traces.get("case14", []):
                    h["deleted"] = st_del in (200, 204) and st_gone == 404
        case("14 a guest cannot push its first entries out of the node's record", c14)

        def c6():
            if probe.get("id"):
                node.request("DELETE", f"/sandboxes/{probe['id']}", timeout=60)
            unique = sorted(set(sandboxes))
            expect(unique, "no sandboxes were made")
            lingering = []
            for sid in unique:
                status, _ = node.request("GET", f"/sandboxes/{sid}", timeout=30)
                if status != 404:
                    lingering.append([sid, status])
            expect(not lingering, f"still there: {lingering}")
            status, listed = node.request("GET", "/sandboxes", timeout=30)
            expect(status == 200, (status, listed))
            expect(not listed, f"the node still lists sandboxes: {listed}")
            not_confirmed = [h["sandbox_id"] for hops in traces.values() for h in hops if h["sandbox_id"] and not h.get("deleted")]
            expect(not not_confirmed, f"the driver did not confirm deletion of {not_confirmed}")
            return {"sandboxes_made": len(unique)}
        case("6 no hop's sandbox exists afterwards", c6)

        report["success"] = all(c["success"] for c in report["cases"])
    except BaseException as error:
        report["fatal"] = f"{type(error).__name__}: {str(error)[:600]}"
        raise
    finally:
        node.stop()
        report["hypervisor_platform"] = node.hypervisor_platform()
        (args.output / "traces").mkdir(exist_ok=True)
        for key, hops in traces.items():
            (args.output / "traces" / f"{key}.json").write_text(json.dumps({"hops": hops}, indent=2) + "\n")
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("success" if report["success"] else "FAILED", flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
