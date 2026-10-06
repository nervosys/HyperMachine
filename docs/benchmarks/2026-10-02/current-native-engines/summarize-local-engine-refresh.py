#!/usr/bin/env python3
"""Summarize captured native comparisons without hiding incomplete batches."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics


def summarize(directory):
    matrix = json.loads((directory / "matrix.json").read_text())
    result = {"purpose": matrix["purpose"], "cohort_success": matrix["success"],
              "profiles": [], "attempts": 0, "passed": 0}
    for execution in matrix["profiles"]:
        path = directory / execution["report"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == execution["sha256"]
        report = json.loads(path.read_text())
        assert report["concurrency"] == execution["concurrency"] and report["pairs"] == execution["pairs"]
        assert report["driver_cpu_affinity"] == matrix["driver_cpu_affinity"]
        assert report["cpu_count"] == 1 and report["memory_mb"] == 1024
        assert report["memory_idle_seconds"] == 5
        for name in ("hypermachine", "firecracker", "kernel", "initrd", "harness"):
            assert report["artifact_sha256"][name] == matrix["artifact_sha256"][name]
        profile = {"concurrency": report["concurrency"], "success": execution["success"],
                   "cleanup_verified": not report["cleanup_errors"] and report.get("remaining_sandbox_count") == 0,
                   "engines": {}}
        for engine in ("hypermachine", "firecracker"):
            batches = [batch for batch in report["batches"] if batch["engine"] == engine]
            samples = [row for batch in batches for row in batch["samples"]]
            passing = [row for row in samples if row["success"] and row["cleanup_success"]]
            values = sorted(row["ready_ms"] for row in passing)
            latency = None if not values else {"n": len(values), "mean": statistics.mean(values),
                       **{f"p{p}": values[math.ceil(p / 100 * len(values)) - 1] for p in (50, 95, 99)}}
            held, incremental = [], []
            for batch in batches:
                if not batch["success"] or not batch.get("all_guests_validated_while_held"):
                    continue
                assert batch["memory_idle_actual_seconds"] >= 5
                assert batch["guest_idle_at_measurement_start_ms"]["min"] >= 5000
                assert batch["held_process_count"] == (1 if engine == "hypermachine" else report["concurrency"])
                memory = batch["idle_process_memory_kib"]["Pss_kib"]
                baseline = batch["empty_process_memory_baseline_kib"]["Pss_kib"]
                delta = batch["incremental_idle_process_memory_kib"]["Pss_kib"]
                assert delta == memory - baseline
                if engine == "firecracker":
                    assert baseline == 0
                held.append(memory / 1024)
                incremental.append(delta / 1024)
            profile["engines"][engine] = {
                "planned": report["concurrency"] * report["pairs"], "attempts": len(samples),
                "passed": len(passing), "failed": len(samples) - len(passing),
                "successful_readiness_ms": latency, "valid_memory_batches": len(held),
                "held_idle_pss_mib_median": statistics.median(held) if held else None,
                "incremental_idle_pss_mib_median": statistics.median(incremental) if incremental else None}
            result["attempts"] += len(samples)
            result["passed"] += len(passing)
        result["profiles"].append(profile)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    result = summarize(args.directory)
    (args.directory / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
