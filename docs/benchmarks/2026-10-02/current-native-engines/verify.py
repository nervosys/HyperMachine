#!/usr/bin/env python3
"""Verify a performance archive, including its explicitly failed cohort."""
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
    assert not matrix["success"] and matrix["artifacts_unchanged"]
    assert [row["concurrency"] for row in matrix["profiles"]] == [1, 8, 50, 100]
    assert [row["exit_code"] for row in matrix["profiles"]] == [0, 0, 0, 1]
    assert all(not row["timed_out"] for row in matrix["profiles"])
    assert matrix["coordinator_sha256"] == manifest["files_sha256"]["refresh-local-engines.py"]
    spec = importlib.util.spec_from_file_location("analyzer", directory / "summarize-local-engine-refresh.py")
    analyzer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analyzer)
    summary = analyzer.summarize(directory)
    assert summary == json.loads((directory / "summary.json").read_text())
    assert summary["attempts"] == 636 and summary["passed"] == 536 and not summary["cohort_success"]
    assert all(row["cleanup_verified"] for row in summary["profiles"])
    context = json.loads((directory / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"]
    assert context["daemon_sha256"] == matrix["artifact_sha256"]["hypermachine"]
    previous = json.loads((directory.parent / "registration-mutations/build-context.json").read_text())
    assert context == previous
    for execution in matrix["profiles"]:
        report = json.loads((directory / execution["report"]).read_text())
        assert report["artifacts_unchanged"] and not report["setup_error"] and not report["cleanup_errors"]
        assert report["remaining_sandbox_count"] == 0
        for key, filename in (("harness", "bench-local-engines-concurrent.py"),
                              ("shared_harness", "bench-local-engines.py"),
                              ("firecracker_harness", "bench-firecracker-local.py")):
            assert report["artifact_sha256"][key] == manifest["files_sha256"][filename]
    failed = json.loads((directory / "c100.json").read_text())
    rows = [row for batch in failed["batches"] for row in batch["samples"] if not row["success"]]
    assert len(rows) == 100 and all(row["failure_phase"] == "create" for row in rows)
    assert all(row["error"].startswith("HTTP 503:") and "15s" in row["error"] for row in rows)
    print(json.dumps({"archive_verified": True, "benchmark_cohort_success": False,
                      "passed": 536, "attempted": 636, "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
