#!/usr/bin/env python3
"""A sandbox's own size on real KVM: processors and memory chosen at create.

Runs an owned hv2-sandboxd whose template is one vCPU and 256 MiB, and
checks, through its API and from inside each guest:

1. A sandbox created without a size is the template's, restored.
2. One created with `cpuCount` 2 and `memoryMB` 1024 has them: the guest
   counts two processors and reports about a gibibyte, and its record says
   so.
3. Paused and resumed, it is still that size; a fork of it is that size too.
4. Its usage counts its own vCPUs.
5. Asking for the template's own size is no change, and restores.
6. A size that cannot be is refused, and nothing is made.

Writes report.json and the daemon's log into --output.
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
    report = {"success": False,
              "purpose": "functional verification of per-sandbox sizes on KVM; the start times are the "
                         "check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-sandbox-sizes-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "6"],
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

    def create(**body):
        began = time.monotonic()
        status, sandbox = request("POST", "/sandboxes", dict(templateID="base", timeout=600, **body))
        assert status in (200, 201), (status, sandbox)
        return sandbox["sandboxID"], round(time.monotonic() - began, 3)

    def seen(sandbox):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec",
                                {"cmd": "grep -c ^processor /proc/cpuinfo; grep MemTotal /proc/meminfo", "timeout_secs": 30})
        assert status == 200, (status, value)
        cpus, mem = value["stdout"].splitlines()
        return int(cpus), int(mem.split()[1])

    def record(sandbox):
        status, value = request("GET", f"/sandboxes/{sandbox}")
        assert status == 200, (status, value)
        return value.get("cpuCount"), value.get("memoryMB")

    try:
        deadline = time.monotonic() + 180
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

        def template_size():
            sandbox, took = create()
            cpus, mem_kb = seen(sandbox)
            assert cpus == 1 and mem_kb < 256 * 1024, (cpus, mem_kb)
            assert record(sandbox) == (1, 256), record(sandbox)
            request("DELETE", f"/sandboxes/{sandbox}")
            return {"processors": cpus, "memory_kb": mem_kb, "start_seconds": took}
        case("without a size, a sandbox is the template's", template_size)

        def own_size():
            sandbox, took = create(cpuCount=2, memoryMB=1024)
            state["big"] = sandbox
            cpus, mem_kb = seen(sandbox)
            assert cpus == 2 and 900 * 1024 < mem_kb <= 1024 * 1024, (cpus, mem_kb)
            assert record(sandbox) == (2, 1024), record(sandbox)
            return {"processors": cpus, "memory_kb": mem_kb, "start_seconds": took}
        case("with cpuCount 2 and memoryMB 1024 it has them, and its record says so", own_size)

        def kept():
            sandbox = state["big"]
            for action in ["pause", "resume"]:
                status, value = request("POST", f"/sandboxes/{sandbox}/{action}", {})
                assert status in (200, 201, 204), (action, status, value)
            assert seen(sandbox) == seen(sandbox) and seen(sandbox)[0] == 2, seen(sandbox)
            assert record(sandbox) == (2, 1024)
            status, forks = request("POST", f"/sandboxes/{sandbox}/fork", {"count": 1})
            assert status in (200, 201) and "sandbox" in forks[0], (status, forks)
            fork = forks[0]["sandbox"]["sandboxID"]
            cpus, mem_kb = seen(fork)
            assert cpus == 2 and 900 * 1024 < mem_kb <= 1024 * 1024, (cpus, mem_kb)
            assert record(fork) == (2, 1024), record(fork)
            request("DELETE", f"/sandboxes/{fork}")
            # Booted, so not made a template or checkpointed: said, not done.
            status, value = request("POST", f"/sandboxes/{sandbox}/snapshots", {"name": "sized-snap"})
            assert status == 409 and "size of its own" in value["message"], (status, value)
            status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "c1"})
            assert status == 409 and "size of its own" in value["message"], (status, value)
            return {"after_resume": seen(sandbox), "fork": [cpus, mem_kb], "snapshot_refused": value["message"]}
        case("paused and resumed it keeps its size, and a fork of it has it too", kept)

        def usage():
            time.sleep(2)
            status, value = request("GET", "/usage?by=sandbox")
            mine = [g for g in value["groups"] if g["key"] == state["big"]][0]
            assert abs(mine["vcpuSeconds"] - 2 * mine["runningSeconds"]) < 1e-6, mine
            assert abs(mine["memoryGiBSeconds"] - mine["runningSeconds"]) < 1e-6, mine
            request("DELETE", f"/sandboxes/{state['big']}")
            return {"usage": mine}
        case("its usage counts its own vCPUs and memory", usage)

        def same_as_template():
            sandbox, took = create(cpuCount=1, memoryMB=256)
            assert seen(sandbox)[0] == 1 and record(sandbox) == (1, 256)
            request("DELETE", f"/sandboxes/{sandbox}")
            return {"start_seconds": took}
        case("asking for the template's own size is no change", same_as_template)

        def refusals():
            results = {}
            before = request("GET", "/sandboxes")[1]
            for label, body in [("no processors", {"cpuCount": 0}),
                                ("more processors than the host has", {"cpuCount": 4096}),
                                ("too little memory", {"memoryMB": 128}),
                                ("an odd amount of memory", {"memoryMB": 1025}),
                                ("too much memory", {"memoryMB": 1 << 20})]:
                status, value = request("POST", "/sandboxes", dict(templateID="base", **body))
                assert status == 400, (label, status, value)
                results[label] = value["message"]
            assert request("GET", "/sandboxes")[1] == before, "a refusal made a sandbox"
            return results
        case("a size that cannot be is refused, and nothing is made", refusals)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
