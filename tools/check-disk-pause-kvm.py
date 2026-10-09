#!/usr/bin/env python3
"""Pause and resume a real KVM sandbox that holds a block disk.

Runs an owned hv2-sandboxd with a template and a --disk-dir, and checks,
through its API:

1. A sandbox holding a disk pauses. While it is paused the disk stays
   claimed: another sandbox cannot attach it and it cannot be deleted.
2. Resumed, it is the same guest: a process it was running is still running,
   a file on its RAM root is still there, and the disk is still mounted with
   what was written to it.
3. A write made just before the pause, with no sync, is whole afterwards: an
   8 MiB random file's SHA-256, computed in the guest before the pause, is the
   same after the resume.
4. That data reached the disk itself: after a sync and the sandbox ending, a
   new sandbox that attaches the disk reads the same SHA-256.
5. It pauses and resumes a second time.
6. A sandbox created with autoPause and a disk is accepted, and ending a
   paused sandbox gives its disk back.
7. Forking a sandbox with a disk is still refused.

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
    report = {"success": False, "purpose": "functional verification of pausing a sandbox with a disk on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-disk-pause-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--disk-dir", str(work / "disks"),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
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
        disk_id = disk["diskID"]
        sandbox = create()
        execute(sandbox, "echo on-the-disk > /data/first && sync && echo in-ram > /tmp/ram-file")
        execute(sandbox, "(i=0; while true; do i=$((i+1)); echo $i > /tmp/count; sleep 0.1; done) "
                         ">/dev/null 2>&1 &")
        time.sleep(1)
        state = {}

        def pauses_and_keeps_its_claim():
            state["count"] = int(execute(sandbox, "cat /tmp/count"))
            state["boot"] = execute(sandbox, "cat /proc/sys/kernel/random/boot_id")
            pause(sandbox)
            status, held = request("GET", f"/disks/{disk_id}")
            assert status == 200 and held["attachedTo"] == sandbox, (status, held)
            status, other = request("POST", "/sandboxes",
                                    {"timeout": 60, "diskMount": {"name": "data", "path": "/data"}})
            assert status == 409, (status, other)
            status, deleted = request("DELETE", f"/disks/{disk_id}")
            assert status == 409, (status, deleted)
            return {"attached_to_while_paused": held["attachedTo"], "second_attach": other["message"]}
        case("a sandbox holding a disk pauses, and the disk stays claimed", pauses_and_keeps_its_claim)

        def resumes_as_the_same_guest():
            resume(sandbox)
            assert execute(sandbox, "cat /proc/sys/kernel/random/boot_id") == state["boot"], "a different boot"
            assert execute(sandbox, "cat /tmp/ram-file") == "in-ram"
            assert execute(sandbox, "cat /data/first") == "on-the-disk"
            mounted = execute(sandbox, "grep ' /data ' /proc/mounts")
            assert "ext4" in mounted and "/dev/vda" in mounted, mounted
            count = int(execute(sandbox, "cat /tmp/count"))
            time.sleep(1)
            later = int(execute(sandbox, "cat /tmp/count"))
            assert count >= state["count"] and later > count, (state["count"], count, later)
            execute(sandbox, "echo after-resume > /data/second")
            return {"counter_before": state["count"], "counter_after": count, "a_second_later": later,
                    "mount": mounted}
        case("resumed, it is the same guest with the disk still mounted", resumes_as_the_same_guest)

        def unflushed_writes_survive():
            # No sync: the file is in the guest's page cache when the pause begins.
            written = execute(sandbox, "dd if=/dev/urandom of=/data/blob bs=1M count=8 2>/dev/null; "
                                       "sha256sum /data/blob | cut -d' ' -f1")
            dirty = execute(sandbox, "grep -E '^(Dirty|Writeback):' /proc/meminfo | tr -s ' ' | tr '\\n' ' '")
            pause(sandbox)
            resume(sandbox)
            read = execute(sandbox, "sha256sum /data/blob | cut -d' ' -f1")
            assert len(written) == 64 and read == written, (written, read)
            state["sha"] = written
            return {"sha256": written, "guest_dirty_before_pause": dirty}
        case("a write made just before the pause, with no sync, is whole afterwards",
             unflushed_writes_survive)

        def again():
            pause(sandbox)
            resume(sandbox)
            assert execute(sandbox, "cat /data/second") == "after-resume"
            assert execute(sandbox, "sha256sum /data/blob | cut -d' ' -f1") == state["sha"]
            return {"pauses": 3}
        case("it pauses and resumes again", again)

        def fork_is_still_refused():
            status, value = request("POST", f"/sandboxes/{sandbox}/fork", {"count": 1})
            assert status == 409, (status, value)
            return {"status": status, "message": value["message"]}
        case("forking a sandbox with a disk is still refused", fork_is_still_refused)

        def reached_the_disk():
            execute(sandbox, "sync")
            status, _ = request("DELETE", f"/sandboxes/{sandbox}")
            assert status == 204, status
            other = create()
            assert execute(other, "sha256sum /data/blob | cut -d' ' -f1") == state["sha"]
            assert execute(other, "cat /data/first /data/second") == "on-the-disk\nafter-resume"
            state["other"] = other
            return {"read_by": other}
        case("the data reached the disk: a new sandbox attaching it reads the same bytes", reached_the_disk)

        def ending_a_paused_one_frees_the_disk():
            request("DELETE", f"/sandboxes/{state['other']}")
            third = create(autoPause=True)
            pause(third)
            status, _ = request("DELETE", f"/sandboxes/{third}")
            assert status == 204, status
            status, held = request("GET", f"/disks/{disk_id}")
            assert status == 200 and held.get("attachedTo") is None, held
            fourth = create()
            assert execute(fourth, "cat /data/first") == "on-the-disk"
            return {"autoPause_with_a_disk": 201, "disk_free_after_delete": True}
        case("autoPause with a disk is accepted, and ending a paused sandbox frees its disk",
             ending_a_paused_one_frees_the_disk)
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
