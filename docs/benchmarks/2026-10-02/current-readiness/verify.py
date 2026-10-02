#!/usr/bin/env python3
"""Verify failed scored evidence and distinct passing stage diagnostics."""
import hashlib
import importlib.util
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    matrix = json.loads((directory / "matrix.json").read_text())
    assert matrix["success"] and matrix["diagnostic_only"] and matrix["artifacts_unchanged"]
    execution = matrix["profiles"][0]
    assert execution["exit_code"] == 0 and not execution["timed_out"]
    assert execution["concurrency"] == 100 and execution["pairs"] == 2
    assert matrix["coordinator_sha256"] == manifest["files_sha256"]["refresh-local-engines.py"]
    diagnostic = directory / "c100.json"
    assert execution["sha256"] == manifest["files_sha256"]["c100.json"]
    report = json.loads(diagnostic.read_text())
    assert report["success"] and report["diagnostic_only"] and report["artifacts_unchanged"]
    assert report["diagnostic_sha256"] == manifest["files_sha256"]["diagnose-concurrent-startup.py"]
    assert report["diagnostic_sha256"] == matrix["artifact_sha256"]["harness"]
    for key, name in (("harness", "bench-local-engines-concurrent.py"),
                      ("shared_harness", "bench-local-engines.py"),
                      ("firecracker_harness", "bench-firecracker-local.py")):
        assert report["artifact_sha256"][key] == manifest["files_sha256"][name]
    spec = importlib.util.spec_from_file_location("analysis", directory / "analyze-current-readiness.py")
    analyzer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analyzer)
    result = analyzer.analyze(directory.parent / "current-native-engines/c100.json", diagnostic)
    assert result == json.loads((directory / "analysis.json").read_text())
    assert result["baseline_failed_attempts"] == 100
    assert result["diagnostic_attempts"] == result["diagnostic_passed"] == 400
    assert result["diagnostic_agent_stage_rows"] == 200 and result["diagnostic_agent_stage_failures"] == 0
    assert result["cleanup_verified"] and not result["runtime_change_adopted"]
    context = json.loads((directory / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"]
    assert context["daemon_sha256"] == report["artifact_sha256"]["hypermachine"]
    print(json.dumps({"archive_verified": True, "diagnostic_only": True,
                      "baseline_failures_retained": 100, "diagnostic_passed": 400}))


if __name__ == "__main__":
    main()
