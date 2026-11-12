#!/usr/bin/env python3
"""Standalone-node VM-to-VM networking (`/networks`), on real KVM.

One `hv2-sandboxd --network` node, no cluster, no mTLS, no control plane. Sandboxes
are joined to a named network under DNS aliases through the daemon's HTTP API, and
the guests are then asked, from inside, to resolve and connect to each other. Each
case is recorded pass/fail with its evidence and the check carries on after a
failure, so one broken case does not hide the others:

1. create a network; it is in the list and in inspect.
2. A and B join under aliases; inspect shows both members and their addresses.
3. in A, B's alias resolves (python3 socket.gethostbyname, i.e. libc through the
   guest's resolv.conf -> the gateway) to the address inspect reported, and a TCP
   round trip gets B's listener's token back. A third member C joins too, and A
   reaches C.
4. the reverse, B to A.
5. a sandbox that is NOT a member cannot resolve the alias, nor connect to the
   member's address directly.
6. disconnect B: A's next lookup of B fails, and so does a connect to B's old
   address; C is still reachable from A. Nothing is restarted.
7. reconnect B: A resolves and reaches B again, B reaches A again.
8. removing a network that still has members is refused (409); deleting a member
   sandbox frees its membership; after the rest leave, removal succeeds.

Not exercised: UDP, a second node, the `hm sandbox vm network` CLI (this drives the
HTTP API directly), compose using this path, anything beyond loopback-adjacent
same-node traffic, non-TCP, IPv6, and more than three members.

Writes report.json and the daemon's log (daemon-log.txt) into --output. Needs root
and /dev/kvm. Exit status is 1 if any case failed.
"""
import argparse, ast, hashlib, json, os, re, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path

NET = "standalone-check"
PORT = 9000

# A tiny listener that answers every connection with a known token, then closes.
LISTENER = """\
setsid python3 -c '
import socket
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", %d))
s.listen(16)
while True:
    c, _ = s.accept()
    c.sendall(b"TOKEN-%s\\n")
    c.close()
' >/tmp/listener.out 2>&1 </dev/null &
sleep 1
python3 -c 'import socket; c = socket.create_connection(("127.0.0.1", %d), 5); print("LOCAL", c.recv(64).decode().strip())'
"""

