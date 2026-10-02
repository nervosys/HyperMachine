#!/usr/bin/env python3
"""Validate reclaim diagnostics; report before/after changes and sham controls."""
import argparse
import json
from pathlib import Path
import statistics


def require(condition, message):
    if not condition:
        raise ValueError(message)


def analyze(report):
    require(report["diagnostic_only"] and report["cold_start_concurrency"] == 8, "scope mismatch")
    require(len(report["runs"]) == report["pairs"] * 2, "missing planned runs")
    runs = []
    for index, row in enumerate(report["runs"]):
        pair = index // 2
        order = ["sham", "trim"] if pair % 2 == 0 else ["trim", "sham"]
        require(row["pair"] == pair and row["variant"] == order[index % 2], "counterbalance mismatch")
        argv = row["daemon_argv"]
        for flag, value in [("--cold-start-concurrency", "8"), ("--cpu-cores", "1"), ("--memory-mb", "1024"), ("--capacity", "128")]:
            require(argv.count(flag) == 1 and argv[argv.index(flag)+1] == value, "actual daemon configuration mismatch")
        samples = row.get("batch", {}).get("samples", [])
        require(len(samples) == report["concurrency"] and {s["index"] for s in samples} == set(range(report["concurrency"])), "missing planned attempts")
        require(all(s["pair"] == pair for s in samples), "attempt identity mismatch")
        successful = all(s["success"] and s["cleanup_success"] for s in samples)
        cleanup = not row["cleanup_errors"] and row.get("remaining_sandbox_count") == 0 and row.get("daemon_exit_code") is not None
        record = {"pair": pair, "variant": row["variant"], "attempted": len(samples),
                  "passed": sum(s["success"] and s["cleanup_success"] for s in samples), "cleanup_verified": cleanup}
        if row["success"]:
            require(successful and cleanup, "false success")
            template = row["template"]
            require(template["snapshot"] is False and template["cpuCount"] == 1 and template["memoryMB"] == 1024, "cold template mismatch")
            require(len(set(row["post_action_guest_checks"])) == report["concurrency"], "missing post-action guest checks")
            snapshots = {s["stage"]: s for s in row["snapshots"]}
            require(len(snapshots) == len(row["snapshots"]) == 5, "snapshot count mismatch")
            require(list(snapshots) == ["initial-empty", "held-before", "held-after", "empty-before", "empty-after"], "snapshot order mismatch")
            times = [s["observed_monotonic_ns"] for s in row["snapshots"]]
            require(all(a < b for a, b in zip(times, times[1:])), "observation time mismatch")
            for stage, snapshot in snapshots.items():
                expected = report["concurrency"] if stage.startswith("held") else 0
                require(snapshot["kvm_vm_handles"] == snapshot["kvm_vcpu_handles"] == expected, "KVM handle mismatch")
                if not stage.startswith("held"): require(snapshot["large_mapping_count"] == 0, "retained guest mapping")
            require([o["stage"] for o in row["operations"]] == ["held", "empty"], "probe count mismatch")
            for operation in row["operations"]:
                require(operation["operation"] == row["variant"] and operation["result"] in (0, 1) and operation["duration_ns"] >= 0, "probe reply mismatch")
                if row["variant"] == "sham": require(operation["result"] == 0, "sham intervention")
            for phase in ["held", "empty"]:
                before = snapshots[phase + "-before"]["process_memory_kib"]["Pss_kib"]
                after = snapshots[phase + "-after"]["process_memory_kib"]["Pss_kib"]
                record[phase + "_pss_before_mib"] = before / 1024
                record[phase + "_pss_after_mib"] = after / 1024
                record[phase + "_pss_reduction_mib"] = (before - after) / 1024
                operation = next(o for o in row["operations"] if o["stage"] == phase)
                record[phase + "_operation_ms"] = operation["duration_ns"] / 1e6
        runs.append(record)
    differences = []
    for pair in range(report["pairs"]):
        values = {r["variant"]: r for r in runs if r["pair"] == pair}
        difference = {"pair": pair}
        for phase in ["held", "empty"]:
            key = phase + "_pss_reduction_mib"
            complete = all(key in value for value in values.values())
            difference[phase + "_sham_adjusted_reduction_mib"] = values["trim"][key] - values["sham"][key] if complete else None
        differences.append(difference)
    held_differences = [d["held_sham_adjusted_reduction_mib"] for d in differences if d["held_sham_adjusted_reduction_mib"] is not None]
    return {"runs": runs, "pairs": differences, "attempted": sum(r["attempted"] for r in runs),
        "passed": sum(r["passed"] for r in runs), "cleanup_verified": all(r["cleanup_verified"] for r in runs),
        "artifacts_unchanged": report["artifacts_unchanged"], "cohort_success": report["success"],
        "runtime_change_adopted": False, "performance_win_established": False,
        "median_sham_adjusted_held_reduction_mib": statistics.median(held_differences) if held_differences else None}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; retain previous analysis")
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())), indent=2) + "\n")
