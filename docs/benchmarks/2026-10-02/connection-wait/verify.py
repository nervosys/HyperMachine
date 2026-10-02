#!/usr/bin/env python3
"""Verify the unadopted connection-wait experiment and its complete cohort."""
import hashlib
import importlib.util
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    report = json.loads((directory / "c100.json").read_text())
    assert report["success"] and report["artifacts_unchanged"]
    assert report["pairs"] == 4 and report["concurrency"] == 100
    assert len(report["driver_cpu_affinity"]) == 8 and not report["added_CPU_load"]
    assert report["cpu_count"] == 1 and report["memory_mb"] == 1024
    assert report["guest_readiness_timeout_s"] == 15
    for key, name in (("harness", "bench-connection-wait.py"),
                      ("burst", "bench-local-engines-concurrent.py"),
                      ("shared", "bench-local-engines.py"),
                      ("firecracker", "bench-firecracker-local.py")):
        assert report["artifact_sha256"][key] == manifest["files_sha256"][name]
    spec = importlib.util.spec_from_file_location("analysis", directory / "analyze-connection-wait.py")
    analyzer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analyzer)
    result = analyzer.analyze(report)
    assert result == json.loads((directory / "analysis.json").read_text())
    assert result["cleanup_verified"] and result["complete_successful_pairs"] == 4
    assert result["candidate_faster_pairs"] == 2
    assert not result["runtime_change_adopted"] and not result["competitor_win_established"]
    for variant in result["variants"].values():
        assert variant["planned"] == variant["attempted"] == variant["passed"] == 400
        assert variant["failed"] == variant["failed_runs"] == 0
    for run in report["runs"]:
        assert run["batch"]["all_guests_validated_while_held"]
        assert run["batch"]["held_process_count"] == 1
        assert len(run["batch"]["samples"]) == 100
    context = json.loads((directory / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"] and not context["runtime_change_adopted"]
    assert context["baseline_daemon_sha256"] == report["artifact_sha256"]["baseline"]
    assert context["candidate_daemon_sha256"] == report["artifact_sha256"]["candidate"]
    for relative, expected in context["candidate_files"].items():
        assert manifest["files_sha256"][Path(relative).name] == expected
    print(json.dumps({"archive_verified": True, "passed": 800, "runtime_change_adopted": False}))


if __name__ == "__main__":
    main()
