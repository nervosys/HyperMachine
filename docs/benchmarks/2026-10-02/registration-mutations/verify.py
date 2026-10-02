#!/usr/bin/env python3
"""Verify real same-guest reconciliation and trusted source provenance."""
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
    assert len(report["cases"]) == 26 and all(row["success"] for row in report["cases"])
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    previous = json.loads((directory.parent / "registration-lifecycle/run-2.json").read_text())
    for name in ("control-plane", "cli", "kernel", "initrd"):
        assert report["artifact_sha256"][name] == previous["artifact_sha256"][name]
    cases = {row["name"]: row["result"] for row in report["cases"]}
    original = json.loads((directory.parent / "node-completion/run-1.json").read_text())
    for row in original["cases"]:
        assert cases[row["name"]] == row["result"]
    result = cases["node-registration-fault-same-guest-reconciliation"]
    assert result["control_plane_status"] == 503
    for field in ("local_guest_preserved_and_executable", "partial_registration_writes_absent",
                  "wrong_cluster_credential_refused", "replacement_owner_refused_without_guest_loss",
                  "original_owner_reconciles_same_VM", "CLI_and_named_SSH_recover_same_guest",
                  "duplicate_creation_refused", "reconciliation_context_cleared_after_success",
                  "operation_not_exposed", "local_guest_deleted_and_bound_name_released", "uncertain_pause_fork_timeout_refused",
                  "timeout_available_after_reconciliation", "pause_resume_fork_available_after_reconciliation",
                  "uncertain_read_only_connect_and_checkpoint_creation_allowed",
                  "uncertain_connect_extension_and_checkpoint_restore_refused",
                  "connect_extension_and_checkpoint_restore_available_after_reconciliation"):
        assert result[field], field
    assert result["uncertain_guest_survives_idle_window_seconds"] == 35
    for mode in ("drop", "timeout"):
        result = cases[f"control-plane-node-response-{mode}-after-commit"]
        assert result["control_plane_status"] == 502 and result["backend_requests"] == 1
        assert result["CLI_recovers_ID"] and result["named_SSH_reaches_same_guest"]
        if mode == "timeout":
            assert 59 <= result["elapsed_seconds"] < 85
    assert cases["node-post-commit-event-publication-fault"]["redis_xadd_denial_observed"]
    context = json.loads((directory / "build-context.json").read_text())
    prior = json.loads((directory.parent / "registration-lifecycle/build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"] and context["release_build_exit_code"] == 0
    assert context["committed_core_sha256"] == prior["committed_core_sha256"]
    assert context["store_sha256"] == prior["store_sha256"]
    assert context["daemon_source_sha256"] == manifest["files_sha256"]["daemon-main.rs"]
    assert context["checkpoint_source_sha256"] == manifest["files_sha256"]["daemon-checkpoints.rs"]
    assert context["daemon_sha256"] == report["artifact_sha256"]["daemon"]
    tests = (directory / "tests.log").read_text()
    assert "39 passed; 0 failed" in tests
    assert "registration_reconciliation_requires_each_requests_cluster_credential ... ok" in tests
    print(json.dumps({"success": True, "cases": 26, "uncertain_mutation_guard_verified": True,
                      "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
