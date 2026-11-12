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
10. characterisation: allowOut alone (no denyOut) does NOT pin; the node then defaults to
    allow whatever --egress-default says. The driver therefore sends denyOut too.

Writes report.json, the daemon's log (daemon-log.txt) and one trace file per case into
--output. Needs root and /dev/kvm. Exit status is 1 if any case failed.
"""
import argparse
import json
import os
import socket
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
            # Characterisation, not a control: the node's own semantics, which is why the
            # driver sends denyOut as well. If the node ever makes allowOut alone a pin,
            # this case fails and the driver's extra denyOut becomes redundant, not wrong.
            h = hop("case10", "https://www.wikipedia.org/", network={"allowOut": ["example.com"]})
            v = view(h)
            expect(v["status"] is not None, f"allowOut alone now blocks other hosts (status {v['status']}): {compact(h)}")
            expect(allowed(h, "tcp"), f"no connection allowed: {compact(h)}")
            return {"status": v["status"], "decisions": compact(h),
                    "meaning": "allowOut: [example.com] with the node's default deny still reached www.wikipedia.org"}
        case("10 (characterisation) allowOut alone does not pin: the node defaults to allow once a rule exists", c10)

        def c6():
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
