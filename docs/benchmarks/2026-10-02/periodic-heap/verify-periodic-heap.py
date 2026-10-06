#!/usr/bin/env python3
"""Verify the rejected periodic heap-reclaim experiment and source provenance."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


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
    require(context["candidate_main_sha256"] == manifest["sha256"]["compiled-main.rs"], "compiled candidate mismatch")
    require(context["baseline_main_sha256"] == manifest["sha256"]["baseline-main.rs"], "baseline source mismatch")
    spec = importlib.util.spec_from_file_location("analysis", root / "analyze-periodic-heap.py")
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    raw = json.loads((root / "c100.json").read_text())
    require(raw["pairs"] == 4 and raw["concurrency"] == 100 and raw["interval_ms"] == 250, "planned cohort mismatch")
    require(raw["driver_cpu_affinity"] == list(range(8)) and not raw["added_CPU_load"], "affinity/load mismatch")
    hashes = set(manifest["sha256"].values())
    for name in ["driver", "comparison", "burst", "shared", "firecracker_harness"]:
        require(raw["artifact_sha256"][name] in hashes, "missing frozen coordinator: " + name)
    require(raw["artifact_sha256"]["daemon"] == context["candidate_daemon_sha256"], "candidate binary mismatch")
    for name in ["kernel", "initrd"]: require(raw["artifact_sha256"][name] == manifest["inputs"][name], "guest input mismatch")
    result = analysis.analyze(raw)
    require(result == json.loads((root / "c100-analysis.json").read_text()), "analysis mismatch")
    require(result["cleanup_verified"] and result["artifacts_unchanged"], "cleanup/artifacts failed")
    require(sum(v["attempted"] for v in result["variants"].values()) == 800, "missing attempts")
    require(not result["runtime_change_adopted"], "rejected candidate adopted")
    invalid = json.loads((root / "invalid-intervals.json").read_text())
    require([v["interval"] for v in invalid] == ["0", "99", "60001", "bad"] and all(v["exit_code"] != 0 for v in invalid), "invalid intervals accepted")
    spec = importlib.util.spec_from_file_location("control", root / "analyze-cold-start-limit.py")
    control = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(control)
    default = json.loads((root / "default-c8.json").read_text())
    for name in ["harness", "comparison", "burst", "shared", "firecracker"]:
        require(default["artifact_sha256"][name] in hashes, "missing default-control coordinator: " + name)
    require(default["pairs"] == 2 and default["concurrency"] == 8 and default["baseline_limit"] == default["candidate_limit"] == 0, "default control mismatch")
    require(default["artifact_sha256"]["baseline"] == default["artifact_sha256"]["candidate"] == context["baseline_daemon_sha256"], "default control binary mismatch")
    for row in default["runs"]:
        require("empty_node_memory_before_batch_kib" not in row and "empty_node_memory_after_cleanup_kib" not in row and "idle_process_memory_kib" not in row["batch"], "default memory holds added")
    default_result = control.analyze(default)
    require(default_result["cleanup_verified"] and default_result["artifacts_unchanged"] and default_result["cohort_success"], "default control failed")
    require(sum(v["attempted"] for v in default_result["variants"].values()) == 32, "missing default control attempts")
    print(json.dumps({"verified_files": len(manifest["sha256"]), "attempts": 832, "candidate_adopted": False, "cleanup": True}))


if __name__ == "__main__":
    main()
