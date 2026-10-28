#!/usr/bin/env python3
"""Forks of a busy sandbox, repeated, on real KVM, with no Docker.

A mid-build fork once restored a guest that never answered on vsock (1 in 30 runs of
the Docker check). This isolates it: a sandbox is kept busy (CPU loops and a process
that keeps dirtying memory), then forked --count times in a row. Each fork must run a
command within --within seconds, and is then deleted. A fork that does not answer is
recorded with its elapsed time and the check carries on, so the failure rate is
counted rather than stopping at the first.

Writes report.json and the daemon's log into --output. Exit status is 1 if any fork
failed.
"""
import argparse, hashlib, json, os, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path

BUSY = ("for i in 1 2; do setsid sh -c 'while :; do :; done' >/dev/null 2>&1 </dev/null & done; "
        "setsid sh -c 'while :; do dd if=/dev/zero of=/dev/shm/dirty bs=1M count=64 2>/dev/null; done' >/dev/null 2>&1 </dev/null & "
        "sleep 1; echo busy")


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
    parser.add_argument("--count", type=int, default=40)
    parser.add_argument("--within", type=int, default=30)
    parser.add_argument("--memory-mb", type=int, default=2048)
    parser.add_argument("--idle", action="store_true", help="control: fork a sandbox that is not busy")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "busy": not args.idle, "count": args.count, "within_seconds": args.within,
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(), "forks": []}
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

    def sh(sandbox, cmd, within=120):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        return value["stdout"].strip()

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
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 3600})
        assert status in (200, 201), (status, made)
        parent = made["sandboxID"]
        assert sh(parent, "echo up") == "up"
        if not args.idle:
            said = sh(parent, BUSY)
            assert said == "busy", said
        for n in range(1, args.count + 1):
            began = time.monotonic()
            row = {"n": n}
            report["forks"].append(row)
            status, made = request("POST", f"/sandboxes/{parent}/fork", {"timeout": 900})
            row["fork_seconds"] = round(time.monotonic() - began, 3)
            if status not in (200, 201):
                row["ok"], row["error"] = False, [status, made]
                print("FAIL fork", n, status, flush=True)
                continue
            child = made[0]["sandbox"]["sandboxID"]
            row["child"] = child
            try:
                began = time.monotonic()
                try:
                    row["ok"] = sh(child, "echo alive", args.within) == "alive"
                except (AssertionError, OSError) as error:
                    row["ok"], row["error"] = False, str(error)[:300]
                row["answer_seconds"] = round(time.monotonic() - began, 3)
            finally:
                request("DELETE", f"/sandboxes/{child}")
            print(("ok" if row["ok"] else "FAIL"), "fork", n, row["answer_seconds"], flush=True)
        failed = [r["n"] for r in report["forks"] if not r.get("ok")]
        report["failed_forks"] = failed
        request("DELETE", f"/sandboxes/{parent}")
        report["success"] = not failed
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
