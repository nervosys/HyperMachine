#!/usr/bin/env python3
"""Verify combined KVM fault evidence and clean-source provenance."""
import hashlib
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    failed = json.loads((directory / "run-1.json").read_text())
    report = json.loads((directory / "run-2.json").read_text())
    assert not failed["success"] and not failed["cases"] and "Permission denied" in failed["error"]
    assert failed["artifact_sha256"] == report["artifact_sha256"]
    assert report["success"] and not report["cleanup_errors"]
    assert len(report["cases"]) == 26 and all(row["success"] for row in report["cases"])
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    cases = {row["name"]: row["result"] for row in report["cases"]}
    previous = json.loads((directory.parent / "node-completion/run-1.json").read_text())
    for row in previous["cases"]:
        assert cases[row["name"]] == row["result"], row["name"]
    for artifact in ("cli", "kernel", "initrd"):
        assert report["artifact_sha256"][artifact] == previous["artifact_sha256"][artifact]
    for mode in ("drop", "timeout"):
        result = cases[f"control-plane-node-response-{mode}-after-commit"]
        assert result["control_plane_status"] == 502 and result["backend_requests"] == 1
        assert all(value for key, value in result.items() if key not in ("elapsed_seconds",))
        if mode == "timeout":
            assert 59 <= result["elapsed_seconds"] < 85
    for name, status in (("node-post-commit-event-publication-fault", 503),
                         ("node-registration-write-permission-fault", 503)):
        result = cases[name]
        assert result["control_plane_status"] == status and all(result.values())
    context = json.loads((directory / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"] and context["release_build_exit_code"] == 0
    prior_context = json.loads((directory.parent / "node-completion/build-context.json").read_text())
    assert context["committed_core_sha256"] == prior_context["committed_core_sha256"]
    assert context["store_sha256"] == hashlib.sha256((directory.parent / "registration-acl/store-after.rs").read_bytes()).hexdigest()
    for artifact, expected in context["artifacts"].items():
        assert report["artifact_sha256"][artifact] == expected
    print(json.dumps({"success": True, "cases": 26, "registration_fault_verified": True,
                      "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
