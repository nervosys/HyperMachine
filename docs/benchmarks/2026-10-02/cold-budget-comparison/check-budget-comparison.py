#!/usr/bin/env python3
"""Verify two enabled admission budgets and exercise malformed report rejection."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--legacy-archive", type=Path, help="also compare recomputed legacy cohorts to their saved analyses")
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("analysis", Path(__file__).with_name("analyze-cold-start-limit.py"))
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    report = json.loads(args.report.read_text())
    result = analysis.analyze(report)
    analysis.require(result["cohort_success"] and result["baseline_limit"] == 8 and result["candidate_limit"] == 16, "requires passing real eight/sixteen cohort")
    mutations = [
        ("baseline budget", lambda r: r.update(baseline_limit=4)),
        ("candidate budget", lambda r: r.update(candidate_limit=8)),
        ("invalid budget", lambda r: r.update(baseline_limit=-1)),
        ("binary identity", lambda r: r.update(same_binary=False)),
        ("guest resources", lambda r: r.update(memory_mb=2048)),
        ("actual resources", lambda r: r["runs"][0]["daemon_argv"].__setitem__(r["runs"][0]["daemon_argv"].index("--memory-mb")+1, "2048")),
        ("cold template", lambda r: r["runs"][0]["template"].update(snapshot=True)),
        ("duplicate attempt", lambda r: r["runs"][0]["batch"]["samples"][0].update(index=1)),
        ("nan", lambda r: r["runs"][0]["batch"]["samples"][0].update(ready_ms=float("nan"))),
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
    print("9 malformed reports refused; cleanup error preserved; valid report accepted")
    if args.legacy_archive:
        manifest = json.loads((args.legacy_archive / "manifest.json").read_text())
        for name in manifest["cohorts"]:
            raw = json.loads((args.legacy_archive / name).read_text())
            expected = json.loads((args.legacy_archive / name.replace(".json", "-analysis.json")).read_text())
            analysis.require(analysis.analyze(raw) == expected, "legacy analysis changed: " + name)
            print("Legacy analysis unchanged: " + name)


if __name__ == "__main__":
    main()
