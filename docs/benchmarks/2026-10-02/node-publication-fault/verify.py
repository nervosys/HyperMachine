#!/usr/bin/env python3
"""Verify real-VM preservation after committed registration/event failure."""
import hashlib
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    failed = json.loads((directory / "run-1.json").read_text())
    assert not failed["success"] and not failed["cleanup_errors"]
    assert failed["remaining_sandboxes"] == 0
    assert "201" in failed["cases"][-1]["error"]
    report = json.loads((directory / "run-2.json").read_text())
    assert report["success"] and not report["cleanup_errors"]
    assert len(report["cases"]) == 23 and all(row["success"] for row in report["cases"])
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    previous = json.loads((directory.parent / "node-completion/run-1.json").read_text())
    for name, expected in previous["artifact_sha256"].items():
        if name != "coordinator":
            assert report["artifact_sha256"][name] == failed["artifact_sha256"][name] == expected
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator-2.py"]
    assert failed["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    cases = {row["name"]: row["result"] for row in report["cases"]}
    for row in previous["cases"]:
        assert cases[row["name"]] == row["result"], row["name"]
    result = cases["node-post-commit-event-publication-fault"]
    assert result["control_plane_status"] == 503
    for field in ("redis_xadd_denial_observed", "committed_VM_preserved", "CLI_recovers_ID",
                  "named_SSH_reaches_same_guest", "duplicate_creation_refused", "no_extra_VM",
                  "publication_permission_restored", "deleted_name_released"):
        assert result[field], field
    print(json.dumps({"success": True, "cases": 23, "post_commit_publication_fault": True,
                      "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
