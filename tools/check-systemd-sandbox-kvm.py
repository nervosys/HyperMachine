#!/usr/bin/env python3
"""A sandbox whose first process is systemd, from a distribution's root, on real KVM.

Runs an owned hv2-sandboxd whose `base` template is the initramfs that
tools/guest-image/build-tools-image.sh makes with tools/guest-image/init-systemd
as its init and systemd in its package list (Ubuntu 24.04), and checks through
its API:

1. PID 1 is systemd, the system reaches `running` (or `degraded`, with the
   failed units listed), and the agent still answers `exec`.
2. A unit written into the guest starts, runs a process of its own, and is
   active; `journalctl` has its line.
3. A fork of the sandbox has systemd as PID 1 and the unit still active.
4. A checkpoint restores to the same: PID 1 is systemd, the unit is active, and
   a file written after the checkpoint is gone.
5. The cost: how long a create takes, the memory used, and the failed units.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path

UNIT = """[Unit]
Description=hv2 probe
[Service]
ExecStart=/bin/sh -c 'echo probe-started; touch /run/probe-up; exec sleep 3600'
[Install]
WantedBy=multi-user.target
"""


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
    parser.add_argument("--memory-mb", type=int, default=1536)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification that a sandbox can run systemd as its first process on KVM; "
                         "the times are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "initrd_bytes": args.initrd.stat().st_size, "memory_mb": args.memory_mb,
              "host_cpus": os.cpu_count(), "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", str(args.memory_mb), "--cpu-cores", "2", "--capacity", "4"],
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

    def sh(sandbox, cmd, within=120, ok=True):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        if ok:
            assert value["exit_code"] == 0, (cmd, value)
        return value["stdout"].strip()

    def create():
        began = time.monotonic()
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200})
        assert status in (200, 201), (status, made)
        return made["sandboxID"], round(time.monotonic() - began, 3)

    def settled(sandbox):
        """systemd's own verdict, once it has one."""
        deadline = time.monotonic() + 90
        while True:
            said = sh(sandbox, "systemctl is-system-running --wait 2>&1; true", 120, ok=False)
            if said and "Failed to connect" not in said:
                return said.splitlines()[-1]
            if time.monotonic() > deadline:
                why = sh(sandbox, "ps -e -o pid,comm | head -30; echo ---; ls -la /run/systemd 2>&1 | head -20; echo ---; "
                                  "journalctl -b --no-pager -o cat 2>&1 | tail -25; echo ---; dmesg 2>&1 | tail -10",
                         30, ok=False)
                raise AssertionError("systemd never answered: " + repr(said) + "\n" + why)
            time.sleep(1)

    try:
        deadline = time.monotonic() + 240
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

        def boots():
            sandbox, took = create()
            state["sandbox"], state["create_seconds"] = sandbox, took
            began = time.monotonic()
            assert sh(sandbox, "cat /proc/1/comm") == "systemd"
            verdict = settled(sandbox)
            state["settle_seconds"] = round(time.monotonic() - began, 3)
            assert verdict in ("running", "degraded"), verdict
            failed = sh(sandbox, "systemctl --failed --no-legend --plain | cut -d' ' -f1", ok=False)
            state["failed"] = failed.splitlines()
            release = sh(sandbox, "grep -E '^VERSION_ID=' /etc/os-release")
            return {"create_seconds": took, "seconds_until_settled_after_create": state["settle_seconds"],
                    "system_state": verdict, "failed_units": state["failed"],
                    "os": release, "systemd": sh(sandbox, "systemctl --version | head -1")}
        case("PID 1 is systemd, the system settles, and exec still works", boots)

        def unit():
            sandbox = state["sandbox"]
            sh(sandbox, "cat > /etc/systemd/system/hv2-probe.service <<'EOF'\n" + UNIT + "EOF\n"
                        "systemctl daemon-reload && systemctl start hv2-probe")
            assert sh(sandbox, "systemctl is-active hv2-probe") == "active"
            assert sh(sandbox, "test -e /run/probe-up && echo up") == "up"
            # journald takes a moment to hear the unit's first line.
            for _ in range(20):
                if "probe-started" in sh(sandbox, "journalctl --sync; journalctl -u hv2-probe --no-pager -o cat | tail -3",
                                         30, ok=False):
                    break
                time.sleep(0.5)
            else:
                raise AssertionError("the unit's line never reached the journal")
            assert "sleep 3600" in sh(sandbox, "systemctl status hv2-probe --no-pager | grep -E 'CGroup|sleep'")
            return {"unit": "active"}
        case("a unit written into the guest starts, runs and logs to the journal", unit)

        def forked():
            sandbox = state["sandbox"]
            status, made = request("POST", f"/sandboxes/{sandbox}/fork", {"timeout": 600})
            assert status in (200, 201), (status, made)
            child = made[0]["sandbox"]["sandboxID"]
            try:
                assert sh(child, "cat /proc/1/comm") == "systemd"
                assert sh(child, "systemctl is-active hv2-probe") == "active"
                after = sh(child, "systemctl is-system-running; true", ok=False)
            finally:
                request("DELETE", f"/sandboxes/{child}")
            return {"fork_system_state": after}
        case("a fork has systemd as PID 1 and the unit still active", forked)

        def restored():
            sandbox = state["sandbox"]
            status, made = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "before"})
            assert status in (200, 201), (status, made)
            sh(sandbox, "touch /root/after-checkpoint; systemctl stop hv2-probe")
            status, back = request("POST", f"/sandboxes/{sandbox}/checkpoints/before/restore")
            assert status in (200, 201, 204), (status, back)
            assert sh(sandbox, "cat /proc/1/comm") == "systemd"
            assert sh(sandbox, "systemctl is-active hv2-probe") == "active"
            assert sh(sandbox, "test -e /root/after-checkpoint && echo there || echo gone") == "gone"
            return {"restored": "systemd, the unit active, the later file gone"}
        case("a checkpoint restores systemd and the unit, and drops what came after it", restored)

        def cost():
            sandbox = state["sandbox"]
            free = sh(sandbox, "grep -E '^(MemTotal|MemAvailable):' /proc/meminfo")
            second, took = create()
            request("DELETE", f"/sandboxes/{second}")
            return {"meminfo_kb": free.replace("\n", " "), "first_create_seconds": state["create_seconds"],
                    "second_create_seconds": took, "initramfs_bytes": args.initrd.stat().st_size,
                    "failed_units": state["failed"]}
        case("what it costs: create time, free memory, failed units", cost)

        request("DELETE", f"/sandboxes/{state['sandbox']}")
        report["success"] = True
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("success" if report["success"] else "FAILED", flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
