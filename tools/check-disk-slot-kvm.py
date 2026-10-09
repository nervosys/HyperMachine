#!/usr/bin/env python3
"""Sandboxes with a disk restored from a template, on real KVM.

Runs two owned hv2-sandboxd daemons from one binary, alike except that one has
`--disk-slot` (and a warm pool). With the slot, a template's guest boots with a
placeholder disk, so a sandbox that asks for a disk is restored like any other
and given the real one, where without it such a sandbox cold-boots.

On the daemon with the slot it checks, through the API:

1. A sandbox created with a disk is restored, not booted: its kernel logged
   the disk changing size under it, which only a running guest sees. The disk
   is mounted at the asked path with the disk's real size, and holds what an
   earlier sandbox wrote to it.
2. A sandbox with no disk sees only the empty placeholder, and what it writes
   there no other sandbox reads.
3. It pauses and resumes, with a write that was never synced whole afterwards.
4. It forks, each fork with its own copy of the disk and the unsynced write.
5. A create with a disk takes a spare from the warm pool.
6. It reboots onto the same disk.

Then it times, on both daemons alternately: create with a disk, pause, resume,
and a one-way fork. These are measurements of this build on this host, not a
comparison with another product.

Writes report.json and both daemons' logs into --output.
"""
import argparse, hashlib, json, os, shutil, socket, statistics, subprocess, tempfile, time
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
    return {"n": len(ordered), "min": round(ordered[0], 1), "p50": round(statistics.median(ordered), 1),
            "max": round(ordered[-1], 1)}


