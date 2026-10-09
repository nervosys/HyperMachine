#!/usr/bin/env python3
"""Fork a real KVM sandbox together with the block disk it holds.

Runs an owned hv2-sandboxd with a template and a --disk-dir, and checks,
through its API:

1. A sandbox holding a disk forks into two. Each fork holds a new disk of its
   own, named after the source's.
2. Each fork is the source as it was: the same boot, a file on its RAM root, a
   running process, the disk mounted, and on it both a synced file and one
   written just before the fork with no sync, by SHA-256.
3. The source ran on: same boot, its process still counting.
4. The three are independent afterwards: what the source or a fork writes to
   its disk, the others do not see.
5. A fork pauses and resumes like any sandbox with a disk.
6. A fork's disk outlives it: once the fork ends, a new sandbox attaches that
   disk by name and reads what the fork had and what it wrote.
7. More forks than the limit in one request is refused, and nothing is left
   behind.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, socket, subprocess, tempfile, time
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
    report = {"success": False, "purpose": "functional verification of forking a sandbox with its disk on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-disk-fork-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--disk-dir", str(work / "disks"),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

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

    def execute(sandbox, command):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, f"guest command failed: {status} {value}"
        return value["stdout"].strip()

    def create(**extra):
        status, value = request("POST", "/sandboxes",
                                {"timeout": 900, "diskMount": {"name": "data", "path": "/data"}, **extra})
        assert status == 201, (status, value)
        return value["sandboxID"]

    def pause(sandbox):
        status, value = request("POST", f"/sandboxes/{sandbox}/pause", {})
        assert status == 204, (status, value)

    def resume(sandbox):
        status, value = request("POST", f"/sandboxes/{sandbox}/resume", {"timeout": 900})
        assert status == 201, (status, value)

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

        status, disk = request("POST", "/disks", {"name": "data", "sizeMiB": 64})
        assert status == 201, (status, disk)
        source = create()
        execute(source, "dd if=/dev/urandom of=/data/synced bs=1M count=4 2>/dev/null && sync && "
                        "echo in-ram > /tmp/ram-file")
        execute(source, "(i=0; while true; do i=$((i+1)); echo $i > /tmp/count; sleep 0.1; done) "
                        ">/dev/null 2>&1 &")
        time.sleep(1)
        sha = "sha256sum {} | cut -d' ' -f1"
        state = {}

        def disks():
            status, listed = request("GET", "/disks")
            assert status == 200, (status, listed)
            return {d["name"]: d.get("attachedTo") for d in listed}

        def forks():
            state["boot"] = execute(source, "cat /proc/sys/kernel/random/boot_id")
            state["synced"] = execute(source, sha.format("/data/synced"))
            # No sync: in the source's page cache when the fork begins.
            state["unsynced"] = execute(source, "dd if=/dev/urandom of=/data/unsynced bs=1M count=4 "
                                                "2>/dev/null; " + sha.format("/data/unsynced"))
            state["dirty"] = execute(source, "grep '^Dirty:' /proc/meminfo | tr -s ' '")
            state["count"] = int(execute(source, "cat /tmp/count"))
            started = time.monotonic()
            status, forked = request("POST", f"/sandboxes/{source}/fork", {"count": 2, "timeout": 900})
            took = round(time.monotonic() - started, 3)
            assert status == 201 and len(forked) == 2, (status, forked)
            assert all("sandbox" in f for f in forked), forked
            state["forks"] = [f["sandbox"]["sandboxID"] for f in forked]
            held = disks()
            assert held["data"] == source, held
            copies = {name: holder for name, holder in held.items() if name != "data"}
            assert len(copies) == 2 and all(name.startswith("data-fork-") for name in copies), held
            assert sorted(copies.values()) == sorted(state["forks"]), held
            state["copies"] = {holder: name for name, holder in copies.items()}
            return {"forks": state["forks"], "disks": held, "source_dirty_before_fork": state["dirty"],
                    "fork_request_seconds": took}
        case("a sandbox holding a disk forks, and each fork holds a disk of its own", forks)

        def each_is_the_source_as_it_was():
            seen = {}
            for fork in state["forks"]:
                assert execute(fork, "cat /proc/sys/kernel/random/boot_id") == state["boot"]
                assert execute(fork, "cat /tmp/ram-file") == "in-ram"
                mounted = execute(fork, "grep ' /data ' /proc/mounts")
                assert "ext4" in mounted and "/dev/vda" in mounted, mounted
                assert execute(fork, sha.format("/data/synced")) == state["synced"]
                assert execute(fork, sha.format("/data/unsynced")) == state["unsynced"]
                count = int(execute(fork, "cat /tmp/count"))
                time.sleep(0.5)
                assert int(execute(fork, "cat /tmp/count")) > count >= state["count"]
                seen[fork] = {"counter": count}
            return {"synced_sha256": state["synced"], "unsynced_sha256": state["unsynced"], "forks": seen}
        case("each fork is the source as it was, unsynced writes included", each_is_the_source_as_it_was)

        def the_source_ran_on():
            assert execute(source, "cat /proc/sys/kernel/random/boot_id") == state["boot"]
            count = int(execute(source, "cat /tmp/count"))
            time.sleep(0.5)
            assert int(execute(source, "cat /tmp/count")) > count > state["count"]
            assert execute(source, sha.format("/data/unsynced")) == state["unsynced"]
            return {"counter_before_fork": state["count"], "counter_after": count}
        case("the source ran on", the_source_ran_on)

        def independent():
            first, second = state["forks"]
            execute(source, "echo source > /data/who && sync")
            execute(first, "echo first > /data/who && echo only-first > /data/only-first && sync")
            execute(second, "echo second > /data/who && sync")
            assert execute(source, "cat /data/who") == "source"
            assert execute(first, "cat /data/who") == "first"
            assert execute(second, "cat /data/who") == "second"
            absent = "ls /data/only-first 2>/dev/null || echo absent"
            assert execute(source, absent) == "absent" and execute(second, absent) == "absent"
            # And the unchanged file is still whole on all three.
            for sandbox in (source, first, second):
                assert execute(sandbox, sha.format("/data/synced")) == state["synced"]
            return {"who": {"source": "source", "first": "first", "second": "second"}}
        case("source and forks are independent afterwards", independent)

        def a_fork_pauses():
            first = state["forks"][0]
            status, value = request("POST", f"/sandboxes/{first}/pause", {})
            assert status == 204, (status, value)
            status, value = request("POST", f"/sandboxes/{first}/resume", {"timeout": 900})
            assert status == 201, (status, value)
            assert execute(first, "cat /data/who") == "first"
            return {"pause": 204, "resume": 201}
        case("a fork pauses and resumes like any sandbox with a disk", a_fork_pauses)

        def a_forks_disk_outlives_it():
            first = state["forks"][0]
            name = state["copies"][first]
            status, _ = request("DELETE", f"/sandboxes/{first}")
            assert status == 204, status
            assert disks()[name] is None
            status, value = request("POST", "/sandboxes",
                                    {"timeout": 600, "diskMount": {"name": name, "path": "/mnt/fork"}})
            assert status == 201, (status, value)
            reader = value["sandboxID"]
            assert execute(reader, "cat /mnt/fork/who /mnt/fork/only-first") == "first\nonly-first"
            assert execute(reader, sha.format("/mnt/fork/unsynced")) == state["unsynced"]
            request("DELETE", f"/sandboxes/{reader}")
            return {"disk": name, "read_by": reader}
        case("a fork's disk outlives it and attaches to a new sandbox by name", a_forks_disk_outlives_it)

        def too_many_is_refused():
            before = disks()
            status, value = request("POST", f"/sandboxes/{source}/fork", {"count": 9})
            assert status == 400, (status, value)
            assert disks() == before, "a refused fork left a disk behind"
            assert execute(source, "cat /data/who") == "source"
            return {"status": status, "message": value["message"]}
        case("more forks than the limit is refused, and nothing is left behind", too_many_is_refused)
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
