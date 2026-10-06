#!/usr/bin/env python3
"""Verify frozen direct-budget and native-engine comparisons under Python -O."""
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
    require(context["main_sha256"] == manifest["sha256"]["compiled-main.rs"], "compiled source mismatch")
    hashes = set(manifest["sha256"].values())
    analyzers = {"budget": module(root / "analyze-cold-start-limit.py", "budget"),
                 "native": module(root / "analyze-budget-engines.py", "native")}
    attempts = 0
    for cohort in manifest["cohorts"]:
        raw = json.loads((root / cohort["report"]).read_text())
        result = analyzers[cohort["kind"]].analyze(raw)
        require(result == json.loads((root / cohort["analysis"]).read_text()), "analysis mismatch")
        require(result["cleanup_verified"] and result["artifacts_unchanged"], "cleanup/artifacts failed")
        identities = raw["artifact_sha256"]
        for name, sha in identities.items():
            if "harness" in name or (cohort["kind"] == "budget" and name in ["comparison", "burst", "shared", "firecracker"]):
                require(sha in hashes, "missing frozen tool: " + name)
        require(raw["driver_cpu_affinity"] == list(range(8)), "CPU affinity mismatch")
        if cohort["kind"] == "budget":
            require(raw["baseline_limit"] == 8 and raw["candidate_limit"] == 16 and raw["same_binary"], "budget mismatch")
            require(identities["baseline"] == identities["candidate"] == context["daemon_sha256"], "binary mismatch")
            require(all(v["attempted"] == v["planned"] for v in result["variants"].values()), "missing attempts")
            attempts += sum(v["attempted"] for v in result["variants"].values())
        else:
            require(raw["cold_start_concurrency"] == 16 and identities["hypermachine"] == context["daemon_sha256"], "native budget/binary mismatch")
            require(all(identities[n] == manifest["native_inputs"][n] for n in ["hypermachine", "firecracker", "kernel", "initrd"]), "native input mismatch")
            attempts += sum(v["attempted"] for v in result["engines"].values())
    print(json.dumps({"verified_files": len(manifest["sha256"]), "attempts": attempts, "cleanup": True}))


if __name__ == "__main__":
    main()
