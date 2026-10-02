#!/usr/bin/env python3
"""Verify guarded reclaim evidence without hiding the failed control batch."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    root = args.archive.resolve()
    manifest = json.loads((root / "manifest.json").read_text())
    for name, sha in manifest["sha256"].items():
        path = (root / name).resolve()
        require(path.is_relative_to(root), "path escapes archive")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == sha, "hash mismatch: " + name)
    context = json.loads((root / "build-context.json").read_text())
    require(context["candidate_main_sha256"] == manifest["sha256"]["compiled-main.rs"] == manifest["sha256"]["reproduced-main.rs"], "compiled/generated source mismatch")
    require(context["baseline_main_sha256"] == manifest["sha256"]["baseline-main.rs"] and context["generator_roundtrip_identical"], "baseline/generator provenance mismatch")
    raw = json.loads((root / "c100.json").read_text())
    require(raw["pairs"] == 4 and raw["concurrency"] == 100 and raw["interval_ms"] == 250, "planned profile mismatch")
    require(raw["driver_cpu_affinity"] == list(range(8)) and not raw["added_CPU_load"], "affinity/load mismatch")
    hashes = set(manifest["sha256"].values())
    for name in ["driver", "comparison", "burst", "shared", "firecracker_harness"]:
        require(raw["artifact_sha256"][name] in hashes, "missing frozen coordinator: " + name)
    require(raw["artifact_sha256"]["daemon"] == context["candidate_daemon_sha256"], "candidate binary mismatch")
    for name in ["kernel", "initrd"]: require(raw["artifact_sha256"][name] == manifest["inputs"][name], "guest input mismatch")
    result = module(root / "analyze-periodic-heap.py", "analysis").analyze(raw)
    require(result == json.loads((root / "c100-analysis.json").read_text()), "analysis mismatch")
    require(result["cleanup_verified"] and result["artifacts_unchanged"], "cleanup/artifacts failure")
    require(result["variants"]["baseline"]["attempted"] == result["variants"]["candidate"]["attempted"] == 400, "missing attempts")
    require(result["variants"]["baseline"]["failed"] == 75 and result["variants"]["candidate"]["failed"] == 0 and not result["cohort_success"], "failed batch hidden")
    failures = [s for r in raw["runs"] for s in r["batch"]["samples"] if not s["success"]]
    require(all(s["failure_phase"] == "create" for s in failures) and sum(s["error"] == "timed out" for s in failures) == 74 and sum(s["error"].startswith("HTTP 503:") for s in failures) == 1, "failure classification mismatch")
    for row in raw["runs"]:
        if row["variant"] == "candidate": require(row["reclaim_worker_summary"]["busy_skips"] > 0, "guard did not skip busy admissions")
    functional = json.loads((root / "functional/report.json").read_text())
    require(functional["success"] and functional["restore_bypass_verified"] and functional["artifacts_unchanged"] and not functional["cleanup_errors"], "functional failure")
    require(functional["artifact_sha256"]["daemon"] == context["candidate_daemon_sha256"] and functional["artifact_sha256"]["coordinator"] == manifest["sha256"]["check-cold-start-limit.py"], "functional provenance mismatch")
    require(functional["processes_stopped"] == [{"phase": "bounded", "exit_code": 0}, {"phase": "failure-release", "exit_code": 0}], "owned process shutdown mismatch")
    require(functional["bounded"] == {"acquired": 8, "released": 8, "maximum_observed": 2} and functional["failure-release"] == {"acquired": 2, "released": 2, "maximum_observed": 1}, "cold admission bounds mismatch")
    invalid = json.loads((root / "invalid-configurations.json").read_text())
    require([(r["interval"], r["budget_configured"]) for r in invalid] == [("0", True), ("99", True), ("60001", True), ("bad", True), ("250", False)] and all(r["exit_code"] != 0 for r in invalid), "invalid configuration acceptance")
    check = module(root / "check-cold-start-limit.py", "functional")
    for phase in ["bounded", "failure-release"]:
        log = (root / "functional" / (phase + ".log")).read_text()
        require(check.admissions(log) == functional[phase] and check.reclaim_admissions(log) == functional[phase + "-reclaim"], "admission trace mismatch")
    print(json.dumps({"verified_files": len(manifest["sha256"]), "attempts": 800, "passed": 725, "failed": 75, "admission_overlap": False, "candidate_adopted": False}))


if __name__ == "__main__":
    main()
