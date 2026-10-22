#!/usr/bin/env python3
"""A sandbox of a size of its own is checkpointed and restored, on real KVM.

Runs an owned hv2-sandboxd whose template is one vCPU and 256 MiB, and
checks through its API and from inside each guest:

1. A sandbox created at two vCPUs and 1 GiB is checkpointed (it used to be
   refused), changed, and restored: its guest has the two vCPUs and the
   memory it had, and what was in its memory when it was checkpointed.
2. A sandbox at its template's size is checkpointed, resized, and restored:
   the restore is at the size of the checkpoint, not the size the sandbox
   had become, and the record says so.
3. A sized sandbox is checkpointed, resized to another size, and restored:
   the size of the checkpoint again.
4. The files of a sized checkpoint are read (the reader boots at the size
   the checkpoint was taken at).
5. A sized sandbox is snapshotted into a template (it used to be refused);
   a sandbox started from it has that size and the memory it was taken with,
   and a different size asked on the create is refused.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.parse, urllib.request
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
              "purpose": "functional verification of checkpointing a sandbox of its own size on KVM; the times "
                         "are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-sized-checkpoint-check-", dir="/var/tmp"))
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
        began = time.monotonic()
        detail = action()
        row["seconds"] = round(time.monotonic() - began, 3)
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

    def create(**size):
        status, made = request("POST", "/sandboxes", dict({"templateID": "base", "timeout": 1200}, **size))
        assert status in (200, 201), (status, made)
        return made["sandboxID"]

    def checkpoint(sandbox, name):
        began = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": name})
        assert status in (200, 201, 204), (name, status, value)
        return round(time.monotonic() - began, 3)

    def restore(sandbox, name):
        began = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints/{name}/restore", {})
        assert status in (200, 201, 204), (name, status, value)
        return round(time.monotonic() - began, 3)

    def resize(sandbox, **size):
        status, value = request("POST", f"/sandboxes/{sandbox}/resize", size)
        assert status == 200, (status, value)

    def big(mem_kb, mb):
        return mb * 1024 * 0.75 < mem_kb <= mb * 1024

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

        def sized_round_trip():
            sandbox = create(cpuCount=2, memoryMB=1024)
            state["a"] = sandbox
            cpus, mem_kb = seen(sandbox)
            assert cpus == 2 and big(mem_kb, 1024), (cpus, mem_kb)
            sh(sandbox, "echo before > /tmp/mark")
            took = checkpoint(sandbox, "sized")
            sh(sandbox, "echo after > /tmp/mark")
            restored = restore(sandbox, "sized")
            cpus, mem_kb = seen(sandbox)
            assert cpus == 2 and big(mem_kb, 1024), (cpus, mem_kb)
            assert record(sandbox) == (2, 1024), record(sandbox)
            assert sh(sandbox, "cat /tmp/mark") == "before"
            return {"checkpoint_seconds": took, "restore_seconds": restored}
        case("a sandbox of its own size is checkpointed and restored at it", sized_round_trip)

        def resized_after_base():
            sandbox = create()
            state["b"] = sandbox
            assert seen(sandbox)[0] == 1 and record(sandbox) == (1, 256)
            sh(sandbox, "echo small > /tmp/mark")
            checkpoint(sandbox, "base")
            resize(sandbox, cpuCount=2, memoryMB=1024)
            assert seen(sandbox)[0] == 2 and record(sandbox) == (2, 1024)
            restore(sandbox, "base")
            cpus, mem_kb = seen(sandbox)
            assert cpus == 1 and big(mem_kb, 256), (cpus, mem_kb)
            assert record(sandbox) == (1, 256), record(sandbox)
            assert sh(sandbox, "cat /tmp/mark") == "small"
            return {"seen": {"cpus": cpus, "mem_kb": mem_kb}}
        case("a restore is at the checkpoint's size, not the size the sandbox became", resized_after_base)

        def sized_then_resized():
            sandbox = state["a"]
            resize(sandbox, cpuCount=1, memoryMB=512)
            assert record(sandbox) == (1, 512) and seen(sandbox)[0] == 1
            restore(sandbox, "sized")
            cpus, mem_kb = seen(sandbox)
            assert cpus == 2 and big(mem_kb, 1024), (cpus, mem_kb)
            assert record(sandbox) == (2, 1024)
            assert sh(sandbox, "cat /tmp/mark") == "before"
            return {"seen": {"cpus": cpus, "mem_kb": mem_kb}}
        case("a sized checkpoint is restored at its size after another resize", sized_then_resized)

        def files_of_a_sized_checkpoint():
            sandbox = state["a"]
            query = urllib.parse.urlencode({"path": "/tmp/mark"})
            began = time.monotonic()
            status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/sized/file?{query}")
            took = round(time.monotonic() - began, 3)
            assert status == 200, (status, value)
            text = value if isinstance(value, str) else json.dumps(value)
            assert "before" in text, text
            return {"read_seconds": took}
        case("the files of a sized checkpoint are read at its size", files_of_a_sized_checkpoint)

        def snapshot_of_a_sized_sandbox():
            sandbox = create(cpuCount=2, memoryMB=1024)
            sh(sandbox, "echo snapped > /tmp/mark")
            began = time.monotonic()
            status, value = request("POST", f"/sandboxes/{sandbox}/snapshots", {"name": "sized-snap"})
            assert status in (200, 201), (status, value)
            took = round(time.monotonic() - began, 3)
            # A sandbox started from it is that size, with that memory.
            began = time.monotonic()
            status, made = request("POST", "/sandboxes", {"templateID": "sized-snap", "timeout": 1200})
            assert status in (200, 201), (status, made)
            started = round(time.monotonic() - began, 3)
            child = made["sandboxID"]
            cpus, mem_kb = seen(child)
            assert cpus == 2 and big(mem_kb, 1024), (cpus, mem_kb)
            assert record(child) == (2, 1024), record(child)
            assert sh(child, "cat /tmp/mark") == "snapped"
            # A size that is not the snapshot's is refused.
            status, value = request("POST", "/sandboxes",
                                    {"templateID": "sized-snap", "cpuCount": 1, "memoryMB": 256})
            assert status == 400, (status, value)
            # And it is a size of its own: it can be checkpointed.
            checkpoint(child, "child")
            return {"snapshot_seconds": took, "start_seconds": started, "refused": value}
        case("a sized sandbox is snapshotted into a template that starts sandboxes of its size",
             snapshot_of_a_sized_sandbox)

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
