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
    for index,row in enumerate(report["runs"]):
        pair = index//2
        expected = ("baseline","candidate") if pair%2 == 0 else ("candidate","baseline")
        require(row["variant"] == expected[index%2] and row["pair"] == pair, "counterbalance mismatch")
        enabled = row["variant"] == "candidate" and limit > 0
        require(row["cold_start_limit"] == (limit if enabled else None), "admission setting mismatch")
        argv = row["daemon_argv"]
        require(argv.count("--cold-start-concurrency") == int(enabled), "actual option mismatch")
        if enabled: require(argv[argv.index("--cold-start-concurrency")+1] == str(limit), "actual limit mismatch")
        samples = row.get("batch",{}).get("samples",[])
        require(len(samples) <= report["concurrency"], "too many attempts")
        require(len({sample["index"] for sample in samples}) == len(samples), "duplicate attempt index")
        for sample in samples:
            require(sample["pair"] == pair, "attempt pair mismatch")
            if sample["success"]:
                require(math.isfinite(sample["ready_ms"]) and sample["ready_ms"] >= 0, "invalid latency")
        if row["success"]:
            require(len(samples) == report["concurrency"] and all(sample["success"] and sample["cleanup_success"] for sample in samples), "incomplete successful run")
    result = paired.analyze(report)
    result.update(candidate_limit=limit or None, same_binary=report["same_binary"],
        queue_included_in_ready_ms=True, experimental_candidate=True,
        note="Incomplete pairs remain in failure totals; their mean latency difference is unavailable")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report",type=Path)
    parser.add_argument("--output",type=Path,required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; preserve earlier analyses")
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())),indent=2)+"\n")
