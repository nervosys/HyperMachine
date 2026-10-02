#!/usr/bin/env python3
"""Summarize first backend-return diagnostics without scoring traced latencies."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import statistics


def summarize(values):
    if not values:
        return None
    if not all(math.isfinite(value) and value >= 0 for value in values):
        raise ValueError("invalid diagnostic duration")
    return {"n": len(values), "mean": statistics.mean(values),
        "median": statistics.median(values), "maximum": max(values)}


def analyze(path):
    report = json.loads(path.read_text())
    if not report.get("diagnostic_only"):
        raise ValueError("requires an explicitly diagnostic cohort")
    rows = [row for batch in report["batches"] if batch["engine"] == "hypermachine"
        for row in batch["samples"]]
    passed = {row["sandbox_id"] for row in rows if row["success"] and row["cleanup_success"]}
    kinds = report["first_exit_kinds"]
    stages = report["dispatch_stages_ms"]
    readiness = report["cold_readiness_stages_ms"]
    matched = sorted(passed & kinds.keys() & stages.keys() & readiness.keys())
    histogram = Counter((kinds[name]["kind"], kinds[name]["io_port"]) for name in matched)
    return {"diagnostic_only": True, "performance_comparison": False,
        "cohort_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "cohort_success": report["success"], "hm_attempted": len(rows),
        "hm_passed_and_cleaned": len(passed), "matched_successful_ids": len(matched),
        "unmatched_passed_ids": sorted(passed - set(matched)),
        "other_first_exit_ids": sorted(kinds.keys() - passed),
        "first_exit_histogram": [{"kind": kind, "io_port": port, "n": count}
            for (kind, port), count in sorted(histogram.items(), key=lambda row: str(row[0]))],
        "successful_dispatch_ms": {field: summarize([stages[name][field] for name in matched])
            for field in ["dispatch_queue_ms", "wrapper_queue_ms", "thread_start_ms", "owner_setup_ms", "first_backend_ms"]},
        "successful_readiness_ms": {field: summarize([readiness[name][field] for name in matched])
            for field in ["blocking_queue_ms", "connect_ms", "ping_ms"]},
        "limitations": ["Summaries are conditional on successful, cleaned-up matched guests; raw failures remain in the cohort",
            "Backend duration includes host scheduling and guest execution; no CPU-work attribution",
            "First exit does not identify where the failed earlier cohort stalled",
            "Tracing affects timing; excluded from competitor latency rankings"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cohort", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output already exists")
    args.output.write_text(json.dumps(analyze(args.cohort), indent=2) + "\n")
