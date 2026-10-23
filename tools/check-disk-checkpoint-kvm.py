#!/usr/bin/env python3
"""Checkpoint a real KVM sandbox together with the block disk it holds, and restore it.

Runs an owned hv2-sandboxd with a template and a --disk-dir, and checks,
through its API:

1. A sandbox holding a disk is checkpointed by name (it was refused). It was
   running a counter, had a synced file and one written just before with no
   sync on its disk, and a file on its RAM root.
2. The sandbox ran on and changed its disk: a new file, a changed file.
3. Restoring the checkpoint gives the sandbox back as it was at that moment:
   the same boot, the counter back near where it was, the RAM file, and on
   its disk, mounted, the synced and unsynced files by SHA-256 and none of
   what was written afterwards.
4. The disk the sandbox held before the restore is not overwritten: it is
   let go as it was, with the later writes, and attaches to a new sandbox by
   name. The restored sandbox holds a new disk, named for the old.
5. Restoring the same checkpoint again works, and again returns the
   checkpointed disk, not the one the second life wrote.
6. A restore that fails leaves the sandbox on the disk it had: restoring a
   checkpoint whose memory file was removed is refused, and no new disk is
   left behind.
7. Deleting the checkpoint removes the copy of the disk it kept, and a
   sandbox ending removes the rest.
8. Still refused: a template made from a sandbox with a disk.
9. A sandbox that was restored (so on a disk made by a restore) is
   checkpointed again, and restored from the newer or the older one.
10. A 1 GiB disk holding 400 MiB of random data is checkpointed and restored
   with every byte back (SHA-256), with the request times recorded.

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
    report = {"success": False,
              "purpose": "functional verification of checkpointing a sandbox with its disk on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-disk-checkpoint-check-", dir="/var/tmp"))
    (work / "tmp").mkdir()
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info",
               TMPDIR=str(work / "tmp"))
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--disk-dir", str(work / "disks"),
         "--snapshot-store", str(work / "store"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=240):
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

    def disks():
        status, listed = request("GET", "/disks")
        assert status == 200, (status, listed)
        return {d["name"]: d.get("attachedTo") for d in listed}

    def copies_on_disk():
        return sorted(str(p.relative_to(work)) for p in work.rglob("*.disk.img"))

    sha = "sha256sum {} | cut -d' ' -f1"
    state = {}
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
        status, value = request("POST", "/sandboxes",
                                {"timeout": 1800, "diskMount": {"name": "data", "path": "/data"}})
        assert status == 201, (status, value)
        box = value["sandboxID"]
        execute(box, "dd if=/dev/urandom of=/data/synced bs=1M count=4 2>/dev/null && sync && "
                     "echo in-ram > /tmp/ram-file")
        execute(box, "(i=0; while true; do i=$((i+1)); echo $i > /tmp/count; sleep 0.1; done) >/dev/null 2>&1 &")
        time.sleep(1)

        def checkpointed():
            state["boot"] = execute(box, "cat /proc/sys/kernel/random/boot_id")
            state["synced"] = execute(box, sha.format("/data/synced"))
            state["unsynced"] = execute(box, "dd if=/dev/urandom of=/data/unsynced bs=1M count=4 2>/dev/null; "
                                             + sha.format("/data/unsynced"))
            state["count"] = int(execute(box, "cat /tmp/count"))
            started = time.monotonic()
            status, value = request("POST", f"/sandboxes/{box}/checkpoints", {"name": "cp1"})
            state["took"] = round(time.monotonic() - started, 3)
            assert status == 201, (status, value)
            copies = copies_on_disk()
            assert len(copies) == 1 and copies[0].endswith("cp1.disk.img"), copies
            return {"checkpoint_request_seconds": state["took"], "copy_on_node": copies[0]}
        case("a sandbox holding a disk is checkpointed, and the node keeps a copy of the disk", checkpointed)

        def ran_on():
            execute(box, "echo after > /data/after && echo changed > /data/synced.txt && sync")
            assert execute(box, "cat /data/after") == "after"
            time.sleep(2)
            state["count_after"] = int(execute(box, "cat /tmp/count"))
            assert state["count_after"] > state["count"]
            return {"counter": [state["count"], state["count_after"]]}
        case("the sandbox ran on and changed its disk", ran_on)

        def restored():
            before = disks()
            assert before["data"] == box, before
            started = time.monotonic()
            status, value = request("POST", f"/sandboxes/{box}/checkpoints/cp1/restore")
            took = round(time.monotonic() - started, 3)
            assert status == 200, (status, value)
            assert execute(box, "cat /proc/sys/kernel/random/boot_id") == state["boot"]
            assert execute(box, "cat /tmp/ram-file") == "in-ram"
            mounted = execute(box, "grep ' /data ' /proc/mounts")
            assert "ext4" in mounted and "/dev/vda" in mounted, mounted
            assert execute(box, sha.format("/data/synced")) == state["synced"]
            assert execute(box, sha.format("/data/unsynced")) == state["unsynced"]
            assert execute(box, "ls /data/after /data/synced.txt 2>/dev/null || echo absent") == "absent"
            counter = int(execute(box, "cat /tmp/count"))
            assert counter < state["count_after"], (counter, state["count_after"])
            time.sleep(0.6)
            assert int(execute(box, "cat /tmp/count")) > counter, "the counter does not run"
            held = disks()
            assert held["data"] is None, held
            mine = [n for n, holder in held.items() if holder == box]
            assert len(mine) == 1 and mine[0].startswith("data-restored-"), held
            state["restored1"] = mine[0]
            return {"restore_request_seconds": took, "counter_after_restore": counter, "disk_now": mine[0]}
        case("restoring gives the sandbox, and its disk, back as they were at the checkpoint", restored)

        def old_disk_kept():
            status, value = request("POST", "/sandboxes",
                                    {"timeout": 600, "diskMount": {"name": "data", "path": "/mnt/old"}})
            assert status == 201, (status, value)
            reader = value["sandboxID"]
            assert execute(reader, "cat /mnt/old/after /mnt/old/synced.txt") == "after\nchanged"
            assert execute(reader, sha.format("/mnt/old/synced")) == state["synced"]
            request("DELETE", f"/sandboxes/{reader}")
            return {"read_by": reader}
        case("the disk it held before is let go as it was, with the later writes", old_disk_kept)

        def restore_again():
            execute(box, "echo second-life > /data/second && sync")
            status, value = request("POST", f"/sandboxes/{box}/checkpoints/cp1/restore")
            assert status == 200, (status, value)
            assert execute(box, sha.format("/data/synced")) == state["synced"]
            assert execute(box, sha.format("/data/unsynced")) == state["unsynced"]
            assert execute(box, "ls /data/second /data/after 2>/dev/null || echo absent") == "absent"
            held = disks()
            mine = [n for n, holder in held.items() if holder == box]
            assert len(mine) == 1 and mine[0] != state["restored1"], held
            assert held[state["restored1"]] is None, held
            return {"second_disk": mine[0]}
        case("restoring again returns the checkpointed disk, not the second life's", restore_again)

        def failed_restore():
            before = disks()
            held_now = [n for n, holder in before.items() if holder == box]
            files = list(work.rglob("cp1.snap"))
            assert len(files) == 1, files
            kept = files[0].read_bytes()
            files[0].unlink()
            status, value = request("POST", f"/sandboxes/{box}/checkpoints/cp1/restore")
            assert status >= 400, (status, value)
            files[0].write_bytes(kept)
            after = disks()
            assert [n for n, holder in after.items() if holder == box] == held_now, (before, after)
            assert set(after) == set(before), "a failed restore left a disk behind"
            assert execute(box, sha.format("/data/synced")) == state["synced"]
            return {"status": status, "message": value.get("message") if isinstance(value, dict) else value}
        case("a restore that fails leaves the sandbox on the disk it had", failed_restore)

        def chained():
            # The sandbox now runs on a disk made by a restore. It is checkpointed
            # again and restored again, and that is its disk too.
            execute(box, "echo generation-two > /data/gen && sync")
            status, value = request("POST", f"/sandboxes/{box}/checkpoints", {"name": "cp-gen2"})
            assert status == 201, (status, value)
            execute(box, "echo generation-three > /data/gen && sync")
            status, value = request("POST", f"/sandboxes/{box}/checkpoints/cp-gen2/restore")
            assert status == 200, (status, value)
            assert execute(box, "cat /data/gen") == "generation-two"
            assert execute(box, sha.format("/data/synced")) == state["synced"]
            status, value = request("POST", f"/sandboxes/{box}/checkpoints/cp1/restore")
            assert status == 200, (status, value)
            assert execute(box, "ls /data/gen 2>/dev/null || echo absent") == "absent"
            for name in ("cp-gen2",):
                status, _ = request("DELETE", f"/sandboxes/{box}/checkpoints/{name}")
                assert status == 204, status
            return {"restored_twice_from_a_restored_sandbox": True}
        case("a sandbox that was restored can be checkpointed, and restored from either", chained)

        def larger_disk():
            status, value = request("POST", "/disks", {"name": "big", "sizeMiB": 1024})
            assert status == 201, (status, value)
            status, value = request("POST", "/sandboxes",
                                    {"timeout": 1800, "diskMount": {"name": "big", "path": "/big"}})
            assert status == 201, (status, value)
            other = value["sandboxID"]
            execute(other, "dd if=/dev/urandom of=/big/blob bs=1M count=400 2>/dev/null && sync")
            digest_before = execute(other, sha.format("/big/blob"))
            started = time.monotonic()
            status, value = request("POST", f"/sandboxes/{other}/checkpoints", {"name": "big1"})
            took = round(time.monotonic() - started, 3)
            assert status == 201, (status, value)
            execute(other, "dd if=/dev/zero of=/big/blob bs=1M count=50 conv=notrunc 2>/dev/null && sync")
            assert execute(other, sha.format("/big/blob")) != digest_before
            started = time.monotonic()
            status, value = request("POST", f"/sandboxes/{other}/checkpoints/big1/restore")
            restore_took = round(time.monotonic() - started, 3)
            assert status == 200, (status, value)
            assert execute(other, sha.format("/big/blob")) == digest_before
            request("DELETE", f"/sandboxes/{other}")
            return {"disk_mib": 1024, "written_mib": 400, "checkpoint_request_seconds": took,
                    "restore_request_seconds": restore_took}
        case("a 1 GiB disk holding 400 MiB of random data is checkpointed and restored byte for byte", larger_disk)

        def refusals():
            status, value = request("POST", f"/sandboxes/{box}/snapshots", {"name": "from-disk"})
            assert status == 409, (status, value)
            return {"snapshot": status}
        case("a template from a sandbox with a disk is still refused", refusals)

        def deleting():
            status, _ = request("DELETE", f"/sandboxes/{box}/checkpoints/cp1")
            assert status == 204, status
            assert copies_on_disk() == [], copies_on_disk()
            status, value = request("POST", f"/sandboxes/{box}/checkpoints", {"name": "cp2"})
            assert status == 201, (status, value)
            assert len(copies_on_disk()) == 1
            status, _ = request("DELETE", f"/sandboxes/{box}")
            assert status == 204, status
            assert copies_on_disk() == [], copies_on_disk()
            return {"copies_left": 0}
        case("deleting a checkpoint, and a sandbox ending, remove the copies of the disk", deleting)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.terminate()
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
