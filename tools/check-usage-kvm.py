#!/usr/bin/env python3
"""Usage on real KVM: what sandboxes ran, added up from their lifecycle.

Runs an owned hv2-sandboxd with a template, makes sandboxes do known things
for known lengths of time, and checks `GET /usage` against a stopwatch held
by this script:

1. A sandbox that runs, is paused, is resumed and is deleted is counted for
   the time it ran and not the time it was paused.
2. A sandbox still running is counted to the moment asked.
3. A sandbox deleted while paused adds nothing after its pause.
4. vCPU-seconds and memory GiB-seconds are its size times its running time.
5. A window cuts what it covers; grouped by template, the groups add up to
   the total.
6. The usage ledger is not in what an E2B client or a webhook is given.
7. What cannot be asked is refused.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path


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
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of usage figures on KVM; durations are the check's own "
                         "stopwatch and are not a timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-usage-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=180):
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

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def act(method, path, want=(200, 201, 204)):
        """Do it, and when: the moments either side of the request."""
        before = time.time()
        body = {"templateID": "base", "timeout": 600} if path == "/sandboxes" else {} if method == "POST" else None
        status, value = request(method, path, body)
        after = time.time()
        assert status in want, (method, path, status, value)
        return value, before, after

    def usage(query=""):
        status, value = request("GET", "/usage" + query)
        assert status == 200, (status, value)
        return value

    def group(answer, key):
        found = [g for g in answer["groups"] if g["key"] == key]
        assert len(found) == 1, (key, answer["groups"])
        return found[0]

    def within(seconds, low, high, what):
        # The node's clock stops and starts inside each request; the
        # stopwatch's bounds are either side of it.
        assert low - 0.05 <= seconds <= high + 0.05, f"{what}: {seconds} not in [{low:.3f}, {high:.3f}]"

    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        state = {"t0": time.time()}

        def paused_time_is_not_counted():
            # A: runs ~4 s, paused ~4 s, resumed and runs ~3 s, deleted.
            a, a_made0, a_made1 = act("POST", "/sandboxes")
            a_id = a["sandboxID"]
            time.sleep(4)
            _, p0, p1 = act("POST", f"/sandboxes/{a_id}/pause")
            time.sleep(4)
            _, r0, r1 = act("POST", f"/sandboxes/{a_id}/resume")
            time.sleep(3)
            _, d0, d1 = act("DELETE", f"/sandboxes/{a_id}")
            # It ran from its creation to its pause, and from its resume to
            # its delete; the node's moments are inside each request.
            low = max(0, p0 - a_made1) + max(0, d0 - r1)
            high = (p1 - a_made0) + (d1 - r0)
            answer = usage()
            got = group(answer, a_id)
            within(got["runningSeconds"], low, high, "sandbox A")
            # Had the pause counted, it would be near 11 s.
            assert got["runningSeconds"] < high, got
            state["a"] = (a_id, got)
            state["size"] = (a.get("cpuCount"), a.get("memoryMB"))
            return {"sandbox": a_id, "runningSeconds": got["runningSeconds"],
                    "stopwatch": [round(low, 3), round(high, 3)], "paused_for": round(r0 - p1, 3)}
        case("run, pause, resume, delete: the time it ran, not the time it was paused", paused_time_is_not_counted)

        def size_times_time():
            a_id, got = state["a"]
            status, events = request("GET", f"/events/sandboxes/{a_id}")
            killed = [e for e in events if e["type"] == "sandbox.lifecycle.killed"][0]
            execution = killed["eventData"]["execution"]
            vcpus, memory_mb = execution["vcpu_count"], execution["memory_mb"]
            assert abs(got["vcpuSeconds"] - vcpus * got["runningSeconds"]) < 1e-6, (got, vcpus)
            assert abs(got["memoryGiBSeconds"] - memory_mb / 1024 * got["runningSeconds"]) < 1e-6, (got, memory_mb)
            return {"vcpus": vcpus, "memoryMB": memory_mb, "vcpuSeconds": got["vcpuSeconds"],
                    "memoryGiBSeconds": got["memoryGiBSeconds"]}
        case("vCPU-seconds and GiB-seconds are its size times its running time", size_times_time)

        def still_running():
            b, b0, b1 = act("POST", "/sandboxes")
            state["b"] = b["sandboxID"]
            time.sleep(3)
            q0 = time.time()
            first = group(usage(), state["b"])["runningSeconds"]
            q1 = time.time()
            within(first, q0 - b1, q1 - b0, "sandbox B, first")
            time.sleep(2)
            second = group(usage(), state["b"])["runningSeconds"]
            assert second >= first + 1.9, (first, second)
            return {"first": first, "second": second}
        case("a sandbox still running is counted to the moment asked", still_running)

        def deleted_while_paused():
            c, c0, c1 = act("POST", "/sandboxes")
            c_id = c["sandboxID"]
            time.sleep(2)
            _, p0, p1 = act("POST", f"/sandboxes/{c_id}/pause")
            at_pause = group(usage(), c_id)["runningSeconds"]
            time.sleep(3)
            act("DELETE", f"/sandboxes/{c_id}")
            time.sleep(1)
            after = group(usage(), c_id)["runningSeconds"]
            assert after == at_pause, (at_pause, after)
            within(after, max(0, p0 - c1), p1 - c0, "sandbox C")
            return {"runningSeconds": after}
        case("a sandbox deleted while paused adds nothing after its pause", deleted_while_paused)

        def windows_and_groups():
            whole = usage(f"?from={int(state['t0'] * 1000) - 1000}")
            by_template = usage(f"?from={int(state['t0'] * 1000) - 1000}&by=template")
            assert [g["key"] for g in by_template["groups"]] == ["base"], by_template["groups"]
            assert abs(by_template["groups"][0]["runningSeconds"] - by_template["total"]["runningSeconds"]) < 1e-6
            assert whole["total"]["sandboxes"] == 3, whole["total"]
            per_sandbox = sum(g["runningSeconds"] for g in whole["groups"])
            assert abs(per_sandbox - whole["total"]["runningSeconds"]) < 0.01, (per_sandbox, whole["total"])
            # Before anything ran: nothing.
            empty = usage(f"?from={int(state['t0'] * 1000) - 60000}&to={int(state['t0'] * 1000) - 30000}")
            assert empty["total"]["sandboxes"] == 0 and empty["groups"] == [], empty
            # A window of the last second holds only B, still running.
            now = int(time.time() * 1000)
            last = usage(f"?from={now - 1000}&to={now}")
            assert [g["key"] for g in last["groups"]] == [state["b"]], last["groups"]
            assert last["total"]["runningSeconds"] <= 1.1, last["total"]
            assert whole["truncated"] is False and whole["coveredFrom"], whole
            return {"total": whole["total"], "coveredFrom": whole["coveredFrom"]}
        case("a window cuts what it covers, and groups add up to the total", windows_and_groups)

        def ledger_not_given_out():
            a_id, _ = state["a"]
            status, events = request("GET", "/events/sandboxes?limit=1000")
            assert status == 200 and events, (status, events)
            assert "hm_usage" not in json.dumps(events), "the ledger is in E2B's events"
            return {"events": len(events)}
        case("the usage ledger is not in what an E2B client is given", ledger_not_given_out)

        def refusals():
            results = {}
            for label, query in [("a time that is not one", "?from=yesterday"),
                                 ("from after to", "?from=2000&to=1000"),
                                 ("a grouping that is not one", "?by=colour")]:
                status, value = request("GET", "/usage" + query)
                assert status == 400, (label, status, value)
                results[label] = value["message"] if isinstance(value, dict) else value[:160]
            return results
        case("what cannot be asked is refused", refusals)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
