#!/usr/bin/env python3
"""Compare two owned HyperMachine daemons in counterbalanced cold-start bursts."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

spec = importlib.util.spec_from_file_location("burst", Path(__file__).with_name("bench-local-engines-concurrent.py"))
burst = importlib.util.module_from_spec(spec)
spec.loader.exec_module(burst)
engines = burst.engines


def run(args, binary, variant, pair):
    process = None
    report = {"variant": variant, "pair": pair, "cleanup_errors": [], "success": False}
    with tempfile.TemporaryDirectory(prefix="hm-connect-wait-", dir="/var/tmp") as directory:
        directory = Path(directory)
        port, proxy = engines.free_port(), engines.free_port()
        while proxy == port:
            proxy = engines.free_port()
        args.url = f"http://127.0.0.1:{port}"
        try:
            with (directory / "node.log").open("wb") as log:
                process = subprocess.Popen([str(binary), "--port", str(port), "--proxy-port", str(proxy),
                    "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "128", "--no-template",
                    "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots")],
                    env=burst.daemon_environment(args), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                args.node_pid = process.pid
                deadline = time.monotonic() + 30
                while True:
                    if process.poll() is not None:
                        raise RuntimeError("owned daemon exited before readiness")
                    try:
                        templates = engines.request(args.url, "GET", "/templates")
                        break
                    except OSError:
                        if time.monotonic() >= deadline:
                            raise
                        time.sleep(.01)
                base = next((item for item in templates if "base" in item.get("aliases", [])), None)
                if not base or base.get("snapshot") is not False or base.get("cpuCount") != 1 or base.get("memoryMB") != 1024:
                    raise RuntimeError("requires cold 1-vCPU/1024-MiB base template")
                report["template"] = base
                report["batch"] = burst.batch(args, "hypermachine", pair)
        except Exception as error:
            report["setup_error"] = str(error)
        finally:
            if process is not None and process.poll() is None:
                try:
                    remaining = engines.request(args.url, "GET", "/sandboxes")
                    report["remaining_sandbox_count"] = len(remaining)
                    if remaining:
                        report["cleanup_errors"].append("owned daemon retained guests")
                        for item in remaining:
                            engines.request(args.url, "DELETE", f"/sandboxes/{item['sandboxID']}")
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            if process is not None:
                try:
                    if not engines.stop(process):
                        report["cleanup_errors"].append("owned daemon did not stop")
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
                report["daemon_exit_code"] = process.returncode
            if (directory / "node.log").exists():
                report["node_log_tail"] = (directory / "node.log").read_bytes()[-8000:].decode(errors="replace")
    report["success"] = (report.get("batch", {}).get("success", False)
        and not report.get("setup_error") and not report["cleanup_errors"]
        and report.get("remaining_sandbox_count") == 0)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline", "candidate", "kernel", "initrd", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=4)
    parser.add_argument("--concurrency", type=int, default=100)
    args = parser.parse_args()
    if not 1 <= args.pairs <= 10 or not 1 <= args.concurrency <= 100:
        parser.error("requires 1..10 pairs and 1..100 guests")
    if args.output.exists():
        parser.error("output already exists; choose a new path to retain previous attempts")
    args.memory_idle_seconds = 0
    args.daemon_log_filter = "warn"
    inputs = {name: getattr(args, name).resolve() for name in ("baseline", "candidate", "kernel", "initrd")}
    for name, path in inputs.items():
        setattr(args, name, path)
    inputs.update(harness=Path(__file__).resolve(), burst=Path(burst.__file__).resolve(),
        shared=Path(engines.__file__).resolve(), firecracker=Path(burst.fc.__file__).resolve())
    identities = {name: engines.digest(path) for name, path in inputs.items()}
    affinity = sorted(os.sched_getaffinity(0))[:8]
    os.sched_setaffinity(0, affinity)
    report = {"purpose": "isolated connection-wait candidate; no competitor win claim",
        "artifact_sha256": identities, "driver_cpu_affinity": affinity, "added_CPU_load": False,
        "concurrency": args.concurrency, "pairs": args.pairs, "order": "fresh-daemon AB/BA",
        "guest_readiness_timeout_s": 15, "cpu_count": 1, "memory_mb": 1024,
        "limitations": ["Shared nested-KVM host", "Cold native startup only",
            "Conditional latency summaries exclude failed attempts; all failures are retained",
            "No measured wakeup count or CPU attribution", "No managed competitor endpoints"],
        "runs": [], "success": False}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for pair in range(args.pairs):
        for variant in (("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")):
            row = run(args, inputs[variant], variant, pair)
            report["runs"].append(row)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({"pair": pair, "variant": variant, "success": row["success"]}), flush=True)
    report["artifacts_unchanged"] = all(engines.digest(path) == identities[name] for name, path in inputs.items())
    report["success"] = report["artifacts_unchanged"] and all(row["success"] for row in report["runs"])
    report["ready_ms"] = {variant: burst.fc.summary([sample["ready_ms"]
        for row in report["runs"] if row["variant"] == variant
        for sample in row.get("batch", {}).get("samples", []) if sample["success"] and sample["cleanup_success"]])
        for variant in ("baseline", "candidate")}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
