#!/usr/bin/env python3
"""Counterbalance cold-start admission budgets, including queue time in readiness."""
import argparse
import importlib.util
import json
import os
from pathlib import Path

spec = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("bench-connection-wait.py"))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)
engines = comparison.engines


def run(args, binary, variant, pair):
    original = comparison.subprocess.Popen
    argv = []
    limit = args.candidate_limit if variant == "candidate" else args.baseline_limit

    def launch(command, *positional, **kwargs):
        command = list(command)
        if Path(command[0]).resolve() == binary and "--no-template" in command:
            if limit:
                command.extend(["--cold-start-concurrency", str(limit)])
            argv.extend(command)
        return original(command, *positional, **kwargs)

    comparison.subprocess.Popen = launch
    try:
        row = comparison.run(args, binary, variant, pair)
    finally:
        comparison.subprocess.Popen = original
    row["cold_start_limit"] = limit or None
    row["daemon_argv"] = argv
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["baseline", "candidate", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=4)
    parser.add_argument("--concurrency", type=int, default=100)
    parser.add_argument("--candidate-limit", type=int, default=8, help="1..1024 enables the candidate budget; 0 leaves it disabled for binary regression controls")
    parser.add_argument("--baseline-limit", type=int, default=0, help="0 leaves the baseline disabled; 1..1024 compares two enabled budgets")
    parser.add_argument("--memory-idle-seconds", type=float, default=0, help="0 disables held-memory measurement; otherwise hold guests for 1..30 seconds")
    args = parser.parse_args()
    if not 1 <= args.pairs <= 10 or not 1 <= args.concurrency <= 100 or not all(0 <= limit <= 1024 for limit in [args.baseline_limit, args.candidate_limit]):
        parser.error("requires 1..10 pairs, 1..100 guests, 0..1024 admission settings")
    if not (args.memory_idle_seconds == 0 or 1 <= args.memory_idle_seconds <= 30):
        parser.error("memory idle hold must be 0 or 1..30 seconds")
    if args.output.exists(): parser.error("output exists; preserve earlier attempts")
    args.daemon_log_filter = "warn"
    inputs = {name:getattr(args,name).resolve(strict=True) for name in ["baseline", "candidate", "kernel", "initrd"]}
    for name,path in inputs.items(): setattr(args,name,path)
    inputs.update(harness=Path(__file__).resolve(), comparison=Path(comparison.__file__).resolve(),
        burst=Path(comparison.burst.__file__).resolve(), shared=Path(engines.__file__).resolve(),
        firecracker=Path(comparison.burst.fc.__file__).resolve())
    identities = {name:engines.digest(path) for name,path in inputs.items()}
    affinity = sorted(os.sched_getaffinity(0))[:8]
    os.sched_setaffinity(0, affinity)
    report = {"purpose":"cold-boot admission experiment; no competitor win claim",
        "artifact_sha256":identities, "driver_cpu_affinity":affinity, "added_CPU_load":False,
        "concurrency":args.concurrency, "pairs":args.pairs, "candidate_limit":args.candidate_limit,
        "baseline_limit":args.baseline_limit,
        "order":"fresh-daemon AB/BA", "guest_readiness_timeout_s":15, "cpu_count":1, "memory_mb":1024,
        "queue_included_in_ready_ms":True, "memory_idle_seconds":args.memory_idle_seconds,
        "memory_method":"held daemon PSS and same-batch empty-daemon subtraction; excludes kernel memory",
        "same_binary":identities["baseline"] == identities["candidate"],
        "limitations":["Shared nested-KVM host", "Cold native startup only",
            "Conditional latency summaries exclude failed attempts; all failures retained",
            "No managed competitor endpoint or optimal admission-limit claim"], "runs":[], "success":False}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for pair in range(args.pairs):
        for variant in (("baseline","candidate") if pair % 2 == 0 else ("candidate","baseline")):
            row = run(args, inputs[variant], variant, pair)
            report["runs"].append(row)
            args.output.write_text(json.dumps(report,indent=2)+"\n")
            print(json.dumps({"pair":pair,"variant":variant,"success":row["success"]}),flush=True)
    report["artifacts_unchanged"] = all(engines.digest(path) == identities[name] for name,path in inputs.items())
    report["success"] = report["artifacts_unchanged"] and all(row["success"] for row in report["runs"])
    report["ready_ms"] = {variant:comparison.burst.fc.summary([sample["ready_ms"]
        for row in report["runs"] if row["variant"] == variant
        for sample in row.get("batch",{}).get("samples",[]) if sample["success"] and sample["cleanup_success"]])
        for variant in ["baseline","candidate"]}
    args.output.write_text(json.dumps(report,indent=2)+"\n")
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
