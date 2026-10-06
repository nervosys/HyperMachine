#!/usr/bin/env python3
"""Exercise periodic-reclaim validation against deliberately invalid reports."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("analysis", Path(__file__).with_name("analyze-periodic-heap.py"))
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    report = json.loads(args.report.read_text())
    analysis.require(analysis.analyze(report)["cohort_success"], "requires a passing real cohort")
    mutations = [
        ("interval", lambda r: r["runs"][1].update(reclaim_interval_ms=None)),
        ("worker absent", lambda r: r["runs"][1].update(reclaim_worker_summary=None)),
        ("worker counts", lambda r: r["runs"][1]["reclaim_worker_summary"].update(release_calls=10**10)),
        ("attempt count", lambda r: r["runs"][0]["batch"]["samples"].pop()),
        ("readiness", lambda r: r["runs"][0]["batch"]["samples"][0].update(ready_ms=float("nan"))),
        ("idle hold", lambda r: r["runs"][0]["batch"].update(memory_idle_actual_seconds=0)),
        ("memory delta", lambda r: r["runs"][0]["batch"]["incremental_idle_process_memory_kib"].update(Pss_kib=0)),
        ("guest resources", lambda r: r["runs"][0]["template"].update(cpuCount=2)),
    ]
    for name, mutate in mutations:
        changed = copy.deepcopy(report)
        mutate(changed)
        try:
            analysis.analyze(changed)
        except ValueError:
            print(name + ": correctly refused")
        else:
            raise ValueError("accepted invalid " + name)
    changed = copy.deepcopy(report)
    changed["runs"][0]["cleanup_errors"].append("retained guest")
    analysis.require(not analysis.analyze(changed)["cleanup_verified"], "cleanup error hidden")
    print("8 malformed reports refused; cleanup error preserved; valid cohort accepted")


if __name__ == "__main__":
    main()
