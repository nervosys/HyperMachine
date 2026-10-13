#!/usr/bin/env python3
"""Forks of a sandbox at another size, on real KVM.

Runs an owned hv2-sandboxd whose template is one vCPU and 256 MiB, with a
disk directory, and checks through its API and from inside each guest:

1. `fork` with `cpuCount` and `memoryMB` starts forks at that size, booted:
   each its own sandbox, its guest counting the new processors and memory,
   its record saying so, the source's variables in it, and none of the
   source's memory. The source runs on, its memory untouched.
2. A fork with no size is still its source's size, with its memory.
3. A fork at its template's size, of a source that is not, is restored from
   the template, and pauses and resumes with its memory.
4. A fork of a sandbox with a disk, at another size, has its own copy of
   the disk as it was.
5. A fork's usage is counted at its own size.
6. A size that cannot be is refused, and no fork is made.

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
              "purpose": "functional verification of forking a sandbox at another size on KVM; the times are "
                         "the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-fork-size-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--disk-dir", str(work / "disks"),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "10"],
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

    def act(sandbox, action, body=None, want=(200, 201, 204)):
        began = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/{action}", body or {})
        assert status in want, (action, status, value)
        return value, round(time.monotonic() - began, 3)

    def fork(sandbox, **body):
        value, took = act(sandbox, "fork", dict(body, timeout=600))
        assert all("sandbox" in one for one in value), value
        return [one["sandbox"]["sandboxID"] for one in value], took

    def create(**body):
        status, made = request("POST", "/sandboxes", dict({"templateID": "base", "timeout": 1200}, **body))
        assert status in (200, 201), (status, made)
        return made["sandboxID"]

    def gone(*sandboxes):
        for sandbox in sandboxes:
            request("DELETE", f"/sandboxes/{sandbox}")

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

        def bigger():
            source = create(envVars={"KEEP": "kept"})
            state["source"] = source
            sh(source, "echo source-memory > /tmp/in-memory")
            forks, took = fork(source, count=2, cpuCount=2, memoryMB=512)
            assert len(set(forks)) == 2 and source not in forks, forks
            seen_by = {}
            for one in forks:
                cpus, mem_kb = seen(one)
                assert cpus == 2 and 400 * 1024 < mem_kb <= 512 * 1024, (cpus, mem_kb)
                assert record(one) == (2, 512), record(one)
                assert sh(one, "echo $KEEP") == "kept"
                assert sh(one, "cat /tmp/in-memory 2>/dev/null || echo gone") == "gone"
                seen_by[one] = {"processors": cpus, "memory_kb": mem_kb}
            # The source runs on at its own size, its memory as it was.
            assert seen(source)[0] == 1 and record(source) == (1, 256)
            assert sh(source, "cat /tmp/in-memory") == "source-memory"
            state["big"] = forks[0]
            gone(forks[1])
            return {"forks": seen_by, "seconds": took}
        case("forks at another size boot at it, with the source's variables and not its memory", bigger)

        def same_size():
            source = state["source"]
            (one,), took = fork(source)
            assert record(one) == (1, 256) and seen(one)[0] == 1
            assert sh(one, "cat /tmp/in-memory") == "source-memory"
            gone(one)
            return {"seconds": took}
        case("a fork with no size is its source's size, with its memory", same_size)

        def template_size():
            # A source of another size, forked at the template's.
            source = state["big"]
            sh(source, "echo big-memory > /tmp/in-memory")
            (one,), took = fork(source, cpuCount=1, memoryMB=256)
            cpus, mem_kb = seen(one)
            assert cpus == 1 and mem_kb < 256 * 1024, (cpus, mem_kb)
            assert record(one) == (1, 256)
            assert sh(one, "echo $KEEP") == "kept"
            assert sh(one, "cat /tmp/in-memory 2>/dev/null || echo gone") == "gone"
            # Restored from the template, so it pauses as such a sandbox
            # does, and keeps its memory.
            sh(one, "echo through-a-pause > /tmp/in-memory")
            act(one, "pause")
            act(one, "resume")
            assert sh(one, "cat /tmp/in-memory") == "through-a-pause"
            gone(one)
            return {"seconds": took}
        case("a fork at its template's size is restored from it, and pauses with its memory", template_size)

        def with_disk():
            status, disk = request("POST", "/disks", {"name": "forked", "sizeMiB": 64})
            assert status in (200, 201), (status, disk)
            source = create(diskMount={"name": "forked", "path": "/data"})
            sh(source, "echo on-disk > /data/kept; sync")
            (one,), took = fork(source, cpuCount=2)
            assert seen(one)[0] == 2 and record(one) == (2, 256)
            assert sh(one, "cat /data/kept") == "on-disk"
            # Its own copy: what the fork writes, the source does not see.
            sh(one, "echo fork-only > /data/kept; sync")
            assert sh(source, "cat /data/kept") == "on-disk"
            gone(one, source)
            return {"seconds": took}
        case("a fork of a sandbox with a disk, at another size, has its own copy of the disk", with_disk)

        def usage():
            one = state["big"]
            time.sleep(2)
            status, value = request("GET", "/usage?by=sandbox")
            assert status == 200, (status, value)
            mine = [g for g in value["groups"] if g["key"] == one][0]
            # Two vCPUs for every second it ran.
            assert abs(mine["vcpuSeconds"] - 2 * mine["runningSeconds"]) < 0.05 * mine["vcpuSeconds"], mine
            return {"usage": mine}
        case("a fork's usage is counted at its own size", usage)

        def refusals():
            source = state["source"]
            before = len(request("GET", "/sandboxes")[1])
            refused = {}
            for label, body in [("no processors", {"cpuCount": 0}),
                                ("more processors than the host", {"cpuCount": 4096}),
                                ("too little memory", {"memoryMB": 128}),
                                ("an odd amount", {"memoryMB": 1023})]:
                value, _ = act(source, "fork", body, want=(400,))
                refused[label] = value["message"]
            assert len(request("GET", "/sandboxes")[1]) == before
            gone(source, state["big"])
            return {"refused": refused}
        case("a size that cannot be is refused, and no fork is made", refusals)

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
