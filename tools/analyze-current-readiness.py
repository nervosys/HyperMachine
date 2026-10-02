#!/usr/bin/env python3
"""Compare preserved failures with unscored stage diagnostics, without causal claims."""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import re
import statistics


def stats(values):
    return None if not values else {"n": len(values), "min": min(values),
        "median": statistics.median(values), "mean": statistics.mean(values), "max": max(values)}


def analyze(baseline_path, diagnostic_path):
    baseline = json.loads(baseline_path.read_text())
    diagnostic = json.loads(diagnostic_path.read_text())
    assert diagnostic["diagnostic_only"] and diagnostic["artifacts_unchanged"]
    assert diagnostic["cold_ids_match_passed_requests"] and diagnostic["stage_ids_match_passed_requests"]
    assert baseline["concurrency"] == diagnostic["concurrency"] == 100
    for name in ("hypermachine", "firecracker", "kernel", "initrd"):
        assert baseline["artifact_sha256"][name] == diagnostic["artifact_sha256"][name]
    failed = [row for batch in baseline["batches"] for row in batch["samples"] if not row["success"]]
    states, addresses = collections.Counter(), collections.Counter()
    for row in failed:
        error = row.get("error", "")
        state = re.search(r"run_state=(\w+)", error)
        address = re.search(r"RIP=(0x[0-9a-f]+)", error)
        if state:
            states[state[1]] += 1
        if address:
            addresses[address[1]] += 1
    cold = list(diagnostic["cold_readiness_stages_ms"].values())
    passed = [row for row in cold if row["succeeded"]]
    requests = [row for batch in diagnostic["batches"] for row in batch["samples"]]
    return {
        "diagnostic_only": True, "runtime_change_adopted": False,
        "baseline_sha256": hashlib.sha256(baseline_path.read_bytes()).hexdigest(),
        "diagnostic_sha256": hashlib.sha256(diagnostic_path.read_bytes()).hexdigest(),
        "baseline_failed_attempts": len(failed),
        "baseline_failure_elapsed_ms": stats([row["failure_elapsed_ms"] for row in failed]),
        "baseline_failed_batch_submission_offset_ms": stats([row["start_offset_ms"] for row in failed]),
        "baseline_guest_run_states": dict(states), "baseline_instruction_addresses_unresolved": dict(addresses),
        "baseline_zero_recent_vcpu_exits": sum("vCPU 0: 0 exits" in row.get("error", "") for row in failed),
        "diagnostic_attempts": len(requests),
        "diagnostic_passed": sum(row["success"] and row["cleanup_success"] for row in requests),
        "diagnostic_agent_stage_rows": len(cold), "diagnostic_agent_stage_failures": len(cold) - len(passed),
        "diagnostic_successful_agent_stages_ms": {
            name: stats([row[name] for row in passed if name in row])
            for name in ("blocking_queue_ms", "connect_ms", "ping_ms")},
        "diagnostic_startup_stages_ms": {
            name: stats([row[name] for row in diagnostic["startup_stages_ms"].values()])
            for name in ("build_ms", "launch_ms", "agent_ms", "network_envd_ms")},
        "cleanup_verified": all(not report["cleanup_errors"] and report["remaining_sandbox_count"] == 0
                                for report in (baseline, diagnostic)),
        "limits": ["Tracing changes timing and the failure was not reproduced",
                   "Client submission offset does not measure server queueing",
                   "Connection wait includes guest boot, driver and listener readiness",
                   "Timeout guest-state samples are not a root cause or matching symbol proof"]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--diagnostic", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = analyze(args.baseline, args.diagnostic)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