# Resolve, then connect, then read; one line of structured output either way.
PROBE = """\
python3 -c '
import socket, sys
name, port, addr = sys.argv[1], int(sys.argv[2]), sys.argv[3]
name = "" if name == "-" else name
addr = "" if addr == "-" else addr
out = {}
socket.setdefaulttimeout(10)
try:
    ip = socket.gethostbyname(name) if not addr else addr
    out["resolved"] = ip
except Exception as e:
    out["resolve_error"] = repr(e)
    print("PROBE", out)
    sys.exit(0)
try:
    c = socket.create_connection((ip, port), 10)
    out["token"] = c.recv(64).decode().strip()
except Exception as e:
    out["connect_error"] = repr(e)
print("PROBE", out)
' %s %d %s
"""


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--memory-mb", type=int, default=2048)
    parser.add_argument("--egress-default", choices=["deny", "allow"], default=None,
                        help="pass --egress-default to the daemon (default: not passed)")
    args = parser.parse_args()
    require_root = os.geteuid() == 0 and Path("/dev/kvm").exists()
    assert require_root, "root and /dev/kvm required"
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "cases": [], "network": NET, "port": PORT,
              "purpose": "functional verification of standalone-node networks through the HTTP API and live guests; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "input_names": {k: getattr(args, k).name for k in ["daemon", "kernel", "initrd"]},
              "egress_default_flag": args.egress_default, "host_cpus": os.cpu_count()}
    report["input_sha256"]["driver"] = digest(__file__)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon-log.txt"
    log = log_path.open("wb")
    argv = [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--network",
            "--memory-mb", str(args.memory_mb), "--cpu-cores", "2", "--capacity", "4"]
    if args.egress_default:
        argv += ["--egress-default", args.egress_default]
    report["daemon_flags"] = argv[1:]
    daemon = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    ids = {}   # name -> sandbox id
    addr = {}  # name -> address from connect

    def request(method, path, body=None, timeout=300):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw.decode(errors="replace")

    def run(who, cmd, within=60):
        status, value = request("POST", f"/sandboxes/{ids[who]}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        return value["stdout"] + value["stderr"]

    def probe(who, name, address=""):
        """What `who` sees trying to reach `name` (or `address`)."""
        said = run(who, PROBE % (f"'{name or '-'}'", PORT, f"'{address or '-'}'"))
        line = [l for l in said.splitlines() if l.startswith("PROBE ")]
        assert line, f"no PROBE line: {said[:300]!r}"
        return ast.literal_eval(line[-1][6:])  # a dict of strings printed by the guest

    def expect(condition, message):
        if not condition:
            raise AssertionError(message)

    def reaches(who, name, token, address=""):
        got = probe(who, name, address)
        expect(got.get("token") == token, f"{who} -> {name}: {got}")
        return got

    def cannot(who, name, address=""):
        got = probe(who, name, address)
        expect("token" not in got, f"{who} -> {name} unexpectedly worked: {got}")
        return got

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
            row["error"] = f"{type(error).__name__}: {str(error)[:600]}"
        row["seconds"] = round(time.monotonic() - began, 3)
        print(("ok:" if row["success"] else "FAIL:"), name, "" if row["success"] else row["error"], flush=True)

    def members():
        status, value = request("GET", f"/networks/{NET}")
        expect(status == 200, (status, value))
        return {m["alias"]: m for m in value["members"]}

    def connect(who, alias):
        status, value = request("POST", f"/networks/{NET}/connect", {"sandboxID": ids[who], "alias": alias})
        expect(status == 200, f"connect {who}: {status} {value}")
        addr[who] = value["address"]
        return value

    try:
        deadline = time.monotonic() + 240
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

        def make(name):
            status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 3600})
            expect(status in (200, 201), (status, made))
            ids[name] = made["sandboxID"]
            expect(run(name, "echo up").strip() == "up", "sandbox did not answer")

        began = time.monotonic()
        for name in ["a", "b", "c", "outsider"]:
            make(name)
        report["sandbox_boot_seconds"] = round(time.monotonic() - began, 1)
        report["guest_net"] = {n: run(n, "cat /proc/net/pnp 2>&1; ip -4 addr 2>&1 | head -20; ip route 2>&1") for n in ["a"]}

        def c1():
            status, made = request("POST", "/networks", {"name": NET})
            expect(status == 201, (status, made))
            status, listed = request("GET", "/networks")
            expect(status == 200 and any(n["name"] == NET for n in listed), listed)
            status, one = request("GET", f"/networks/{NET}")
            expect(status == 200 and one == {"name": NET, "members": []}, (status, one))
            return {"created": made, "list": listed, "inspect": one}
        case("1 create network; in list and inspect", c1)

        def c2():
            ra, rb = connect("a", "a.test"), connect("b", "b.test")
            found = members()
            expect(set(found) == {"a.test", "b.test"}, found)
            expect(found["a.test"]["sandboxID"] == ids["a"] and found["a.test"]["address"] == ra["address"], found)
            expect(found["b.test"]["sandboxID"] == ids["b"] and found["b.test"]["address"] == rb["address"], found)
            expect(ra["address"] != rb["address"], "same address")
            return found
        case("2 two sandboxes join under aliases; inspect shows both with addresses", c2)

        def c3():
            out = {}
            out["listener_b"] = run("b", LISTENER % (PORT, "B", PORT)).strip()
            expect("LOCAL TOKEN-B" in out["listener_b"], out["listener_b"])
            got = reaches("a", "b.test", "TOKEN-B")
            expect(got["resolved"] == addr["b"], f"resolved {got['resolved']} but inspect said {addr['b']}")
            out["a_to_b"] = got
            return out
        case("3 A resolves B's alias and gets B's token over TCP", c3)

        def c3b():
            connect("c", "c.test")
            out = {"listener_a": run("a", LISTENER % (PORT, "A", PORT)).strip(),
                   "listener_c": run("c", LISTENER % (PORT, "C", PORT)).strip()}
            expect("LOCAL TOKEN-A" in out["listener_a"] and "LOCAL TOKEN-C" in out["listener_c"], out)
            out["a_to_c"] = reaches("a", "c.test", "TOKEN-C")
            out["c_to_b"] = reaches("c", "b.test", "TOKEN-B")
            out["c_to_a"] = reaches("c", "a.test", "TOKEN-A")
            return out
        case("3b third member C joins; A->C, C->A, C->B", c3b)

        def c4():
            got = reaches("b", "a.test", "TOKEN-A")
            expect(got["resolved"] == addr["a"], f"resolved {got['resolved']} but inspect said {addr['a']}")
            return got
        case("4 reverse direction, B to A", c4)

        def c5():
            out = {"by_name": cannot("outsider", "b.test")}
            out["by_address"] = cannot("outsider", "", addr["b"])
            return out
        case("5 a non-member cannot resolve or reach a member", c5)

        def c6():
            status, value = request("POST", f"/networks/{NET}/disconnect", {"sandboxID": ids["b"]})
            expect(status == 204, (status, value))
            expect("b.test" not in members(), members())
            out = {"a_by_name": cannot("a", "b.test"), "a_by_old_address": cannot("a", "", addr["b"]),
                   "b_to_a": cannot("b", "a.test")}
            out["a_to_c_still"] = reaches("a", "c.test", "TOKEN-C")
            return out
        case("6 disconnect B: A cannot look up or reach it; A->C unaffected", c6)

        def c7():
            old = addr["b"]
            connect("b", "b.test")
            report["after_reconnect_members"] = members()
            expect(addr["b"] not in (addr["a"], addr["c"]),
                   f"reconnected B was given {addr['b']}, already held by a current member (a={addr['a']}, c={addr['c']})")
            out = {"old_address": old, "new_address": addr["b"],
                   "a_to_b": reaches("a", "b.test", "TOKEN-B"), "b_to_a": reaches("b", "a.test", "TOKEN-A")}
            return out
        case("7 reconnect B: works again both ways, nothing restarted", c7)

        def c8():
            out = {}
            status, value = request("DELETE", f"/networks/{NET}")
            expect(status == 409, f"remove with members: {status} {value}")
            out["refused"] = [status, value]
            status, value = request("DELETE", f"/sandboxes/{ids['c']}")
            expect(status in (200, 204), (status, value))
            expect("c.test" not in members(), f"deleted sandbox still a member: {members()}")
            out["deleted_member_left"] = True
            for who in ["a", "b"]:
                status, value = request("POST", f"/networks/{NET}/disconnect", {"sandboxID": ids[who]})
                expect(status == 204, (who, status, value))
            status, value = request("DELETE", f"/networks/{NET}")
            expect(status == 204, f"remove when empty: {status} {value}")
            status, value = request("GET", f"/networks/{NET}")
            expect(status == 404, f"inspect after remove: {status} {value}")
            status, listed = request("GET", "/networks")
            expect(all(n["name"] != NET for n in listed), listed)
            return out
        case("8 remove refused with members; deleted sandbox leaves; remove succeeds when empty", c8)

        for who, sid in ids.items():
            request("DELETE", f"/sandboxes/{sid}")
        report["success"] = all(c["success"] for c in report["cases"])
    except BaseException as error:
        report["fatal"] = f"{type(error).__name__}: {str(error)[:600]}"
        raise
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        text = log_path.read_bytes().decode(errors="replace")
        report["hypervisor_platform"] = re.findall(r"Detected hypervisor platform: (\w+)", text)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("success" if report["success"] else "FAILED", flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
