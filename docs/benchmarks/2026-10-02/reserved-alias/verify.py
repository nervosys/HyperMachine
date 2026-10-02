#!/usr/bin/env python3
"""Check frozen reserved-alias regression evidence, including the failed run."""
import hashlib
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    failed, passed = [json.loads((directory / f"run-{number}.json").read_text()) for number in (1, 2)]
    assert not failed["success"] and passed["success"]
    assert len(failed["cases"]) == len(passed["cases"]) == 20
    assert all(row["success"] for row in failed["cases"][:-1])
    assert not failed["cases"][-1]["success"]
    assert "409 Conflict" in failed["cases"][-1]["error"]
    assert all(row["success"] for row in passed["cases"])
    assert [row["name"] for row in failed["cases"]] == [row["name"] for row in passed["cases"]]
    for report in (failed, passed):
        assert not report["cleanup_errors"] and report["remaining_sandboxes"] == 0
        assert len(report["owned_processes_stopped"]) == 22
        assert report["ssh_fixture"]["resolution"] == "reserved alias"
        assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    before, after = failed["artifact_sha256"], passed["artifact_sha256"]
    assert before.keys() == after.keys()
    assert [key for key in before if before[key] != after[key]] == ["control-plane"]
    assert before == manifest["failed_artifacts"] and after == manifest["passed_artifacts"]
    cases = {row["name"]: row["result"] for row in passed["cases"]}
    assignment = cases["reserved-alias-CLI-TLS-assignment-and-lookup"]
    assert all(assignment[key] for key in ("CLI_assignment_and_replay", "CLI_inspection", "metadata_name_absent", "inventory_assignment_refused"))
    assert cases["OpenSSH-API-TLS-node-mTLS-binary-roundtrip"]["bytes"] == 1048576
    assert cases["OpenSSH-preserves-remote-exit-code"]["exit_code"] == 7
    fork = cases["fork-preserves-guest-listener"]
    assert fork["alias_transfer_refused"] and fork["alias_keeps_parent_after_fork"]
    assert cases["delete-closes-active-tunnel-and-refuses-reopen"]["alias_deleted_and_reused"]
    print(json.dumps({"success": True, "cases": 20, "failed_run_preserved": True, "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
