#!/usr/bin/env python3
"""Reboot real KVM sandboxes in place, and check what survives.

Runs an owned hv2-sandboxd with a template, and checks:

1. A guest that reboots itself (`reboot -f`, which ends in a triple fault and
   a stopped VM) is brought back without being asked: the same sandbox ID and
   access token answer again, from a fresh guest (a file written to the RAM
   root before is gone), with its `envVars` back.
   A kernel panic (`panic=1`) comes back the same way.
2. Its URL survives: a guest web server reached through the proxy at
   `{port}-{sandboxID}` before the reboot is reached there again after it,
   once the fresh guest starts one.
3. A sandbox holding a disk reboots holding it, mounted where it was, with
   what was written before the reboot.
4. `POST /sandboxes/{id}/reboot` reboots a running guest on request.
5. A sandbox rebooted more than the limit within its window is ended as
   lost, rather than rebooted forever.
6. `hv2_node_transitions_total{kind="reboot"}` counts them.

Writes a JSON report to --output.
"""
import argparse
import http.client
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

    work = Path(tempfile.mkdtemp(prefix="hm-reboot-check-"))
    report = {"daemon": str(args.daemon), "kernel": str(args.kernel),
              "initrd": str(args.initrd), "checks": []}
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd))
    api, proxy = port(), port()
    while api == proxy:
        proxy = port()
    base = f"http://127.0.0.1:{api}"
    log = (work / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy),
         "--disk-dir", str(work / "disks"), "--memory-mb", "512", "--cpu-cores", "1",
         "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)

    def check(name, **detail):
        report["checks"].append({"name": name, **detail})
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=120):
        req = urllib.request.Request(
            base + path, method=method, headers={"content-type": "application/json"},
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

    def execute(sandbox, command, timeout=10):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec",
                                {"cmd": command, "timeout_secs": timeout})
        require(status == 200 and value["exit_code"] == 0, f"guest command failed: {value}")
        return value["stdout"].strip()

    def through_proxy(sandbox, guest_port):
        connection = http.client.HTTPConnection("127.0.0.1", proxy, timeout=20)
        try:
            connection.request("GET", "/index.html",
                               headers={"Host": f"{guest_port}-{sandbox}.localhost"})
            response = connection.getresponse()
            return response.status, response.read().decode(errors="replace").strip()
        finally:
            connection.close()

    def uptime(sandbox):
        return float(execute(sandbox, "cut -d' ' -f1 /proc/uptime"))

    def wait_rebooted(sandbox, marker, deadline_seconds=60):
        """Until the sandbox answers from a guest without `marker` in its RAM root."""
        started = time.monotonic()
        while time.monotonic() - started < deadline_seconds:
            status, value = request("POST", f"/sandboxes/{sandbox}/exec",
                                    {"cmd": f"test -e {marker} && echo old || echo new",
                                     "timeout_secs": 5}, timeout=30)
            if status == 200 and value.get("stdout", "").strip() == "new":
                return round(time.monotonic() - started, 3)
            time.sleep(0.25)
        raise ValueError(f"{sandbox} did not come back within {deadline_seconds}s")

    def serve(sandbox, text):
        execute(sandbox, f"mkdir -p /tmp/www && echo {text} > /tmp/www/index.html && "
                         "(/bin/busybox httpd -p 8080 -h /tmp/www &) ; sleep 0.3")

    def metric_reboots():
        with urllib.request.urlopen(base + "/metrics", timeout=10) as response:
            for line in response.read().decode().splitlines():
                if line.startswith('hv2_node_transitions_total{kind="reboot"}'):
                    return int(float(line.split()[-1]))
        return None

    try:
        deadline = time.monotonic() + 300
        while True:
            require(daemon.poll() is None, "daemon exited during startup")
            try:
                status, templates = request("GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    break
            except OSError:
                pass
            require(time.monotonic() < deadline, "daemon readiness timeout")
            time.sleep(0.1)

        # 1 and 2: a guest that reboots itself.
        status, created = request("POST", "/sandboxes", {
            "timeout": 600, "envVars": {"HM_REBOOT_CHECK": "kept-across-reboot"}})
        require(status == 201, f"create answered {status}: {created}")
        sandbox, token = created["sandboxID"], created["envdAccessToken"]
        serve(sandbox, "before-reboot")
        before = through_proxy(sandbox, 8080)
        require(before == (200, "before-reboot"), f"proxy before reboot: {before}")
        execute(sandbox, "touch /tmp/marker && (sleep 1; /bin/busybox reboot -f) >/dev/null 2>&1 &")
        seconds = wait_rebooted(sandbox, "/tmp/marker")
        status, detail = request("GET", f"/sandboxes/{sandbox}")
        require(status == 200 and detail["envdAccessToken"] == token,
                f"identity changed: {detail}")
        up = uptime(sandbox)
        env_value = execute(sandbox, "echo $HM_REBOOT_CHECK")
        require(env_value == "kept-across-reboot", f"envVars after reboot: {env_value!r}")
        check("a guest that reboots itself comes back as the same sandbox",
              sandbox=sandbox, back_after_seconds=seconds, guest_uptime_after=up,
              same_access_token=True, env_after=env_value)
        down = through_proxy(sandbox, 8080)
        serve(sandbox, "after-reboot")
        after = through_proxy(sandbox, 8080)
        require(after == (200, "after-reboot"), f"proxy after reboot: {after}")
        check("its URL reaches the rebooted guest", before=before[1],
              before_server_restarted={"status": down[0]}, after=after[1])

        execute(sandbox, "touch /tmp/marker-panic && "
                         "(sleep 1; echo c > /proc/sysrq-trigger) >/dev/null 2>&1 &")
        seconds = wait_rebooted(sandbox, "/tmp/marker-panic")
        env_value = execute(sandbox, "echo $HM_REBOOT_CHECK")
        require(env_value == "kept-across-reboot", f"envVars after panic: {env_value!r}")
        check("a kernel panic comes back the same way", back_after_seconds=seconds)

        # 3: a sandbox holding a disk.
        status, disk = request("POST", "/disks", {"name": "reboot-disk", "sizeMiB": 32})
        require(status == 201, f"disk create answered {status}: {disk}")
        status, held = request("POST", "/sandboxes", {
            "timeout": 600, "diskMount": {"name": "reboot-disk", "path": "/data"}})
        require(status == 201, f"disk sandbox create answered {status}: {held}")
        held = held["sandboxID"]
        written = execute(held, "head -c 1048576 /dev/urandom > /data/f && sync && "
                                "sha256sum /data/f | cut -d' ' -f1")
        execute(held, "touch /tmp/marker && (sleep 1; /bin/busybox reboot -f) >/dev/null 2>&1 &")
        seconds = wait_rebooted(held, "/tmp/marker")
        read = execute(held, "sha256sum /data/f | cut -d' ' -f1")
        require(read == written, f"disk after reboot: {read} != {written}")
        mount = execute(held, "grep ' /data ' /proc/mounts")
        status, value = request("GET", f"/disks/{disk['diskID']}")
        require(value["attachedTo"] == held, f"disk not held after reboot: {value}")
        check("a sandbox with a disk reboots holding it, data intact", sandbox=held,
              back_after_seconds=seconds, mount=mount, sha256=read)

        # 4: on request.
        execute(sandbox, "touch /tmp/marker2")
        started = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/reboot")
        require(status == 200, f"reboot answered {status}: {value}")
        seconds = round(time.monotonic() - started, 3)
        require(execute(sandbox, "test -e /tmp/marker2 && echo old || echo new") == "new",
                "a requested reboot left the old guest")
        check("POST /sandboxes/{id}/reboot reboots a running guest",
              request_seconds=seconds)

        # 5: the limit. Three reboots so far in this window; two more fill it.
        answers = []
        for _ in range(3):
            status, value = request("POST", f"/sandboxes/{sandbox}/reboot")
            answers.append(status)
            if status != 200:
                break
        require(answers == [200, 200, 409],
                f"reboot answers within the window: {answers}")
        status, _ = request("GET", f"/sandboxes/{sandbox}")
        require(status == 404, f"a crash-looping sandbox was not ended: {status}")
        check("past the limit within the window, the sandbox is ended", answers=answers)

        count = metric_reboots()
        require(count is not None and count >= 6, f"reboot metric: {count}")
        check("reboots are counted", hv2_node_transitions_total_reboot=count)

        request("DELETE", f"/sandboxes/{held}")
        report["result"] = "pass"
    except Exception as error:
        report["result"] = "fail"
        report["error"] = str(error)
        raise
    finally:
        if daemon.poll() is None:
            daemon.terminate()
            try:
                daemon.wait(timeout=10)
            except subprocess.TimeoutExpired:
                daemon.kill()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        shutil.copy(work / "daemon.log", args.output.parent / "daemon.log")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
