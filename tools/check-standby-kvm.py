#!/usr/bin/env python3
"""Standby on real KVM: a sandbox stopped in memory, woken by the next request.

Runs an owned hv2-sandboxd with a template and `--idle-standby-after`, and
checks, through its API:

1. `POST /sandboxes/{id}/standby` stops the guest: its vCPUs take no exits for
   as long as it is left alone, across two of the node's metric samples, and a
   counter a guest process increments ten times a second does not move.
2. The next command wakes it without being asked to: it answers, the counter
   runs again from where it stopped, and the node records one wake.
3. A request to one of the sandbox's own ports through the proxy wakes it too.
4. A sandbox in standby can still be paused to disk and resumed.
5. A sandbox left idle goes into standby by itself, and wakes the same way.

Then it measures, with the sandbox awake and in standby alternately: how long a
command takes end to end, and how long the node's part of a wake takes (its
vCPUs being told to run again). These are measurements of this build on this
host, not a comparison with anything.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, http.client, json, os, shutil, socket, statistics, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def summary(values):
    ordered = sorted(values)

    def at(q):
        return round(ordered[min(len(ordered) - 1, int(q * len(ordered)))], 3)
    return {"n": len(ordered), "min": round(ordered[0], 3), "p50": round(statistics.median(ordered), 3),
            "p90": at(0.9), "max": round(ordered[-1], 3)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=40)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of standby on KVM, with same-build timings; "
                                           "no comparison with another product",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_load_1m_at_start": os.getloadavg()[0], "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-standby-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api, proxy = port(), port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy), "--memory-mb", "512",
         "--cpu-cores", "1", "--capacity", "4", "--idle-standby-after", "30"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=120):
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

    def execute(sandbox, command):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 20})
        assert status == 200 and value["exit_code"] == 0, f"guest command failed: {status} {value}"
        return value["stdout"].strip()

    def status_of(sandbox):
        status, value = request("GET", f"/sandboxes/{sandbox}/standby")
        assert status == 200, (status, value)
        return value

    def standby(sandbox):
        status, value = request("POST", f"/sandboxes/{sandbox}/standby")
        assert status == 200 and value["standby"] is True, (status, value)
        return value

    def through_proxy(sandbox, guest_port):
        connection = http.client.HTTPConnection("127.0.0.1", proxy, timeout=20)
        try:
            connection.request("GET", "/", headers={"host": f"{guest_port}-{sandbox}.localhost"})
            response = connection.getresponse()
            return response.status, response.read().decode(errors="replace").strip()
        finally:
            connection.close()

    def create():
        status, created = request("POST", "/sandboxes", {"timeout": 900})
        assert status == 201, (status, created)
        return created["sandboxID"]

    try:
        deadline = time.monotonic() + 300
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                status, templates = request("GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

        sandbox = create()
        execute(sandbox, "(i=0; while true; do i=$((i+1)); echo $i > /tmp/count; sleep 0.1; done) "
                         ">/dev/null 2>&1 &")
        time.sleep(1)
        state = {}

        def stops():
            state["count"] = int(execute(sandbox, "cat /tmp/count"))
            before = standby(sandbox)
            # Two of the node's five-second metric samples pass; neither may wake it.
            time.sleep(12)
            after = status_of(sandbox)
            assert after["standby"] is True, after
            assert after["vcpuExits"] == before["vcpuExits"], (before, after)
            assert after["wakes"] == before["wakes"], (before, after)
            return {"seconds_in_standby": 12, "vcpu_exits_before": before["vcpuExits"],
                    "vcpu_exits_after": after["vcpuExits"]}
        case("standby stops the guest: no vCPU exits while it is left alone", stops)

        def wakes_on_a_command():
            wakes = status_of(sandbox)["wakes"]
            count = int(execute(sandbox, "cat /tmp/count"))
            after = status_of(sandbox)
            assert after["standby"] is False and after["wakes"] == wakes + 1, after
            # Twelve seconds at ten a second would be 120; a stopped guest adds a few.
            advanced = count - state["count"]
            assert 0 <= advanced < 30, (state["count"], count)
            time.sleep(1)
            later = int(execute(sandbox, "cat /tmp/count"))
            assert later > count, (count, later)
            return {"counter_before": state["count"], "counter_on_wake": count, "counter_a_second_later": later,
                    "node_wake_micros": round(after["lastWakeMicros"], 1)}
        case("the next command wakes it, and the guest carries on from where it stopped", wakes_on_a_command)

        def wakes_on_its_own_port():
            execute(sandbox, "mkdir -p /tmp/www && echo from-the-guest > /tmp/www/index.html && "
                             "(cd /tmp/www && /bin/busybox httpd -f -p 8080 >/dev/null 2>&1 &) ; sleep 0.5")
            assert through_proxy(sandbox, 8080) == (200, "from-the-guest")
            wakes = standby(sandbox)["wakes"]
            assert through_proxy(sandbox, 8080) == (200, "from-the-guest")
            after = status_of(sandbox)
            assert after["standby"] is False and after["wakes"] == wakes + 1, after
            return {"node_wake_micros": round(after["lastWakeMicros"], 1)}
        case("a request to one of its ports through the proxy wakes it too", wakes_on_its_own_port)

        def pauses_from_standby():
            execute(sandbox, "echo kept > /tmp/kept")
            standby(sandbox)
            status, value = request("POST", f"/sandboxes/{sandbox}/pause", {})
            assert status == 204, (status, value)
            assert request("GET", f"/sandboxes/{sandbox}/standby")[0] == 404
            status, value = request("POST", f"/sandboxes/{sandbox}/resume", {"timeout": 900})
            assert status == 201, (status, value)
            assert execute(sandbox, "cat /tmp/kept") == "kept"
            assert status_of(sandbox)["standby"] is False
            return {"pause": 204, "resume": 201}
        case("a sandbox in standby can be paused to disk and resumed", pauses_from_standby)

        def measured():
            awake, asleep, node = [], [], []
            for _ in range(args.rounds):
                started = time.perf_counter()
                execute(sandbox, "true")
                awake.append((time.perf_counter() - started) * 1000)
                standby(sandbox)
                started = time.perf_counter()
                execute(sandbox, "true")
                asleep.append((time.perf_counter() - started) * 1000)
                node.append(status_of(sandbox)["lastWakeMicros"])
            return {"command_ms_awake": summary(awake), "command_ms_from_standby": summary(asleep),
                    "node_wake_micros": summary(node), "host_load_1m": os.getloadavg()[0]}
        case("timings: a command to an awake sandbox and to one in standby, alternately", measured)

        def idles_into_standby():
            # No request for longer than the window; the guest's counter loop is light.
            wakes = status_of(sandbox)["wakes"]
            deadline = time.monotonic() + 90
            while not status_of(sandbox)["standby"]:
                assert time.monotonic() < deadline, "it never went into standby by itself"
                time.sleep(2)
            assert execute(sandbox, "cat /tmp/kept") == "kept"
            after = status_of(sandbox)
            assert after["standby"] is False and after["wakes"] == wakes + 1, after
            return {"window_seconds": 30}
        case("a sandbox left idle goes into standby by itself, and wakes the same way", idles_into_standby)
        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.terminate()
            try:
                daemon.wait(timeout=20)
            except subprocess.TimeoutExpired:
                daemon.kill()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
