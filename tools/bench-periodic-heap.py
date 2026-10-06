#!/usr/bin/env python3
"""Counterbalance an isolated periodic reclaim candidate, keeping all attempts."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re

spec = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("bench-connection-wait.py"))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)
engines = comparison.engines


def run(args, variant, pair):
    original = comparison.subprocess.Popen
    argv = []
    def launch(command, *positional, **kwargs):
        command = list(command)
        if Path(command[0]).resolve() == args.daemon and "--no-template" in command:
            command.extend(["--cold-start-concurrency", "8"])
            env = dict(kwargs["env"])
            if "HV2_EXPERIMENT_HEAP_RECLAIM_MS" in env: raise ValueError("inherited experiment setting")
            if variant == "candidate": env["HV2_EXPERIMENT_HEAP_RECLAIM_MS"] = str(args.interval_ms)
            kwargs["env"] = env
            argv.extend(command)
        return original(command, *positional, **kwargs)
    comparison.subprocess.Popen = launch
    try:
        row = comparison.run(args, args.daemon, variant, pair)
    finally:
        comparison.subprocess.Popen = original
    row["daemon_argv"] = argv
    row["reclaim_interval_ms"] = args.interval_ms if variant == "candidate" else None
    summaries = re.findall(r"HV2_HEAP_RECLAIM_EXPERIMENT (\{[^\n]*\})", row["node_log_tail"])
    row["reclaim_worker_summary"] = json.loads(summaries[0]) if len(summaries) == 1 else None
    if (variant == "candidate" and (len(summaries) != 1 or row["reclaim_worker_summary"]["calls"] <= 0)) or (variant == "baseline" and summaries):
        row["success"] = False
        row["worker_error"] = "worker summary does not match configured interval"
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]: parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=4)
    parser.add_argument("--concurrency", type=int, default=100)
    parser.add_argument("--interval-ms", type=int, default=250)
    args = parser.parse_args()
    if not 1 <= args.pairs <= 4 or not 1 <= args.concurrency <= 100 or not 100 <= args.interval_ms <= 60000: parser.error("invalid bounded experiment settings")
    if args.output.exists(): parser.error("output exists; preserve earlier attempts")
    args.memory_idle_seconds = 5
    args.daemon_log_filter = "warn"
    paths = {name: getattr(args, name).resolve(strict=True) for name in ["daemon", "kernel", "initrd"]}
    for name, path in paths.items(): setattr(args, name, path)
    paths.update(driver=Path(__file__).resolve(), comparison=Path(comparison.__file__).resolve(),
                 burst=Path(comparison.burst.__file__).resolve(), shared=Path(engines.__file__).resolve(),
                 firecracker_harness=Path(comparison.burst.fc.__file__).resolve())
    identities = {name: engines.digest(path) for name, path in paths.items()}
    affinity = sorted(os.sched_getaffinity(0))[:8]
    os.sched_setaffinity(0, affinity)
    report = {"purpose": "periodic free-heap reclaim candidate; no competitor win claim",
        "pairs": args.pairs, "concurrency": args.concurrency, "interval_ms": args.interval_ms,
        "artifact_sha256": identities, "driver_cpu_affinity": affinity, "added_CPU_load": False,
        "order": "fresh-daemon AB/BA", "cpu_count": 1, "memory_mb": 1024,
        "cold_start_concurrency": 8, "guest_readiness_timeout_s": 15, "memory_idle_seconds": 5,
        "queue_included_in_ready_ms": True, "runs": [], "success": False,
        "limitations": ["Shared nested-KVM host; background load uncontrolled", "Cold native startup only",
            "All attempts retained; successful latency is conditional", "No managed competitor or whole-host density claim"]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for pair in range(args.pairs):
        for variant in (["baseline", "candidate"] if pair % 2 == 0 else ["candidate", "baseline"]):
            row = run(args, variant, pair)
            report["runs"].append(row)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({"pair": pair, "variant": variant, "success": row["success"]}), flush=True)
    report["artifacts_unchanged"] = all(engines.digest(path) == identities[name] for name, path in paths.items())
    report["success"] = report["artifacts_unchanged"] and all(row["success"] for row in report["runs"])
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
