#!/usr/bin/env python3
"""Validate periodic reclaim attempts and paired memory/readiness tradeoffs."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import statistics

spec = importlib.util.spec_from_file_location("paired", Path(__file__).with_name("analyze-connection-wait.py"))
paired = importlib.util.module_from_spec(spec)
spec.loader.exec_module(paired)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def analyze(report):
    require(isinstance(report["interval_ms"], int) and 100 <= report["interval_ms"] <= 60000, "invalid reclaim interval")
    require(report["cpu_count"] == 1 and report["memory_mb"] == 1024 and report["cold_start_concurrency"] == 8, "resource/budget mismatch")
    require(report["queue_included_in_ready_ms"] and report["guest_readiness_timeout_s"] == 15 and report["memory_idle_seconds"] == 5, "timing scope mismatch")
    require(len(report["runs"]) == report["pairs"] * 2, "missing planned runs")
    for index, row in enumerate(report["runs"]):
        pair = index // 2
        order = ["baseline", "candidate"] if pair % 2 == 0 else ["candidate", "baseline"]
        require(row["pair"] == pair and row["variant"] == order[index % 2], "order mismatch")
        enabled = row["variant"] == "candidate"
        require(row["reclaim_interval_ms"] == (report["interval_ms"] if enabled else None), "interval mismatch")
        argv = row["daemon_argv"]
        for flag, value in [("--cold-start-concurrency", "8"), ("--cpu-cores", "1"), ("--memory-mb", "1024")]:
            require(argv.count(flag) == 1 and argv[argv.index(flag)+1] == value, "actual guest configuration mismatch")
        summary = row["reclaim_worker_summary"]
        if enabled:
            require(summary is not None and summary["calls"] > 0 and 0 <= summary["release_calls"] <= summary["calls"] and 0 <= summary["maximum_ns"] <= summary["total_ns"], "worker summary mismatch")
        else:
            require(summary is None, "unexpected baseline worker")
        samples = row.get("batch", {}).get("samples", [])
        require(len(samples) == report["concurrency"] and {s["index"] for s in samples} == set(range(report["concurrency"])), "missing planned attempts")
        require(all(s["pair"] == pair for s in samples), "attempt pair mismatch")
        require(all(math.isfinite(s["ready_ms"]) and s["ready_ms"] >= 0 for s in samples if s["success"]), "invalid readiness")
        if row["success"]:
            batch = row["batch"]
            template = row["template"]
            require(template["cpuCount"] == 1 and template["memoryMB"] == 1024 and template["snapshot"] is False, "cold template mismatch")
            require(all(s["success"] and s["cleanup_success"] for s in samples), "false success")
            require(batch["all_guests_validated_while_held"] and batch["memory_idle_actual_seconds"] >= 5 and batch["guest_idle_at_measurement_start_ms"]["min"] >= 5000, "idle hold mismatch")
            before = row["empty_node_memory_before_batch_kib"]["Pss_kib"]
            require(batch["held_process_count"] == 1 and batch["empty_process_memory_baseline_kib"]["Pss_kib"] == before, "memory baseline mismatch")
            require(batch["incremental_idle_process_memory_kib"]["Pss_kib"] == batch["idle_process_memory_kib"]["Pss_kib"] - before, "memory delta mismatch")
    result = paired.analyze(report)
    for pair in result["pairs"]:
        rows = report["runs"][pair["pair"]*2:pair["pair"]*2+2]
        complete = pair["complete_successful_pair"]
        for quantile in [.5, .99]:
            values = {r["variant"]: paired.percentile([s["ready_ms"] for s in r["batch"]["samples"]], quantile) for r in rows} if complete else {}
            pair[f"candidate_p{int(quantile*100)}_reduction_ms"] = values["baseline"] - values["candidate"] if complete else None
        for name, read in [
            ("held", lambda r: r["batch"]["idle_process_memory_kib"]["Pss_kib"]),
            ("incremental", lambda r: r["batch"]["incremental_idle_process_memory_kib"]["Pss_kib"]),
            ("after_cleanup", lambda r: r["empty_node_memory_after_cleanup_kib"]["Pss_kib"]),
        ]:
            values = {r["variant"]: read(r) for r in rows} if complete else {}
            pair[f"candidate_{name}_pss_reduction_mib"] = (values["baseline"] - values["candidate"]) / 1024 if complete else None
    result["memory"] = {}
    for variant in ["baseline", "candidate"]:
        rows = [r for r in report["runs"] if r["variant"] == variant and r["success"]]
        result["memory"][variant] = {"valid_batches": len(rows),
            "median_held_pss_mib": statistics.median(r["batch"]["idle_process_memory_kib"]["Pss_kib"] / 1024 for r in rows) if rows else None,
            "median_incremental_pss_mib": statistics.median(r["batch"]["incremental_idle_process_memory_kib"]["Pss_kib"] / 1024 for r in rows) if rows else None,
            "median_after_cleanup_pss_mib": statistics.median(r["empty_node_memory_after_cleanup_kib"]["Pss_kib"] / 1024 for r in rows) if rows else None}
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; retain earlier analyses")
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())), indent=2) + "\n")
