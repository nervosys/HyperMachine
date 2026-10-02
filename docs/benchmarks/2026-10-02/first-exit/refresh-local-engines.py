#!/usr/bin/env python3
"""Capture a bounded native-engine comparison matrix on an owned Linux host."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    for name in ("hypermachine", "firecracker", "kernel", "initrd", "harness", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=2)
    parser.add_argument("--profiles", default="1,8,50,100")
    parser.add_argument("--cold-readiness", action="store_true", help="use the diagnostic wrapper's agent-stage tracing; timings are unscored")
    parser.add_argument("--first-exit", action="store_true", help="also require first backend exit kinds in the diagnostic wrapper")
    args = parser.parse_args()
    if args.first_exit: args.cold_readiness = True
    profiles = [int(value) for value in args.profiles.split(",")]
    if not 1 <= args.pairs <= 10 or not 1 <= len(profiles) <= 4 or any(not 1 <= value <= 100 for value in profiles):
        parser.error("requires 1..10 pairs and 1..4 concurrency profiles in 1..100")
    args.output.mkdir(parents=True, exist_ok=True)
    affinity = sorted(os.sched_getaffinity(0))[:8]
    os.sched_setaffinity(0, affinity)
    inputs = {name: getattr(args, name).resolve() for name in ("hypermachine", "firecracker", "kernel", "initrd", "harness")}
    identities = {name: digest(path) for name, path in inputs.items()}
    matrix = {"purpose": "current native-engine baseline; no causal improvement claim",
              "driver_cpu_affinity": affinity, "added_CPU_load": False,
              "diagnostic_only": args.cold_readiness,
              "artifact_sha256": identities, "coordinator_sha256": digest(Path(__file__)),
              "profiles": [], "success": False}
    for concurrency in profiles:
        command = [sys.executable, str(inputs["harness"])]
        for name in ("hypermachine", "firecracker", "kernel", "initrd"):
            command.extend(["--" + name, str(inputs[name])])
        command.extend(["--environment", "shared-WSL-nested-KVM-eight-host-CPU-affinity-no-added-load",
                        "--pairs", str(args.pairs), "--concurrency", str(concurrency),
                        "--memory-idle-seconds", "5", "--timeout", "30"])
        if args.cold_readiness:
            command.append("--collect-cold-readiness")
        if args.first_exit:
            command.append("--collect-first-exit")
        report_path = args.output / f"c{concurrency}.json"
        with report_path.open("wb") as output, report_path.with_suffix(".stderr.log").open("wb") as errors:
            child = subprocess.Popen(command, stdout=output, stderr=errors, start_new_session=True)
            timed_out = False
            try:
                child.wait(timeout=1200)
            except subprocess.TimeoutExpired:
                timed_out = True
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)
        row = {"concurrency": concurrency, "pairs": args.pairs, "exit_code": child.returncode,
               "timed_out": timed_out, "report": report_path.name, "sha256": digest(report_path)}
        try:
            report = json.loads(report_path.read_text())
            row["success"] = child.returncode == 0 and report["success"]
            row["ready_ms"] = report["ready_ms"]
        except (ValueError, KeyError) as error:
            row.update(success=False, parse_error=str(error))
        matrix["profiles"].append(row)
        (args.output / "matrix.json").write_text(json.dumps(matrix, indent=2) + "\n")
        print(json.dumps(row), flush=True)
    matrix["artifacts_unchanged"] = all(digest(path) == identities[name] for name, path in inputs.items())
    matrix["success"] = matrix["artifacts_unchanged"] and all(row["success"] for row in matrix["profiles"])
    (args.output / "matrix.json").write_text(json.dumps(matrix, indent=2) + "\n")
    return 0 if matrix["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
