#!/usr/bin/env python3
"""Summarize every connection-wait experiment attempt, including failures."""
import argparse
import json
import math
from pathlib import Path
import statistics


def percentile(values, quantile):
    return sorted(values)[max(0, math.ceil(len(values) * quantile) - 1)] if values else None


def analyze(report):
    assert len(report["runs"]) == report["pairs"] * 2
    variants = {}
    for variant in ("baseline", "candidate"):
        runs = [row for row in report["runs"] if row["variant"] == variant]
        samples = [sample for row in runs for sample in row.get("batch", {}).get("samples", [])]
        successful = [sample["ready_ms"] for sample in samples if sample["success"] and sample["cleanup_success"]]
        variants[variant] = {"planned": report["pairs"] * report["concurrency"], "attempted": len(samples),
            "passed": len(successful), "failed": len(samples) - len(successful),
            "successful_p50_ms": percentile(successful, .5), "successful_p99_ms": percentile(successful, .99),
            "failed_runs": sum(not row["success"] for row in runs)}
    paired = []
    for pair in range(report["pairs"]):
        runs = report["runs"][pair * 2:pair * 2 + 2]
        expected = ["baseline", "candidate"] if pair % 2 == 0 else ["candidate", "baseline"]
        assert [row["variant"] for row in runs] == expected
        assert all(row["pair"] == pair for row in runs)
        valid = all(row["success"] for row in runs)
        row = {"pair": pair, "complete_successful_pair": valid, "candidate_mean_reduction_ms": None}
        if valid:
            means = {run["variant"]: statistics.mean(sample["ready_ms"] for sample in run["batch"]["samples"]) for run in runs}
            row["candidate_mean_reduction_ms"] = means["baseline"] - means["candidate"]
        paired.append(row)
    differences = [row["candidate_mean_reduction_ms"] for row in paired if row["complete_successful_pair"]]
    cleanup = all(row.get("remaining_sandbox_count") == 0 and not row["cleanup_errors"]
        and row.get("daemon_exit_code") is not None
        and all(sample["cleanup_success"] or (sample.get("failure_phase") == "create"
            and not sample.get("cleanup_error"))
            for sample in row.get("batch", {}).get("samples", [])) for row in report["runs"])
    return {"variants": variants, "pairs": paired, "complete_successful_pairs": len(differences),
        "candidate_faster_pairs": sum(value > 0 for value in differences),
        "median_paired_mean_reduction_ms": statistics.median(differences) if differences else None,
        "cleanup_verified": cleanup, "artifacts_unchanged": report["artifacts_unchanged"],
        "cohort_success": report["success"], "latencies_conditional_on_success": True,
        "runtime_change_adopted": False, "competitor_win_established": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(json.loads(args.report.read_text())), indent=2))


if __name__ == "__main__":
    main()
