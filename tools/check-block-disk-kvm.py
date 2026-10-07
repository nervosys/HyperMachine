#!/usr/bin/env python3
"""Move one block disk between real KVM sandboxes, and check it holds.

Runs an owned hv2-sandboxd with a template, whose sandboxes restore from it
unless they ask for a disk, and checks:

1. A disk is created formatted, and a sandbox mounts it at the asked path.
2. While that sandbox holds it, a second sandbox and a delete are refused.
3. Ended, the sandbox gives it back, and the next sandbox reads every byte
   the first wrote -- a 4 MiB random file, compared by SHA-256 in the guest.
4. A daemon killed while a sandbox holds a disk leaves no claim that
   outlives it: a restarted daemon attaches the disk to a new sandbox, and
   the file is still there.
5. A sandbox holding a disk refuses to fork, snapshot or checkpoint; one
   asking for a disk with autoPause is refused before anything boots.

Writes a JSON report to --output.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def require(condition, message):
    if not condition:
        raise ValueError(message)


def port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--daemon", type=Path, required=True)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--initrd", type=Path, required=True)
    args = parser.parse_args()
    require(os.access("/dev/kvm", os.R_OK | os.W_OK), "this check needs /dev/kvm")
    require(shutil.which("mkfs.ext4"), "this check needs mkfs.ext4 (e2fsprogs)")

    work = Path(tempfile.mkdtemp(prefix="hm-disk-check-"))
    disks = work / "disks"
    report = {"daemon": str(args.daemon), "kernel": str(args.kernel),
              "initrd": str(args.initrd), "checks": []}
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd))
    processes = []

    def check(name, **detail):
        report["checks"].append({"name": name, **detail})
        print("ok:", name, flush=True)

    def start_daemon(label):
        api, proxy = port(), port()
        while api == proxy:
            proxy = port()
        log = (work / f"{label}.log").open("wb")
        process = subprocess.Popen(
            [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy),
             "--disk-dir", str(disks), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
            env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        processes.append(process)
        base = f"http://127.0.0.1:{api}"
        # Ready once its template is: pause, fork and the rest refuse every
        # sandbox on a node without one, which would hide the refusals here.
        deadline = time.monotonic() + 300
        while True:
            require(process.poll() is None, f"{label} exited during startup")
            try:
                status, templates = request(base, "GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    return process, base
            except OSError:
                pass
            require(time.monotonic() < deadline, f"{label} readiness timeout")
            time.sleep(0.05)

    def request(base, method, path, body=None):
        req = urllib.request.Request(
            base + path, method=method, headers={"content-type": "application/json"},
            data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=120)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            return response.status, json.loads(raw) if raw else None

    def execute(base, sandbox, command):
        status, value = request(base, "POST", f"/sandboxes/{sandbox}/exec",
                                {"cmd": command, "timeout_secs": 60})
        require(status == 200, f"exec answered {status}: {value}")
        require(value["exit_code"] == 0, f"guest command failed: {value}")
        return value["stdout"].strip()

    def create(base, disk, **extra):
        body = {"timeout": 600, "diskMount": {"name": disk, "path": "/data"}, **extra}
        started = time.monotonic()
        status, value = request(base, "POST", "/sandboxes", body)
        return status, value, round(time.monotonic() - started, 3)

    try:
        _, base = start_daemon("daemon-1")

        status, disk = request(base, "POST", "/disks", {"name": "movable", "sizeMiB": 64})
        require(status == 201, f"create disk answered {status}: {disk}")
        require(disk["attachedTo"] is None and disk["sizeMiB"] == 64, f"new disk: {disk}")
        image = disks / disk["diskID"] / "disk.img"
        require(image.stat().st_size == 64 << 20, "image is not the size asked for")
        check("disk created", disk=disk, allocated_bytes=image.stat().st_blocks * 512)

        started = time.monotonic()
        status, plain = request(base, "POST", "/sandboxes", {"timeout": 60})
        require(status == 201, f"a sandbox without a disk answered {status}: {plain}")
        plain_seconds = round(time.monotonic() - started, 3)
        require(request(base, "DELETE", f"/sandboxes/{plain['sandboxID']}")[0] == 204,
                "deleting the plain sandbox failed")
        check("a sandbox without a disk still restores from the template",
              create_seconds=plain_seconds)

        status, a, seconds = create(base, "movable")
        require(status == 201, f"create A answered {status}: {a}")
        a = a["sandboxID"]
        mounts = execute(base, a, "grep ' /data ' /proc/mounts")
        require(mounts.startswith("/dev/vda /data ext4"), f"not mounted as expected: {mounts}")
        written = execute(base, a, "head -c 4194304 /dev/urandom > /data/payload && sync && "
                                   "sha256sum /data/payload | cut -d' ' -f1")
        check("sandbox A mounts the disk and writes to it", sandbox=a, mount=mounts,
              create_seconds=seconds, sha256=written)

        status, value = request(base, "GET", f"/disks/{disk['diskID']}")
        require(value["attachedTo"] == a, f"disk not shown attached to A: {value}")
        status, b, _ = create(base, "movable")
        require(status == 409, f"a second attach answered {status}: {b}")
        status, deleted = request(base, "DELETE", f"/disks/{disk['diskID']}")
        require(status == 409, f"deleting an attached disk answered {status}")
        check("an attached disk is exclusive", second_attach=b["message"],
              delete=deleted["message"])

        refusals = {}
        for name, method, path in [("fork", "POST", f"/sandboxes/{a}/fork"),
                                   ("snapshot", "POST", f"/sandboxes/{a}/snapshots"),
                                   ("checkpoint", "POST", f"/sandboxes/{a}/checkpoints"),
                                   ("pause", "POST", f"/sandboxes/{a}/pause")]:
            status, value = request(base, method, path, {})
            require(status in (409, 404), f"{name} of a disk sandbox answered {status}: {value}")
            refusals[name] = {"status": status, "message": (value or {}).get("message")}
        status, value, _ = create(base, "movable", autoPause=True)
        require(status == 400, f"autoPause with a disk answered {status}")
        refusals["autoPause"] = {"status": status, "message": value["message"]}
        check("operations that would restore memory against a moved disk are refused",
              refusals=refusals)

        status, _ = request(base, "DELETE", f"/sandboxes/{a}")
        require(status == 204, f"deleting A answered {status}")
        status, value = request(base, "GET", f"/disks/{disk['diskID']}")
        require(value["attachedTo"] is None, f"disk still attached after A ended: {value}")
        status, b, seconds = create(base, "movable")
        require(status == 201, f"create B answered {status}: {b}")
        b = b["sandboxID"]
        read = execute(base, b, "sha256sum /data/payload | cut -d' ' -f1")
        require(read == written, f"B read {read}, A wrote {written}")
        check("ended, A gives the disk back and B reads what A wrote", sandbox=b,
              create_seconds=seconds, sha256=read)

        processes[0].kill()
        processes[0].wait()
        _, base = start_daemon("daemon-2")
        status, value = request(base, "GET", f"/disks/{disk['diskID']}")
        require(value["attachedTo"] is None,
                f"a dead daemon's claim was believed: {value}")
        status, c, _ = create(base, "movable")
        require(status == 201, f"create C after restart answered {status}: {c}")
        c = c["sandboxID"]
        read = execute(base, c, "sha256sum /data/payload | cut -d' ' -f1")
        require(read == written, f"C read {read}, A wrote {written}")
        check("a killed daemon's claim does not outlive it", sandbox=c, sha256=read)

        status, _ = request(base, "DELETE", f"/sandboxes/{c}")
        require(status == 204, f"deleting C answered {status}")
        status, _ = request(base, "DELETE", f"/disks/{disk['diskID']}")
        require(status == 204, f"deleting the free disk answered {status}")
        require(not image.exists(), "the image outlived its disk")
        check("a free disk deletes, image and all")
        report["result"] = "pass"
    except Exception as error:
        report["result"] = "fail"
        report["error"] = str(error)
        raise
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        for log in work.glob("*.log"):
            shutil.copy(log, args.output.parent / log.name)
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
