#!/usr/bin/env python3
"""Validate and summarize matched native engines with an explicit cold budget."""
import argparse
import json
import math
from pathlib import Path
import statistics


def require(condition, message):
    if not condition:
        raise ValueError(message)


def percentile(values, fraction):
    return sorted(values)[math.ceil(len(values) * fraction) - 1] if values else None


def analyze(report):
    require(report["cpu_count"] == 1 and report["memory_mb"] == 1024, "guest resource mismatch")
    require(report["queue_included_in_ready_ms"], "missing queue timing")
    require(report["guest_readiness_timeout_s"] == {"hypermachine": 15, "firecracker": 15}, "deadline mismatch")
    require(report["memory_idle_seconds"] == 5, "idle hold mismatch")
    argv = report["daemon_argv"]
    require(argv.count("--cold-start-concurrency") == 1, "missing actual budget")
    require(int(argv[argv.index("--cold-start-concurrency") + 1]) == report["cold_start_concurrency"], "budget mismatch")
    require(len(report["batches"]) == report["pairs"] * 2, "missing batches")
    cleanup = not report["cleanup_errors"] and report["remaining_sandbox_count"] == 0 and report["daemon_exit_code"] is not None
    paired = []
    for pair in range(report["pairs"]):
        batches = report["batches"][pair * 2:pair * 2 + 2]
        expected = ["hypermachine", "firecracker"] if pair % 2 == 0 else ["firecracker", "hypermachine"]
        require([b["engine"] for b in batches] == expected and all(b["pair"] == pair for b in batches), "order mismatch")
        for batch in batches:
            samples = batch["samples"]
            require(len(samples) == report["concurrency"], "missing planned attempts")
            require({s["index"] for s in samples} == set(range(report["concurrency"])), "attempt identity mismatch")
            for sample in samples:
                require(sample["pair"] == pair and sample["engine"] == batch["engine"], "sample identity mismatch")
                if sample["success"]:
                    require(math.isfinite(sample["ready_ms"]) and sample["ready_ms"] >= 0, "invalid readiness")
                cleanup = cleanup and (sample["cleanup_success"] or (sample.get("failure_phase") == "create" and not sample.get("cleanup_error")))
            if batch["success"]:
                require(all(s["success"] and s["cleanup_success"] for s in samples), "false batch success")
                require(batch["all_guests_validated_while_held"] and batch["memory_idle_actual_seconds"] >= 5 and batch["guest_idle_at_measurement_start_ms"]["min"] >= 5000, "invalid held measurement")
                require(batch["held_process_count"] == (1 if batch["engine"] == "hypermachine" else report["concurrency"]), "process count mismatch")
                held = batch["idle_process_memory_kib"]["Pss_kib"]
                base = batch["empty_process_memory_baseline_kib"]["Pss_kib"]
                require(batch["incremental_idle_process_memory_kib"]["Pss_kib"] == held - base, "memory delta mismatch")
                if batch["engine"] == "firecracker": require(base == 0, "Firecracker baseline mismatch")
        complete = all(b["success"] for b in batches)
        means = {b["engine"]: statistics.mean(s["ready_ms"] for s in b["samples"]) for b in batches} if complete else {}
        paired.append({"pair": pair, "complete": complete, "hypermachine_mean_reduction_ms": means["firecracker"] - means["hypermachine"] if complete else None})
    engines = {}
    for engine in ["hypermachine", "firecracker"]:
        batches = [b for b in report["batches"] if b["engine"] == engine]
        samples = [s for b in batches for s in b["samples"]]
        ready = [s["ready_ms"] for s in samples if s["success"] and s["cleanup_success"]]
        valid = [b for b in batches if b["success"]]
        engines[engine] = {"attempted": len(samples), "passed": len(ready), "failed": len(samples) - len(ready),
            "successful_p50_ms": percentile(ready, .5), "successful_p99_ms": percentile(ready, .99),
            "valid_memory_batches": len(valid),
            "held_pss_mib": statistics.median(b["idle_process_memory_kib"]["Pss_kib"] / 1024 for b in valid) if valid else None,
            "incremental_pss_mib": statistics.median(b["incremental_idle_process_memory_kib"]["Pss_kib"] / 1024 for b in valid) if valid else None}
    return {"engines": engines, "pairs": paired, "cleanup_verified": cleanup,
        "artifacts_unchanged": report["artifacts_unchanged"], "cohort_success": report["success"],
        "latencies_conditional_on_success": True, "managed_competitor_win_established": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; retain earlier results")
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())), indent=2) + "\n")
