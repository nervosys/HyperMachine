#!/usr/bin/env python3
"""Pause, snapshot and fork a sandbox on a network while a peer keeps sending it traffic, on KVM.

Since 23d805bd a pause holds host-to-guest delivery (vsock and network) so a snapshot cannot
tear, and `resume` delivers what was held. This checks the network side end to end: sandbox A
serves a token on TCP 9000, sandbox B connects to it every 0.25 s and logs each try, and A is
paused and resumed, snapshotted, and forked, CYCLES times over, with B sending throughout.
After each step A must answer an exec and B must connect to A again within 20 s, and no reply
may ever be corrupted.

What it does not show: TCP retries a lost SYN, so a packet dropped (rather than held) during
a pause would still pass; it shows traffic recovers and nothing is mangled, not that every
held frame was delivered. A fork child is checked to answer an exec, not to be on the network.

Needs an image whose guest has python3 (the tools image, `/var/tmp/hm-docker/tools.cpio.gz`
in the 2026-11-11 evidence), run as root:

    tools/check-net-pause-kvm.py --daemon D --kernel K --initrd I --output DIR [--cycles 6]
"""
import argparse
import hashlib
import json
import os
import socket
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


LISTENER = """setsid python3 -c '
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", 9000)); s.listen(16)
while True:
    c, _ = s.accept(); c.sendall(b"TOKEN-A\\n"); c.close()
' >/tmp/listener.out 2>&1 </dev/null &
sleep 1; echo started"""

