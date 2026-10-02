#!/usr/bin/env python3
"""Validate admission-setting cohorts and summarize all retained attempts."""
import argparse
import importlib.util
import json
import math
from pathlib import Path

spec = importlib.util.spec_from_file_location("paired", Path(__file__).with_name("analyze-connection-wait.py"))
paired = importlib.util.module_from_spec(spec)
spec.loader.exec_module(paired)


def require(condition, message):
    if not condition: raise ValueError(message)


def analyze(report):
    require(len(report["runs"]) == report["pairs"]*2, "incomplete planned runs")
    require(report["queue_included_in_ready_ms"] and report["guest_readiness_timeout_s"] == 15, "latency/deadline mismatch")
    limit = report["candidate_limit"]
    baseline_limit = report.get("baseline_limit", 0)
    require(all(isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= 1024 for value in [limit, baseline_limit]), "invalid admission budgets")
    require(report["cpu_count"] == 1 and report["memory_mb"] == 1024, "guest resource metadata mismatch")
    require(report["same_binary"] == (report["artifact_sha256"]["baseline"] == report["artifact_sha256"]["candidate"]), "binary identity mismatch")
    for index,row in enumerate(report["runs"]):
        pair = index//2
        expected = ("baseline","candidate") if pair%2 == 0 else ("candidate","baseline")
        require(row["variant"] == expected[index%2] and row["pair"] == pair, "counterbalance mismatch")
        actual_limit = limit if row["variant"] == "candidate" else baseline_limit
        enabled = actual_limit > 0
        require(row["cold_start_limit"] == (actual_limit if enabled else None), "admission setting mismatch")
        argv = row["daemon_argv"]
        for option, value in [("--cpu-cores", "1"), ("--memory-mb", "1024"), ("--capacity", "128")]:
            require(argv.count(option) == 1 and argv[argv.index(option)+1] == value, "actual guest/node configuration mismatch")
        require("--no-template" in argv, "cold path not selected")
        require(argv.count("--cold-start-concurrency") == int(enabled), "actual option mismatch")
        if enabled: require(argv[argv.index("--cold-start-concurrency")+1] == str(actual_limit), "actual limit mismatch")
        samples = row.get("batch",{}).get("samples",[])
        require(len(samples) <= report["concurrency"], "too many attempts")
        require(len({sample["index"] for sample in samples}) == len(samples), "duplicate attempt index")
        require(all(isinstance(sample["index"], int) and 0 <= sample["index"] < report["concurrency"] for sample in samples), "attempt index out of range")
        for sample in samples:
            require(sample["pair"] == pair, "attempt pair mismatch")
            if sample["success"]:
                require(math.isfinite(sample["ready_ms"]) and sample["ready_ms"] >= 0, "invalid latency")
        if row["success"]:
            template = row["template"]
            require(template["cpuCount"] == 1 and template["memoryMB"] == 1024 and template["snapshot"] is False, "cold template mismatch")
            require(len(samples) == report["concurrency"] and all(sample["success"] and sample["cleanup_success"] for sample in samples), "incomplete successful run")
    result = paired.analyze(report)
    result.update(candidate_limit=limit or None, same_binary=report["same_binary"],
        queue_included_in_ready_ms=True, experimental_candidate=True,
        note="Incomplete pairs remain in failure totals; their mean latency difference is unavailable")
    if "baseline_limit" in report:
        result["baseline_limit"] = baseline_limit or None
        for pair in result["pairs"]:
            runs = report["runs"][pair["pair"]*2:pair["pair"]*2+2]
            for name, quantile in [("p50", .5), ("p99", .99)]:
                values = {row["variant"]: paired.percentile([s["ready_ms"] for s in row["batch"]["samples"]], quantile) for row in runs} if pair["complete_successful_pair"] else {}
                pair[f"candidate_{name}_reduction_ms"] = values["baseline"] - values["candidate"] if values else None
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report",type=Path)
    parser.add_argument("--output",type=Path,required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; preserve earlier analyses")
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())),indent=2)+"\n")
