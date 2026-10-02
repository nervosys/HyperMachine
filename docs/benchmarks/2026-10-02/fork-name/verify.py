#!/usr/bin/env python3
"""Verify named SSH across a real KVM fork and preserved old conflict evidence."""
import hashlib
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    report = json.loads((directory / "run-1.json").read_text())
    before = json.loads((directory.parent / "reserved-create/run-1.json").read_text())
    assert report["success"] and not report["cleanup_errors"]
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    assert len(report["cases"]) == 20 and all(row["success"] for row in report["cases"])
    assert report["artifact_sha256"] == manifest["artifacts"]
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    for artifact in ("control-plane", "cli", "kernel", "initrd"):
        assert report["artifact_sha256"][artifact] == before["artifact_sha256"][artifact]
    assert report["artifact_sha256"]["daemon"] != before["artifact_sha256"]["daemon"]
    assert report["ssh_fixture"]["resolution"] == "reserved creation"
    cases = {row["name"]: row["result"] for row in report["cases"]}
    old_cases = {row["name"]: row["result"] for row in before["cases"]}
    assert old_cases["fork-preserves-guest-listener"]["reserved_lookup_refuses_legacy_fork_conflict"]
    fork = cases["fork-preserves-guest-listener"]
    for field in ("alias_transfer_refused", "reserved_name_keeps_parent_after_fork", "child_name_not_inherited", "parent_metadata_name_unchanged", "other_metadata_preserved", "name_resolves_after_duplicate_deleted"):
        assert fork[field], field
    assert "reserved_lookup_refuses_legacy_fork_conflict" not in fork
    assert cases["delete-closes-active-tunnel-and-refuses-reopen"]["creation_name_deleted_and_reused"]
    assert cases["OpenSSH-API-TLS-node-mTLS-binary-roundtrip"]["bytes"] == 1048576
    context = json.loads((directory / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"]
    assert context["overlay_main_sha256"] == manifest["files_sha256"]["daemon-source.rs"]
    assert context["base_commit"] == manifest["base_commit"]
    assert context["linux_clippy_baseline_exception"]
    assert context["release_build_exit_code"] == 0
    assert context["linux_daemon_tests_passed"] == 36 and context["windows_daemon_tests_passed"] == 32
    for source, expected in context["committed_core_sha256"].items():
        frozen = "core--" + source.replace("/", "--") + ".txt"
        assert manifest["files_sha256"][frozen] == expected, source
    print(json.dumps({"success": True, "cases": 20, "parent_name_survives_fork": True, "provisional_boot_excluded": True, "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
