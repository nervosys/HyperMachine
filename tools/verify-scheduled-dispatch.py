#!/usr/bin/env python3
"""Verify the archived functional run; optionally check a frozen CLI artifact."""
import argparse
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parents[1] / "docs/benchmarks/2026-10-01"
    manifest = json.loads((directory / "scheduled-dispatch-manifest.json").read_text())
    raw = (directory / "scheduled-dispatch-run-1.json").read_bytes()
    patch = (directory / "scheduled-dispatch-source.patch").read_bytes()
    assert hashlib.sha256(raw).hexdigest() == manifest["report_sha256"]
    assert hashlib.sha256(patch).hexdigest() == manifest["source_patch_sha256"]
    report = json.loads(raw)
    assert report["success"] and not report["cleanup_errors"]
    assert report["remaining_sandboxes"] == 0
    assert len(report["cases"]) == manifest["verified_cases"] == 16
    assert all(case["success"] for case in report["cases"])
    assert report["artifact_sha256"] == manifest["artifact_sha256"]
    result = report["cases"][0]
    assert result["name"] == "scheduled-VM-dispatch-TLS-resume-receipt-and-no-replay"
    assert result["result"]["guest_exit_code"] == 7
    assert all(result["result"][key] is True for key in [
        "paused_guest_resumed", "literal_environment_preserved", "durable_output_recovered",
        "duplicate_guest_execution_refused", "history_survives_cancellation"])
    assert len(report["owned_processes_stopped"]) == manifest["registered_processes_stopped"] == 22
    if args.cli:
        assert hashlib.sha256(args.cli.read_bytes()).hexdigest() == manifest["artifact_sha256"]["cli"]
    print("Verified archived 16-case functional run and cleanup; no performance comparison.")


if __name__ == "__main__":
    main()
