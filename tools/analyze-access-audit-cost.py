#!/usr/bin/env python3
"""Summarize complete measured and warmup audit-cost cohorts without hiding failures."""
import argparse
import json
import math
from pathlib import Path
import statistics


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def analyze(matrix):
    profiles = []
    for concurrency in matrix["profiles"]:
        runs = [row for row in matrix["runs"] if row["concurrency"] == concurrency]
        variants = {}
        for variant in ("previous", "audit-off", "audit-on"):
            matching = [row for row in runs if row["variant"] == variant]
            samples = [sample for row in matching for batch in row["batches"] for sample in batch["samples"]]
            measured = [sample for sample in samples if sample["phase"] == "measured"]
            passing = [sample["latency_ms"] for sample in measured if sample["success"]]
            batches = [batch for row in matching for batch in row["batches"] if batch["phase"] == "measured" and batch["success"]]
            variants[variant] = {"planned_total": len(matrix["orders"]) * concurrency * (matrix["warmup_per_worker"] + matrix["requests_per_worker"]),
                "attempted_total": len(samples), "passed_total": sum(sample["success"] for sample in samples),
                "failed_total": sum(not sample["success"] for sample in samples),
                "measured_attempted": len(measured), "measured_passed": len(passing),
                "successful_p50_ms": percentile(passing, .5), "successful_p95_ms": percentile(passing, .95),
                "successful_p99_ms": percentile(passing, .99),
                "successful_batch_median_requests_per_second": statistics.median(batch["passing_requests_per_second"] for batch in batches) if batches else None,
                "successful_batches": len(batches), "failed_runs": sum(not row["success"] for row in matching)}
        comparisons = {}
        for reference, candidate in (("previous", "audit-off"), ("audit-off", "audit-on"), ("previous", "audit-on")):
            differences, ratios = [], []
            for block in range(len(matrix["orders"])):
                paired = {row["variant"]: row for row in runs if row["block"] == block and row["variant"] in (reference, candidate)}
                if set(paired) != {reference, candidate} or not all(row["success"] for row in paired.values()): continue
                batch = {variant: next(item for item in row["batches"] if item["phase"] == "measured") for variant, row in paired.items()}
                mean = {variant: statistics.mean(sample["latency_ms"] for sample in item["samples"]) for variant, item in batch.items()}
                differences.append(mean[reference] - mean[candidate])
                ratios.append(batch[candidate]["passing_requests_per_second"] / batch[reference]["passing_requests_per_second"])
            comparisons[candidate + "_versus_" + reference] = {"complete_pairs": len(differences),
                "candidate_faster_mean_pairs": sum(value > 0 for value in differences),
                "median_paired_mean_reduction_ms": statistics.median(differences) if differences else None,
                "median_paired_throughput_ratio": statistics.median(ratios) if ratios else None}
        profiles.append({"concurrency": concurrency, "variants": variants, "comparisons": comparisons})
    return {"profiles": profiles, "total_attempted": sum(len(batch["samples"]) for row in matrix["runs"] for batch in row["batches"]),
        "total_passed": sum(sample["success"] for row in matrix["runs"] for batch in row["batches"] for sample in batch["samples"]),
        "audit_records_verified": sum(row.get("audit", {}).get("verified_records", 0) for row in matrix["runs"]),
        "cleanup_verified": all(not row["cleanup_errors"] and row.get("daemon_exit_code") is not None for row in matrix["runs"]),
        "cohort_success": matrix["success"], "artifacts_unchanged": matrix["artifacts_unchanged"],
        "previous_audited": matrix.get("previous_audit", False), "competitor_win_established": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("matrix", type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(json.loads(args.matrix.read_text())), indent=2))


if __name__ == "__main__":
    main()
