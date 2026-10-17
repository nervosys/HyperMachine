#!/usr/bin/env python3
"""V8's Web Tooling Benchmark in a 4 vCPU, 8 GiB sandbox from the tools image, on real KVM.

Runs an owned hv2-sandboxd with the network on and the tools image
(docs/TOOLS_IMAGE.md) as its template, creates one sandbox of 4 vCPUs and
8 GiB, and inside it, with the Internet:

1. `git clone` of https://github.com/v8/web-tooling-benchmark, the commit
   recorded in the report;
2. `npm install` and `npm run build`, each timed;
3. `node dist/cli.js` --runs times; each prints a geometric mean in runs/s.

This is the benchmark behind the "Node.js web tooling" row of the public
hpc-sandbox-benchmarks leaderboard (runs/s, 4 vCPU, 8 GiB). It is not
necessarily that leaderboard's exact invocation (it runs Phoronix Test Suite's
profile), and the Node.js here is Ubuntu 24.04's 18.19.1. The times are the
check's own stopwatch.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, re, socket, statistics, subprocess, time
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
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--cpus", type=int, default=4)
    parser.add_argument("--memory-mb", type=int, default=8192)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "V8 Web Tooling Benchmark inside a tools-image sandbox on KVM; the check's own stopwatch",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "vcpus": args.cpus, "memory_mb": args.memory_mb,
              "host_cpus": os.cpu_count(), "steps": [], "runs_per_second": []}
    report["input_sha256"]["driver"] = digest(__file__)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", str(args.memory_mb), "--cpu-cores", str(args.cpus), "--capacity", "2",
         "--network", "--egress-default", "allow"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=1800):
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

    sandbox = None

    def sh(cmd, within=1500):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        return value

    def step(name, cmd, within=1500, must=True):
        began = time.monotonic()
        value = sh(cmd, within)
        took = round(time.monotonic() - began, 2)
        row = {"step": name, "seconds": took, "exit_code": value.get("exit_code"),
               "output_tail": (value.get("stdout", "") + value.get("stderr", ""))[-1500:]}
        report["steps"].append(row)
        print(f"{name}: {took}s exit={row['exit_code']}", flush=True)
        if must:
            assert row["exit_code"] == 0, row
        return value

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
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 3600,
                                                      "cpuCount": args.cpus, "memoryMB": args.memory_mb})
        assert status in (200, 201), (status, made)
        sandbox = made["sandboxID"]
        seen = sh("grep -c ^processor /proc/cpuinfo; grep MemTotal /proc/meminfo; node --version; npm --version")
        report["guest"] = seen["stdout"].split("\n")
        assert int(report["guest"][0]) == args.cpus, report["guest"]
        step("clone", "cd /root && rm -rf wtb && git clone -q https://github.com/v8/web-tooling-benchmark wtb"
                      " && git -C wtb rev-parse HEAD")
        report["commit"] = report["steps"][-1]["output_tail"].strip().splitlines()[-1]
        step("npm install", "cd /root/wtb && npm install --no-audit --no-fund 2>&1 | tail -5", 1700)
        step("npm run build", "cd /root/wtb && npm run build 2>&1 | tail -5", 1700)
        for i in range(args.runs):
            value = step(f"run {i + 1}", "cd /root/wtb && node dist/cli.js 2>&1 | tail -30", 900)
            found = re.findall(r"Geometric mean:\s*([0-9.]+)\s*runs/s", value["stdout"] + value["stderr"])
            assert found, value
            report["runs_per_second"].append(float(found[-1]))
        report["median_runs_per_second"] = statistics.median(report["runs_per_second"])
        report["success"] = True
    finally:
        if sandbox:
            request("DELETE", f"/sandboxes/{sandbox}")
        daemon.terminate()
        try:
            daemon.wait(timeout=20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("runs/s:", report["runs_per_second"], "median", report.get("median_runs_per_second"), flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
