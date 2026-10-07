#!/usr/bin/env python3
"""Persistent machines on real KVM: they boot from their own disk and keep it.

Runs an owned hv2-sandboxd with a --machine-dir, then checks, through its API:

1. A machine created from the base template boots with its root filesystem
   on /dev/vda (ext4), not an initramfs, and runs commands.
2. A file it writes survives stop and start.
3. A file it writes survives the guest rebooting itself, which the daemon
   answers by booting it again.
4. A file it writes survives the daemon being killed: a new daemon on the
   same machine directory starts the machine by itself; a machine created
   with autostart off stays stopped.
5. A running machine cannot be deleted; a stopped one can, disk and all.

Writes report.json and the daemons' logs into --output.
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
    report = {"success": False, "purpose": "functional verification of persistent machines on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machines-check-", dir="/var/tmp"))
    machines = work / "machines"
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    state = {"daemon": None, "base": None}

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def start_daemon(label):
        api, proxy = port(), port()
        log = (args.output / f"{label}.log").open("wb")
        daemon = subprocess.Popen(
            [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy), "--no-template",
             "--machine-dir", str(machines), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8"],
            env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        state["daemon"], state["base"] = daemon, f"http://127.0.0.1:{api}"
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, f"{label} exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, f"{label} readiness"
            time.sleep(0.1)

    def request(method, path, body=None, timeout=180):
        req = urllib.request.Request(state["base"] + path, method=method,
                                     headers={"content-type": "application/json"},
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

    def run(name, cmd):
        status, value = request("POST", f"/machines/{name}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, f"{name}: {cmd!r}: {status} {value}"
        return value["stdout"].strip()

    def eventually(action, seconds=120):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.5)

    try:
        start_daemon("daemon-1")

        def boots_from_disk():
            status, created = request("POST", "/machines", {
                "name": "web-01", "diskGiB": 1, "memoryMB": 512, "cpuCount": 1})
            assert status == 201 and created["state"] == "running", (status, created)
            root = run("web-01", "grep ' / ' /proc/mounts")
            # The kernel lists a root given by `root=` as /dev/root.
            device, mountpoint, fstype = root.split()[:3]
            assert device in ("/dev/vda", "/dev/root") and mountpoint == "/" and fstype == "ext4", root
            cmdline = run("web-01", "cat /proc/cmdline")
            assert "root=/dev/vda" in cmdline and "rdinit" not in cmdline, cmdline
            image = machines / created["machineID"] / "root.img"
            return {"root_mount": root, "image_bytes": image.stat().st_size,
                    "image_allocated": image.stat().st_blocks * 512}
        case("a machine boots with its root on its own disk", boots_from_disk)

        written = run("web-01", "head -c 1048576 /dev/urandom > /root/persist && sync && "
                                "sha256sum /root/persist | cut -d' ' -f1")

        def survives_stop_start():
            status, stopped = request("POST", "/machines/web-01/stop")
            assert status == 200 and stopped["state"] == "stopped", stopped
            assert request("POST", "/machines/web-01/exec", {"cmd": "true"})[0] == 409
            status, started = request("POST", "/machines/web-01/start")
            assert status == 200 and started["state"] == "running", started
            read = run("web-01", "sha256sum /root/persist | cut -d' ' -f1")
            assert read == written
            return {"sha256": read}
        case("what it wrote survives stop and start", survives_stop_start)

        def survives_reboot():
            run("web-01", "touch /tmp/before-reboot && (sleep 1; /bin/busybox reboot -f) >/dev/null 2>&1 &")

            def fresh():
                assert run("web-01", "test -e /tmp/before-reboot && echo old || echo new") == "new"
            eventually(fresh, 120)
            read = run("web-01", "sha256sum /root/persist | cut -d' ' -f1")
            assert read == written
            return {"sha256": read}
        case("what it wrote survives the guest rebooting itself", survives_reboot)

        def survives_daemon_kill():
            status, manual = request("POST", "/machines", {
                "name": "manual-01", "diskGiB": 1, "memoryMB": 256, "cpuCount": 1, "autostart": False})
            assert status == 201 and manual["state"] == "running", manual
            state["daemon"].send_signal(signal.SIGKILL)
            state["daemon"].wait()
            start_daemon("daemon-2")

            def back():
                status, machine = request("GET", "/machines/web-01")
                assert machine["state"] == "running", machine
            eventually(back, 120)
            read = run("web-01", "sha256sum /root/persist | cut -d' ' -f1")
            assert read == written
            status, manual = request("GET", "/machines/manual-01")
            assert manual["state"] == "stopped" and manual["desiredState"] == "stopped", manual
            return {"sha256": read, "autostart_off": manual["state"]}
        case("a killed daemon's successor starts it again with its data", survives_daemon_kill)

        def delete():
            status, _ = request("DELETE", "/machines/web-01")
            assert status == 409, status
            assert request("POST", "/machines/web-01/stop")[0] == 200
            status, _ = request("DELETE", "/machines/web-01")
            assert status == 204, status
            assert request("GET", "/machines/web-01")[0] == 404
            assert not any(p.name.startswith("vm-") and (p / "machine.json").exists() and
                           json.loads((p / "machine.json").read_text())["name"] == "web-01"
                           for p in machines.iterdir())
            return {"running_delete": 409, "stopped_delete": 204}
        case("a running machine cannot be deleted; a stopped one can", delete)
        report["success"] = True
    finally:
        if state["daemon"] and state["daemon"].poll() is None:
            state["daemon"].terminate()
            try:
                state["daemon"].wait(timeout=20)
            except subprocess.TimeoutExpired:
                state["daemon"].kill()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
