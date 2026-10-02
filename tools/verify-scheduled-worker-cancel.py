#!/usr/bin/env python3
"""Verify the archived optimized-worker KVM cancellation run."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "docs/benchmarks/2026-10-01"
m = json.loads((root / "scheduled-worker-cancel-manifest.json").read_text())
raw = (root / "scheduled-worker-cancel-run-1.json").read_bytes()
assert hashlib.sha256(raw).hexdigest() == m["report_sha256"]
for name, expected in m["source_sha256"].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected
r = json.loads(raw)
assert r["artifact_sha256"] == m["artifact_sha256"]
assert r["success"] and r["remaining_sandboxes"] == 0 and not r["cleanup_errors"]
assert len(r["cases"]) == m["verified_cases"] == 16
assert all(c["success"] for c in r["cases"])
assert len(r["owned_processes_stopped"]) == m["registered_processes_stopped"] == 22
c = r["cases"][0]
assert c["name"] == "scheduled-VM-worker-TLS-resume-restart-and-no-replay"
assert c["result"]["guest_exit_code"] == 7
assert all(c["result"][key] is True for key in [
    "paused_guest_resumed", "literal_environment_preserved", "durable_output_recovered",
    "duplicate_guest_execution_refused", "history_survives_cancellation", "automatic_worker",
    "restart_continues_next_occurrence", "cancelled_worker_leaves_pending_work_untouched"])
print("Verified optimized worker KVM cancellation run, 16 cases and cleanup; no performance score.")
