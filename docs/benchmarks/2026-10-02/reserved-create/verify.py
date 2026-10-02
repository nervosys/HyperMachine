#!/usr/bin/env python3
"""Verify real named-creation evidence without treating the fork gap as resolved."""
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
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    assert len(report["cases"]) == 20 and all(row["success"] for row in report["cases"])
    assert report["artifact_sha256"] == manifest["artifacts"]
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    alias = json.loads((directory.parent / "reserved-alias/manifest.json").read_text())
    for artifact in ("daemon", "cli", "kernel", "initrd"):
        assert report["artifact_sha256"][artifact] == alias["passed_artifacts"][artifact]
    assert report["ssh_fixture"]["resolution"] == "reserved creation"
    cases = {row["name"]: row["result"] for row in report["cases"]}
    creation = cases["reserved-name-creation-and-duplicate-refusal"]
    assert all(creation[key] for key in ("creation_bound_name", "CLI_inspection", "both_create_routes_refuse_duplicate", "inventory_creation_refused"))
    assert cases["OpenSSH-API-TLS-node-mTLS-binary-roundtrip"]["bytes"] == 1048576
    assert cases["OpenSSH-preserves-remote-exit-code"]["exit_code"] == 7
    assert cases["delete-closes-active-tunnel-and-refuses-reopen"]["creation_name_deleted_and_reused"]
    fork = cases["fork-preserves-guest-listener"]
    assert fork["reserved_lookup_refuses_legacy_fork_conflict"]
    assert fork["fork_duplicate_name_rejected"] and fork["name_resolves_after_duplicate_deleted"]
    assert manifest["unresolved_fork_metadata_conflict"]
    print(json.dumps({"success": True, "cases": 20, "fork_gap_remains": True, "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
