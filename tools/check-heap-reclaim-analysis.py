#!/usr/bin/env python3
"""Exercise heap diagnostic validation using deliberate report corruptions."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("analysis", Path(__file__).with_name("analyze-heap-reclaim.py"))
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    report = json.loads(args.report.read_text())
    analysis.require(analysis.analyze(report)["cohort_success"], "requires a passing real report")
    mutations = [
        ("counterbalance", lambda r: r["runs"].reverse()),
        ("attempts", lambda r: r["runs"][0]["batch"]["samples"].pop()),
        ("KVM handles", lambda r: r["runs"][0]["snapshots"][1].update(kvm_vm_handles=0)),
        ("guest checks", lambda r: r["runs"][0]["post_action_guest_checks"].pop()),
        ("probe command", lambda r: r["runs"][0]["operations"][0].update(operation="trim")),
        ("probe time", lambda r: r["runs"][0]["operations"][0].update(duration_ns=-1)),
        ("snapshot order", lambda r: r["runs"][0]["snapshots"].reverse()),
        ("retained mapping", lambda r: r["runs"][0]["snapshots"][-1].update(large_mapping_count=1)),
        ("guest resources", lambda r: r["runs"][0]["template"].update(memoryMB=2048)),
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
    changed["runs"][0]["success"] = False
    changed["success"] = False
    analysis.require(not analysis.analyze(changed)["cleanup_verified"], "cleanup error hidden")
    print("9 malformed reports refused; cleanup error preserved; valid report accepted")


if __name__ == "__main__":
    main()
