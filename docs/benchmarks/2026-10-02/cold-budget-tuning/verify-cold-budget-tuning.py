#!/usr/bin/env python3
"""Verify admission tuning and matched native engine evidence under Python -O."""
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
    hashes = set(manifest["sha256"].values())
    context = json.loads((root / "build-context.json").read_text())
    require(context["main_sha256"] == manifest["sha256"]["compiled-main.rs"], "source identity mismatch")
    attempts = 0
    for kind, path, analyzer in [("budget", "off-on/limit4-c100.json", module(root / "off-on/analyze-cold-start-limit.py", "budget")),
                                  ("engines", "engines/c100.json", module(root / "analyze-budget-engines.py", "engines"))]:
        raw = json.loads((root / path).read_text())
        result = analyzer.analyze(raw)
        require(result == json.loads((root / path.replace('.json', '-analysis.json')).read_text()), "analysis differs")
        require(result["cleanup_verified"] and result["artifacts_unchanged"], "cleanup/artifacts failed")
        identities = raw["artifact_sha256"]
        for name, sha in identities.items():
            if "harness" in name or (kind == "budget" and name in ["comparison", "burst", "shared", "firecracker"]):
                require(sha in hashes, "missing frozen tool: " + name)
        require(identities["candidate" if kind == "budget" else "hypermachine"] == context["daemon_sha256"], "binary provenance mismatch")
        if kind == "budget":
            require(raw["same_binary"] and raw["candidate_limit"] == 4, "incorrect tuning comparison")
            require(all(v["attempted"] == v["planned"] for v in result["variants"].values()), "missing attempts")
            attempts += sum(v["attempted"] for v in result["variants"].values())
        else:
            require(raw["cold_start_concurrency"] == 8 and raw["driver_cpu_affinity"] == list(range(8)), "native budget/affinity mismatch")
            require(all(identities[n] == manifest["inputs"][n] for n in ["hypermachine", "firecracker", "kernel", "initrd"]), "native inputs mismatch")
            attempts += sum(v["attempted"] for v in result["engines"].values())
    print(json.dumps({"verified_files": len(manifest["sha256"]), "attempts": attempts, "cleanup": True}))


if __name__ == "__main__":
    main()
