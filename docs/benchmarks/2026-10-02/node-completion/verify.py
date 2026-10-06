#!/usr/bin/env python3
"""Verify real-node atomic completion and discarded-descriptor recovery."""
import hashlib
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    report = json.loads((directory / "run-1.json").read_text())
    assert report["success"] and not report["cleanup_errors"]
    assert len(report["cases"]) == 22 and all(row["success"] for row in report["cases"])
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    assert report["artifact_sha256"] == manifest["artifacts"]
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    previous = json.loads((directory.parent / "fork-name/run-1.json").read_text())
    for name in ("cli", "kernel", "initrd"):
        assert report["artifact_sha256"][name] == previous["artifact_sha256"][name]
    cases = {row["name"]: row["result"] for row in report["cases"]}
    admission = cases["node-name-operation-refused-before-boot"]
    assert admission == {"missing_context_status": 409, "invalid_context_status": 400,
                         "unowned_context_status": 409, "cluster_authentication_status": 401,
                         "inventory_unchanged": True}
    recovery = cases["node-atomic-completion-with-discarded-descriptor"]
    for field in ("creation_descriptor_discarded", "CLI_inspection_recovers_ID", "named_SSH_reaches_recovered_guest",
                  "duplicate_creation_refused", "operation_absent_from_guest_environment", "deleted_name_released"):
        assert recovery[field], field
    assert manifest["full_control_plane_transport_loss_unverified"]
    assert cases["fork-preserves-guest-listener"]["reserved_name_keeps_parent_after_fork"]
    context = json.loads((directory / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"] and context["release_build_exit_code"] == 0
    assert context["windows_daemon_tests_passed"] == 34 and context["linux_daemon_tests_passed"] == 38
    for source, expected in context["overlay_sha256"].items():
        assert manifest["files_sha256"][source.replace("/", "--") + ".txt"] == expected
    for source, expected in context["committed_core_sha256"].items():
        assert manifest["files_sha256"]["core--" + source.replace("/", "--") + ".txt"] == expected
    print(json.dumps({"success": True, "cases": 22, "discarded_descriptor_recovery": True,
                      "transport_loss_unverified": True, "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
