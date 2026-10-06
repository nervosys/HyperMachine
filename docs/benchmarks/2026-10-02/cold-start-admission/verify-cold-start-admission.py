#!/usr/bin/env python3
"""Verify archived cold admission hashes, attempts, cleanup and live bounds."""
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
    for name, expected in manifest["sha256"].items():
        path = (root / name).resolve()
        require(path.is_relative_to(root), "path escapes archive")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, "hash mismatch: " + name)
    spec = importlib.util.spec_from_file_location("analysis", root / "analyze-cold-start-limit.py")
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    attempts = 0
    for name in manifest["cohorts"]:
        raw = json.loads((root / name).read_text())
        identities = raw["artifact_sha256"]
        frozen_hashes = set(manifest["sha256"].values())
        for tool in ["harness", "comparison", "burst", "shared", "firecracker"]:
            require(identities[tool] in frozen_hashes, "missing frozen coordinator: " + tool)
        context = json.loads((root / name.split('/')[0] / "build-context.json").read_text())
        daemon_hash = context.get("candidate_daemon_sha256", context.get("daemon_sha256"))
        require(identities["candidate"] == daemon_hash, "candidate provenance mismatch")
        result = analysis.analyze(raw)
        require(result["cleanup_verified"] and result["artifacts_unchanged"], "cleanup/artifact failure: " + name)
        require(all(v["attempted"] == v["planned"] for v in result["variants"].values()), "missing attempts")
        expected = json.loads((root / name.replace(".json", "-analysis.json")).read_text())
        require(result == expected, "analysis mismatch: " + name)
        attempts += sum(v["attempted"] for v in result["variants"].values())
    spec = importlib.util.spec_from_file_location("functional", root / "check-cold-start-limit.py")
    functional = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(functional)
    report = json.loads((root / "adoption/functional/report.json").read_text())
    context = json.loads((root / "adoption/build-context.json").read_text())
    require(report["artifact_sha256"]["daemon"] == context["daemon_sha256"], "functional binary mismatch")
    require(context["main_sha256"] == manifest["sha256"]["adoption/compiled-main.rs"], "compiled source mismatch")
    require(report["success"] and report["restore_bypass_verified"] and report["artifacts_unchanged"], "functional failure")
    require(not report["cleanup_errors"] and len(report["processes_stopped"]) == 2, "functional cleanup")
    for phase, count, limit in [("bounded", 8, 2), ("failure-release", 2, 1)]:
        bounds = functional.admissions((root / f"adoption/functional/{phase}.log").read_text())
        require(bounds == report[phase], "log bounds differ")
        require(bounds["acquired"] == bounds["released"] == count and bounds["maximum_observed"] == limit, "invalid bound")
    kvm = json.loads((root / "adoption/kvm-verified.json").read_text())
    require(kvm["success"] and len(kvm["cases"]) == 23 and all(c["success"] for c in kvm["cases"]), "KVM regression")
    require(kvm["remaining_sandboxes"] == 0 and not kvm["cleanup_errors"] and len(kvm["owned_processes_stopped"]) == 22, "KVM cleanup")
    require(kvm["artifact_sha256"]["daemon"] == context["daemon_sha256"], "KVM binary mismatch")
    require(kvm["access_audit"]["verified_records"] == 222 and kvm["access_audit"]["uncompleted_admissions"] == 0 and kvm["access_audit"]["credentials_absent"], "KVM audit")
    print(json.dumps({"verified_files": len(manifest["sha256"]), "attempts": attempts, "functional": True}))


if __name__ == "__main__":
    main()