# The peer: every 0.25 s connect to A and log "<epoch> ok|bad|fail".
LOOP = """cat > /tmp/loop.py <<'PYEOF'
import socket, time, sys
addr = sys.argv[1]
f = open("/tmp/hits", "a", buffering=1)
while True:
    try:
        c = socket.create_connection((addr, 9000), 4)
        d = c.recv(64); c.close()
        f.write("%f ok\\n" % time.time() if b"TOKEN-A" in d else "%f bad\\n" % time.time())
    except Exception:
        f.write("%f fail\\n" % time.time())
    time.sleep(0.25)
PYEOF
setsid python3 /tmp/loop.py ADDR >/tmp/loop.out 2>&1 </dev/null &
sleep 1; echo loop"""


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--cycles", type=int, default=6)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of pausing, snapshotting and forking a networked sandbox under traffic, on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cycles": args.cycles, "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)

    api = free_port()
    base = f"http://127.0.0.1:{api}"
    store = args.output / "snapshot-store"
    store.mkdir()
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    log_path = args.output / "daemon.log"
    log = log_path.open("wb")
    daemon = subprocess.Popen([str(args.daemon), "--port", str(api), "--proxy-port", str(free_port()), "--network",
                               "--memory-mb", "2048", "--cpu-cores", "2", "--capacity", "6",
                               "--snapshot-store", str(store)],
                              env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def req(method, path, body=None, timeout=300):
        r = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                   data=None if body is None else json.dumps(body).encode())
        try:
            resp = urllib.request.urlopen(r, timeout=timeout)
        except urllib.error.HTTPError as e:
            resp = e
        with resp:
            raw = resp.read()
            try:
                return resp.status, json.loads(raw) if raw else None
            except ValueError:
                return resp.status, raw.decode(errors="replace")

    ids = {}

    def run(who, cmd, within=60):
        st, v = req("POST", f"/sandboxes/{ids.get(who, who)}/exec", {"cmd": cmd, "timeout_secs": within})
        assert st == 200, (st, v)
        return v["stdout"] + v["stderr"]

    def case(name, fn):
        try:
            detail = fn()
            report["cases"].append({"name": name, "success": True, "detail": detail})
            print("ok:", name, detail if detail is not None else "", flush=True)
        except Exception as e:
            report["cases"].append({"name": name, "success": False, "error": f"{type(e).__name__}: {str(e)[:500]}"})
            print("FAIL:", name, e, flush=True)

    def guest_now():
        return float(run("b", "python3 -c 'import time;print(time.time())'").strip())

    def traffic_after(t0, within=20):
        """Seconds until the peer logs a successful connection made after t0 (guest clock)."""
        began = time.monotonic()
        while time.monotonic() - began < within:
            rows = run("b", "cat /tmp/hits").strip().splitlines()
            if any(r.endswith(" ok") and float(r.split()[0]) > t0 for r in rows):
                return round(time.monotonic() - began, 2)
            time.sleep(0.5)
        raise AssertionError(f"no successful peer connection to A within {within}s")

    try:
        deadline = time.monotonic() + 240
        while True:
            assert daemon.poll() is None, "daemon exited"
            try:
                if req("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon did not come up"
            time.sleep(0.2)

        def setup():
            for n in ["a", "b"]:
                st, m = req("POST", "/sandboxes", {"templateID": "base", "timeout": 3600})
                assert st in (200, 201), (st, m)
                ids[n] = m["sandboxID"]
                assert run(n, "echo up").strip() == "up"
            st, v = req("POST", "/networks", {"name": "np"})
            assert st == 201, (st, v)
            addr = {}
            for n, alias in [("a", "a.test"), ("b", "b.test")]:
                st, v = req("POST", "/networks/np/connect", {"sandboxID": ids[n], "alias": alias})
                assert st == 200, (st, v)
                addr[n] = v["address"]
            assert "started" in run("a", LISTENER)
            assert "loop" in run("b", LOOP.replace("ADDR", addr["a"]))
            time.sleep(2)
            rows = run("b", "cat /tmp/hits").split("\n")
            assert any(r.endswith(" ok") for r in rows), rows
            return {"addresses": addr}
        case("two sandboxes on a network, B reaching A's listener", setup)
        assert report["cases"][-1]["success"], "setup failed"

        for i in range(args.cycles):
            def pause_resume():
                st, v = req("POST", f"/sandboxes/{ids['a']}/pause", {}, timeout=300)
                assert st == 204, (st, v)
                time.sleep(2)  # B keeps sending while A is paused
                t0 = guest_now()
                st, v = req("POST", f"/sandboxes/{ids['a']}/resume", {"timeout": 900})
                assert st in (200, 201), (st, v)
                assert run("a", "echo alive").strip() == "alive"
                return {"traffic_back_after_s": traffic_after(t0)}
            case(f"cycle {i}: pause and resume A under traffic", pause_resume)

            def snapshot():
                st, v = req("POST", f"/sandboxes/{ids['a']}/snapshots", {"name": f"np-snap-{i}"}, timeout=600)
                assert st in (200, 201), (st, v)
                t0 = guest_now()
                assert run("a", "echo alive").strip() == "alive"
                return {"traffic_back_after_s": traffic_after(t0)}
            case(f"cycle {i}: snapshot A under traffic", snapshot)

            def fork():
                st, v = req("POST", f"/sandboxes/{ids['a']}/fork", {"timeout": 900}, timeout=900)
                assert st in (200, 201), (st, v)
                child = v[0] if isinstance(v, list) else v
                child = child.get("sandbox", child)["sandboxID"]
                assert run(child, "echo child").strip() == "child"
                req("DELETE", f"/sandboxes/{child}")
                t0 = guest_now()
                assert run("a", "echo alive").strip() == "alive"
                return {"traffic_back_after_s": traffic_after(t0)}
            case(f"cycle {i}: fork A under traffic", fork)

        def tally():
            rows = run("b", "cat /tmp/hits").strip().splitlines()
            counts = {k: sum(1 for r in rows if r.endswith(" " + k)) for k in ("ok", "fail", "bad")}
            assert counts["bad"] == 0, counts
            return counts
        case("the peer's tries: no corrupted reply", tally)
    finally:
        daemon.terminate()
        try:
            daemon.wait(20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        text = log_path.read_text(errors="replace")
        report["platform_lines"] = {"Kvm": text.count("platform: Kvm"), "Tcg": text.count("platform: Tcg")}
        if report["platform_lines"]["Tcg"] or not report["platform_lines"]["Kvm"]:
            report["cases"].append({"name": "ran on KVM", "success": False, "detail": report["platform_lines"]})
        report["success"] = bool(report["cases"]) and all(c["success"] for c in report["cases"])
        (args.output / "report.json").write_text(json.dumps(report, indent=1))
        subprocess.run(["rm", "-rf", str(store)], check=False)
        print("success" if report["success"] else "FAILED", flush=True)


if __name__ == "__main__":
    main()
