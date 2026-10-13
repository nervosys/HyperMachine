#!/usr/bin/env python3
"""A sandbox's size changed after it was created, on real KVM.

Runs an owned hv2-sandboxd whose template is one vCPU and 256 MiB, with a
disk directory, and checks through its API and from inside each guest:

1. `resize` of a running sandbox reboots it at the new size: the same
   sandbox, its guest counting the new processors and memory, its record
   saying so, its variables kept, and what was only in its memory gone.
2. A sandbox's disk is kept across a resize.
3. Asking for the size it has changes nothing: no reboot, its memory kept.
4. Resized back to its template's size, it is restored from the template,
   and pauses and resumes with its memory, as any sandbox that size does.
5. `resume` with another size brings a paused sandbox back at that size,
   booted: its variables kept, its memory not. Paused and resumed again
   without one, it keeps that size and its memory.
6. Its usage counts each size for the time it ran at it.
7. What cannot be done is refused, each with why: no size, a size no guest
   here can be, no such sandbox, a paused one, and a resume at a size that
   cannot be, which leaves it paused and resumable.

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
              "purpose": "functional verification of changing a sandbox's size on KVM; the times are the "
                         "check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-sandbox-resize-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--disk-dir", str(work / "disks"),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "6"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

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

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def sh(sandbox, cmd):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, (status, value)
        return value["stdout"].strip()

    def seen(sandbox):
        cpus, mem = sh(sandbox, "grep -c ^processor /proc/cpuinfo; grep MemTotal /proc/meminfo").splitlines()
        return int(cpus), int(mem.split()[1])

    def record(sandbox):
        status, value = request("GET", f"/sandboxes/{sandbox}")
        assert status == 200, (status, value)
        return value.get("cpuCount"), value.get("memoryMB")

    def resize(sandbox, want=200, **size):
        began = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/resize", size)
        assert status == want, (status, value)
        return value, round(time.monotonic() - began, 3)

    def act(sandbox, action, body=None, want=(200, 201, 204)):
        began = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/{action}", body or {})
        assert status in want, (action, status, value)
        return value, round(time.monotonic() - began, 3)

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

        def resized_up():
            status, made = request("POST", "/sandboxes",
                                   {"templateID": "base", "timeout": 1200, "envVars": {"KEEP": "kept"}})
            assert status in (200, 201), (status, made)
            sandbox = made["sandboxID"]
            state["a"] = sandbox
            assert seen(sandbox)[0] == 1 and record(sandbox) == (1, 256)
            sh(sandbox, "echo here > /tmp/in-memory")
            value, took = resize(sandbox, cpuCount=2, memoryMB=1024)
            assert value == {"sandboxID": sandbox, "cpuCount": 2, "memoryMB": 1024, "resized": True}, value
            cpus, mem_kb = seen(sandbox)
            assert cpus == 2 and 900 * 1024 < mem_kb <= 1024 * 1024, (cpus, mem_kb)
            assert record(sandbox) == (2, 1024), record(sandbox)
            assert sh(sandbox, "echo $KEEP") == "kept"
            assert sh(sandbox, "cat /tmp/in-memory 2>/dev/null || echo gone") == "gone"
            # Still the one sandbox, listed once.
            listed = [s["sandboxID"] for s in request("GET", "/sandboxes")[1]]
            assert listed.count(sandbox) == 1, listed
            return {"processors": cpus, "memory_kb": mem_kb, "seconds": took}
        case("resize reboots a running sandbox at its new size, keeping its variables, not its memory", resized_up)

        def disk_kept():
            status, disk = request("POST", "/disks", {"name": "keeps", "sizeMiB": 64})
            assert status in (200, 201), (status, disk)
            status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 600,
                                                          "diskMount": {"name": "keeps", "path": "/data"}})
            assert status in (200, 201), (status, made)
            sandbox = made["sandboxID"]
            sh(sandbox, "echo on-disk > /data/kept; sync")
            value, took = resize(sandbox, cpuCount=2)
            assert value["resized"] and value["memoryMB"] == 256, value
            assert seen(sandbox)[0] == 2
            assert sh(sandbox, "cat /data/kept") == "on-disk"
            request("DELETE", f"/sandboxes/{sandbox}")
            return {"seconds": took}
        case("a sandbox's disk is kept across a resize", disk_kept)

        def unchanged():
            sandbox = state["a"]
            sh(sandbox, "echo still > /tmp/in-memory")
            value, took = resize(sandbox, cpuCount=2, memoryMB=1024)
            assert value["resized"] is False, value
            assert sh(sandbox, "cat /tmp/in-memory") == "still"
            return {"seconds": took}
        case("asking for the size it has changes nothing, and keeps its memory", unchanged)

        def back_down():
            sandbox = state["a"]
            value, took = resize(sandbox, cpuCount=1, memoryMB=256)
            assert value["resized"] is True, value
            cpus, mem_kb = seen(sandbox)
            assert cpus == 1 and mem_kb < 256 * 1024, (cpus, mem_kb)
            # At its template's size it was restored from the template, so it
            # pauses as such a sandbox does, and keeps its memory.
            sh(sandbox, "echo through-a-pause > /tmp/in-memory")
            act(sandbox, "pause")
            act(sandbox, "resume")
            assert sh(sandbox, "cat /tmp/in-memory") == "through-a-pause"
            assert record(sandbox) == (1, 256)
            return {"seconds": took}
        case("back at its template's size it is restored, and pauses and resumes with its memory", back_down)

        def resumed_bigger():
            sandbox = state["a"]
            sh(sandbox, "echo before-pause > /tmp/in-memory")
            act(sandbox, "pause")
            value, took = act(sandbox, "resume", {"cpuCount": 2, "memoryMB": 512})
            cpus, mem_kb = seen(sandbox)
            assert cpus == 2 and 400 * 1024 < mem_kb <= 512 * 1024, (cpus, mem_kb)
            assert record(sandbox) == (2, 512), record(sandbox)
            assert sh(sandbox, "echo $KEEP") == "kept"
            assert sh(sandbox, "cat /tmp/in-memory 2>/dev/null || echo gone") == "gone"
            # Its own size now: a pause and resume without one keep it, and
            # its memory.
            sh(sandbox, "echo at-its-new-size > /tmp/in-memory")
            act(sandbox, "pause")
            act(sandbox, "resume")
            assert seen(sandbox)[0] == 2 and record(sandbox) == (2, 512)
            assert sh(sandbox, "cat /tmp/in-memory") == "at-its-new-size"
            return {"processors": cpus, "memory_kb": mem_kb, "seconds": took}
        case("resume at another size boots it at that size; resumed again, it keeps it", resumed_bigger)

        def usage():
            time.sleep(2)
            status, value = request("GET", "/usage?by=sandbox")
            assert status == 200, (status, value)
            mine = [g for g in value["groups"] if g["key"] == state["a"]][0]
            running = mine["runningSeconds"]
            # It ran at one vCPU and at two: more than one a second, fewer than two.
            assert running < mine["vcpuSeconds"] < 2 * running, mine
            return {"usage": mine}
        case("its usage counts each size for the time it ran at it", usage)

        def refusals():
            sandbox = state["a"]
            refused = {}
            for label, body, want in [
                ("no size", {}, 400),
                ("no processors", {"cpuCount": 0}, 400),
                ("more processors than the host", {"cpuCount": 4096}, 400),
                ("too little memory", {"memoryMB": 128}, 400),
                ("an odd amount", {"memoryMB": 1023}, 400),
            ]:
                value, _ = resize(sandbox, want=want, **body)
                refused[label] = value["message"]
            value, _ = resize("absent-sandbox", want=404, cpuCount=2)
            refused["no such sandbox"] = value["message"]
            act(sandbox, "pause")
            value, _ = resize(sandbox, want=409, cpuCount=1)
            assert "resume" in value["message"], value
            refused["a paused sandbox"] = value["message"]
            value, _ = act(sandbox, "resume", {"memoryMB": 100}, want=(400,))
            refused["resuming at a size that cannot be"] = value["message"]
            # Still paused, and still resumable.
            act(sandbox, "resume")
            assert record(sandbox) == (2, 512)
            request("DELETE", f"/sandboxes/{sandbox}")
            return {"refused": refused}
        case("what cannot be done is refused, each with why", refusals)

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
