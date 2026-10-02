#!/usr/bin/env python3
"""Verify owned heap-probe evidence and all frozen source identities."""
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
    require(context["main_sha256"] == manifest["sha256"]["compiled-main.rs"], "daemon source mismatch")
    require(context["helper_source_sha256"] == manifest["sha256"]["heap-reclaim-probe.c"], "helper source mismatch")
    require(context["helper_rebuild_sha256"] == context["helper_sha256"], "helper rebuild mismatch")
    spec = importlib.util.spec_from_file_location("analysis", root / "analyze-heap-reclaim.py")
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    hashes = set(manifest["sha256"].values())
    attempts = 0
    for name, concurrency in [("c8", 8), ("c100", 100)]:
        raw = json.loads((root / (name + ".json")).read_text())
        require(raw["concurrency"] == concurrency and raw["pairs"] == 2, "planned profile mismatch")
        require(raw["cpu_affinity"] == list(range(8)) and not raw["added_CPU_load"], "CPU setting mismatch")
        identities = raw["artifact_sha256"]
        require(identities["daemon"] == context["daemon_sha256"] and identities["helper"] == context["helper_sha256"], "binary identity mismatch")
        for key in ["driver", "retention", "burst", "shared", "firecracker_harness"]:
            require(identities[key] in hashes, "missing frozen coordinator: " + key)
        for key in ["kernel", "initrd"]: require(identities[key] == manifest["inputs"][key], "guest image mismatch")
        result = analysis.analyze(raw)
        require(result == json.loads((root / (name + "-analysis.json")).read_text()), "analysis mismatch")
        require(result["cleanup_verified"] and result["artifacts_unchanged"], "cleanup/artifacts failed")
        require(result["attempted"] == concurrency * 4, "missing planned attempts")
        attempts += result["attempted"]
    print(json.dumps({"verified_files": len(manifest["sha256"]), "attempts": attempts, "cleanup": True}))


if __name__ == "__main__":
    main()