class Node:
    def __init__(self, args, work, label, extra):
        self.label = label
        self.api = port()
        self.base = f"http://127.0.0.1:{self.api}"
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
        self.log = (args.output / f"{label}.log").open("wb")
        self.process = subprocess.Popen(
            [str(args.daemon), "--port", str(self.api), "--proxy-port", str(port()), "--memory-mb", "512",
             "--cpu-cores", "1", "--capacity", "16", "--disk-dir", str(work / f"disks-{label}")] + extra,
            env=env, stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT)

    def request(self, method, path, body=None, timeout=180):
        req = urllib.request.Request(self.base + path, method=method, headers={"content-type": "application/json"},
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

    def ready(self):
        deadline = time.monotonic() + 300
        while True:
            assert self.process.poll() is None, f"{self.label} exited"
            try:
                status, templates = self.request("GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

    def disk(self, name, mib=64):
        status, value = self.request("POST", "/disks", {"name": name, "sizeMiB": mib})
        assert status == 201, (status, value)
        return value["diskID"]

    def create(self, disk=None, path="/data", **extra):
        body = {"timeout": 900, **extra}
        if disk:
            body["diskMount"] = {"name": disk, "path": path}
        status, value = self.request("POST", "/sandboxes", body)
        assert status == 201, (status, value)
        return value["sandboxID"]

    def execute(self, sandbox, command):
        status, value = self.request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, f"guest command failed: {status} {value}"
        return value["stdout"].strip()

    def delete(self, sandbox):
        status, _ = self.request("DELETE", f"/sandboxes/{sandbox}")
        assert status == 204, status

    def pause(self, sandbox):
        status, value = self.request("POST", f"/sandboxes/{sandbox}/pause", {})
        assert status == 204, (status, value)

    def resume(self, sandbox):
        status, value = self.request("POST", f"/sandboxes/{sandbox}/resume", {"timeout": 900})
        assert status == 201, (status, value)

    def fork(self, sandbox, count=1):
        status, value = self.request("POST", f"/sandboxes/{sandbox}/fork", {"count": count, "timeout": 900})
        assert status == 201 and all("sandbox" in f for f in value), (status, value)
        return [f["sandbox"]["sandboxID"] for f in value]

    def pool(self):
        return self.request("GET", "/pool")[1]

    def wait_full(self):
        deadline = time.monotonic() + 60
        while True:
            pool = self.pool()
            if pool["ready"] == pool["target"]:
                return pool
            assert time.monotonic() < deadline, f"the pool did not fill: {pool}"
            time.sleep(0.05)

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=6)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of the disk slot on KVM, with same-build "
                                           "timings; no comparison with another product",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_load_1m_at_start": os.getloadavg()[0], "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-disk-slot-check-", dir="/var/tmp"))
    slot = Node(args, work, "with-slot", ["--disk-slot", "--warm-pool", "2"])
    plain = Node(args, work, "without-slot", [])
    sha = "sha256sum {} | cut -d' ' -f1"

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    try:
        slot.ready()
        plain.ready()
        slot.disk("data")
        state = {}

        def restored_with_its_disk():
            writer = slot.create("data")
            slot.execute(writer, "echo from-the-first > /data/first && sync")
            slot.delete(writer)
            bare = slot.create()
            sandbox = slot.create("data")
            # The guest's driver logs a resize when the placeholder is swapped
            # for the disk. A guest that booted with the disk never sees one.
            resized = slot.execute(sandbox, "dmesg | grep -i 'vda' | grep -i 'new size' | tail -1")
            assert resized, "its kernel never saw the disk change size: it booted with it"
            assert slot.execute(bare, "dmesg | grep -i 'vda' | grep -ic 'new size' || true") == "0"
            mounted = slot.execute(sandbox, "grep ' /data ' /proc/mounts")
            assert "ext4" in mounted and "/dev/vda" in mounted, mounted
            size = int(slot.execute(sandbox, "cat /sys/block/vda/size")) * 512
            assert size == 64 << 20, size
            assert slot.execute(sandbox, "cat /data/first") == "from-the-first"
            state.update(sandbox=sandbox, bare=bare)
            return {"kernel_log": resized, "disk_bytes": size, "mount": mounted}
        case("a sandbox created with a disk is restored from the template and given the disk",
             restored_with_its_disk)

        def placeholder_is_private():
            bare = state["bare"]
            size = int(slot.execute(bare, "cat /sys/block/vda/size")) * 512
            assert size == 1 << 20, size
            assert slot.execute(bare, "dd if=/dev/vda bs=1M count=1 2>/dev/null | tr -d '\\000' | wc -c") == "0"
            slot.execute(bare, "echo placeholder-secret | dd of=/dev/vda conv=fsync 2>/dev/null")
            other = slot.create()
            assert slot.execute(other, "dd if=/dev/vda bs=1M count=1 2>/dev/null | tr -d '\\000' | wc -c") == "0"
            slot.delete(other)
            slot.delete(bare)
            return {"placeholder_bytes": size}
        case("a sandbox with no disk sees only an empty placeholder of its own", placeholder_is_private)

        def pauses():
            sandbox = state["sandbox"]
            state["blob"] = slot.execute(sandbox, "dd if=/dev/urandom of=/data/blob bs=1M count=4 2>/dev/null; "
                                                  + sha.format("/data/blob"))
            dirty = slot.execute(sandbox, "grep '^Dirty:' /proc/meminfo | tr -s ' '")
            slot.pause(sandbox)
            slot.resume(sandbox)
            assert slot.execute(sandbox, sha.format("/data/blob")) == state["blob"]
            assert slot.execute(sandbox, "cat /data/first") == "from-the-first"
            return {"guest_dirty_before_pause": dirty, "sha256": state["blob"]}
        case("it pauses and resumes, an unsynced write whole afterwards", pauses)

        def forks():
            sandbox = state["sandbox"]
            fresh = slot.execute(sandbox, "dd if=/dev/urandom of=/data/late bs=1M count=2 2>/dev/null; "
                                          + sha.format("/data/late"))
            children = slot.fork(sandbox, 2)
            for child in children:
                assert slot.execute(child, sha.format("/data/late")) == fresh
                assert slot.execute(child, sha.format("/data/blob")) == state["blob"]
            slot.execute(children[0], "echo child > /data/who && sync")
            slot.execute(sandbox, "echo parent > /data/who && sync")
            assert slot.execute(children[0], "cat /data/who") == "child"
            assert slot.execute(sandbox, "cat /data/who") == "parent"
            assert slot.execute(children[1], "ls /data/who 2>/dev/null || echo absent") == "absent"
            for child in children:
                slot.delete(child)
            return {"forks": 2}
        case("it forks, each fork with its own copy of the disk", forks)

        def from_the_pool():
            slot.disk("pooled")
            before = slot.wait_full()
            sandbox = slot.create("pooled")
            after = slot.pool()
            assert after["handedOut"] == before["handedOut"] + 1, (before, after)
            slot.execute(sandbox, "echo pooled > /data/p && sync")
            assert slot.execute(sandbox, "cat /data/p") == "pooled"
            slot.delete(sandbox)
            return {"handed_out": after["handedOut"]}
        case("a create with a disk takes a spare from the warm pool", from_the_pool)

        def reboots():
            sandbox = state["sandbox"]
            slot.execute(sandbox, "sync")
            status, value = slot.request("POST", f"/sandboxes/{sandbox}/reboot", {})
            assert status in (200, 204), (status, value)
            deadline = time.monotonic() + 60
            while True:
                try:
                    if slot.execute(sandbox, "cat /data/who") == "parent":
                        break
                except (AssertionError, OSError):
                    pass
                assert time.monotonic() < deadline, "it did not come back with its disk"
                time.sleep(0.5)
            assert slot.execute(sandbox, sha.format("/data/blob")) == state["blob"]
            slot.delete(sandbox)
            return {"reboot": status}
        case("it reboots onto the same disk", reboots)

        def timed():
            results = {}
            for node in (plain, slot):
                node.disk("timed")
            order = [plain, slot, slot, plain]
            for node in order:
                row = results.setdefault(node.label, {"create": [], "pause": [], "resume": [], "fork": []})
                for _ in range(args.rounds):
                    if node is slot:
                        node.wait_full()
                    started = time.perf_counter()
                    sandbox = node.create("timed")
                    row["create"].append((time.perf_counter() - started) * 1000)
                    node.execute(sandbox, "dd if=/dev/urandom of=/data/x bs=1M count=1 2>/dev/null")
                    started = time.perf_counter()
                    node.pause(sandbox)
                    row["pause"].append((time.perf_counter() - started) * 1000)
                    started = time.perf_counter()
                    node.resume(sandbox)
                    row["resume"].append((time.perf_counter() - started) * 1000)
                    started = time.perf_counter()
                    children = node.fork(sandbox, 1)
                    row["fork"].append((time.perf_counter() - started) * 1000)
                    assert node.execute(children[0], "ls /data/x") == "/data/x"
                    node.delete(children[0])
                    node.delete(sandbox)
                    # The fork's copy of the disk is not wanted again.
                    for listed in node.request("GET", "/disks")[1]:
                        if listed["name"].startswith("timed-fork-"):
                            node.request("DELETE", f"/disks/{listed['diskID']}")
            return {"order": [node.label for node in order], "rounds_per_block": args.rounds,
                    "milliseconds": {label: {op: summary(v) for op, v in row.items()}
                                     for label, row in results.items()},
                    "host_load_1m": os.getloadavg()[0]}
        case("timings: create, pause, resume and fork of a sandbox with a disk, without and with the slot",
             timed)
        report["success"] = True
    finally:
        slot.stop()
        plain.stop()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
