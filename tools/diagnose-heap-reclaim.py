#!/usr/bin/env python3
"""Counterbalance sham/heap reclaim in fresh owned daemons; no scored win."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location("retention", Path(__file__).with_name("diagnose-memory-retention.py"))
retention = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retention)
burst = retention.bursts
engines = burst.engines


def require(condition, message):
    if not condition:
        raise ValueError(message)


def run(args, variant, pair):
    row = {"variant": variant, "pair": pair, "success": False, "snapshots": [], "operations": [], "cleanup_errors": []}
    process = None
    parent, child = socket.socketpair()
    parent.settimeout(15)
    stream = parent.makefile("rb")
    command = b"T" if variant == "trim" else b"N"
    with tempfile.TemporaryDirectory(prefix="hm-heap-probe-", dir="/var/tmp") as scratch:
        scratch = Path(scratch)
        port, proxy = engines.free_port(), engines.free_port()
        while proxy == port: proxy = engines.free_port()
        url = f"http://127.0.0.1:{port}"
        env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "HV2_KERNEL": str(args.kernel),
               "HV2_INITRD": str(args.initrd), "RUST_LOG": "warn", "LD_PRELOAD": str(args.helper),
               "HM_MEMORY_PROBE_FD": str(child.fileno())}
        argv = [str(args.daemon), "--port", str(port), "--proxy-port", str(proxy),
                "--cpu-cores", "1", "--memory-mb", "1024", "--capacity", "128", "--no-template",
                "--cold-start-concurrency", "8", "--volume-dir", str(scratch / "volumes"),
                "--snapshot-store", str(scratch / "snapshots")]
        row["daemon_argv"] = argv
        def observe(stage):
            value = {"stage": stage, "observed_monotonic_ns": time.monotonic_ns(), **retention.snapshot(process.pid)}
            row["snapshots"].append(value)
            return value
        def act(stage):
            parent.sendall(command)
            value = json.loads(stream.readline(256))
            require(value["operation"] == variant and value["result"] in (0, 1) and value["duration_ns"] >= 0, "invalid probe reply")
            row["operations"].append({"stage": stage, **value})
        try:
            with (scratch / "node.log").open("wb") as log:
                process = subprocess.Popen(argv, env=env, pass_fds=(child.fileno(),),
                    stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                child.close()
                deadline = time.monotonic() + 30
                while True:
                    require(process.poll() is None, "owned daemon exited")
                    try:
                        templates = engines.request(url, "GET", "/templates")
                        break
                    except OSError:
                        if time.monotonic() >= deadline: raise
                        time.sleep(.01)
                template = next(t for t in templates if "base" in t.get("aliases", []))
                require(template["snapshot"] is False and template["cpuCount"] == 1 and template["memoryMB"] == 1024, "cold template mismatch")
                row["template"] = template
                time.sleep(args.idle_seconds)
                observe("initial-empty")
                def held(pid):
                    time.sleep(args.idle_seconds)
                    observe("held-before")
                    act("held")
                    time.sleep(args.idle_seconds)
                    after = observe("held-after")
                    guests = engines.request(url, "GET", "/sandboxes")
                    require(len(guests) == args.concurrency, "held guest count mismatch")
                    checks = []
                    for guest in guests:
                        sid = guest["sandboxID"]
                        marker = "post-reclaim-" + sid
                        reply = engines.request(url, "POST", f"/sandboxes/{sid}/exec", {"cmd": f"printf '%s' '{marker}'", "timeout_secs": 10})
                        require(reply["exit_code"] == 0 and reply["stdout"] == marker and not reply.get("timed_out") and not reply.get("truncated"), "guest failed after probe")
                        checks.append(sid)
                    require(len(set(checks)) == args.concurrency, "duplicate held guest check")
                    row["post_action_guest_checks"] = checks
                    return after["process_memory_kib"]
                options = SimpleNamespace(concurrency=args.concurrency, node_pid=process.pid, url=url)
                row["batch"] = burst.batch(options, "hypermachine", pair, memory_reader=held)
                require(engines.request(url, "GET", "/sandboxes") == [], "batch retained guests")
                time.sleep(args.idle_seconds)
                observe("empty-before")
                act("empty")
                time.sleep(args.idle_seconds)
                observe("empty-after")
                row["success"] = row["batch"]["success"]
        except Exception as error:
            row["error"] = str(error)
        finally:
            if process is not None and process.poll() is None:
                try:
                    remaining = engines.request(url, "GET", "/sandboxes")
                    row["remaining_sandbox_count"] = len(remaining)
                    if remaining: row["cleanup_errors"].append("retained guests")
                    for guest in remaining: engines.request(url, "DELETE", f"/sandboxes/{guest['sandboxID']}")
                except Exception as error: row["cleanup_errors"].append(str(error))
            if process is not None:
                if not engines.stop(process): row["cleanup_errors"].append("daemon did not stop")
                row["daemon_exit_code"] = process.returncode
            stream.close()
            parent.close()
            child.close()
            if (scratch / "node.log").exists(): row["node_log_tail"] = (scratch / "node.log").read_bytes()[-8000:].decode(errors="replace")
    row["success"] = row["success"] and not row["cleanup_errors"] and row.get("remaining_sandbox_count") == 0 and row.get("daemon_exit_code") is not None
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "helper", "output"]: parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=2)
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument("--idle-seconds", type=float, default=5)
    args = parser.parse_args()
    if not 1 <= args.pairs <= 4 or not 1 <= args.concurrency <= 100 or not 1 <= args.idle_seconds <= 10: parser.error("invalid bounds")
    if args.output.exists(): parser.error("output exists; retain earlier attempts")
    paths = {name: getattr(args, name).resolve(strict=True) for name in ["daemon", "kernel", "initrd", "helper"]}
    for name, path in paths.items(): setattr(args, name, path)
    paths.update(driver=Path(__file__).resolve(), retention=Path(retention.__file__).resolve(),
        burst=Path(burst.__file__).resolve(), shared=Path(engines.__file__).resolve(), firecracker_harness=Path(engines.fc.__file__).resolve())
    hashes = {name: engines.digest(path) for name, path in paths.items()}
    affinity = sorted(os.sched_getaffinity(0))[:8]
    os.sched_setaffinity(0, affinity)
    report = {"diagnostic_only": True, "purpose": "free heap reclaimability; not a scored latency comparison",
        "artifact_sha256": hashes, "pairs": args.pairs, "concurrency": args.concurrency,
        "idle_seconds": args.idle_seconds, "cpu_affinity": affinity, "cold_start_concurrency": 8,
        "order": "fresh-daemon sham/trim AB/BA", "added_CPU_load": False, "runs": [], "success": False}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for pair in range(args.pairs):
        for variant in (["sham", "trim"] if pair % 2 == 0 else ["trim", "sham"]):
            row = run(args, variant, pair)
            report["runs"].append(row)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({"pair": pair, "variant": variant, "success": row["success"]}), flush=True)
    report["artifacts_unchanged"] = all(engines.digest(path) == hashes[name] for name, path in paths.items())
    report["success"] = report["artifacts_unchanged"] and all(row["success"] for row in report["runs"])
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
