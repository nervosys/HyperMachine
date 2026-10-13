#!/usr/bin/env python3
"""A guest's processes listed from outside, on real KVM.

Runs an owned hv2-sandboxd with a template and a --machine-dir, one sandbox
and one machine, and checks through its API:

1. A sandbox's processes: one spinning is `R` with CPU time, one sleeping
   is `S` with its command line, a zombie is `Z`; init is PID 1; and none
   of it was started through the API's runs, only by a shell in the guest.
2. Kernel threads only with `?all=true`, and never the listing itself.
3. A process killed is gone from the next listing.
4. A machine's processes, the same way, with memory counted.
5. What cannot be listed is refused: a paused sandbox, a missing one, a
   stopped machine.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

MACHINE = "ps-01"


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
              "purpose": "functional verification of listing a guest's processes on KVM; the times are the "
                         "check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-guest-processes-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6"],
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

    def sh(path, cmd):
        status, value = request("POST", f"{path}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, (status, value)
        return value["stdout"]

    def ps(path, all=False, want=200):
        began = time.monotonic()
        status, value = request("GET", f"{path}/processes" + ("?all=true" if all else ""))
        assert status == want, (status, value)
        return value, round(time.monotonic() - began, 3)

    def by_args(listed, args):
        found = [p for p in listed if p["args"] == args]
        assert len(found) == 1, (args, [p["args"] for p in listed])
        return found[0]

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
        state = {}
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200})
        assert status in (200, 201), (status, made)
        sbx = f"/sandboxes/{made['sandboxID']}"
        state["sbx"] = sbx

        def sandbox_listed():
            sh(sbx, "setsid sh -c 'while :; do :; done' >/dev/null 2>&1 & "
                    "setsid sleep 4321 >/dev/null 2>&1 & "
                    "setsid sh -c 'sleep 0.1 & exec sleep 3333' >/dev/null 2>&1 &")
            time.sleep(1.5)
            value, took = ps(sbx)
            listed = value["processes"]
            spinner = by_args(listed, "sh -c while :; do :; done")
            assert spinner["state"] == "R" and spinner["cpuSeconds"] > 0.5, spinner
            sleeper = by_args(listed, "sleep 4321")
            assert sleeper["state"] == "S" and sleeper["command"] == "sleep", sleeper
            assert sleeper["uid"] == 0 and sleeper["startedSecondsAgo"] >= 1, sleeper
            parent = by_args(listed, "sleep 3333")
            zombies = [p for p in listed if p["ppid"] == parent["pid"]]
            assert len(zombies) == 1 and zombies[0]["state"] == "Z" and zombies[0]["args"] == "", zombies
            init = [p for p in listed if p["pid"] == 1]
            assert init and init[0]["ppid"] == 0, init
            state["spinner"] = spinner["pid"]
            return {"seconds": took, "count": len(listed),
                    "spinner": spinner, "sleeper": sleeper, "zombie": zombies[0]}
        case("a sandbox's processes: running, sleeping and zombie, with their command lines", sandbox_listed)

        def kernel_threads():
            plain, _ = ps(sbx)
            every, _ = ps(sbx, all=True)
            kernel = [p for p in every["processes"] if p["ppid"] == 2 or p["pid"] == 2]
            assert kernel and not [p for p in plain["processes"] if p["ppid"] == 2 or p["pid"] == 2]
            assert len(every["processes"]) == len(plain["processes"]) + len(kernel)
            for listed in (plain, every):
                assert not [p for p in listed["processes"] if "/proc/uptime" in p["args"]]
            return {"without": len(plain["processes"]), "with_kernel_threads": len(every["processes"])}
        case("kernel threads only with all, and never the listing itself", kernel_threads)

        def killed():
            sh(sbx, f"kill -9 {state['spinner']}")
            time.sleep(0.3)
            value, _ = ps(sbx)
            left = [p for p in value["processes"] if p["pid"] == state["spinner"]]
            # Gone, or a zombie until init reaps it: this guest's init is a
            # shell script, which reaps an orphan only when it next waits.
            assert not left or left[0]["state"] == "Z", left
            return {"after_kill": left[0]["state"] if left else "gone"}
        case("a process killed no longer runs: gone, or a zombie its init has not reaped", killed)

        def machine_listed():
            status, created = request("POST", "/machines", {
                "name": MACHINE, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, timeout=180)
            assert status == 201, (status, created)
            path = f"/machines/{MACHINE}"
            deadline = time.monotonic() + 120
            while request("POST", f"{path}/exec", {"cmd": "true"})[0] != 200:
                assert time.monotonic() < deadline, "machine agent"
                time.sleep(0.5)
            sh(path, "setsid sleep 5555 >/dev/null 2>&1 &")
            time.sleep(0.5)
            value, took = ps(path)
            sleeper = by_args(value["processes"], "sleep 5555")
            assert sleeper["rssKiB"] > 0 and sleeper["threads"] == 1, sleeper
            return {"seconds": took, "count": len(value["processes"]), "sleeper": sleeper}
        case("a machine's processes, the same way", machine_listed)

        def refusals():
            refused = {}
            status, value = request("POST", f"{sbx}/pause", {})
            assert status in (200, 204), (status, value)
            value, _ = ps(sbx, want=409)
            refused["a paused sandbox"] = value["message"]
            value, _ = ps("/sandboxes/sbx-absent", want=404)
            refused["no such sandbox"] = value["message"]
            status, value = request("POST", f"/machines/{MACHINE}/stop", {})
            assert status in (200, 204), (status, value)
            value, _ = ps(f"/machines/{MACHINE}", want=409)
            refused["a stopped machine"] = value["message"]
            return {"refused": refused}
        case("what cannot be listed is refused, each with why", refusals)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
