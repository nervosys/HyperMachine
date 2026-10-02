#!/usr/bin/env python3
"""Verify archived committed-name recovery after real response loss."""
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
    assert len(report["cases"]) == 24 and all(row["success"] for row in report["cases"])
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    assert report["artifact_sha256"] == manifest["artifacts"]
    assert report["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["coordinator.py"]
    previous = json.loads((directory.parent / "node-completion/run-1.json").read_text())
    for name, expected in previous["artifact_sha256"].items():
        if name != "coordinator":
            assert report["artifact_sha256"][name] == expected, name
    cases = {row["name"]: row["result"] for row in report["cases"]}
    for row in previous["cases"]:
        assert cases[row["name"]] == row["result"], row["name"]
    for mode in ("drop", "timeout"):
        result = cases[f"control-plane-node-response-{mode}-after-commit"]
        assert result["control_plane_status"] == 502 and result["backend_requests"] == 1
        for field in ("node_committed_before_response_loss", "CLI_recovers_ID",
                      "named_SSH_reaches_same_guest", "duplicate_creation_refused",
                      "no_extra_VM", "deleted_name_released", "relay_listener_and_handler_stopped"):
            assert result[field], field
        if mode == "timeout":
            assert 59 <= result["elapsed_seconds"] < 85
    print(json.dumps({"success": True, "cases": 24, "response_loss_recovery": True,
                      "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
