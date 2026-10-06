#!/usr/bin/env python3
"""Exercise native budget report validation against deliberate corruptions."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("analysis", Path(__file__).with_name("analyze-budget-engines.py"))
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    report = json.loads(args.report.read_text())
    analysis.require(analysis.analyze(report)["cohort_success"], "requires a passing real report")
    mutations = [
        ("deadline", lambda r: r["guest_readiness_timeout_s"].update(hypermachine=20)),
        ("budget", lambda r: r.update(cold_start_concurrency=4)),
        ("attempts", lambda r: r["batches"][0]["samples"].pop()),
        ("order", lambda r: r["batches"].reverse()),
        ("nan", lambda r: r["batches"][0]["samples"][0].update(ready_ms=float("nan"))),
        ("memory", lambda r: r["batches"][0]["incremental_idle_process_memory_kib"].update(Pss_kib=0)),
        ("hold", lambda r: r["batches"][0].update(memory_idle_actual_seconds=0)),
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
    changed["cleanup_errors"].append("fixture retained guest")
    analysis.require(not analysis.analyze(changed)["cleanup_verified"], "cleanup failure hidden")
    print("7 malformed reports refused; cleanup failure preserved; valid report accepted")


if __name__ == "__main__":
    main()
